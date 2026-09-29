use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

use async_trait::async_trait;
use fishmuse_ai::{
    AI_APPLICATION_CONTRACT_VERSION, AIApplicationEvent, AIApplicationFailureReason,
    AIEventEnvelope, AIService, AIServiceState, AIServiceStatus, AIServiceTurn,
    AIServiceTurnRequest, AITurnId, CredentialStore, ProviderId,
    ServiceImplementation as AIImplementation,
};
use fishmuse_desktop::{
    error::CommandError,
    events::{
        AI_EVENT, ApplicationEvent, MemoryEventSink, PLAYBACK_STATE_EVENT, SCAN_PROGRESS_EVENT,
        SERVICE_STATE_EVENT,
    },
    state::{
        AppState, AppStatusDto, LibraryScanService, PlaybackApplicationService, PlaybackCommandDto,
        SearchQueryDto, StartTurnDto, UnavailablePlaybackService,
    },
};
use fishmuse_domain::{
    AppError, AppResult, ConversationId, ErrorCategory, ErrorCode, LibraryItem, ListenSummary,
    MediaAssetId, OperationId, PlayableSource, ScanId, TrackId, TrackSummary, UserId,
};
use fishmuse_library::{ScanProgress, ScanRequest, ScanStatus, ScanSummary, SearchQuery};
use fishmuse_playback::{
    PlaybackBackendKind, PlaybackCommand, PlaybackServiceState, PlaybackServiceStatus,
    PlaybackSnapshot, PlaybackStatus,
};
use fishmuse_storage::Database;
use futures_util::stream;
use secrecy::SecretString;
use serde_json::{Value, json};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

#[test]
fn event_names_are_stable() {
    assert_eq!(SCAN_PROGRESS_EVENT, "fishmuse://scan-progress");
    assert_eq!(AI_EVENT, "fishmuse://ai-event");
    assert_eq!(PLAYBACK_STATE_EVENT, "fishmuse://playback-state");
    assert_eq!(SERVICE_STATE_EVENT, "fishmuse://service-state");
}

#[test]
fn start_turn_accepts_optional_versioned_structured_context() {
    let without: StartTurnDto = serde_json::from_value(json!({
        "conversation_id": fishmuse_domain::ConversationId::new(),
        "user_text": "hello",
        "context": null
    }))
    .expect("optional context");
    assert!(without.context.is_none());

    let with: StartTurnDto = serde_json::from_value(json!({
        "conversation_id": fishmuse_domain::ConversationId::new(),
        "user_text": "explain this",
        "context": {
            "contract_version": 1,
            "current_view": "library",
            "selected_entity": {"entity_type": "track", "track_id": TrackId::new(), "display_title": "Song"},
            "selected_text": "selected words",
            "now_playing": null
        }
    }))
    .expect("structured context");
    assert!(with.context.is_some());

    let unsupported = serde_json::from_value::<StartTurnDto>(json!({
        "conversation_id": fishmuse_domain::ConversationId::new(),
        "user_text": "hello",
        "context": {
            "contract_version": 2,
            "current_view": null,
            "selected_entity": null,
            "selected_text": null,
            "now_playing": null
        }
    }));
    assert!(unsupported.is_err());
}

#[test]
fn invalid_ids_and_unbounded_search_limits_are_rejected() {
    let invalid_playback = serde_json::from_value::<PlaybackCommandDto>(json!({
        "kind": "pause",
        "operation_id": "00000000-0000-4000-8000-000000000000"
    }));
    assert!(invalid_playback.is_err());

    let query = SearchQueryDto {
        text: "music".to_owned(),
        artist: None,
        release: None,
        limit: 101,
        offset: 0,
    };
    assert!(query.validate().is_err());

    let valid = PlaybackCommandDto::Pause {
        operation_id: OperationId::new(),
    };
    assert_eq!(serde_json::to_value(valid).unwrap()["kind"], "pause");
}

