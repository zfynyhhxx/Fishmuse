#[cfg(all(windows, not(feature = "e2e")))]
use std::{ffi::OsString, os::windows::ffi::OsStringExt, path::PathBuf, process::Command};
use std::{
    path::Path,
    sync::{Arc, RwLock},
    time::Duration,
};

use async_trait::async_trait;
#[cfg(any(not(windows), feature = "e2e"))]
use fishmuse_ai::UnsupportedCredentialStore;
#[cfg(all(windows, not(feature = "e2e")))]
use fishmuse_ai::WindowsCredentialStore;
use fishmuse_ai::{
    AIService, AIServiceState, AIServiceStatus, AgentAIService, AgentLimits, AgentRunner,
    ConversationAgentStore, CredentialStore, DeepSeekClient, DeepSeekConfig, MusicToolExecutor,
    ProviderId, ServiceImplementation as AIImplementation, ToolRegistry,
};
#[cfg(all(windows, not(feature = "e2e")))]
use fishmuse_domain::MediaAssetId;
use fishmuse_domain::{AppError, AppResult, ErrorCategory, ErrorCode};
use fishmuse_library::{LibraryScanner, LoftyTagReader};
#[cfg(all(windows, not(feature = "e2e")))]
use fishmuse_playback::foobar::{
    ConnectionState as FoobarConnectionState, FoobarBackend, FoobarConfig, MediaPathResolver,
};
use fishmuse_playback::{
    PlaybackBackend, PlaybackBackendKind, PlaybackCommand, PlaybackControl, PlaybackEvent,
    PlaybackManager, PlaybackServiceState, PlaybackServiceStatus, PlaybackSnapshot, PlaybackStatus,
};
use fishmuse_storage::{
    Database, SqliteConversationRepository, SqliteLibraryRepository, SqliteOperationStore,
};
#[cfg(all(windows, not(feature = "e2e")))]
use sha2::{Digest, Sha256};
use tauri::{Manager, RunEvent};
use tokio::sync::{broadcast, watch};

pub mod commands;
pub mod error;
pub mod events;
pub mod playback_lifecycle;
pub mod playback_queue;
pub mod state;

use commands::{
    ai::{cancel_ai_turn, start_ai_turn},
    choose_library_folders, get_app_status,
    library::{cancel_library_scan, get_library_item, search_library, start_library_scan},
    playback::{
        execute_playback, execute_queue_command, get_playback_queue, get_playback_state,
        get_track_artwork, retry_playback_service,
    },
    settings::{configure_deepseek_key, delete_deepseek_key, get_ai_settings},
};
use events::TauriEventSink;
#[cfg(all(windows, not(feature = "e2e")))]
use playback_lifecycle::WindowsPlaybackLauncher;
use playback_lifecycle::{
    ManagedPlaybackService, PlaybackConnection, PlaybackConnectionState,
    UnavailablePlaybackLauncher,
};
use playback_queue::PlaybackQueueService;
use state::{AppState, PlaybackApplicationService, UnavailableAIService};

