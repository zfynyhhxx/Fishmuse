use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex as StdMutex},
    time::Duration,
};

use async_trait::async_trait;
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use fishmuse_ai::{
    AI_APPLICATION_CONTRACT_VERSION, AIApplicationEvent, AIApplicationFailureReason,
    AIEventEnvelope, AIService, AIServiceState, AIServiceStatus, AIServiceTurn,
    AIServiceTurnRequest, AITurnId, ContextEnvelope, CostPolicy, CredentialStore, ProviderId,
    ServiceImplementation as AIImplementation,
};
use fishmuse_domain::{
    AppError, AppResult, ConversationId, ErrorCategory, ErrorCode, LibraryItem, OperationId,
    PlayableSource, ScanId, TrackId, TrackSummary, UserId,
};
use fishmuse_library::{
    ArtworkResolver, LibraryQueryPort, LibraryScanner, ScanProgress, ScanRequest, ScanStatus,
    ScanSummary, SearchQuery, TagReader,
};
use fishmuse_playback::{
    PlaybackCommand, PlaybackControl, PlaybackEvent, PlaybackManager, PlaybackServiceState,
    PlaybackServiceStatus, PlaybackSnapshot, PlaybackStatus,
};
use fishmuse_storage::Database;
use futures_util::{StreamExt, stream};
use secrecy::SecretString;
use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, Notify, broadcast, mpsc, watch};
use tokio_util::sync::CancellationToken;

use crate::{
    error::{CommandError, invalid_input, unavailable},
    events::{ApplicationEvent, ApplicationEventSink},
    playback_lifecycle::ManagedPlaybackService,
    playback_queue::{QueueCommand, QueueSnapshot},
};

