use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
};

use async_trait::async_trait;
use fishmuse_ai::{MusicToolExecutor, ToolRegistry};
use fishmuse_desktop::{
    playback_queue::{PlaybackQueueService, QueueCommand},
    state::PlaybackApplicationService,
};
use fishmuse_domain::{
    AppResult, LibraryItem, ListenSummary, MediaAssetId, OperationId, PlayableSource, TrackId,
    TrackSummary, UserId,
};
use fishmuse_library::{LibraryQueryPort, SearchQuery};
use fishmuse_playback::{
    PlaybackBackendKind, PlaybackCommand, PlaybackControl, PlaybackEvent, PlaybackSnapshot,
    PlaybackStatus,
};
use serde_json::json;
use tokio::sync::broadcast;

struct FakeLibrary {
    unplayable: Mutex<HashSet<TrackId>>,
}

impl FakeLibrary {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            unplayable: Mutex::new(HashSet::new()),
        })
    }

    fn make_unplayable(&self, track_id: TrackId) {
        self.unplayable.lock().unwrap().insert(track_id);
    }
}

#[async_trait]
impl LibraryQueryPort for FakeLibrary {
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
        if self.unplayable.lock().unwrap().contains(&track_id) {
            return Ok(None);
        }
        Ok(Some(PlayableSource {
            track_id,
            media_asset_id: MediaAssetId::new(),
            subsong_index: None,
            start_ms: None,
            end_ms: None,
        }))
    }
}

struct RecordingApplication {
    commands: Mutex<Vec<PlaybackCommand>>,
    events: broadcast::Sender<PlaybackEvent>,
}

impl RecordingApplication {
    fn new() -> Arc<Self> {
        let (events, _) = broadcast::channel(8);
        Arc::new(Self {
            commands: Mutex::new(Vec::new()),
            events,
        })
    }

    fn commands(&self) -> Vec<PlaybackCommand> {
        self.commands.lock().unwrap().clone()
    }
}

#[async_trait]
impl PlaybackControl for RecordingApplication {
    async fn execute(&self, command: PlaybackCommand) -> AppResult<PlaybackSnapshot> {
        let (status, track_id) = match &command {
            PlaybackCommand::Play { source, .. } => {
                (PlaybackStatus::Playing, Some(source.track_id))
            }
            PlaybackCommand::Stop { .. } => (PlaybackStatus::Stopped, None),
            _ => (PlaybackStatus::Paused, None),
        };
        self.commands.lock().unwrap().push(command.clone());
        Ok(PlaybackSnapshot {
            revision: self.commands.lock().unwrap().len() as u64,
            status,
            track_id,
            position_ms: 0,
            duration_ms: Some(1_000),
            volume: 1.0,
            backend: PlaybackBackendKind::Foobar2000,
        })
    }

    async fn snapshot(&self) -> AppResult<PlaybackSnapshot> {
        Ok(PlaybackSnapshot {
            revision: 0,
            status: PlaybackStatus::Stopped,
            track_id: None,
            position_ms: 0,
            duration_ms: None,
            volume: 1.0,
            backend: PlaybackBackendKind::Foobar2000,
        })
    }

    fn subscribe(&self) -> broadcast::Receiver<PlaybackEvent> {
        self.events.subscribe()
    }
}

#[async_trait]
impl PlaybackApplicationService for RecordingApplication {
    async fn execute(&self, command: PlaybackCommand) -> AppResult<PlaybackSnapshot> {
        PlaybackControl::execute(self, command).await
    }

    async fn snapshot(&self) -> AppResult<PlaybackSnapshot> {
        PlaybackControl::snapshot(self).await
    }

    async fn shutdown(&self) {}

    fn subscribe(&self) -> Option<broadcast::Receiver<PlaybackEvent>> {
        Some(PlaybackControl::subscribe(self))
    }
}