async fn bootstrap(data_dir: &Path, events: Arc<TauriEventSink>) -> AppResult<Arc<AppState>> {
    std::fs::create_dir_all(data_dir).map_err(|error| AppError {
        code: ErrorCode::StorageFailure,
        category: ErrorCategory::Storage,
        user_message: "FishMuse could not create its application data directory.".to_owned(),
        retryable: false,
        suggested_action: None,
        technical_context: Some(error.to_string()),
    })?;
    let database = Database::open(&data_dir.join("fishmuse.sqlite3")).await?;
    let user_id = database.ensure_local_user().await?;
    let library = Arc::new(SqliteLibraryRepository::new(
        database.pool().clone(),
        user_id,
    ));
    let scanner = Arc::new(LibraryScanner::new(database.pool().clone(), LoftyTagReader));
    let secure_credentials = platform_credential_store();
    let api_key = secure_credentials
        .load_api_key(ProviderId::deepseek())
        .await
        .unwrap_or(None);
    let (playback_control, playback, playback_state) =
        build_playback_service(database.pool().clone(), user_id, library.clone()).await;
    let initial_ai = build_ai_service(
        api_key,
        user_id,
        library.clone(),
        playback_control.clone(),
        database.pool().clone(),
    );
    let ai = Arc::new(ReloadableAIService::new(initial_ai));
    let credentials: Arc<dyn CredentialStore> = Arc::new(ReloadingCredentialStore {
        secure: secure_credentials,
        ai: ai.clone(),
        user_id,
        library: library.clone(),
        playback: playback_control,
        pool: database.pool().clone(),
    });

    Ok(AppState::new(
        database,
        user_id,
        scanner,
        library,
        playback,
        playback_state,
        ai,
        credentials,
        events,
    ))
}

struct ReloadableAIService {
    current: RwLock<Arc<dyn AIService>>,
}

impl ReloadableAIService {
    fn new(current: Arc<dyn AIService>) -> Self {
        Self {
            current: RwLock::new(current),
        }
    }

    fn replace(&self, service: Arc<dyn AIService>) {
        *self.current.write().expect("AI service lock poisoned") = service;
    }

    fn current(&self) -> Arc<dyn AIService> {
        self.current
            .read()
            .expect("AI service lock poisoned")
            .clone()
    }
}

impl AIService for ReloadableAIService {
    fn state(&self) -> AIServiceState {
        self.current().state()
    }

    fn start_turn(&self, request: fishmuse_ai::AIServiceTurnRequest) -> fishmuse_ai::AIServiceTurn {
        self.current().start_turn(request)
    }
}

struct ReloadingCredentialStore {
    secure: Arc<dyn CredentialStore>,
    ai: Arc<ReloadableAIService>,
    user_id: fishmuse_domain::UserId,
    library: Arc<SqliteLibraryRepository>,
    playback: Arc<dyn PlaybackControl>,
    pool: sqlx::SqlitePool,
}

#[async_trait]
impl CredentialStore for ReloadingCredentialStore {
    async fn save_api_key(
        &self,
        provider: ProviderId,
        value: secrecy::SecretString,
    ) -> AppResult<()> {
        self.secure.save_api_key(provider, value).await?;
        let key = self
            .secure
            .load_api_key(ProviderId::deepseek())
            .await?
            .ok_or_else(|| AppError {
                code: ErrorCode::StorageFailure,
                category: ErrorCategory::Configuration,
                user_message: "The API key could not be loaded after saving.".to_owned(),
                retryable: true,
                suggested_action: Some("save_api_key_again".to_owned()),
                technical_context: None,
            })?;
        self.ai.replace(build_ai_service(
            Some(key),
            self.user_id,
            self.library.clone(),
            self.playback.clone(),
            self.pool.clone(),
        ));
        Ok(())
    }

    async fn load_api_key(&self, provider: ProviderId) -> AppResult<Option<secrecy::SecretString>> {
        self.secure.load_api_key(provider).await
    }

    async fn delete_api_key(&self, provider: ProviderId) -> AppResult<()> {
        self.secure.delete_api_key(provider).await?;
        self.ai.replace(Arc::new(UnavailableAIService::new(false)));
        Ok(())
    }
}

