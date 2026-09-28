use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

use async_trait::async_trait;
use fishmuse_domain::{AppError, AppResult, ErrorCategory, ErrorCode, TrackId};
use fishmuse_playback::{
    Clock, ListenTracker, ListeningEvent, ListeningSink, PlaybackBackendKind, PlaybackEvent,
    PlaybackSnapshot, PlaybackStatus,
};
use time::{Duration, OffsetDateTime};

#[derive(Clone)]
struct TestClock(Arc<Mutex<OffsetDateTime>>);

impl TestClock {
    fn at(unix_seconds: i64) -> Self {
        Self(Arc::new(Mutex::new(
            OffsetDateTime::from_unix_timestamp(unix_seconds).expect("timestamp"),
        )))
    }

    fn advance_seconds(&self, seconds: i64) {
        let mut now = self.0.lock().expect("clock lock");
        *now += Duration::seconds(seconds);
    }

    fn advance_milliseconds(&self, milliseconds: i64) {
        let mut now = self.0.lock().expect("clock lock");
        *now += Duration::milliseconds(milliseconds);
    }
}

impl Clock for TestClock {
    fn now(&self) -> OffsetDateTime {
        *self.0.lock().expect("clock lock")
    }
}

#[derive(Clone, Default)]
struct MemorySink {
    events: Arc<Mutex<Vec<ListeningEvent>>>,
    recovered_at: Arc<Mutex<Vec<OffsetDateTime>>>,
    fail_next_append: Arc<AtomicBool>,
}

impl MemorySink {
    fn fail_once(&self) {
        self.fail_next_append.store(true, Ordering::SeqCst);
    }
}

#[async_trait]
impl ListeningSink for MemorySink {
    async fn append(&self, event: ListeningEvent) -> AppResult<()> {
        if self.fail_next_append.swap(false, Ordering::SeqCst) {
            return Err(AppError {
                code: ErrorCode::StorageFailure,
                category: ErrorCategory::Storage,
                user_message: "storage_failure".to_owned(),
                retryable: true,
                suggested_action: None,
                technical_context: None,
            });
        }
        self.events.lock().expect("events lock").push(event);
        Ok(())
    }

    async fn recover_interrupted(&self, ended_at: OffsetDateTime) -> AppResult<()> {
        self.recovered_at
            .lock()
            .expect("recovery lock")
            .push(ended_at);
        Ok(())
    }
}

fn event(
    revision: u64,
    status: PlaybackStatus,
    track_id: Option<TrackId>,
    duration_ms: Option<u64>,
) -> PlaybackEvent {
    PlaybackEvent::Snapshot(PlaybackSnapshot {
        revision,
        status,
        track_id,
        position_ms: 0,
        duration_ms,
        backend: PlaybackBackendKind::Foobar2000,
    })
}

fn final_for(sink: &MemorySink, track_id: TrackId) -> ListeningEvent {
    sink.events
        .lock()
        .expect("events lock")
        .iter()
        .rev()
        .find(|event| event.track_id == track_id && event.ended_at.is_some())
        .expect("final event")
        .clone()
}

#[tokio::test]
async fn pause_time_is_excluded_and_resume_continues_the_same_listen() {
    let clock = TestClock::at(1_800_000_000);
    let sink = MemorySink::default();
    let track_id = TrackId::new();
    let mut tracker = ListenTracker::start(sink.clone(), clock.clone())
        .await
        .expect("tracker");

    tracker
        .handle(event(
            1,
            PlaybackStatus::Playing,
            Some(track_id),
            Some(20_000),
        ))
        .await
        .expect("play");
    clock.advance_seconds(10);
    tracker
        .handle(event(
            2,
            PlaybackStatus::Paused,
            Some(track_id),
            Some(20_000),
        ))
        .await
        .expect("pause");
    clock.advance_seconds(30);
    tracker
        .handle(event(
            3,
            PlaybackStatus::Playing,
            Some(track_id),
            Some(20_000),
        ))
        .await
        .expect("resume");
    clock.advance_seconds(5);
    tracker
        .handle(event(4, PlaybackStatus::Stopped, None, None))
        .await
        .expect("stop");

    let completed = final_for(&sink, track_id);
    assert_eq!(completed.listened_ms, 15_000);
    assert!(completed.completed);
    assert!(!completed.interrupted);
    let ids: Vec<_> = sink
        .events
        .lock()
        .expect("events lock")
        .iter()
        .filter(|event| event.track_id == track_id)
        .map(|event| event.id)
        .collect();
    assert!(ids.iter().all(|id| *id == ids[0]));
}

