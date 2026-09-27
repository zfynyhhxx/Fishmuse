use std::path::Path;

use fishmuse_storage::Database;
use sqlx::{Connection, Row, SqliteConnection};
use tempfile::TempDir;

fn database_path(directory: &TempDir) -> std::path::PathBuf {
    directory.path().join("nested").join("fishmuse.db")
}

async fn scalar_i64(database: &Database, sql: &str) -> i64 {
    sqlx::query(sql)
        .fetch_one(database.pool())
        .await
        .expect("query should succeed")
        .get(0)
}

#[tokio::test]
async fn empty_database_migrates_to_version_one_with_complete_schema() {
    let directory = TempDir::new().expect("temporary directory");
    let database = Database::open(&database_path(&directory))
        .await
        .expect("database should open");

    assert_eq!(scalar_i64(&database, "PRAGMA user_version").await, 1);

    let required = [
        "users",
        "artists",
        "releases",
        "recordings",
        "tracks",
        "track_artists",
        "media_assets",
        "local_import_metadata",
        "recording_possible_matches",
        "media_roots",
        "scan_runs",
        "scan_diagnostics",
        "listening_events",
        "conversations",
        "conversation_messages",
        "app_settings",
        "ai_usage_ledger",
        "applied_operations",
        "library_fts",
    ];
    for table in required {
        let exists: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM sqlite_master WHERE type IN ('table', 'view') AND name = ?",
        )
        .bind(table)
        .fetch_one(database.pool())
        .await
        .expect("schema query should succeed");
        assert_eq!(exists, 1, "missing schema object {table}");
    }
}

#[tokio::test]
async fn reopening_is_idempotent_and_returns_the_same_local_user() {
    let directory = TempDir::new().expect("temporary directory");
    let path = database_path(&directory);

    let first = Database::open(&path).await.expect("first open");
    let first_user = first.ensure_local_user().await.expect("first local user");
    first.close().await;

    let second = Database::open(&path).await.expect("second open");
    let second_user = second.ensure_local_user().await.expect("second local user");

    assert_eq!(first_user, second_user);
    assert_eq!(scalar_i64(&second, "SELECT COUNT(*) FROM users").await, 1);
    assert_eq!(scalar_i64(&second, "PRAGMA user_version").await, 1);
}

#[tokio::test]
async fn file_database_enables_safety_and_contention_pragmas() {
    let directory = TempDir::new().expect("temporary directory");
    let database = Database::open(&database_path(&directory))
        .await
        .expect("database should open");

    assert_eq!(scalar_i64(&database, "PRAGMA foreign_keys").await, 1);
    assert_eq!(
        sqlx::query_scalar::<_, String>("PRAGMA journal_mode")
            .fetch_one(database.pool())
            .await
            .expect("journal mode"),
        "wal"
    );
    assert_eq!(scalar_i64(&database, "PRAGMA busy_timeout").await, 5_000);
}

#[tokio::test]
async fn failed_migration_rolls_back_version_and_preserves_existing_data() {
    let directory = TempDir::new().expect("temporary directory");
    let path = database_path(&directory);
    std::fs::create_dir_all(path.parent().expect("database parent")).expect("create parent");
    let url = format!("sqlite://{}?mode=rwc", path.display());
    let mut connection = SqliteConnection::connect(&url)
        .await
        .expect("seed database");
    sqlx::query("CREATE TABLE legacy_data(value TEXT NOT NULL)")
        .execute(&mut connection)
        .await
        .expect("legacy schema");
    sqlx::query("INSERT INTO legacy_data(value) VALUES (?)")
        .bind("still-readable")
        .execute(&mut connection)
        .await
        .expect("legacy row");
    sqlx::query("CREATE TABLE releases(conflicting_column TEXT)")
        .execute(&mut connection)
        .await
        .expect("migration conflict");
    connection.close().await.expect("close seed connection");

    assert!(Database::open(&path).await.is_err());

    let mut connection = SqliteConnection::connect(&url)
        .await
        .expect("reopen database");
    let version: i64 = sqlx::query_scalar("PRAGMA user_version")
        .fetch_one(&mut connection)
        .await
        .expect("version query");
    let value: String = sqlx::query_scalar("SELECT value FROM legacy_data")
        .fetch_one(&mut connection)
        .await
        .expect("legacy row remains");
    assert_eq!(version, 0);
    assert_eq!(value, "still-readable");

    let users_exists: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'users'",
    )
    .fetch_one(&mut connection)
    .await
    .expect("schema query");
    assert_eq!(users_exists, 0, "partial schema must be rolled back");
}