#[test]
fn command_errors_never_serialize_technical_context() {
    let error = CommandError::from(AppError {
        code: ErrorCode::StorageFailure,
        category: ErrorCategory::Storage,
        user_message: "The database is unavailable.".to_owned(),
        retryable: true,
        suggested_action: Some("retry".to_owned()),
        technical_context: Some(r#"C:\Users\alice\Music\private.db api_key=secret"#.to_owned()),
    });

    let encoded = serde_json::to_value(error).expect("safe command error");
    assert_eq!(encoded["code"], "storage_failure");
    let text = encoded.to_string().to_ascii_lowercase();
    assert!(!text.contains("technical_context"));
    assert!(!text.contains("c:\\users"));
    assert!(!text.contains("secret"));
}

#[test]
fn app_status_exposes_generic_service_states_only() {
    let value: Value = serde_json::to_value(AppStatusDto::unavailable("0.1.0")).unwrap();
    assert_eq!(value["playback"]["status"], "unavailable");
    assert_eq!(value["ai"]["status"], "not_configured");
    assert!(value.get("foobar").is_none());
    assert!(value.get("deepseek").is_none());
}

#[test]
fn library_search_accepts_bounded_user_page_offsets() {
    let dto: SearchQueryDto = serde_json::from_value(json!({
        "text": "river",
        "artist": null,
        "release": null,
        "limit": 100,
        "offset": 100
    }))
    .expect("paged search DTO");
    assert_eq!(dto.validate().expect("valid page").offset, 100);

    let excessive: SearchQueryDto = serde_json::from_value(json!({
        "text": "",
        "artist": null,
        "release": null,
        "limit": 100,
        "offset": 1_000_001
    }))
    .expect("bounded search DTO shape");
    assert!(excessive.validate().is_err());
}

#[test]
fn presentation_boundary_does_not_name_concrete_runtime_types() {
    let sources = [
        include_str!("../src/state.rs"),
        include_str!("../src/commands/ai.rs"),
        include_str!("../src/commands/playback.rs"),
        include_str!("../../src/contracts.ts"),
        include_str!("../../src/lib/ipc.ts"),
    ]
    .join("\n");

    assert!(!sources.contains("DeepSeekClient"));
    assert!(!sources.contains("AgentRunner"));
    assert!(!sources.contains("FoobarBackend"));
}

#[derive(Default)]
struct FakeLibrary;

#[async_trait]
impl fishmuse_library::LibraryQueryPort for FakeLibrary {
    async fn search(&self, _user_id: UserId, _query: SearchQuery) -> AppResult<Vec<TrackSummary>> {
        Ok(Vec::new())
    }

    async fn get_item(&self, _user_id: UserId, _id: TrackId) -> AppResult<Option<LibraryItem>> {
        Ok(None)
    }

    async fn recent_listens(&self, _user_id: UserId, _limit: u32) -> AppResult<Vec<ListenSummary>> {
        Ok(Vec::new())
    }

    async fn playable_source(
        &self,
        _user_id: UserId,
        track_id: TrackId,
    ) -> AppResult<Option<PlayableSource>> {
        Ok(Some(PlayableSource {
            track_id,
            media_asset_id: MediaAssetId::new(),
            subsong_index: None,
            start_ms: None,
            end_ms: None,
        }))
    }
}

struct BlockingScanner;

#[async_trait]
impl LibraryScanService for BlockingScanner {
    async fn scan(
        &self,
        _request: ScanRequest,
        cancellation: CancellationToken,
        progress: mpsc::Sender<ScanProgress>,
    ) -> AppResult<ScanSummary> {
        let internal_id = ScanId::new();
        progress
            .send(ScanProgress {
                scan_id: internal_id,
                discovered: 1,
                parsed: 0,
                unchanged: 0,
                failed: 0,
            })
            .await
            .expect("progress receiver");
        cancellation.cancelled().await;
        Ok(ScanSummary {
            scan_id: internal_id,
            status: ScanStatus::Cancelled,
            discovered: 1,
            parsed: 0,
            unchanged: 0,
            failed: 0,
        })
    }
}

struct FakePlayback {
    shutdown: Arc<AtomicBool>,
}

#[async_trait]
impl PlaybackApplicationService for FakePlayback {
    async fn execute(&self, command: PlaybackCommand) -> AppResult<PlaybackSnapshot> {
        let track_id = match command {
            PlaybackCommand::Play { source, .. } => Some(source.track_id),
            _ => None,
        };
        Ok(PlaybackSnapshot {
            revision: 7,
            status: PlaybackStatus::Paused,
            track_id,
            position_ms: 42,
            duration_ms: Some(100),
            backend: PlaybackBackendKind::Foobar2000,
        })
    }

    async fn snapshot(&self) -> AppResult<PlaybackSnapshot> {
        self.execute(PlaybackCommand::Pause {
            operation_id: OperationId::new(),
        })
        .await
    }

    async fn shutdown(&self) {
        self.shutdown.store(true, Ordering::SeqCst);
    }
}

struct FakeAI {
    state: AIServiceState,
    saw_context: Arc<AtomicBool>,
}

impl FakeAI {
    fn ready(saw_context: Arc<AtomicBool>) -> Self {
        Self {
            state: AIServiceState {
                status: AIServiceStatus::Ready,
                implementation: Some(AIImplementation {
                    id: "fake".to_owned(),
                    display_name: "Fake AI".to_owned(),
                }),
            },
            saw_context,
        }
    }
}

impl AIService for FakeAI {
    fn state(&self) -> AIServiceState {
        self.state.clone()
    }

    fn start_turn(&self, request: AIServiceTurnRequest) -> AIServiceTurn {
        self.saw_context
            .store(request.context.is_some(), Ordering::SeqCst);
        let turn_id = AITurnId::new();
        AIServiceTurn {
            turn_id,
            events: Box::pin(stream::iter(vec![Ok(AIEventEnvelope {
                contract_version: AI_APPLICATION_CONTRACT_VERSION,
                turn_id,
                sequence: 1,
                event: AIApplicationEvent::TurnStarted,
            })])),
        }
    }
}

struct StorageFailureAI;

impl AIService for StorageFailureAI {
    fn state(&self) -> AIServiceState {
        AIServiceState {
            status: AIServiceStatus::Ready,
            implementation: Some(AIImplementation {
                id: "fake".to_owned(),
                display_name: "Fake AI".to_owned(),
            }),
        }
    }

    fn start_turn(&self, _request: AIServiceTurnRequest) -> AIServiceTurn {
        let turn_id = AITurnId::new();
        AIServiceTurn {
            turn_id,
            events: Box::pin(stream::iter(vec![Err(AppError {
                code: ErrorCode::StorageFailure,
                category: ErrorCategory::Storage,
                user_message: "The response could not be saved.".to_owned(),
                retryable: true,
                suggested_action: None,
                technical_context: Some("foreign key".to_owned()),
            })])),
        }
    }
}

#[derive(Default)]
struct MemoryCredentials(Mutex<bool>);

#[async_trait]
impl CredentialStore for MemoryCredentials {
    async fn save_api_key(&self, _provider: ProviderId, _value: SecretString) -> AppResult<()> {
        *self.0.lock().expect("credentials lock") = true;
        Ok(())
    }

    async fn load_api_key(&self, _provider: ProviderId) -> AppResult<Option<SecretString>> {
        Ok(self
            .0
            .lock()
            .expect("credentials lock")
            .then(|| SecretString::from("configured".to_owned().into_boxed_str())))
    }

    async fn delete_api_key(&self, _provider: ProviderId) -> AppResult<()> {
        *self.0.lock().expect("credentials lock") = false;
        Ok(())
    }
}

async fn test_state(
    ai: Arc<dyn AIService>,
) -> (
    Arc<AppState>,
    Arc<MemoryEventSink>,
    Arc<AtomicBool>,
    sqlx::SqlitePool,
) {
    let database = Database::open_in_memory().await.expect("database");
    let pool = database.pool().clone();
    let user_id = database.ensure_local_user().await.expect("local user");
    let events = Arc::new(MemoryEventSink::default());
    let playback_shutdown = Arc::new(AtomicBool::new(false));
    let state = AppState::new(
        database,
        user_id,
        Arc::new(BlockingScanner),
        Arc::new(FakeLibrary),
        Arc::new(FakePlayback {
            shutdown: playback_shutdown.clone(),
        }),
        PlaybackServiceState {
            status: PlaybackServiceStatus::Ready,
            implementation: None,
        },
        ai,
        Arc::new(MemoryCredentials::default()),
        events.clone(),
    );
    (state, events, playback_shutdown, pool)
}

#[tokio::test]
async fn fake_services_preserve_events_cancellation_and_shutdown_contracts() {
    let saw_context = Arc::new(AtomicBool::new(false));
    let (state, events, playback_shutdown, _) =
        test_state(Arc::new(FakeAI::ready(saw_context.clone()))).await;

    let scan = state
        .start_scan(vec![
            std::env::current_dir()
                .expect("current directory")
                .display()
                .to_string(),
        ])
        .await
        .expect("start scan");
    assert!(
        state
            .start_scan(vec![
                std::env::current_dir()
                    .expect("current directory")
                    .display()
                    .to_string()
            ])
            .await
            .is_err()
    );
    state.cancel_scan(scan.scan_id).await.expect("cancel scan");

    let turn = state
        .start_ai_turn(StartTurnDto {
            conversation_id: ConversationId::new(),
            user_text: "hello".to_owned(),
            context: Some(
                serde_json::from_value(json!({
                    "contract_version": 1,
                    "current_view": "library",
                    "selected_entity": null,
                    "selected_text": null,
                    "now_playing": null
                }))
                .unwrap(),
            ),
        })
        .await
        .expect("start AI turn");
    assert_eq!(turn.turn_id.as_uuid().get_version_num(), 7);
    assert!(saw_context.load(Ordering::SeqCst));

    let playback = state
        .execute_playback(PlaybackCommandDto::Pause {
            operation_id: OperationId::new(),
        })
        .await
        .expect("fake playback");
    assert_eq!(playback.revision, 7);
    assert!(
        serde_json::to_value(playback)
            .unwrap()
            .get("backend")
            .is_none()
    );

    for _ in 0..10 {
        if events
            .events()
            .iter()
            .any(|event| matches!(event, ApplicationEvent::Ai(_)))
        {
            break;
        }
        tokio::task::yield_now().await;
    }
    assert!(events.events().iter().any(|event| {
        matches!(event, ApplicationEvent::Ai(envelope) if envelope.contract_version == 1 && envelope.sequence == 1)
    }));

    state.shutdown().await;
    assert!(playback_shutdown.load(Ordering::SeqCst));
    assert_eq!(
        state.shutdown_steps().await,
        vec!["ai_turns", "scans", "playback", "database"]
    );
}

#[tokio::test]
async fn ai_storage_failure_is_not_reported_as_a_music_tool_failure() {
    let (state, events, _, _) = test_state(Arc::new(StorageFailureAI)).await;
    state
        .start_ai_turn(StartTurnDto {
            conversation_id: ConversationId::new(),
            user_text: "hello".to_owned(),
            context: None,
        })
        .await
        .expect("start AI turn");

    for _ in 0..20 {
        if events.events().iter().any(|event| {
            matches!(
                event,
                ApplicationEvent::Ai(AIEventEnvelope {
                    event: AIApplicationEvent::TurnFailed { .. },
                    ..
                })
            )
        }) {
            break;
        }
        tokio::task::yield_now().await;
    }

    assert!(events.events().iter().any(|event| {
        matches!(
            event,
            ApplicationEvent::Ai(AIEventEnvelope {
                event: AIApplicationEvent::TurnFailed {
                    reason: AIApplicationFailureReason::Provider,
                },
                ..
            })
        )
    }));
    state.shutdown().await;
}

#[tokio::test]
async fn unavailable_ai_does_not_disable_core_status_or_library_search() {
    let (state, _, _, _) = test_state(Arc::new(
        fishmuse_desktop::state::UnavailableAIService::new(false),
    ))
    .await;

    assert_eq!(
        state.status().database,
        fishmuse_desktop::state::CoreServiceStatus::Ready
    );
    assert_eq!(state.status().ai.status, AIServiceStatus::NotConfigured);
    assert!(
        state
            .search_library(SearchQueryDto {
                text: String::new(),
                artist: None,
                release: None,
                limit: 20,
                offset: 0,
            })
            .await
            .is_ok()
    );
    assert!(
        state
            .start_ai_turn(StartTurnDto {
                conversation_id: ConversationId::new(),
                user_text: "hello".to_owned(),
                context: None,
            })
            .await
            .is_err()
    );
    state.shutdown().await;
}

#[tokio::test]
async fn ai_settings_reports_persisted_spend_and_budget_thresholds() {
    let (state, _, _, pool) = test_state(Arc::new(
        fishmuse_desktop::state::UnavailableAIService::new(false),
    ))
    .await;
    let user_id: String = sqlx::query_scalar("SELECT user_id FROM users LIMIT 1")
        .fetch_one(&pool)
        .await
        .expect("local user");
    sqlx::query(
        "INSERT INTO ai_usage_ledger (usage_id, user_id, conversation_id, model, input_tokens, output_tokens, cost_microunits, created_at) VALUES (?, ?, NULL, ?, ?, ?, ?, ?)",
    )
    .bind("usage-settings-budget")
    .bind(user_id)
    .bind("deepseek-flash")
    .bind(100_i64)
    .bind(50_i64)
    .bind(10_000_001_i64)
    .bind(1_i64)
    .execute(&pool)
    .await
    .expect("usage row");

    let value = serde_json::to_value(state.ai_settings().await.expect("AI settings"))
        .expect("serialize AI settings");
    assert_eq!(value["budget"]["spent_microunits"], 10_000_001);
    assert_eq!(value["budget"]["warning_at_microunits"], 10_000_000);
    assert_eq!(value["budget"]["hard_stop_at_microunits"], 20_000_000);
    assert!(value.get("api_key").is_none());
}

#[tokio::test]
async fn disconnected_playback_does_not_disable_non_playback_features() {
    let database = Database::open_in_memory().await.expect("database");
    let user_id = database.ensure_local_user().await.expect("local user");
    let state = AppState::new(
        database,
        user_id,
        Arc::new(BlockingScanner),
        Arc::new(FakeLibrary),
        Arc::new(UnavailablePlaybackService),
        PlaybackServiceState {
            status: PlaybackServiceStatus::Disconnected,
            implementation: None,
        },
        Arc::new(fishmuse_desktop::state::UnavailableAIService::new(false)),
        Arc::new(MemoryCredentials::default()),
        Arc::new(MemoryEventSink::default()),
    );

    assert_eq!(
        state.status().playback.status,
        PlaybackServiceStatus::Disconnected
    );
    assert!(
        state
            .search_library(SearchQueryDto {
                text: "still works".to_owned(),
                artist: None,
                release: None,
                limit: 20,
                offset: 0,
            })
            .await
            .is_ok()
    );
    assert!(
        state
            .execute_playback(PlaybackCommandDto::Pause {
                operation_id: OperationId::new(),
            })
            .await
            .is_err()
    );
    state.shutdown().await;
}