#[tokio::test]
async fn stale_and_duplicate_revisions_do_not_add_listening_time() {
    let clock = TestClock::at(1_800_000_000);
    let sink = MemorySink::default();
    let track_id = TrackId::new();
    let mut tracker = ListenTracker::start(sink.clone(), clock.clone())
        .await
        .expect("tracker");
    tracker
        .handle(event(
            10,
            PlaybackStatus::Playing,
            Some(track_id),
            Some(60_000),
        ))
        .await
        .expect("play");
    clock.advance_seconds(8);
    tracker
        .handle(event(
            11,
            PlaybackStatus::Paused,
            Some(track_id),
            Some(60_000),
        ))
        .await
        .expect("pause");
    clock.advance_seconds(100);
    assert!(
        !tracker
            .handle(event(
                10,
                PlaybackStatus::Playing,
                Some(track_id),
                Some(60_000)
            ))
            .await
            .expect("stale")
    );
    assert!(
        !tracker
            .handle(event(
                11,
                PlaybackStatus::Playing,
                Some(track_id),
                Some(60_000)
            ))
            .await
            .expect("duplicate")
    );
    tracker
        .handle(event(12, PlaybackStatus::Stopped, None, None))
        .await
        .expect("stop");

    assert_eq!(final_for(&sink, track_id).listened_ms, 8_000);
}

#[tokio::test]
async fn track_change_and_disconnect_settle_each_active_listen() {
    let clock = TestClock::at(1_800_000_000);
    let sink = MemorySink::default();
    let first_track = TrackId::new();
    let second_track = TrackId::new();
    let mut tracker = ListenTracker::start(sink.clone(), clock.clone())
        .await
        .expect("tracker");
    tracker
        .handle(event(1, PlaybackStatus::Playing, Some(first_track), None))
        .await
        .expect("first");
    clock.advance_seconds(3);
    tracker
        .handle(event(2, PlaybackStatus::Playing, Some(second_track), None))
        .await
        .expect("track change");
    clock.advance_seconds(4);
    tracker
        .handle(PlaybackEvent::Disconnected {
            revision: 3,
            backend: PlaybackBackendKind::Foobar2000,
        })
        .await
        .expect("disconnect");

    let first = final_for(&sink, first_track);
    let second = final_for(&sink, second_track);
    assert_eq!(first.listened_ms, 3_000);
    assert!(!first.interrupted);
    assert_eq!(second.listened_ms, 4_000);
    assert!(second.interrupted);
}

#[tokio::test]
async fn completion_threshold_is_half_duration_capped_at_four_minutes() {
    let clock = TestClock::at(1_800_000_000);
    let sink = MemorySink::default();
    let short = TrackId::new();
    let long = TrackId::new();
    let mut tracker = ListenTracker::start(sink.clone(), clock.clone())
        .await
        .expect("tracker");

    tracker
        .handle(event(
            1,
            PlaybackStatus::Playing,
            Some(short),
            Some(100_000),
        ))
        .await
        .expect("short play");
    clock.advance_seconds(49);
    tracker
        .handle(event(2, PlaybackStatus::Stopped, None, None))
        .await
        .expect("short stop");
    assert!(!final_for(&sink, short).completed);

    tracker
        .handle(event(3, PlaybackStatus::Playing, Some(long), Some(600_000)))
        .await
        .expect("long play");
    clock.advance_seconds(241);
    tracker
        .handle(event(4, PlaybackStatus::Stopped, None, None))
        .await
        .expect("long stop");
    let completed = final_for(&sink, long);
    assert_eq!(completed.listened_ms, 241_000);
    assert!(completed.completed);
}