#[tokio::test]
async fn schema_does_not_treat_content_fingerprint_as_canonical_identity() {
    let database = Database::open_in_memory().await.expect("database");
    let user = database.ensure_local_user().await.expect("local user");
    let first_asset = uuid::Uuid::now_v7().to_string();
    let second_asset = uuid::Uuid::now_v7().to_string();

    for (asset, path) in [
        (&first_asset, b"first".as_slice()),
        (&second_asset, b"second".as_slice()),
    ] {
        sqlx::query(
            "INSERT INTO media_assets(media_asset_id, user_id, normalized_path, original_path, content_fingerprint, availability) VALUES (?, ?, ?, ?, ?, 'available')",
        )
        .bind(asset)
        .bind(user.as_uuid().to_string())
        .bind(path)
        .bind(path)
        .bind("same-fingerprint")
        .execute(database.pool())
        .await
        .expect("same fingerprint must be allowed for separate assets");
    }
}

#[tokio::test]
async fn media_paths_are_unique_per_user_but_not_globally() {
    let database = Database::open_in_memory().await.expect("database");
    let local_user = database.ensure_local_user().await.expect("local user");
    let other_user = fishmuse_domain::UserId::new();
    sqlx::query("INSERT INTO users(user_id, local_slot, created_at) VALUES (?, NULL, ?)")
        .bind(other_user.as_uuid().to_string())
        .bind(1_800_000_000_i64)
        .execute(database.pool())
        .await
        .expect("other user");
    let path = b"same-normalized-path".as_slice();

    for user in [local_user, other_user] {
        sqlx::query("INSERT INTO media_assets(media_asset_id, user_id, normalized_path, original_path, availability) VALUES (?, ?, ?, ?, 'available')")
            .bind(uuid::Uuid::now_v7().to_string())
            .bind(user.as_uuid().to_string())
            .bind(path)
            .bind(path)
            .execute(database.pool())
            .await
            .expect("same path is allowed for a different user");
    }

    let duplicate = sqlx::query("INSERT INTO media_assets(media_asset_id, user_id, normalized_path, original_path, availability) VALUES (?, ?, ?, ?, 'available')")
        .bind(uuid::Uuid::now_v7().to_string())
        .bind(local_user.as_uuid().to_string())
        .bind(path)
        .bind(path)
        .execute(database.pool())
        .await;
    assert!(duplicate.is_err());
}

