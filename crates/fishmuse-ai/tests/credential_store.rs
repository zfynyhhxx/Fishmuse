use std::{collections::HashMap, fmt, sync::Mutex};

use async_trait::async_trait;
#[cfg(windows)]
use fishmuse_ai::WindowsCredentialStore;
use fishmuse_ai::{CredentialStore, ProviderId, credential_is_configured};
use fishmuse_domain::AppResult;
use secrecy::{ExposeSecret, SecretString};

#[derive(Default)]
struct FakeCredentialStore {
    values: Mutex<HashMap<String, String>>,
}

impl fmt::Debug for FakeCredentialStore {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("FakeCredentialStore")
            .field("values", &"[REDACTED]")
            .finish()
    }
}

#[async_trait]
impl CredentialStore for FakeCredentialStore {
    async fn save_api_key(&self, provider: ProviderId, value: SecretString) -> AppResult<()> {
        self.values.lock().expect("lock").insert(
            provider.as_str().to_owned(),
            value.expose_secret().to_owned(),
        );
        Ok(())
    }

    async fn load_api_key(&self, provider: ProviderId) -> AppResult<Option<SecretString>> {
        Ok(self
            .values
            .lock()
            .expect("lock")
            .get(provider.as_str())
            .map(|value| SecretString::from(value.clone().into_boxed_str())))
    }

    async fn delete_api_key(&self, provider: ProviderId) -> AppResult<()> {
        self.values.lock().expect("lock").remove(provider.as_str());
        Ok(())
    }
}

fn secret(value: &str) -> SecretString {
    SecretString::from(value.to_owned().into_boxed_str())
}

#[tokio::test]
async fn fake_store_saves_overwrites_deletes_and_reports_missing() {
    let store = FakeCredentialStore::default();
    let provider = ProviderId::deepseek();
    assert!(
        store
            .load_api_key(provider.clone())
            .await
            .expect("load")
            .is_none()
    );

    store
        .save_api_key(provider.clone(), secret("fixture-key-first"))
        .await
        .expect("save");
    assert_eq!(
        store
            .load_api_key(provider.clone())
            .await
            .expect("load")
            .expect("present")
            .expose_secret(),
        "fixture-key-first"
    );

    store
        .save_api_key(provider.clone(), secret("fixture-key-second"))
        .await
        .expect("overwrite");
    assert_eq!(
        store
            .load_api_key(provider.clone())
            .await
            .expect("load")
            .expect("present")
            .expose_secret(),
        "fixture-key-second"
    );
    assert!(
        credential_is_configured(&store, provider.clone())
            .await
            .expect("status")
    );

    store
        .delete_api_key(provider.clone())
        .await
        .expect("delete");
    assert!(store.load_api_key(provider).await.expect("load").is_none());
}

#[test]
fn credential_debug_output_never_contains_secret_material() {
    let sentinel = "fixture-key-must-not-appear";
    let client =
        fishmuse_ai::DeepSeekClient::new(secret(sentinel), Default::default()).expect("client");
    assert!(!format!("{client:?}").contains(sentinel));
    let store = FakeCredentialStore::default();
    store
        .values
        .lock()
        .expect("lock")
        .insert("deepseek".to_owned(), sentinel.to_owned());
    assert!(!format!("{store:?}").contains(sentinel));
}

#[test]
#[cfg(windows)]
fn windows_store_uses_the_fixed_current_user_generic_credential_target() {
    let store = WindowsCredentialStore::new();
    assert_eq!(
        store
            .target_name(&ProviderId::deepseek())
            .expect("supported provider"),
        "FishMuse/DeepSeek"
    );
    assert_eq!(store.persistence_scope(), "current_user");
    assert_eq!(store.credential_kind(), "generic");
}

#[cfg(windows)]
#[tokio::test]
#[ignore = "mutates and then removes the current user's FishMuse/DeepSeek credential"]
async fn windows_credential_manager_round_trip_is_current_user_scoped() {
    let store = WindowsCredentialStore::new();
    let provider = ProviderId::deepseek();
    let before = store
        .load_api_key(provider.clone())
        .await
        .expect("preflight read");
    assert!(
        before.is_none(),
        "integration test refuses to overwrite an existing credential"
    );

    let exercise = async {
        store
            .save_api_key(provider.clone(), secret("fixture-key-windows-first"))
            .await?;
        let first = store.load_api_key(provider.clone()).await?;
        store
            .save_api_key(provider.clone(), secret("fixture-key-windows-second"))
            .await?;
        let second = store.load_api_key(provider.clone()).await?;
        Ok::<_, fishmuse_domain::AppError>((first, second))
    }
    .await;

    let cleanup = store.delete_api_key(provider.clone()).await;
    let (first, second) = exercise.expect("credential round trip");
    cleanup.expect("credential cleanup");
    assert_eq!(
        first.expect("first value").expose_secret(),
        "fixture-key-windows-first"
    );
    assert_eq!(
        second.expect("second value").expose_secret(),
        "fixture-key-windows-second"
    );
    assert!(
        store
            .load_api_key(provider)
            .await
            .expect("post-cleanup read")
            .is_none()
    );
}
