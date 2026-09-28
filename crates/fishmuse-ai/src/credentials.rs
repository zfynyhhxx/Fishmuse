use async_trait::async_trait;
use fishmuse_domain::{AppError, AppResult, ErrorCategory, ErrorCode};
use secrecy::SecretString;

use crate::ProviderId;

#[cfg(windows)]
const DEEPSEEK_TARGET: &str = "FishMuse/DeepSeek";

#[async_trait]
pub trait CredentialStore: Send + Sync {
    async fn save_api_key(&self, provider: ProviderId, value: SecretString) -> AppResult<()>;
    async fn load_api_key(&self, provider: ProviderId) -> AppResult<Option<SecretString>>;
    async fn delete_api_key(&self, provider: ProviderId) -> AppResult<()>;
}

pub async fn credential_is_configured(
    store: &dyn CredentialStore,
    provider: ProviderId,
) -> AppResult<bool> {
    Ok(store.load_api_key(provider).await?.is_some())
}

#[derive(Clone, Copy, Debug, Default)]
pub struct UnsupportedCredentialStore;

#[async_trait]
impl CredentialStore for UnsupportedCredentialStore {
    async fn save_api_key(&self, _provider: ProviderId, _value: SecretString) -> AppResult<()> {
        Err(unsupported())
    }

    async fn load_api_key(&self, _provider: ProviderId) -> AppResult<Option<SecretString>> {
        Err(unsupported())
    }

    async fn delete_api_key(&self, _provider: ProviderId) -> AppResult<()> {
        Err(unsupported())
    }
}

fn unsupported() -> AppError {
    app_error(
        ErrorCode::Unavailable,
        "Secure credential storage is not available on this platform.",
        false,
        None,
    )
}

fn app_error(
    code: ErrorCode,
    user_message: &str,
    retryable: bool,
    technical_context: Option<String>,
) -> AppError {
    AppError {
        code,
        category: ErrorCategory::Configuration,
        user_message: user_message.to_owned(),
        retryable,
        suggested_action: None,
        technical_context,
    }
}

#[cfg(windows)]
fn target_for(provider: &ProviderId) -> AppResult<&'static str> {
    if provider.as_str() == "deepseek" {
        Ok(DEEPSEEK_TARGET)
    } else {
        Err(app_error(
            ErrorCode::InvalidInput,
            "This AI provider does not have a credential target.",
            false,
            None,
        ))
    }
}

#[cfg(windows)]
mod windows {
    use std::{ptr, slice};

    use async_trait::async_trait;
    use fishmuse_domain::{AppResult, ErrorCode};
    use secrecy::{ExposeSecret, SecretString};
    use windows_sys::Win32::{
        Foundation::ERROR_NOT_FOUND,
        Security::Credentials::{
            CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC, CREDENTIALW, CredDeleteW, CredFree,
            CredReadW, CredWriteW,
        },
    };

    use super::{CredentialStore, app_error, target_for};
    use crate::ProviderId;

    const MAX_CREDENTIAL_BYTES: usize = 2_560;

    #[derive(Clone, Copy, Debug, Default)]
    pub struct WindowsCredentialStore;

    impl WindowsCredentialStore {
        #[must_use]
        pub const fn new() -> Self {
            Self
        }

        pub fn target_name(&self, provider: &ProviderId) -> AppResult<&'static str> {
            target_for(provider)
        }