const MAX_LIBRARY_SEARCH_LIMIT: u32 = 100;
const MAX_LIBRARY_SEARCH_OFFSET: u32 = 1_000_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CoreServiceStatus {
    Ready,
    Unavailable,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AppStatusDto {
    pub version: String,
    pub database: CoreServiceStatus,
    pub playback: PlaybackServiceState,
    pub ai: AIServiceState,
}

impl AppStatusDto {
    #[must_use]
    pub fn unavailable(version: &str) -> Self {
        Self {
            version: version.to_owned(),
            database: CoreServiceStatus::Unavailable,
            playback: PlaybackServiceState {
                status: PlaybackServiceStatus::Unavailable,
                implementation: None,
            },
            ai: AIServiceState {
                status: AIServiceStatus::NotConfigured,
                implementation: None,
            },
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SearchQueryDto {
    pub text: String,
    pub artist: Option<String>,
    pub release: Option<String>,
    pub limit: u32,
    #[serde(default)]
    pub offset: u32,
}

impl SearchQueryDto {
    pub fn validate(self) -> Result<SearchQuery, CommandError> {
        if self.limit == 0 || self.limit > MAX_LIBRARY_SEARCH_LIMIT {
            return Err(invalid_input("Search limit must be between 1 and 100."));
        }
        if self.offset > MAX_LIBRARY_SEARCH_OFFSET {
            return Err(invalid_input("Search offset must not exceed 1000000."));
        }
        Ok(SearchQuery {
            text: self.text,
            artist: self.artist,
            release: self.release,
            limit: self.limit,
            offset: self.offset,
        })
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StartTurnDto {
    pub conversation_id: ConversationId,
    pub user_text: String,
    pub context: Option<ContextEnvelope>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TurnStartedDto {
    pub turn_id: AITurnId,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PlaybackCommandDto {
    Play {
        track_id: TrackId,
        operation_id: OperationId,
    },
    Pause {
        operation_id: OperationId,
    },
    Resume {
        operation_id: OperationId,
    },
    Seek {
        position_ms: u64,
        operation_id: OperationId,
    },
    SkipNext {
        operation_id: OperationId,
    },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum QueueCommandDto {
    PlayNow {
        track_id: TrackId,
        context: Vec<TrackId>,
        operation_id: OperationId,
    },
    Add {
        track_id: TrackId,
    },
    PlayAt {
        index: usize,
    },
    Remove {
        index: usize,
    },
    Clear,
    Previous,
    Next,
}

impl From<QueueCommandDto> for QueueCommand {
    fn from(command: QueueCommandDto) -> Self {
        match command {
            QueueCommandDto::PlayNow {
                track_id,
                context,
                operation_id,
            } => Self::PlayNow {
                track_id,
                context,
                operation_id,
            },
            QueueCommandDto::Add { track_id } => Self::Add { track_id },
            QueueCommandDto::PlayAt { index } => Self::PlayAt { index },
            QueueCommandDto::Remove { index } => Self::Remove { index },
            QueueCommandDto::Clear => Self::Clear,
            QueueCommandDto::Previous => Self::Previous,
            QueueCommandDto::Next => Self::Next,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct QueueSnapshotDto {
    pub track_ids: Vec<TrackId>,
    pub current_index: Option<usize>,
    pub can_previous: bool,
    pub can_next: bool,
}

impl From<QueueSnapshot> for QueueSnapshotDto {
    fn from(snapshot: QueueSnapshot) -> Self {
        Self {
            track_ids: snapshot.track_ids,
            current_index: snapshot.current_index,
            can_previous: snapshot.can_previous,
            can_next: snapshot.can_next,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PlaybackTrackDto {
    pub id: TrackId,
    pub title: String,
    pub artist_names: Vec<String>,
    pub release_title: Option<String>,
    pub artwork_available: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PlaybackViewDto {
    pub revision: u64,
    pub status: PlaybackStatus,
    pub position_ms: u64,
    pub duration_ms: Option<u64>,
    pub volume: f32,
    pub muted: bool,
    pub track: Option<PlaybackTrackDto>,
    pub queue: QueueSnapshotDto,
    pub external: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ArtworkDto {
    pub data_url: String,
}

#[derive(Default)]
pub struct PlaybackViewProjector {
    current: StdMutex<Option<PlaybackViewDto>>,
}

impl PlaybackViewProjector {
    pub fn accept(&self, candidate: PlaybackViewDto) -> bool {
        self.accepted(candidate).is_some()
    }

    fn accepted(&self, mut candidate: PlaybackViewDto) -> Option<PlaybackViewDto> {
        let mut current = self.current.lock().expect("playback view lock poisoned");
        if current
            .as_ref()
            .is_some_and(|existing| candidate.revision <= existing.revision)
        {
            return None;
        }
        if candidate.status == PlaybackStatus::Unavailable
            && candidate.track.is_none()
            && let Some(existing) = current.as_ref()
        {
            candidate.track.clone_from(&existing.track);
            candidate.external = existing.external;
        }
        *current = Some(candidate.clone());
        Some(candidate)
    }

    #[must_use]
    pub fn current(&self) -> Option<PlaybackViewDto> {
        self.current
            .lock()
            .expect("playback view lock poisoned")
            .clone()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ScanStartedDto {
    pub scan_id: ScanId,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ScanProgressDto {
    pub scan_id: ScanId,
    pub discovered: u64,
    pub parsed: u64,
    pub unchanged: u64,
    pub failed: u64,
    pub status: ScanEventStatus,
    pub error: Option<CommandError>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ScanEventStatus {
    Running,
    Completed,
    Cancelled,
    Failed,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ServiceStateEventDto {
    pub playback: PlaybackServiceState,
    pub ai: AIServiceState,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AISettingsDto {
    pub configured: bool,
    pub provider: String,
    pub model: String,
    pub service: AIServiceState,
    pub budget: AISettingsBudgetDto,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AISettingsBudgetDto {
    pub spent_microunits: u64,
    pub warning_at_microunits: u64,
    pub hard_stop_at_microunits: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SecretStringDto {
    pub api_key: String,
}

impl std::fmt::Debug for SecretStringDto {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SecretStringDto")
            .field("api_key", &"[REDACTED]")
            .finish()
    }
}

#[async_trait]
pub trait LibraryScanService: Send + Sync {
    async fn scan(
        &self,
        request: ScanRequest,
        cancellation: CancellationToken,
        progress: mpsc::Sender<ScanProgress>,
    ) -> AppResult<ScanSummary>;
}

#[async_trait]
impl<R> LibraryScanService for LibraryScanner<R>
where
    R: TagReader + 'static,
{
    async fn scan(
        &self,
        request: ScanRequest,
        cancellation: CancellationToken,
        progress: mpsc::Sender<ScanProgress>,
    ) -> AppResult<ScanSummary> {
        self.scan(request, cancellation, progress).await
    }
}

#[async_trait]
pub trait PlaybackApplicationService: Send + Sync {
    async fn retry(&self) -> AppResult<()> {
        Err(AppError {
            code: ErrorCode::BackendUnavailable,
            category: ErrorCategory::Playback,
            user_message: "The playback service could not be started.".to_owned(),
            retryable: true,
            suggested_action: Some("open_advanced_playback_diagnostics".to_owned()),
            technical_context: None,
        })
    }

    async fn execute(&self, command: PlaybackCommand) -> AppResult<PlaybackSnapshot>;
    async fn snapshot(&self) -> AppResult<PlaybackSnapshot>;
    async fn shutdown(&self);

    fn subscribe(&self) -> Option<broadcast::Receiver<PlaybackEvent>> {
        None
    }

    fn subscribe_service_state(&self) -> Option<watch::Receiver<PlaybackServiceState>> {
        None
    }

    async fn apply_queue(&self, _command: QueueCommand) -> AppResult<QueueSnapshot> {
        Err(AppError {
            code: ErrorCode::BackendUnavailable,
            category: ErrorCategory::Playback,
            user_message: "The playback queue is unavailable.".to_owned(),
            retryable: true,
            suggested_action: Some("retry".to_owned()),
            technical_context: None,
        })
    }

    async fn queue_snapshot(&self) -> AppResult<QueueSnapshot> {
        Err(AppError {
            code: ErrorCode::BackendUnavailable,
            category: ErrorCategory::Playback,
            user_message: "The playback queue is unavailable.".to_owned(),
            retryable: true,
            suggested_action: Some("retry".to_owned()),
            technical_context: None,
        })
    }
}

#[async_trait]
impl PlaybackApplicationService for PlaybackManager {
    async fn execute(&self, command: PlaybackCommand) -> AppResult<PlaybackSnapshot> {
        self.execute(command).await
    }

    async fn snapshot(&self) -> AppResult<PlaybackSnapshot> {
        self.snapshot().await
    }

    async fn shutdown(&self) {}
}

#[async_trait]
impl PlaybackApplicationService for ManagedPlaybackService {
    async fn retry(&self) -> AppResult<()> {
        ManagedPlaybackService::retry(self).await
    }

    async fn execute(&self, command: PlaybackCommand) -> AppResult<PlaybackSnapshot> {
        PlaybackControl::execute(self, command).await
    }

    async fn snapshot(&self) -> AppResult<PlaybackSnapshot> {
        PlaybackControl::snapshot(self).await
    }

    async fn shutdown(&self) {
        ManagedPlaybackService::shutdown(self).await;
    }

    fn subscribe(&self) -> Option<broadcast::Receiver<PlaybackEvent>> {
        Some(PlaybackControl::subscribe(self))
    }

    fn subscribe_service_state(&self) -> Option<watch::Receiver<PlaybackServiceState>> {
        Some(ManagedPlaybackService::subscribe_service_state(self))
    }
}

pub struct AppState {
    database: Mutex<Option<Database>>,
    pool: sqlx::SqlitePool,
    user_id: UserId,
    scanner: Arc<dyn LibraryScanService>,
    library: Arc<dyn LibraryQueryPort>,
    playback: Arc<dyn PlaybackApplicationService>,
    artwork: ArtworkResolver,
    playback_views: PlaybackViewProjector,
    ai: Arc<dyn AIService>,
    credentials: Arc<dyn CredentialStore>,
    events: Arc<dyn ApplicationEventSink>,
    scans: Mutex<HashMap<ScanId, CancellationToken>>,
    ai_turns: Mutex<HashMap<AITurnId, CancellationToken>>,
    playback_cancellation: CancellationToken,
    jobs_changed: Notify,
    playback_state: watch::Sender<PlaybackServiceState>,
    ai_state: watch::Sender<AIServiceState>,
    shutdown_steps: Mutex<Vec<&'static str>>,
}

impl AppState {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        database: Database,
        user_id: UserId,
        scanner: Arc<dyn LibraryScanService>,
        library: Arc<dyn LibraryQueryPort>,
        playback: Arc<dyn PlaybackApplicationService>,
        playback_state: PlaybackServiceState,
        ai: Arc<dyn AIService>,
        credentials: Arc<dyn CredentialStore>,
        events: Arc<dyn ApplicationEventSink>,
    ) -> Arc<Self> {
        let pool = database.pool().clone();
        let (playback_state, _) = watch::channel(playback_state);
        let (ai_state, _) = watch::channel(ai.state());
        let state = Arc::new(Self {
            database: Mutex::new(Some(database)),
            pool: pool.clone(),
            user_id,
            scanner,
            library,
            playback,
            artwork: ArtworkResolver::new(pool.clone()),
            playback_views: PlaybackViewProjector::default(),
            ai,
            credentials,
            events,
            scans: Mutex::new(HashMap::new()),
            ai_turns: Mutex::new(HashMap::new()),
            playback_cancellation: CancellationToken::new(),
            jobs_changed: Notify::new(),
            playback_state,
            ai_state,
            shutdown_steps: Mutex::new(Vec::new()),
        });
        state.start_playback_event_bridge();
        state.start_playback_service_state_bridge();
        state
    }

    fn start_playback_service_state_bridge(self: &Arc<Self>) {
        let Some(mut receiver) = self.playback.subscribe_service_state() else {
            return;
        };
        let state = self.clone();
        let cancellation = self.playback_cancellation.clone();
        tokio::spawn(async move {
            loop {
                let changed = tokio::select! {
                    () = cancellation.cancelled() => break,
                    changed = receiver.changed() => changed,
                };
                if changed.is_err() {
                    break;
                }
                let mut service = receiver.borrow().clone();
                if service.implementation.is_none() {
                    service.implementation = state.playback_state.borrow().implementation.clone();
                }
                state.playback_state.send_replace(service);
                state.emit_service_state();
            }
        });
    }

    fn start_playback_event_bridge(self: &Arc<Self>) {
        let Some(mut receiver) = self.playback.subscribe() else {
            return;
        };
        let state = self.clone();
        let cancellation = self.playback_cancellation.clone();
        tokio::spawn(async move {
            if let Ok(snapshot) = state.playback.snapshot().await
                && let Ok(Some(view)) = state.project_playback_snapshot(snapshot).await
            {
                state.publish_playback_view(view);
            }
            loop {
                let event = tokio::select! {
                    () = cancellation.cancelled() => break,
                    event = receiver.recv() => event,
                };
                let event = match event {
                    Ok(event) => event,
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => break,
                };
                if let Ok(Some(view)) = state.project_playback_snapshot(event.into_snapshot()).await
                {
                    state.publish_playback_view(view);
                }
            }
        });
    }

    fn publish_playback_view(&self, view: PlaybackViewDto) {
        let status = service_status_for_snapshot(self.playback_state.borrow().status, view.status);
        replace_playback_service_status(&self.playback_state, status);
        let _ = self.events.emit(ApplicationEvent::PlaybackState(view));
        self.emit_service_state();
    }

    async fn project_playback_snapshot(
        &self,
        snapshot: PlaybackSnapshot,
    ) -> AppResult<Option<PlaybackViewDto>> {
        let queue = self
            .playback
            .queue_snapshot()
            .await
            .map(QueueSnapshotDto::from)
            .unwrap_or_else(|_| empty_queue());
        let (track, external) = match snapshot.track_id {
            Some(track_id) => match self.library.get_item(self.user_id, track_id).await? {
                Some(item) => {
                    let artwork_available = self
                        .artwork
                        .resolve(self.user_id, track_id)
                        .await
                        .ok()
                        .flatten()
                        .is_some();
                    (
                        Some(PlaybackTrackDto {
                            id: item.track.id,
                            title: item.track.title,
                            artist_names: item.track.artist_names,
                            release_title: item.track.release_title,
                            artwork_available,
                        }),
                        false,
                    )
                }
                None => (None, true),
            },
            None => (None, false),
        };
        let candidate = PlaybackViewDto {
            revision: snapshot.revision,
            status: snapshot.status,
            position_ms: snapshot.position_ms,
            duration_ms: snapshot.duration_ms,
            volume: snapshot.volume,
            muted: snapshot.volume <= f32::EPSILON,
            track,
            queue,
            external,
        };
        Ok(self.playback_views.accepted(candidate))
    }

    #[must_use]
    pub fn status(&self) -> AppStatusDto {
        AppStatusDto {
            version: env!("CARGO_PKG_VERSION").to_owned(),
            database: CoreServiceStatus::Ready,
            playback: self.playback_state.borrow().clone(),
            ai: self.ai_state.borrow().clone(),
        }
    }

    pub async fn search_library(
        &self,
        query: SearchQueryDto,
    ) -> Result<Vec<TrackSummary>, CommandError> {
        self.library
            .search(self.user_id, query.validate()?)
            .await
            .map_err(Into::into)
    }

    pub async fn get_library_item(
        &self,
        track_id: TrackId,
    ) -> Result<Option<LibraryItem>, CommandError> {
        self.library
            .get_item(self.user_id, track_id)
            .await
            .map_err(Into::into)
    }

    pub async fn start_scan(
        self: &Arc<Self>,
        roots: Vec<String>,
    ) -> Result<ScanStartedDto, CommandError> {
        if roots.is_empty() {
            return Err(invalid_input("Choose at least one library folder."));
        }
        let roots: Vec<PathBuf> = roots.into_iter().map(PathBuf::from).collect();
        if roots.iter().any(|path| !path.is_absolute()) {
            return Err(invalid_input("Library folders must be absolute paths."));
        }
        let scan_id = ScanId::new();
        let cancellation = CancellationToken::new();
        {
            let mut scans = self.scans.lock().await;
            if !scans.is_empty() {
                return Err(invalid_input("A library scan is already running."));
            }
            scans.insert(scan_id, cancellation.clone());
        }
        let state = self.clone();
        tokio::spawn(async move {
            state.run_scan(scan_id, roots, cancellation).await;
        });
        Ok(ScanStartedDto { scan_id })
    }

    async fn run_scan(
        self: Arc<Self>,
        scan_id: ScanId,
        roots: Vec<PathBuf>,
        cancellation: CancellationToken,
    ) {
        let (progress_tx, mut progress_rx) = mpsc::channel(8);
        let scanner = self.scanner.clone();
        let user_id = self.user_id;
        let worker_cancellation = cancellation.clone();
        let worker = tokio::spawn(async move {
            scanner
                .scan(
                    ScanRequest { user_id, roots },
                    worker_cancellation,
                    progress_tx,
                )
                .await
        });
        while let Some(progress) = progress_rx.recv().await {
            let _ = self
                .events
                .emit(ApplicationEvent::ScanProgress(ScanProgressDto {
                    scan_id,
                    discovered: progress.discovered,
                    parsed: progress.parsed,
                    unchanged: progress.unchanged,
                    failed: progress.failed,
                    status: ScanEventStatus::Running,
                    error: None,
                }));
        }
        let terminal = match worker.await {
            Ok(Ok(summary)) => ScanProgressDto {
                scan_id,
                discovered: summary.discovered,
                parsed: summary.parsed,
                unchanged: summary.unchanged,
                failed: summary.failed,
                status: match summary.status {
                    ScanStatus::Completed => ScanEventStatus::Completed,
                    ScanStatus::Cancelled => ScanEventStatus::Cancelled,
                },
                error: None,
            },
            Ok(Err(error)) => ScanProgressDto {
                scan_id,
                discovered: 0,
                parsed: 0,
                unchanged: 0,
                failed: 0,
                status: ScanEventStatus::Failed,
                error: Some(error.into()),
            },
            Err(_) => ScanProgressDto {
                scan_id,
                discovered: 0,
                parsed: 0,
                unchanged: 0,
                failed: 0,
                status: ScanEventStatus::Failed,
                error: Some(unavailable(
                    ErrorCategory::Library,
                    "The library scan stopped unexpectedly.",
                )),
            },
        };
        let _ = self.events.emit(ApplicationEvent::ScanProgress(terminal));
        self.scans.lock().await.remove(&scan_id);
        self.jobs_changed.notify_waiters();
    }

    pub async fn cancel_scan(&self, scan_id: ScanId) -> Result<(), CommandError> {
        let cancellation = self.scans.lock().await.get(&scan_id).cloned();
        match cancellation {
            Some(token) => {
                token.cancel();
                Ok(())
            }
            None => Err(invalid_input("The scan is not active.")),
        }
    }

    pub async fn start_ai_turn(
        self: &Arc<Self>,
        request: StartTurnDto,
    ) -> Result<TurnStartedDto, CommandError> {
        if request.user_text.trim().is_empty() {
            return Err(invalid_input("Ask FishMuse requires a message."));
        }
        if self.ai_state.borrow().status != AIServiceStatus::Ready {
            return Err(unavailable(
                ErrorCategory::Ai,
                "AI is not configured or is currently unavailable.",
            ));
        }
        let cancellation = CancellationToken::new();
        let turn = self.ai.start_turn(AIServiceTurnRequest {
            user_id: self.user_id,
            conversation_id: request.conversation_id,
            user_text: request.user_text,
            context: request.context,
            cancellation: cancellation.clone(),
        });
        let turn_id = turn.turn_id;
        self.ai_turns.lock().await.insert(turn_id, cancellation);
        let state = self.clone();
        tokio::spawn(async move {
            state.forward_ai_events(turn).await;
        });
        Ok(TurnStartedDto { turn_id })
    }

    async fn forward_ai_events(self: Arc<Self>, mut turn: AIServiceTurn) {
        let mut last_sequence = 0_u64;
        while let Some(event) = turn.events.next().await {
            match event {
                Ok(event) => {
                    last_sequence = last_sequence.max(event.sequence);
                    let _ = self.events.emit(ApplicationEvent::Ai(event));
                }
                Err(error) => {
                    let reason = match error.category {
                        ErrorCategory::Library | ErrorCategory::Playback => {
                            AIApplicationFailureReason::Tool
                        }
                        _ => AIApplicationFailureReason::Provider,
                    };
                    let _ = self.events.emit(ApplicationEvent::Ai(AIEventEnvelope {
                        contract_version: AI_APPLICATION_CONTRACT_VERSION,
                        turn_id: turn.turn_id,
                        sequence: last_sequence.saturating_add(1),
                        event: AIApplicationEvent::TurnFailed { reason },
                    }));
                    break;
                }
            }
        }
        self.ai_turns.lock().await.remove(&turn.turn_id);
        self.jobs_changed.notify_waiters();
    }

    pub async fn cancel_ai_turn(&self, turn_id: AITurnId) -> Result<(), CommandError> {
        let cancellation = self.ai_turns.lock().await.get(&turn_id).cloned();
        match cancellation {
            Some(token) => {
                token.cancel();
                Ok(())
            }
            None => Err(invalid_input("The AI turn is not active.")),
        }
    }

    pub async fn execute_playback(
        &self,
        command: PlaybackCommandDto,
    ) -> Result<PlaybackViewDto, CommandError> {
        let command = match command {
            PlaybackCommandDto::Play {
                track_id,
                operation_id,
            } => {
                let source = self
                    .library
                    .playable_source(self.user_id, track_id)
                    .await
                    .map_err(CommandError::from)?
                    .ok_or_else(|| unavailable(ErrorCategory::Playback, "Track is unavailable."))?;
                PlaybackCommand::Play {
                    source,
                    operation_id,
                }
            }
            PlaybackCommandDto::Pause { operation_id } => PlaybackCommand::Pause { operation_id },
            PlaybackCommandDto::Resume { operation_id } => PlaybackCommand::Resume { operation_id },
            PlaybackCommandDto::Seek {
                position_ms,
                operation_id,
            } => PlaybackCommand::Seek {
                position_ms,
                operation_id,
            },
            PlaybackCommandDto::SkipNext { operation_id } => {
                PlaybackCommand::SkipNext { operation_id }
            }
        };
        let snapshot = self
            .playback
            .execute(command)
            .await
            .map_err(CommandError::from)?;
        let view = self
            .project_playback_snapshot(snapshot)
            .await
            .map_err(CommandError::from)?;
        if let Some(view) = view {
            self.publish_playback_view(view.clone());
            return Ok(view);
        }
        self.playback_views.current().ok_or_else(|| {
            CommandError::from(AppError {
                code: ErrorCode::Internal,
                category: ErrorCategory::Playback,
                user_message: "Playback state is temporarily unavailable.".to_owned(),
                retryable: true,
                suggested_action: Some("retry".to_owned()),
                technical_context: None,
            })
        })
    }

    pub async fn playback_snapshot(&self) -> Result<PlaybackViewDto, CommandError> {
        let snapshot = self.playback.snapshot().await.map_err(CommandError::from)?;
        self.project_playback_snapshot(snapshot)
            .await
            .map_err(CommandError::from)?
            .or_else(|| self.playback_views.current())
            .ok_or_else(|| {
                CommandError::from(AppError {
                    code: ErrorCode::Internal,
                    category: ErrorCategory::Playback,
                    user_message: "Playback state is temporarily unavailable.".to_owned(),
                    retryable: true,
                    suggested_action: Some("retry".to_owned()),
                    technical_context: None,
                })
            })
    }

    pub async fn track_artwork(
        &self,
        track_id: TrackId,
    ) -> Result<Option<ArtworkDto>, CommandError> {
        self.artwork
            .resolve(self.user_id, track_id)
            .await
            .map(|artwork| {
                artwork.map(|artwork| ArtworkDto {
                    data_url: format!(
                        "data:{};base64,{}",
                        artwork.mime_type,
                        BASE64_STANDARD.encode(artwork.bytes)
                    ),
                })
            })
            .map_err(Into::into)
    }

    pub async fn execute_queue_command(
        &self,
        command: QueueCommandDto,
    ) -> Result<QueueSnapshotDto, CommandError> {
        self.playback
            .apply_queue(command.into())
            .await
            .map(Into::into)
            .map_err(Into::into)
    }

    pub async fn playback_queue(&self) -> Result<QueueSnapshotDto, CommandError> {
        self.playback
            .queue_snapshot()
            .await
            .map(Into::into)
            .map_err(Into::into)
    }

    pub async fn retry_playback_service(&self) -> Result<(), CommandError> {
        self.playback.retry().await.map_err(Into::into)
    }

    pub async fn configure_ai_key(&self, secret: SecretStringDto) -> Result<(), CommandError> {
        if secret.api_key.trim().is_empty() {
            return Err(invalid_input("The API key must not be empty."));
        }
        self.credentials
            .save_api_key(
                ProviderId::deepseek(),
                SecretString::from(secret.api_key.into_boxed_str()),
            )
            .await
            .map_err(CommandError::from)?;
        let mut state = self.ai.state();
        if state.status == AIServiceStatus::NotConfigured {
            state.status = AIServiceStatus::Unavailable;
        }
        self.ai_state.send_replace(state);
        self.emit_service_state();
        Ok(())
    }

    pub async fn delete_ai_key(&self) -> Result<(), CommandError> {
        self.credentials
            .delete_api_key(ProviderId::deepseek())
            .await
            .map_err(CommandError::from)?;
        self.ai_state.send_replace(AIServiceState {
            status: AIServiceStatus::NotConfigured,
            implementation: self.ai_state.borrow().implementation.clone(),
        });
        self.emit_service_state();
        Ok(())
    }

    pub async fn ai_settings(&self) -> Result<AISettingsDto, CommandError> {
        let configured = self
            .credentials
            .load_api_key(ProviderId::deepseek())
            .await
            .map_err(CommandError::from)?
            .is_some();
        let spent_microunits = sqlx::query_scalar::<_, i64>(
            "SELECT COALESCE(SUM(cost_microunits), 0) FROM ai_usage_ledger WHERE user_id = ?",
        )
        .bind(self.user_id.as_uuid().to_string())
        .fetch_one(&self.pool)
        .await
        .map_err(|error| AppError {
            code: ErrorCode::StorageFailure,
            category: ErrorCategory::Storage,
            user_message: "AI budget information is temporarily unavailable.".to_owned(),
            retryable: true,
            suggested_action: Some("retry".to_owned()),
            technical_context: Some(error.to_string()),
        })?;
        let spent_microunits = u64::try_from(spent_microunits).map_err(|error| AppError {
            code: ErrorCode::StorageFailure,
            category: ErrorCategory::Storage,
            user_message: "AI budget information is temporarily unavailable.".to_owned(),
            retryable: false,
            suggested_action: None,
            technical_context: Some(error.to_string()),
        })?;
        let policy = CostPolicy::default();
        Ok(AISettingsDto {
            configured,
            provider: "deepseek".to_owned(),
            model: "deepseek-flash".to_owned(),
            service: self.ai_state.borrow().clone(),
            budget: AISettingsBudgetDto {
                spent_microunits,
                warning_at_microunits: policy.warning_at.0,
                hard_stop_at_microunits: policy.hard_stop_at.0,
            },
        })
    }

    fn emit_service_state(&self) {
        let _ = self
            .events
            .emit(ApplicationEvent::ServiceState(ServiceStateEventDto {
                playback: self.playback_state.borrow().clone(),
                ai: self.ai_state.borrow().clone(),
            }));
    }

    pub async fn shutdown(&self) {
        self.shutdown_steps.lock().await.push("ai_turns");
        for cancellation in self.ai_turns.lock().await.values() {
            cancellation.cancel();
        }
        self.wait_for_jobs(&self.ai_turns).await;
        self.shutdown_steps.lock().await.push("scans");
        for cancellation in self.scans.lock().await.values() {
            cancellation.cancel();
        }
        self.wait_for_jobs(&self.scans).await;
        self.shutdown_steps.lock().await.push("playback");
        self.playback_cancellation.cancel();
        self.playback.shutdown().await;
        self.shutdown_steps.lock().await.push("database");
        if let Some(database) = self.database.lock().await.take() {
            database.close().await;
        }
    }

    pub async fn shutdown_steps(&self) -> Vec<&'static str> {
        self.shutdown_steps.lock().await.clone()
    }

    async fn wait_for_jobs<K>(&self, jobs: &Mutex<HashMap<K, CancellationToken>>)
    where
        K: Eq + std::hash::Hash,
    {
        let wait = async {
            loop {
                let changed = self.jobs_changed.notified();
                if jobs.lock().await.is_empty() {
                    return;
                }
                changed.await;
            }
        };
        let _ = tokio::time::timeout(Duration::from_secs(5), wait).await;
    }
}

fn service_status_for_snapshot(
    current: PlaybackServiceStatus,
    snapshot: PlaybackStatus,
) -> PlaybackServiceStatus {
    match (current, snapshot) {
        (PlaybackServiceStatus::Starting, PlaybackStatus::Unavailable) => {
            PlaybackServiceStatus::Starting
        }
        (_, PlaybackStatus::Unavailable) => PlaybackServiceStatus::Disconnected,
        _ => PlaybackServiceStatus::Ready,
    }
}

fn empty_queue() -> QueueSnapshotDto {
    QueueSnapshotDto {
        track_ids: Vec::new(),
        current_index: None,
        can_previous: false,
        can_next: false,
    }
}

pub struct UnavailableAIService {
    state: AIServiceState,
}

impl UnavailableAIService {
    #[must_use]
    pub fn new(configured: bool) -> Self {
        Self {
            state: AIServiceState {
                status: if configured {
                    AIServiceStatus::Unavailable
                } else {
                    AIServiceStatus::NotConfigured
                },
                implementation: Some(AIImplementation {
                    id: "deepseek".to_owned(),
                    display_name: "DeepSeek".to_owned(),
                }),
            },
        }
    }
}

impl AIService for UnavailableAIService {
    fn state(&self) -> AIServiceState {
        self.state.clone()
    }

    fn start_turn(&self, _request: AIServiceTurnRequest) -> AIServiceTurn {
        AIServiceTurn {
            turn_id: AITurnId::new(),
            events: Box::pin(stream::empty()),
        }
    }
}

pub struct UnavailablePlaybackService;

#[async_trait]
impl PlaybackApplicationService for UnavailablePlaybackService {
    async fn execute(&self, _command: PlaybackCommand) -> AppResult<PlaybackSnapshot> {
        Err(AppError {
            code: ErrorCode::BackendUnavailable,
            category: ErrorCategory::Playback,
            user_message: "The playback service is unavailable.".to_owned(),
            retryable: true,
            suggested_action: Some("open_advanced_playback_diagnostics".to_owned()),
            technical_context: None,
        })
    }

    async fn snapshot(&self) -> AppResult<PlaybackSnapshot> {
        Ok(PlaybackSnapshot {
            revision: 0,
            status: PlaybackStatus::Unavailable,
            track_id: None,
            position_ms: 0,
            duration_ms: None,
            volume: 1.0,
            backend: fishmuse_playback::PlaybackBackendKind::Foobar2000,
        })
    }

    async fn shutdown(&self) {}
}

#[allow(dead_code)]
fn _assert_path_safe_source(_source: PlayableSource) {}

pub fn default_playback_state() -> PlaybackServiceState {
    PlaybackServiceState {
        status: PlaybackServiceStatus::Disconnected,
        implementation: None,
    }
}

fn replace_playback_service_status(
    state: &watch::Sender<PlaybackServiceState>,
    status: PlaybackServiceStatus,
) {
    let implementation = { state.borrow().implementation.clone() };
    state.send_replace(PlaybackServiceState {
        status,
        implementation,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn playback_service_state_replace_releases_read_borrow_before_write() {
        let implementation = fishmuse_playback::ServiceImplementation {
            id: "managed".to_owned(),
            display_name: "Managed playback service".to_owned(),
        };
        let (state, _) = watch::channel(PlaybackServiceState {
            status: PlaybackServiceStatus::Disconnected,
            implementation: Some(implementation.clone()),
        });

        replace_playback_service_status(&state, PlaybackServiceStatus::Ready);

        assert_eq!(state.borrow().status, PlaybackServiceStatus::Ready);
        assert_eq!(state.borrow().implementation, Some(implementation));
    }

    #[test]
    fn unavailable_snapshot_does_not_hide_managed_startup_progress() {
        assert_eq!(
            service_status_for_snapshot(
                PlaybackServiceStatus::Starting,
                PlaybackStatus::Unavailable,
            ),
            PlaybackServiceStatus::Starting
        );
        assert_eq!(
            service_status_for_snapshot(PlaybackServiceStatus::Ready, PlaybackStatus::Unavailable,),
            PlaybackServiceStatus::Disconnected
        );
    }
}