#[tokio::test]
async fn startup_marks_persisted_open_events_interrupted_at_clock_time() {
    let clock = TestClock::at(1_800_000_123);
    let sink = MemorySink::default();

    ListenTracker::start(sink.clone(), clock.clone())
        .await
        .expect("tracker");

    assert_eq!(
        sink.recovered_at.lock().expect("recovery lock").as_slice(),
        &[clock.now()]
    );
}

#[tokio::test]
async fn failed_sink_write_does_not_consume_the_event_revision() {
    let clock = TestClock::at(1_800_000_000);
    let sink = MemorySink::default();
    let track_id = TrackId::new();
    let mut tracker = ListenTracker::start(sink.clone(), clock)
        .await
        .expect("tracker");
    sink.fail_once();

    assert!(
        tracker
            .handle(event(1, PlaybackStatus::Playing, Some(track_id), None))
            .await
            .is_err()
    );
    assert!(
        tracker
            .handle(event(1, PlaybackStatus::Playing, Some(track_id), None))
            .await
            .expect("same revision retries")
    );
    assert_eq!(sink.events.lock().expect("events lock").len(), 1);
}

#[tokio::test]
async fn same_track_loading_pauses_time_and_playing_resumes_the_same_listen() {
    let clock = TestClock::at(1_800_000_000);
    let sink = MemorySink::default();
    let track_id = TrackId::new();
    let mut tracker = ListenTracker::start(sink.clone(), clock.clone())
        .await
        .expect("tracker");

    tracker
        .handle(event(
            1,
            PlaybackStatus::Playing,
            Some(track_id),
            Some(100_000),
        ))
        .await
        .expect("play");
    clock.advance_seconds(10);
    tracker
        .handle(event(
            2,
            PlaybackStatus::Loading,
            Some(track_id),
            Some(100_000),
        ))
        .await
        .expect("loading");
    clock.advance_seconds(30);
    tracker
        .handle(event(
            3,
            PlaybackStatus::Playing,
            Some(track_id),
            Some(100_000),
        ))
        .await
        .expect("resume");
    clock.advance_seconds(5);
    tracker
        .handle(event(4, PlaybackStatus::Stopped, None, None))
        .await
        .expect("stop");

    let completed = final_for(&sink, track_id);
    assert_eq!(completed.listened_ms, 15_000);
    let ids: Vec<_> = sink
        .events
        .lock()
        .expect("events lock")
        .iter()
        .filter(|event| event.track_id == track_id)
        .map(|event| event.id)
        .collect();
    assert!(ids.iter().all(|id| *id == ids[0]));
}

#[tokio::test]
async fn terminal_duration_is_merged_before_exact_completion_threshold() {
    let clock = TestClock::at(1_800_000_000);
    let sink = MemorySink::default();
    let below = TrackId::new();
    let exact = TrackId::new();
    let mut tracker = ListenTracker::start(sink.clone(), clock.clone())
        .await
        .expect("tracker");

    tracker
        .handle(event(1, PlaybackStatus::Playing, Some(below), None))
        .await
        .expect("below play");
    clock.advance_milliseconds(49_999);
    tracker
        .handle(event(
            2,
            PlaybackStatus::Stopped,
            Some(below),
            Some(100_000),
        ))
        .await
        .expect("below stop");

    tracker
        .handle(event(3, PlaybackStatus::Playing, Some(exact), None))
        .await
        .expect("exact play");
    clock.advance_milliseconds(50_000);
    tracker
        .handle(event(
            4,
            PlaybackStatus::Stopped,
            Some(exact),
            Some(100_000),
        ))
        .await
        .expect("exact stop");

    let below_event = final_for(&sink, below);
    assert_eq!(below_event.listened_ms, 49_999);
    assert!(!below_event.completed);
    let exact_event = final_for(&sink, exact);
    assert_eq!(exact_event.listened_ms, 50_000);
    assert!(exact_event.completed);
}
