use fishmuse_domain::{ListenId, ListenSummary, RecordingId, ReleaseId, TrackId, UserId};
use fishmuse_storage::{
    ConversationRepository, Database, LibraryRepository, ListeningRepository, SettingsRepository,
    SqliteConversationRepository, SqliteLibraryRepository, SqliteListeningRepository,
    SqliteSettingsRepository,
};
use time::OffsetDateTime;

async fn insert_track(database: &Database, user: UserId, title: &str) -> TrackId {
    let recording = RecordingId::new();
    let release = ReleaseId::new();
    let track = TrackId::new();
    sqlx::query("INSERT INTO recordings(recording_id, user_id, title) VALUES (?, ?, ?)")
        .bind(recording.as_uuid().to_string())
        .bind(user.as_uuid().to_string())
        .bind(title)
        .execute(database.pool())
        .await
        .expect("recording");
    sqlx::query("INSERT INTO releases(release_id, user_id, title) VALUES (?, ?, ?)")
        .bind(release.as_uuid().to_string())
        .bind(user.as_uuid().to_string())
        .bind(format!("{title} release"))
        .execute(database.pool())
        .await
        .expect("release");
    sqlx::query("INSERT INTO tracks(track_id, user_id, recording_id, release_id, title, playable) VALUES (?, ?, ?, ?, ?, 1)")
        .bind(track.as_uuid().to_string())
        .bind(user.as_uuid().to_string())
        .bind(recording.as_uuid().to_string())
        .bind(release.as_uuid().to_string())
        .bind(title)
        .execute(database.pool())
        .await
        .expect("track");
    track
}

async fn insert_user(database: &Database) -> UserId {
    let user = UserId::new();
    sqlx::query("INSERT INTO users(user_id, local_slot, created_at) VALUES (?, NULL, ?)")
        .bind(user.as_uuid().to_string())
        .bind("2026-09-27T00:00:00Z")
        .execute(database.pool())
        .await
        .expect("second user");
    user
}

#[tokio::test]
async fn library_repository_never_reads_another_users_tracks() {
    let database = Database::open_in_memory().await.expect("database");
    let local_user = database.ensure_local_user().await.expect("local user");
    let other_user = insert_user(&database).await;
    let local_track = insert_track(&database, local_user, "Local").await;
    let other_track = insert_track(&database, other_user, "Other").await;
    let repository = SqliteLibraryRepository::new(database.pool().clone(), local_user);

    let items = repository.list_tracks().await.expect("list tracks");

    assert_eq!(items.len(), 1);
    assert_eq!(items[0].track.id, local_track);
    assert_eq!(items[0].track.title, "Local");
    assert!(
        repository
            .find_track(other_track)
            .await
            .expect("lookup")
            .is_none()
    );
}

#[tokio::test]
async fn library_repository_rejects_non_v7_ids_read_from_storage() {
    let database = Database::open_in_memory().await.expect("database");
    let user = database.ensure_local_user().await.expect("local user");
    let recording = RecordingId::new();
    sqlx::query("INSERT INTO recordings(recording_id, user_id, title) VALUES (?, ?, ?)")
        .bind(recording.as_uuid().to_string())
        .bind(user.as_uuid().to_string())
        .bind("Recording")
        .execute(database.pool())
        .await
        .expect("recording");
    sqlx::query("INSERT INTO tracks(track_id, user_id, recording_id, title, playable) VALUES (?, ?, ?, ?, 1)")
        .bind("550e8400-e29b-41d4-a716-446655440000")
        .bind(user.as_uuid().to_string())
        .bind(recording.as_uuid().to_string())
        .bind("Invalid ID")
        .execute(database.pool())
        .await
        .expect("invalid legacy row");
    let repository = SqliteLibraryRepository::new(database.pool().clone(), user);

    let error = repository
        .list_tracks()
        .await
        .expect_err("invalid stored ID must not enter domain");

    assert_eq!(error.user_message, "storage_failure");
}

#[tokio::test]
async fn listening_conversation_and_settings_repositories_are_user_scoped() {
    let database = Database::open_in_memory().await.expect("database");
    let local_user = database.ensure_local_user().await.expect("local user");
    let other_user = insert_user(&database).await;
    let local_track = insert_track(&database, local_user, "Local").await;
    let other_track = insert_track(&database, other_user, "Other").await;

    let local_listening = SqliteListeningRepository::new(database.pool().clone(), local_user);
    let other_listening = SqliteListeningRepository::new(database.pool().clone(), other_user);
    local_listening
        .append(&ListenSummary {
            id: ListenId::new(),
            track_id: local_track,
            started_at: OffsetDateTime::from_unix_timestamp(1_800_000_000).expect("timestamp"),
            listened_ms: 42_000,
            completed: true,
        })
        .await
        .expect("local listen");
    other_listening
        .append(&ListenSummary {
            id: ListenId::new(),
            track_id: other_track,
            started_at: OffsetDateTime::from_unix_timestamp(1_800_000_001).expect("timestamp"),
            listened_ms: 1_000,
            completed: false,
        })
        .await
        .expect("other listen");
    assert_eq!(local_listening.recent(10).await.expect("recent").len(), 1);

    let conversation_id = fishmuse_domain::ConversationId::new();
    let local_conversations =
        SqliteConversationRepository::new(database.pool().clone(), local_user);
    let other_conversations =
        SqliteConversationRepository::new(database.pool().clone(), other_user);
    local_conversations
        .create(conversation_id, Some("Private"))
        .await
        .expect("conversation");
    local_conversations
        .append_message(conversation_id, "user", "hello")
        .await
        .expect("message");
    assert_eq!(
        local_conversations
            .messages(conversation_id)
            .await
            .expect("messages")
            .len(),
        1
    );
    assert!(
        other_conversations
            .messages(conversation_id)
            .await
            .expect("other messages")
            .is_empty()
    );

    let local_settings = SqliteSettingsRepository::new(database.pool().clone(), local_user);
    let other_settings = SqliteSettingsRepository::new(database.pool().clone(), other_user);
    local_settings
        .set("theme", &serde_json::json!("dark"))
        .await
        .expect("local setting");
    other_settings
        .set("theme", &serde_json::json!("light"))
        .await
        .expect("other setting");
    assert_eq!(
        local_settings.get("theme").await.expect("local setting"),
        Some(serde_json::json!("dark"))
    );
}