fn build_ai_service(
    api_key: Option<secrecy::SecretString>,
    user_id: fishmuse_domain::UserId,
    library: Arc<SqliteLibraryRepository>,
    playback: Arc<dyn PlaybackControl>,
    pool: sqlx::SqlitePool,
) -> Arc<dyn AIService> {
    let Some(api_key) = api_key else {
        return Arc::new(UnavailableAIService::new(false));
    };
    let Ok(provider) = DeepSeekClient::new(api_key, DeepSeekConfig::default()) else {
        return Arc::new(UnavailableAIService::new(true));
    };
    let tools = ToolRegistry::new(Arc::new(MusicToolExecutor::new(user_id, library, playback)));
    let repository = Arc::new(SqliteConversationRepository::new(pool, user_id));
    let store = Arc::new(ConversationAgentStore::new(repository));
    let runner = AgentRunner::new(provider, tools, store, AgentLimits::default());
    Arc::new(AgentAIService::new(
        runner,
        AIServiceState {
            status: AIServiceStatus::Ready,
            implementation: Some(AIImplementation {
                id: "deepseek".to_owned(),
                display_name: "DeepSeek".to_owned(),
            }),
        },
    ))
}

async fn build_playback_service(
    pool: sqlx::SqlitePool,
    user_id: fishmuse_domain::UserId,
    library: Arc<SqliteLibraryRepository>,
) -> (
    Arc<dyn PlaybackControl>,
    Arc<dyn PlaybackApplicationService>,
    PlaybackServiceState,
) {
    #[cfg(all(windows, not(feature = "e2e")))]
    if let Ok(pipe_name) = current_user_pipe_name() {
        let resolver = Arc::new(SqliteMediaPathResolver {
            pool: pool.clone(),
            user_id,
        });
        let config = FoobarConfig::for_pipe(pipe_name).with_media_path_resolver(resolver);
        if let Ok(backend) = FoobarBackend::connect(config).await {
            let backend = Arc::new(backend);
            let operations = Arc::new(SqliteOperationStore::new(pool, user_id));
            let manager = Arc::new(PlaybackManager::new(backend.clone(), operations));
            let connection = Arc::new(FoobarPlaybackConnection::new(backend));
            let managed = Arc::new(ManagedPlaybackService::new(
                manager,
                Arc::new(WindowsPlaybackLauncher),
                connection,
                Duration::from_secs(5),
            ));
            let queue = Arc::new(PlaybackQueueService::new(user_id, library.clone(), managed));
            let control: Arc<dyn PlaybackControl> = queue.clone();
            let service: Arc<dyn PlaybackApplicationService> = queue;
            return (control, service, playback_service_state());
        }
    }

    let operations = Arc::new(SqliteOperationStore::new(pool, user_id));
    let backend = Arc::new(DisconnectedPlaybackBackend::new());
    let manager = Arc::new(PlaybackManager::new(backend, operations));
    let managed = Arc::new(ManagedPlaybackService::new(
        manager,
        Arc::new(UnavailablePlaybackLauncher),
        Arc::new(DisconnectedPlaybackConnection::new()),
        Duration::from_secs(5),
    ));
    let queue = Arc::new(PlaybackQueueService::new(user_id, library, managed));
    let control: Arc<dyn PlaybackControl> = queue.clone();
    let service: Arc<dyn PlaybackApplicationService> = queue;
    (control, service, playback_service_state())
}

fn playback_service_state() -> PlaybackServiceState {
    PlaybackServiceState {
        status: PlaybackServiceStatus::Disconnected,
        implementation: None,
    }
}

#[cfg(all(windows, not(feature = "e2e")))]
struct SqliteMediaPathResolver {
    pool: sqlx::SqlitePool,
    user_id: fishmuse_domain::UserId,
}