fn fixture() -> (
    UserId,
    Arc<FakeLibrary>,
    Arc<RecordingApplication>,
    Arc<PlaybackQueueService>,
) {
    let user_id = UserId::new();
    let library = FakeLibrary::new();
    let application = RecordingApplication::new();
    let queue = Arc::new(PlaybackQueueService::new(
        user_id,
        library.clone(),
        application.clone(),
    ));
    (user_id, library, application, queue)
}

async fn play_now(queue: &PlaybackQueueService, track_id: TrackId, context: Vec<TrackId>) {
    queue
        .apply(QueueCommand::PlayNow {
            track_id,
            context,
            operation_id: OperationId::new(),
        })
        .await
        .expect("play now");
}

fn played_track(command: &PlaybackCommand) -> Option<TrackId> {
    match command {
        PlaybackCommand::Play { source, .. } => Some(source.track_id),
        _ => None,
    }
}

#[tokio::test]
async fn replaces_appends_and_plays_an_index_without_losing_context() {
    let (_, _, application, queue) = fixture();
    let [a, b, c, d] = [
        TrackId::new(),
        TrackId::new(),
        TrackId::new(),
        TrackId::new(),
    ];
    let operation_id = OperationId::new();

    let snapshot = queue
        .apply(QueueCommand::PlayNow {
            track_id: b,
            context: vec![a, b, c],
            operation_id,
        })
        .await
        .expect("replace queue");
    assert_eq!(snapshot.track_ids, vec![a, b, c]);
    assert_eq!(snapshot.current_index, Some(1));
    assert!(matches!(
        &application.commands()[0],
        PlaybackCommand::Play { source, operation_id: actual } if source.track_id == b && *actual == operation_id
    ));

    queue
        .apply(QueueCommand::Add { track_id: d })
        .await
        .expect("append");
    let snapshot = queue
        .apply(QueueCommand::PlayAt { index: 2 })
        .await
        .expect("play index");
    assert_eq!(snapshot.track_ids, vec![a, b, c, d]);
    assert_eq!(snapshot.current_index, Some(2));
    assert_eq!(
        played_track(application.commands().last().unwrap()),
        Some(c)
    );
}

#[tokio::test]
async fn remove_before_at_and_after_current_repairs_the_index_atomically() {
    let (_, _, _, queue) = fixture();
    let [a, b, c, d] = [
        TrackId::new(),
        TrackId::new(),
        TrackId::new(),
        TrackId::new(),
    ];
    play_now(&queue, b, vec![a, b, c, d]).await;

    let before = queue
        .apply(QueueCommand::Remove { index: 0 })
        .await
        .unwrap();
    assert_eq!(before.track_ids, vec![b, c, d]);
    assert_eq!(before.current_index, Some(0));

    let after = queue
        .apply(QueueCommand::Remove { index: 2 })
        .await
        .unwrap();
    assert_eq!(after.track_ids, vec![b, c]);
    assert_eq!(after.current_index, Some(0));

    let current = queue
        .apply(QueueCommand::Remove { index: 0 })
        .await
        .unwrap();
    assert_eq!(current.track_ids, vec![c]);
    assert_eq!(current.current_index, None);

    let cleared = queue.apply(QueueCommand::Clear).await.unwrap();
    assert!(cleared.track_ids.is_empty());
    assert_eq!(cleared.current_index, None);
}

#[tokio::test]
async fn accepts_one_thousand_tracks_and_rejects_the_next_without_mutation() {
    let (_, _, _, queue) = fixture();
    let tracks: Vec<_> = (0..1_000).map(|_| TrackId::new()).collect();
    play_now(&queue, tracks[0], tracks.clone()).await;

    let error = queue
        .apply(QueueCommand::Add {
            track_id: TrackId::new(),
        })
        .await
        .expect_err("queue limit");
    assert_eq!(
        error.user_message,
        "The playback queue can contain at most 1000 tracks."
    );
    assert_eq!(queue.queue_snapshot().await.track_ids, tracks);
}