#[tokio::test]
async fn fts_projection_tracks_insert_update_and_delete() {
    let database = Database::open_in_memory().await.expect("database");
    let user = database.ensure_local_user().await.expect("local user");
    let recording = uuid::Uuid::now_v7().to_string();
    let track = uuid::Uuid::now_v7().to_string();
    sqlx::query("INSERT INTO recordings(recording_id, user_id, title) VALUES (?, ?, ?)")
        .bind(&recording)
        .bind(user.as_uuid().to_string())
        .bind("Recording")
        .execute(database.pool())
        .await
        .expect("recording");
    sqlx::query("INSERT INTO tracks(track_id, user_id, recording_id, title) VALUES (?, ?, ?, ?)")
        .bind(&track)
        .bind(user.as_uuid().to_string())
        .bind(&recording)
        .bind("First title")
        .execute(database.pool())
        .await
        .expect("track");
    assert_eq!(
        scalar_i64(
            &database,
            "SELECT COUNT(*) FROM library_fts WHERE library_fts MATCH 'First'"
        )
        .await,
        1
    );

    sqlx::query("UPDATE tracks SET title = ? WHERE user_id = ? AND track_id = ?")
        .bind("Changed title")
        .bind(user.as_uuid().to_string())
        .bind(&track)
        .execute(database.pool())
        .await
        .expect("update track");
    assert_eq!(
        scalar_i64(
            &database,
            "SELECT COUNT(*) FROM library_fts WHERE library_fts MATCH 'First'"
        )
        .await,
        0
    );
    assert_eq!(
        scalar_i64(
            &database,
            "SELECT COUNT(*) FROM library_fts WHERE library_fts MATCH 'Changed'"
        )
        .await,
        1
    );

    sqlx::query("DELETE FROM tracks WHERE user_id = ? AND track_id = ?")
        .bind(user.as_uuid().to_string())
        .bind(&track)
        .execute(database.pool())
        .await
        .expect("delete track");
    assert_eq!(
        scalar_i64(
            &database,
            "SELECT COUNT(*) FROM library_fts WHERE library_fts MATCH 'Changed'"
        )
        .await,
        0
    );
}

#[tokio::test]
async fn fts_projection_tracks_credit_artist_and_release_mutations() {
    let database = Database::open_in_memory().await.expect("database");
    let user = database.ensure_local_user().await.expect("local user");
    let user_text = user.as_uuid().to_string();
    let recording = uuid::Uuid::now_v7().to_string();
    let release = uuid::Uuid::now_v7().to_string();
    let first_artist = uuid::Uuid::now_v7().to_string();
    let second_artist = uuid::Uuid::now_v7().to_string();
    let track = uuid::Uuid::now_v7().to_string();
    sqlx::query("INSERT INTO recordings(recording_id, user_id, title) VALUES (?, ?, ?)")
        .bind(&recording)
        .bind(&user_text)
        .bind("Recording")
        .execute(database.pool())
        .await
        .expect("recording");
    sqlx::query("INSERT INTO releases(release_id, user_id, title) VALUES (?, ?, ?)")
        .bind(&release)
        .bind(&user_text)
        .bind("First Release")
        .execute(database.pool())
        .await
        .expect("release");
    for (artist, name) in [
        (&first_artist, "First Artist"),
        (&second_artist, "Second Artist"),
    ] {
        sqlx::query("INSERT INTO artists(artist_id, user_id, name) VALUES (?, ?, ?)")
            .bind(artist)
            .bind(&user_text)
            .bind(name)
            .execute(database.pool())
            .await
            .expect("artist");
    }
    sqlx::query("INSERT INTO tracks(track_id, user_id, recording_id, release_id, title) VALUES (?, ?, ?, ?, ?)")
        .bind(&track)
        .bind(&user_text)
        .bind(&recording)
        .bind(&release)
        .bind("Track")
        .execute(database.pool())
        .await
        .expect("track");
    sqlx::query(
        "INSERT INTO track_artists(user_id, track_id, artist_id, position) VALUES (?, ?, ?, 0)",
    )
    .bind(&user_text)
    .bind(&track)
    .bind(&first_artist)
    .execute(database.pool())
    .await
    .expect("credit");
    assert_eq!(
        scalar_i64(&database, "SELECT COUNT(*) FROM library_fts WHERE library_fts MATCH 'artist:\"First Artist\" AND release_title:\"First Release\"'").await,
        1
    );

    sqlx::query("UPDATE track_artists SET artist_id = ? WHERE user_id = ? AND track_id = ? AND artist_id = ?")
        .bind(&second_artist)
        .bind(&user_text)
        .bind(&track)
        .bind(&first_artist)
        .execute(database.pool())
        .await
        .expect("update credit");
    assert_eq!(
        scalar_i64(
            &database,
            "SELECT COUNT(*) FROM library_fts WHERE library_fts MATCH 'artist:\"First Artist\"'"
        )
        .await,
        0
    );
    assert_eq!(
        scalar_i64(
            &database,
            "SELECT COUNT(*) FROM library_fts WHERE library_fts MATCH 'artist:\"Second Artist\"'"
        )
        .await,
        1
    );

    sqlx::query("UPDATE artists SET name = ? WHERE user_id = ? AND artist_id = ?")
        .bind("Renamed Artist")
        .bind(&user_text)
        .bind(&second_artist)
        .execute(database.pool())
        .await
        .expect("rename artist");
    assert_eq!(
        scalar_i64(
            &database,
            "SELECT COUNT(*) FROM library_fts WHERE library_fts MATCH 'artist:\"Second Artist\"'"
        )
        .await,
        0
    );
    assert_eq!(
        scalar_i64(
            &database,
            "SELECT COUNT(*) FROM library_fts WHERE library_fts MATCH 'artist:\"Renamed Artist\"'"
        )
        .await,
        1
    );

    sqlx::query("UPDATE releases SET title = ? WHERE user_id = ? AND release_id = ?")
        .bind("Renamed Release")
        .bind(&user_text)
        .bind(&release)
        .execute(database.pool())
        .await
        .expect("rename release");
    assert_eq!(
        scalar_i64(&database, "SELECT COUNT(*) FROM library_fts WHERE library_fts MATCH 'release_title:\"First Release\"'").await,
        0
    );
    assert_eq!(
        scalar_i64(&database, "SELECT COUNT(*) FROM library_fts WHERE library_fts MATCH 'release_title:\"Renamed Release\"'").await,
        1
    );
}