#[cfg(all(windows, not(feature = "e2e")))]
#[async_trait]
impl MediaPathResolver for SqliteMediaPathResolver {
    async fn resolve(&self, media_asset_id: MediaAssetId) -> AppResult<PathBuf> {
        let bytes = sqlx::query_scalar::<_, Vec<u8>>(
            "SELECT original_path FROM media_assets WHERE user_id = ? AND media_asset_id = ? AND availability = 'available'",
        )
        .bind(self.user_id.as_uuid().to_string())
        .bind(media_asset_id.as_uuid().to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(|error| playback_path_error(ErrorCode::StorageFailure, error.to_string()))?
        .ok_or_else(|| playback_path_error(ErrorCode::NotFound, "media asset is unavailable"))?;
        if bytes.len() % 2 != 0 {
            return Err(playback_path_error(
                ErrorCode::StorageFailure,
                "stored Windows path has invalid byte length",
            ));
        }
        let (pairs, remainder) = bytes.as_chunks::<2>();
        debug_assert!(remainder.is_empty());
        let wide = pairs
            .iter()
            .map(|pair| u16::from_le_bytes(*pair))
            .collect::<Vec<_>>();
        Ok(PathBuf::from(OsString::from_wide(&wide)))
    }
}

#[cfg(all(windows, not(feature = "e2e")))]
fn playback_path_error(code: ErrorCode, context: impl Into<String>) -> AppError {
    AppError {
        code,
        category: ErrorCategory::Playback,
        user_message: "The selected media source is unavailable.".to_owned(),
        retryable: false,
        suggested_action: Some("rescan_library".to_owned()),
        technical_context: Some(context.into()),
    }
}

#[cfg(all(windows, not(feature = "e2e")))]
fn current_user_pipe_name() -> AppResult<String> {
    let output = Command::new("whoami.exe")
        .args(["/user", "/fo", "csv", "/nh"])
        .output()
        .map_err(|error| playback_path_error(ErrorCode::Unavailable, error.to_string()))?;
    if !output.status.success() {
        return Err(playback_path_error(
            ErrorCode::Unavailable,
            "whoami failed while resolving the current user",
        ));
    }
    let line = String::from_utf8(output.stdout)
        .map_err(|error| playback_path_error(ErrorCode::Unavailable, error.to_string()))?;
    let sid = line
        .trim()
        .trim_matches('"')
        .rsplit_once("\",\"")
        .map(|(_, sid)| sid.trim_matches('"'))
        .filter(|sid| sid.starts_with("S-1-"))
        .ok_or_else(|| {
            playback_path_error(ErrorCode::Unavailable, "current user SID was not returned")
        })?;
    let digest = Sha256::digest(sid.as_bytes());
    Ok(format!(r"\\.\pipe\FishMuse.Foobar.v1.{digest:x}"))
}

#[cfg(all(windows, not(feature = "e2e")))]
struct FoobarPlaybackConnection {
    backend: Arc<FoobarBackend>,
    state: watch::Sender<PlaybackConnectionState>,
}

#[cfg(all(windows, not(feature = "e2e")))]
impl FoobarPlaybackConnection {
    fn new(backend: Arc<FoobarBackend>) -> Self {
        let mut backend_state = backend.connection_state();
        let initial = map_foobar_connection(&backend_state.borrow());
        let (state, _) = watch::channel(initial);
        let state_bridge = state.clone();
        tokio::spawn(async move {
            while backend_state.changed().await.is_ok() {
                state_bridge.send_replace(map_foobar_connection(&backend_state.borrow()));
            }
        });
        Self { backend, state }
    }
}

#[cfg(all(windows, not(feature = "e2e")))]
fn map_foobar_connection(state: &FoobarConnectionState) -> PlaybackConnectionState {
    if matches!(state, FoobarConnectionState::Connected { .. }) {
        PlaybackConnectionState::Ready
    } else {
        PlaybackConnectionState::Unavailable
    }
}

#[cfg(all(windows, not(feature = "e2e")))]
#[async_trait]
impl PlaybackConnection for FoobarPlaybackConnection {
    fn subscribe(&self) -> watch::Receiver<PlaybackConnectionState> {
        self.state.subscribe()
    }

    fn reconnect_now(&self) {
        self.backend.reconnect_now();
    }

    async fn shutdown(&self) {
        self.backend.shutdown().await;
    }
}

struct DisconnectedPlaybackConnection {
    state: watch::Sender<PlaybackConnectionState>,
}

impl DisconnectedPlaybackConnection {
    fn new() -> Self {
        let (state, _) = watch::channel(PlaybackConnectionState::Unavailable);
        Self { state }
    }
}

#[async_trait]
impl PlaybackConnection for DisconnectedPlaybackConnection {
    fn subscribe(&self) -> watch::Receiver<PlaybackConnectionState> {
        self.state.subscribe()
    }