#[tokio::test]
async fn previous_history_is_capped_at_one_hundred_and_then_seeks_to_zero() {
    let (_, _, application, queue) = fixture();
    let tracks: Vec<_> = (0..102).map(|_| TrackId::new()).collect();
    play_now(&queue, tracks[0], tracks.clone()).await;
    for _ in 0..101 {
        queue.apply(QueueCommand::Next).await.expect("next");
    }
    for _ in 0..100 {
        queue.apply(QueueCommand::Previous).await.expect("previous");
    }
    let before_fallback = queue.queue_snapshot().await;
    assert_eq!(before_fallback.current_index, Some(1));

    queue
        .apply(QueueCommand::Previous)
        .await
        .expect("seek fallback");
    assert!(matches!(
        application.commands().last(),
        Some(PlaybackCommand::Seek { position_ms: 0, .. })
    ));
    assert_eq!(queue.queue_snapshot().await.current_index, Some(1));
}

#[tokio::test]
async fn next_at_the_end_stops_and_unplayable_selection_leaves_state_unchanged() {
    let (_, library, application, queue) = fixture();
    let [a, b] = [TrackId::new(), TrackId::new()];
    play_now(&queue, a, vec![a, b]).await;
    library.make_unplayable(b);
    let before = queue.queue_snapshot().await;

    assert!(queue.apply(QueueCommand::Next).await.is_err());
    assert_eq!(queue.queue_snapshot().await, before);
    library.unplayable.lock().unwrap().remove(&b);
    queue.apply(QueueCommand::Next).await.expect("play b");
    queue.apply(QueueCommand::Next).await.expect("stop at end");
    assert!(matches!(
        application.commands().last(),
        Some(PlaybackCommand::Stop { .. })
    ));
}

#[tokio::test]
async fn rapid_next_and_previous_commands_select_each_track_once() {
    let (_, _, application, queue) = fixture();
    let [a, b, c] = [TrackId::new(), TrackId::new(), TrackId::new()];
    play_now(&queue, a, vec![a, b, c]).await;

    let first_next = tokio::spawn({
        let queue = queue.clone();
        async move { queue.apply(QueueCommand::Next).await }
    });
    let second_next = tokio::spawn({
        let queue = queue.clone();
        async move { queue.apply(QueueCommand::Next).await }
    });
    first_next.await.unwrap().unwrap();
    second_next.await.unwrap().unwrap();
    assert_eq!(queue.queue_snapshot().await.current_index, Some(2));

    let first_previous = tokio::spawn({
        let queue = queue.clone();
        async move { queue.apply(QueueCommand::Previous).await }
    });
    let second_previous = tokio::spawn({
        let queue = queue.clone();
        async move { queue.apply(QueueCommand::Previous).await }
    });
    first_previous.await.unwrap().unwrap();
    second_previous.await.unwrap().unwrap();
    assert_eq!(queue.queue_snapshot().await.current_index, Some(0));

    let played: Vec<_> = application
        .commands()
        .iter()
        .filter_map(played_track)
        .collect();
    assert_eq!(played, vec![a, b, c, b, a]);
}

#[tokio::test]
async fn ai_play_replaces_the_queue_and_ai_skip_uses_the_shared_transition() {
    let (user_id, library, application, queue) = fixture();
    let [a, b, c] = [TrackId::new(), TrackId::new(), TrackId::new()];
    play_now(&queue, a, vec![a, b, c]).await;
    let control: Arc<dyn PlaybackControl> = queue.clone();
    let tools = ToolRegistry::new(Arc::new(MusicToolExecutor::new(user_id, library, control)));

    tools
        .execute("skip_next", &json!({}))
        .await
        .expect("AI next");
    assert_eq!(queue.queue_snapshot().await.current_index, Some(1));

    tools
        .execute("play_track", &json!({"track_id": c.as_uuid().to_string()}))
        .await
        .expect("AI play");
    let snapshot = queue.queue_snapshot().await;
    assert_eq!(snapshot.track_ids, vec![c]);
    assert_eq!(snapshot.current_index, Some(0));
    assert_eq!(
        played_track(application.commands().last().unwrap()),
        Some(c)
    );
}