#[tokio::test]
async fn deleting_media_asset_never_deletes_canonical_or_history_rows() {
    let database = Database::open_in_memory().await.expect("database");
    let user = database.ensure_local_user().await.expect("local user");
    let recording = uuid::Uuid::now_v7().to_string();
    let track = uuid::Uuid::now_v7().to_string();
    let asset = uuid::Uuid::now_v7().to_string();
    sqlx::query("INSERT INTO recordings(recording_id, user_id, title) VALUES (?, ?, ?)")
        .bind(&recording)
        .bind(user.as_uuid().to_string())
        .bind("Recording")
        .execute(database.pool())
        .await
        .expect("recording");
    sqlx::query("INSERT INTO tracks(track_id, user_id, recording_id, title) VALUES (?, ?, ?, ?)")
        .bind(&track)
        .bind(user.as_uuid().to_string())
        .bind(&recording)
        .bind("Track")
        .execute(database.pool())
        .await
        .expect("track");
    sqlx::query("INSERT INTO media_assets(media_asset_id, user_id, track_id, normalized_path, original_path, availability) VALUES (?, ?, ?, ?, ?, 'available')")
        .bind(&asset)
        .bind(user.as_uuid().to_string())
        .bind(&track)
        .bind(b"asset".as_slice())
        .bind(b"asset".as_slice())
        .execute(database.pool())
        .await
        .expect("asset");
    sqlx::query("INSERT INTO listening_events(listen_id, user_id, track_id, started_at, listened_ms, completed) VALUES (?, ?, ?, ?, ?, ?)")
        .bind(uuid::Uuid::now_v7().to_string())
        .bind(user.as_uuid().to_string())
        .bind(&track)
        .bind(1_800_000_000_i64)
        .bind(1_000_i64)
        .bind(true)
        .execute(database.pool())
        .await
        .expect("listen");

    sqlx::query("DELETE FROM media_assets WHERE user_id = ? AND media_asset_id = ?")
        .bind(user.as_uuid().to_string())
        .bind(&asset)
        .execute(database.pool())
        .await
        .expect("delete asset");

    assert_eq!(
        scalar_i64(&database, "SELECT COUNT(*) FROM recordings").await,
        1
    );
    assert_eq!(
        scalar_i64(&database, "SELECT COUNT(*) FROM tracks").await,
        1
    );
    assert_eq!(
        scalar_i64(&database, "SELECT COUNT(*) FROM listening_events").await,
        1
    );
}

#[allow(dead_code)]
fn _path_contract(_: &Path) {}