    fn reconnect_now(&self) {}

    async fn shutdown(&self) {}
}

struct DisconnectedPlaybackBackend {
    events: broadcast::Sender<PlaybackEvent>,
}

impl DisconnectedPlaybackBackend {
    fn new() -> Self {
        let (events, _) = broadcast::channel(1);
        Self { events }
    }

    fn snapshot() -> PlaybackSnapshot {
        PlaybackSnapshot {
            revision: 0,
            status: PlaybackStatus::Unavailable,
            track_id: None,
            position_ms: 0,
            duration_ms: None,
            volume: 1.0,
            backend: PlaybackBackendKind::Foobar2000,
        }
    }
}

#[async_trait]
impl PlaybackBackend for DisconnectedPlaybackBackend {
    async fn execute(&self, _command: PlaybackCommand) -> AppResult<PlaybackSnapshot> {
        Err(AppError {
            code: ErrorCode::BackendUnavailable,
            category: ErrorCategory::Playback,
            user_message: "The playback backend is unavailable.".to_owned(),
            retryable: true,
            suggested_action: Some("start_playback_backend".to_owned()),
            technical_context: None,
        })
    }

    async fn snapshot(&self) -> AppResult<PlaybackSnapshot> {
        Ok(Self::snapshot())
    }

    fn subscribe(&self) -> broadcast::Receiver<PlaybackEvent> {
        self.events.subscribe()
    }
}

#[cfg(windows)]
fn platform_credential_store() -> Arc<dyn CredentialStore> {
    #[cfg(feature = "e2e")]
    return Arc::new(UnsupportedCredentialStore);

    #[cfg(not(feature = "e2e"))]
    Arc::new(WindowsCredentialStore::new())
}

#[cfg(not(windows))]
fn platform_credential_store() -> Arc<dyn CredentialStore> {
    Arc::new(UnsupportedCredentialStore)
}

pub fn run() {
    let builder = tauri::Builder::default();
    #[cfg(feature = "e2e")]
    let builder = builder
        .plugin(tauri_plugin_wdio::init())
        .plugin(tauri_plugin_wdio_webdriver::init());
    let app = builder
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            let events = Arc::new(TauriEventSink::new(app.handle().clone()));
            let state = tauri::async_runtime::block_on(bootstrap(&data_dir, events))?;
            app.manage(state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_app_status,
            choose_library_folders,
            start_library_scan,
            cancel_library_scan,
            search_library,
            get_library_item,
            configure_deepseek_key,
            delete_deepseek_key,
            get_ai_settings,
            start_ai_turn,
            cancel_ai_turn,
            execute_playback,
            execute_queue_command,
            get_playback_queue,
            get_playback_state,
            get_track_artwork,
            retry_playback_service,
        ])
        .build(tauri::generate_context!())
        .expect("error while building FishMuse desktop application");

    app.run(|handle, event| {
        if matches!(event, RunEvent::Exit) {
            let state = handle.state::<Arc<AppState>>().inner().clone();
            tauri::async_runtime::block_on(state.shutdown());
        }
    });
}

#[cfg(all(test, windows))]
mod tests {
    use super::current_user_pipe_name;

    #[test]
    fn derives_the_same_user_scoped_foobar_pipe_shape() {
        let pipe = current_user_pipe_name().expect("current user pipe name");
        let digest = pipe
            .strip_prefix(r"\\.\pipe\FishMuse.Foobar.v1.")
            .expect("FishMuse pipe prefix");
        assert_eq!(digest.len(), 64);
        assert!(digest.bytes().all(|byte| byte.is_ascii_hexdigit()));
    }
}
