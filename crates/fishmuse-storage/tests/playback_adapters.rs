use std::time::Duration;

use fishmuse_domain::{
    AppError, ErrorCategory, ErrorCode, ListenId, OperationId, RecordingId, TrackId, UserId,
};
use fishmuse_playback::{
    ListeningEvent, ListeningSink, OperationClaim, OperationStore, PlaybackBackendKind,
    PlaybackCommand, PlaybackSnapshot, PlaybackStatus,
};
use fishmuse_storage::{Database, SqliteListeningSink, SqliteOperationStore};
use time::OffsetDateTime;

async fn insert_track(database: &Database, user: UserId) -> TrackId {
    let recording = RecordingId::new();
    let track = TrackId::new();
    sqlx::query("INSERT INTO recordings(recording_id, user_id, title) VALUES (?, ?, ?)")
        .bind(recording.as_uuid().to_string())
        .bind(user.as_uuid().to_string())
        .bind("Recording")
        .execute(database.pool())
        .await
        .expect("recording");
    sqlx::query("INSERT INTO tracks(track_id, user_id, recording_id, title) VALUES (?, ?, ?, ?)")
        .bind(track.as_uuid().to_string())
        .bind(user.as_uuid().to_string())
        .bind(recording.as_uuid().to_string())
        .bind("Track")
        .execute(database.pool())
        .await
        .expect("track");
    track
}

fn snapshot(track_id: TrackId) -> PlaybackSnapshot {
    PlaybackSnapshot {
        revision: 7,
        status: PlaybackStatus::Playing,
        track_id: Some(track_id),
        position_ms: 2_000,
        duration_ms: Some(20_000),
        volume: 0.5,
        backend: PlaybackBackendKind::Foobar2000,
    }
}

#[tokio::test]
async fn sqlite_operation_store_replays_a_durable_result_and_rejects_conflicts() {
    let database = Database::open_in_memory().await.expect("database");
    let user = database.ensure_local_user().await.expect("user");
    let track = insert_track(&database, user).await;
    let store = SqliteOperationStore::new(database.pool().clone(), user);
    let operation_id = OperationId::new();
    let pause = PlaybackCommand::Pause { operation_id }.fingerprint();
    let seek = PlaybackCommand::Seek {
        position_ms: 1,
        operation_id,
    }
    .fingerprint();

    assert_eq!(
        store.try_begin(operation_id, pause).await.expect("claim"),
        OperationClaim::Acquired
    );
    assert_eq!(
        store.try_begin(operation_id, pause).await.expect("pending"),
        OperationClaim::InFlight
    );
    store
        .complete(operation_id, &Ok(snapshot(track)))
        .await
        .expect("complete");

    let restarted = SqliteOperationStore::new(database.pool().clone(), user);
    assert_eq!(
        restarted
            .try_begin(operation_id, pause)
            .await
            .expect("replay"),
        OperationClaim::Completed(snapshot(track))
    );
    assert_eq!(
        restarted
            .try_begin(operation_id, seek)
            .await
            .expect("conflict"),
        OperationClaim::Conflict
    );
}

#[tokio::test]
async fn sqlite_operation_store_persists_safe_failures_without_technical_context() {
    let database = Database::open_in_memory().await.expect("database");
    let user = database.ensure_local_user().await.expect("user");
    let store = SqliteOperationStore::new(database.pool().clone(), user);
    let operation_id = OperationId::new();
    let fingerprint = PlaybackCommand::Resume { operation_id }.fingerprint();
    store
        .try_begin(operation_id, fingerprint)
        .await
        .expect("claim");
    let failure = AppError {
        code: ErrorCode::BackendUnavailable,
        category: ErrorCategory::Playback,
        user_message: "backend_unavailable".to_owned(),
        retryable: true,
        suggested_action: Some("retry".to_owned()),
        technical_context: Some("SECRET pipe and path details".to_owned()),
    };
    store
        .complete(operation_id, &Err(failure))
        .await
        .expect("complete");

    let persisted: String = sqlx::query_scalar(
        "SELECT result_json FROM applied_operations WHERE operation_id = ? AND user_id = ?",
    )
    .bind(operation_id.as_uuid().to_string())
    .bind(user.as_uuid().to_string())
    .fetch_one(database.pool())
    .await
    .expect("stored result");
    assert!(!persisted.contains("SECRET"));
    let replayed = store
        .try_begin(operation_id, fingerprint)
        .await
        .expect_err("stored failure");
    assert_eq!(replayed.user_message, "backend_unavailable");
    assert_eq!(replayed.technical_context, None);
}

#[tokio::test]
async fn stale_persisted_pending_operation_becomes_terminal_instead_of_waiting_or_reexecuting() {
    let database = Database::open_in_memory().await.expect("database");
    let user = database.ensure_local_user().await.expect("user");
    let store = SqliteOperationStore::with_orphan_timeout(
        database.pool().clone(),
        user,
        Duration::from_secs(1),
    );
    let operation_id = OperationId::new();
    let fingerprint = PlaybackCommand::SkipNext { operation_id }.fingerprint();
    store
        .try_begin(operation_id, fingerprint)
        .await
        .expect("claim");
    sqlx::query("UPDATE applied_operations SET completed_at = 0 WHERE operation_id = ?")
        .bind(operation_id.as_uuid().to_string())
        .execute(database.pool())
        .await
        .expect("age pending row");

    let interrupted = store
        .try_begin(operation_id, fingerprint)
        .await
        .expect_err("orphan is terminal");
    assert_eq!(interrupted.user_message, "operation_interrupted");
    assert!(!interrupted.retryable);
    let late_completion = store
        .complete(operation_id, &Ok(snapshot(TrackId::new())))
        .await
        .expect_err("late leader cannot replace terminal orphan result");
    assert_eq!(late_completion.user_message, "storage_failure");
    let replayed = store
        .try_begin(operation_id, fingerprint)
        .await
        .expect_err("terminal result is replayed");
    assert_eq!(replayed.user_message, "operation_interrupted");
}

#[tokio::test]
async fn sqlite_listening_sink_upserts_progress_recovers_open_rows_and_reads_recent_events() {
    let database = Database::open_in_memory().await.expect("database");
    let user = database.ensure_local_user().await.expect("user");
    let track = insert_track(&database, user).await;
    let sink = SqliteListeningSink::new(database.pool().clone(), user);
    let started_at = OffsetDateTime::from_unix_timestamp(1_800_000_000).expect("start");
    let id = ListenId::new();

    sink.append(ListeningEvent {
        id,
        track_id: track,
        started_at,
        ended_at: None,
        listened_ms: 12_000,
        completed: false,
        interrupted: false,
    })
    .await
    .expect("open listen");
    let recovered_at = OffsetDateTime::from_unix_timestamp(1_800_000_100).expect("recovery");
    sink.recover_interrupted(recovered_at)
        .await
        .expect("recover");

    let recent = sink.get_recent_listens(10).await.expect("recent");
    assert_eq!(recent.len(), 1);
    assert_eq!(recent[0].id, id);
    assert_eq!(recent[0].listened_ms, 12_000);
    assert_eq!(recent[0].ended_at, Some(recovered_at));
    assert!(recent[0].interrupted);
    assert!(!recent[0].completed);
}