        #[must_use]
        pub const fn persistence_scope(&self) -> &'static str {
            "current_user"
        }

        #[must_use]
        pub const fn credential_kind(&self) -> &'static str {
            "generic"
        }
    }

    #[async_trait]
    impl CredentialStore for WindowsCredentialStore {
        async fn save_api_key(&self, provider: ProviderId, value: SecretString) -> AppResult<()> {
            let target = wide(target_for(&provider)?);
            let secret = value.expose_secret().as_bytes();
            if secret.is_empty() || secret.len() > MAX_CREDENTIAL_BYTES {
                return Err(app_error(
                    ErrorCode::InvalidInput,
                    "The API key has an invalid length.",
                    false,
                    None,
                ));
            }
            let credential = CREDENTIALW {
                Type: CRED_TYPE_GENERIC,
                TargetName: target.as_ptr().cast_mut(),
                CredentialBlobSize: u32::try_from(secret.len())
                    .map_err(|_| credential_failure("credential length overflow"))?,
                CredentialBlob: secret.as_ptr().cast_mut(),
                Persist: CRED_PERSIST_LOCAL_MACHINE,
                ..Default::default()
            };
            // SAFETY: all pointers reference live buffers for the duration of the call; the API
            // copies the input credential and is passed the documented zero flags value.
            let succeeded = unsafe { CredWriteW(&credential, 0) };
            if succeeded == 0 {
                return Err(last_credential_error("CredWriteW"));
            }
            Ok(())
        }

        async fn load_api_key(&self, provider: ProviderId) -> AppResult<Option<SecretString>> {
            let target = wide(target_for(&provider)?);
            let mut raw = ptr::null_mut::<CREDENTIALW>();
            // SAFETY: target is NUL-terminated and `raw` is a valid output pointer. A successful
            // result is owned by Credential Manager and released by CredentialGuard.
            let succeeded = unsafe { CredReadW(target.as_ptr(), CRED_TYPE_GENERIC, 0, &mut raw) };
            if succeeded == 0 {
                let error = std::io::Error::last_os_error();
                if error.raw_os_error() == Some(ERROR_NOT_FOUND as i32) {
                    return Ok(None);
                }
                return Err(credential_failure(format!(
                    "CredReadW failed with OS error {:?}",
                    error.raw_os_error()
                )));
            }
            let guard = CredentialGuard(raw);
            // SAFETY: a successful CredReadW returns a valid CREDENTIALW until CredFree. The blob
            // has exactly CredentialBlobSize bytes and is copied before the guard is dropped.
            let bytes = unsafe {
                let credential = &*guard.0;
                if credential.CredentialBlobSize == 0 {
                    return Err(credential_failure("credential blob is empty"));
                }
                slice::from_raw_parts(
                    credential.CredentialBlob,
                    credential.CredentialBlobSize as usize,
                )
            };
            let value = std::str::from_utf8(bytes)
                .map_err(|_| credential_failure("credential blob is not UTF-8"))?;
            Ok(Some(SecretString::from(value.to_owned().into_boxed_str())))
        }

        async fn delete_api_key(&self, provider: ProviderId) -> AppResult<()> {
            let target = wide(target_for(&provider)?);
            // SAFETY: target is a live NUL-terminated UTF-16 buffer and flags are documented zero.
            let succeeded = unsafe { CredDeleteW(target.as_ptr(), CRED_TYPE_GENERIC, 0) };
            if succeeded == 0 {
                let error = std::io::Error::last_os_error();
                if error.raw_os_error() == Some(ERROR_NOT_FOUND as i32) {
                    return Ok(());
                }
                return Err(credential_failure(format!(
                    "CredDeleteW failed with OS error {:?}",
                    error.raw_os_error()
                )));
            }
            Ok(())
        }
    }

    struct CredentialGuard(*mut CREDENTIALW);

    impl Drop for CredentialGuard {
        fn drop(&mut self) {
            // SAFETY: the pointer came from a successful CredReadW and is freed exactly once here.
            unsafe { CredFree(self.0.cast()) };
        }
    }

    fn wide(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn last_credential_error(operation: &str) -> fishmuse_domain::AppError {
        let error = std::io::Error::last_os_error();
        credential_failure(format!(
            "{operation} failed with OS error {:?}",
            error.raw_os_error()
        ))
    }

    fn credential_failure(context: impl Into<String>) -> fishmuse_domain::AppError {
        app_error(
            ErrorCode::StorageFailure,
            "The API key could not be accessed securely.",
            false,
            Some(context.into()),
        )
    }
}

#[cfg(windows)]
pub use windows::WindowsCredentialStore;
