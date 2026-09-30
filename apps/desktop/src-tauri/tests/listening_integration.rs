use std::{
    path::Path,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use async_trait::async_trait;
use fishmuse_ai::{MusicToolExecutor, ToolRegistry, UnsupportedCredentialStore};
use fishmuse_desktop::{
    events::MemoryEventSink,
    playback_queue::PlaybackQueueService,
    state::{
        AppState, LibraryScanService, PlaybackApplicationService, PlaybackCommandDto,
        QueueCommandDto, UnavailableAIService, default_playback_state,
    },
};
use fishmuse_domain::{
    AppResult, ListenId, MediaAssetId, OperationId, RecordingId, TrackId, UserId,
};
use fishmuse_library::{ScanProgress, ScanRequest, ScanSummary};
use fishmuse_playback::{
    PlaybackBackendKind, PlaybackCommand, PlaybackControl, PlaybackEvent, PlaybackSnapshot,
    PlaybackStatus,
};
use fishmuse_storage::{Database, SqliteLibraryRepository, SqliteListeningSink};
use serde_json::json;
use tokio::sync::{Mutex, broadcast, mpsc};
use tokio_util::sync::CancellationToken;

struct NoopScanner;

#[async_trait]
impl LibraryScanService for NoopScanner {
    async fn scan(
        &self,
        _request: ScanRequest,
        _cancellation: CancellationToken,
        _progress: mpsc::Sender<ScanProgress>,
    ) -> AppResult<ScanSummary> {
        unreachable!("listening integration never starts a scan")
    }
}

struct EventPlayback {
    snapshot: Mutex<PlaybackSnapshot>,
    events: broadcast::Sender<PlaybackEvent>,
}

impl EventPlayback {
    fn new() -> Arc<Self> {
        let (events, _) = broadcast::channel(32);
        Arc::new(Self {
            snapshot: Mutex::new(PlaybackSnapshot {
                revision: 0,
                status: PlaybackStatus::Stopped,
                track_id: None,
                position_ms: 0,
                duration_ms: None,
                volume: 1.0,
                backend: PlaybackBackendKind::Foobar2000,
            }),
            events,
        })
    }

    fn emit(&self, snapshot: PlaybackSnapshot) {
        let _ = self.events.send(PlaybackEvent::Snapshot(snapshot));
    }
}

#[async_trait]
impl PlaybackApplicationService for EventPlayback {
    async fn execute(&self, command: PlaybackCommand) -> AppResult<PlaybackSnapshot> {
        let snapshot = {
            let mut snapshot = self.snapshot.lock().await;
            snapshot.revision += 1;
            match command {
                PlaybackCommand::Play { source, .. } => {
                    snapshot.status = PlaybackStatus::Playing;
                    snapshot.track_id = Some(source.track_id);
                    snapshot.position_ms = 0;
                    snapshot.duration_ms = Some(80);
                }
                PlaybackCommand::Pause { .. } => snapshot.status = PlaybackStatus::Paused,
                PlaybackCommand::Resume { .. } => snapshot.status = PlaybackStatus::Playing,
                PlaybackCommand::Stop { .. } => {
                    snapshot.status = PlaybackStatus::Stopped;
                    snapshot.track_id = None;
                    snapshot.position_ms = 0;
                }
                PlaybackCommand::Seek { position_ms, .. } => snapshot.position_ms = position_ms,
                PlaybackCommand::SetVolume { volume, .. } => snapshot.volume = volume,
                PlaybackCommand::SkipNext { .. } => snapshot.status = PlaybackStatus::Playing,
            }
            snapshot.clone()
        };
        self.emit(snapshot.clone());
        Ok(snapshot)
    }

    async fn snapshot(&self) -> AppResult<PlaybackSnapshot> {
        Ok(self.snapshot.lock().await.clone())
    }

    async fn shutdown(&self) {}

    fn subscribe(&self) -> Option<broadcast::Receiver<PlaybackEvent>> {
        Some(self.events.subscribe())
    }
}

async fn insert_track(database: &Database, user_id: UserId, suffix: u8) -> TrackId {
    let recording_id = RecordingId::new();
    let track_id = TrackId::new();
    let media_asset_id = MediaAssetId::new();
    sqlx::query("INSERT INTO recordings(recording_id, user_id, title) VALUES (?, ?, ?)")
        .bind(recording_id.as_uuid().to_string())
        .bind(user_id.as_uuid().to_string())
        .bind(format!("Recording {suffix}"))
        .execute(database.pool())
        .await
        .expect("recording");
    sqlx::query("INSERT INTO tracks(track_id, user_id, recording_id, title, duration_ms, playable) VALUES (?, ?, ?, ?, 80, 1)")
        .bind(track_id.as_uuid().to_string())
        .bind(user_id.as_uuid().to_string())
        .bind(recording_id.as_uuid().to_string())
        .bind(format!("Track {suffix}"))
        .execute(database.pool())
        .await
        .expect("track");
    sqlx::query("INSERT INTO media_assets(media_asset_id, user_id, track_id, normalized_path, original_path, availability) VALUES (?, ?, ?, ?, ?, 'available')")
        .bind(media_asset_id.as_uuid().to_string())
        .bind(user_id.as_uuid().to_string())
        .bind(track_id.as_uuid().to_string())
        .bind(vec![suffix])
        .bind(vec![suffix])
        .execute(database.pool())
        .await
        .expect("media asset");
    track_id
}

async fn wait_for_rows(sink: &SqliteListeningSink, expected: usize) {
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if sink
                .get_recent_listens(20)
                .await
                .expect("recent listens")
                .len()
                >= expected
            {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("listening rows");
}

async fn wait_for_settled(sink: &SqliteListeningSink, track_id: TrackId) {
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if sink
                .get_recent_listens(20)
                .await
                .expect("recent listens")
                .iter()
                .any(|event| event.track_id == track_id && event.ended_at.is_some())
            {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("settled listen");
}

fn test_database_path() -> std::path::PathBuf {
    std::env::temp_dir()
        .join(format!("fishmuse-listening-{}", uuid::Uuid::new_v4()))
        .join("fishmuse.sqlite3")
}

async fn insert_open_listen(database: &Database, user_id: UserId, track_id: TrackId) -> ListenId {
    let listen_id = ListenId::new();
    sqlx::query("INSERT INTO listening_events(listen_id, user_id, track_id, started_at, ended_at, listened_ms, completed, interrupted) VALUES (?, ?, ?, ?, NULL, 12, 0, 0)")
        .bind(listen_id.as_uuid().to_string())
        .bind(user_id.as_uuid().to_string())
        .bind(track_id.as_uuid().to_string())
        .bind(
            i64::try_from(
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .expect("system time")
                    .as_secs(),
            )
            .expect("unix seconds")
                - 60,
        )
        .execute(database.pool())
        .await
        .expect("open listen");
    listen_id
}

#[tokio::test]
async fn production_state_persists_ui_ai_and_shutdown_listens() {
    let database_path = test_database_path();
    let database = Database::open(&database_path).await.expect("database");
    let user_id = database.ensure_local_user().await.expect("user");
    let first = insert_track(&database, user_id, 1).await;
    let second = insert_track(&database, user_id, 2).await;
    let third = insert_track(&database, user_id, 3).await;
    let recovered = insert_open_listen(&database, user_id, third).await;
    let sink = SqliteListeningSink::new(database.pool().clone(), user_id);
    let library = Arc::new(SqliteLibraryRepository::new(
        database.pool().clone(),
        user_id,
    ));
    let backend = EventPlayback::new();
    let queue = Arc::new(PlaybackQueueService::new(
        user_id,
        library.clone(),
        backend.clone(),
    ));
    let playback: Arc<dyn PlaybackApplicationService> = queue.clone();
    let state = AppState::new(
        database,
        user_id,
        Arc::new(NoopScanner),
        library.clone(),
        playback,
        default_playback_state(),
        Arc::new(UnavailableAIService::new(false)),
        Arc::new(UnsupportedCredentialStore),
        Arc::new(MemoryEventSink::default()),
    )
    .await
    .expect("state with production listening tracker");

    wait_for_rows(&sink, 1).await;
    let recovered_row = sink
        .get_recent_listens(20)
        .await
        .expect("recovered listens")
        .into_iter()
        .find(|event| event.id == recovered)
        .expect("recovered row");
    assert!(recovered_row.ended_at.is_some());
    assert!(recovered_row.interrupted);

    state
        .execute_queue_command(QueueCommandDto::PlayNow {
            track_id: first,
            context: vec![first, second],
            operation_id: OperationId::new(),
        })
        .await
        .expect("UI play");
    tokio::time::sleep(Duration::from_millis(30)).await;
    state
        .execute_playback(PlaybackCommandDto::Pause {
            operation_id: OperationId::new(),
        })
        .await
        .expect("pause");
    tokio::time::sleep(Duration::from_millis(40)).await;
    state
        .execute_playback(PlaybackCommandDto::Resume {
            operation_id: OperationId::new(),
        })
        .await
        .expect("resume");
    tokio::time::sleep(Duration::from_millis(30)).await;

    let control: Arc<dyn PlaybackControl> = queue.clone();
    let tools = ToolRegistry::new(Arc::new(MusicToolExecutor::new(user_id, library, control)));
    tools
        .execute(
            "play_track",
            &json!({"track_id": second.as_uuid().to_string()}),
        )
        .await
        .expect("AI play");
    backend.emit(PlaybackSnapshot {
        revision: 1,
        status: PlaybackStatus::Playing,
        track_id: Some(first),
        position_ms: 0,
        duration_ms: Some(80),
        volume: 1.0,
        backend: PlaybackBackendKind::Foobar2000,
    });
    tokio::time::sleep(Duration::from_millis(100)).await;
    state
        .execute_playback(PlaybackCommandDto::Stop {
            operation_id: OperationId::new(),
        })
        .await
        .expect("stop");
    wait_for_rows(&sink, 3).await;
    wait_for_settled(&sink, second).await;

    let normal = sink.get_recent_listens(20).await.expect("normal listens");
    let first_row = normal
        .iter()
        .find(|event| event.track_id == first)
        .expect("first listen");
    let second_row = normal
        .iter()
        .find(|event| event.track_id == second)
        .expect("second listen");
    assert!(first_row.completed);
    assert!(!first_row.interrupted);
    assert!(
        first_row.listened_ms < 100,
        "paused time leaked into listening time"
    );
    assert!(second_row.completed);
    assert!(!second_row.interrupted);

    state
        .execute_queue_command(QueueCommandDto::PlayNow {
            track_id: third,
            context: vec![third],
            operation_id: OperationId::new(),
        })
        .await
        .expect("play before shutdown");
    tokio::time::sleep(Duration::from_millis(30)).await;
    state.shutdown().await;
    drop(state);
    drop(queue);
    drop(backend);
    drop(sink);

    let reopened = Database::open(Path::new(&database_path))
        .await
        .expect("reopen database");
    let persisted = SqliteListeningSink::new(reopened.pool().clone(), user_id)
        .get_recent_listens(20)
        .await
        .expect("persisted listens");
    let shutdown_row = persisted
        .iter()
        .find(|event| event.track_id == third && event.id != recovered)
        .expect("shutdown listen");
    assert!(shutdown_row.ended_at.is_some());
    assert!(shutdown_row.interrupted);
    assert_eq!(persisted.len(), 4);
    reopened.close().await;
    if let Some(parent) = database_path.parent() {
        let _ = std::fs::remove_dir_all(parent);
    }
}
