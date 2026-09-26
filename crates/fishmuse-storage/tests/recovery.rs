use fishmuse_storage::Database;
use sqlx::Row;
use tempfile::TempDir;
use tokio::time::{Duration, timeout};

#[tokio::test]
async fn corrupt_database_returns_safe_error_without_overwriting_source() {
    let directory = TempDir::new().expect("temporary directory");
    let path = directory.path().join("corrupt.db");
    let original = b"not a sqlite database";
    std::fs::write(&path, original).expect("write corrupt fixture");

    let error = Database::open(&path)
        .await
        .expect_err("corruption must fail");

    assert_eq!(error.user_message, "database_corrupt");
    assert!(!error.user_message.contains(path.to_string_lossy().as_ref()));
    assert_eq!(std::fs::read(&path).expect("source remains"), original);
}

#[tokio::test]
async fn backup_is_a_consistent_sqlite_snapshot_in_wal_mode() {
    let directory = TempDir::new().expect("temporary directory");
    let source = directory.path().join("source.db");
    let destination = directory.path().join("backup.db");
    let database = Database::open(&source).await.expect("source database");
    let user = database.ensure_local_user().await.expect("local user");
    sqlx::query("INSERT INTO app_settings(user_id, key, value_json) VALUES (?, ?, ?)")
        .bind(user.as_uuid().to_string())
        .bind("theme")
        .bind(r#""dark""#)
        .execute(database.pool())
        .await
        .expect("uncheckpointed WAL write");

    database
        .create_backup(&destination)
        .await
        .expect("online backup");

    let backup = Database::open(&destination).await.expect("backup database");
    let row = sqlx::query("SELECT value_json FROM app_settings WHERE user_id = ? AND key = ?")
        .bind(user.as_uuid().to_string())
        .bind("theme")
        .fetch_one(backup.pool())
        .await
        .expect("backup includes committed WAL data");
    assert_eq!(row.get::<String, _>(0), r#""dark""#);
}

#[tokio::test]
async fn failed_backup_does_not_report_or_publish_a_backup_file() {
    let directory = TempDir::new().expect("temporary directory");
    let source = directory.path().join("source.db");
    let database = Database::open(&source).await.expect("source database");
    let blocker = directory.path().join("not-a-directory");
    std::fs::write(&blocker, b"block parent creation").expect("write blocker");
    let destination = blocker.join("backup.db");

    let result = database.create_backup(&destination).await;

    assert!(result.is_err());
    assert!(!destination.is_file());
}

#[tokio::test]
async fn publish_failure_after_snapshot_removes_the_exact_partial_backup() {
    let directory = TempDir::new().expect("temporary directory");
    let source = directory.path().join("source.db");
    let destination = directory.path().join("backup.db");
    let database = Database::open(&source).await.expect("source database");
    sqlx::query("CREATE TABLE backup_payload(data BLOB NOT NULL)")
        .execute(database.pool())
        .await
        .expect("backup payload table");
    sqlx::query("INSERT INTO backup_payload(data) VALUES (zeroblob(67108864))")
        .execute(database.pool())
        .await
        .expect("large backup payload");

    let parent = directory.path().to_path_buf();
    let blocker_destination = destination.clone();
    let publish_blocker = tokio::spawn(async move {
        loop {
            let mut entries = tokio::fs::read_dir(&parent)
                .await
                .expect("read valid backup parent");
            while let Some(entry) = entries.next_entry().await.expect("read backup entry") {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                if name.starts_with(".backup.db.") && name.ends_with(".partial") {
                    let partial = entry.path();
                    tokio::fs::create_dir(&blocker_destination)
                        .await
                        .expect("block atomic publication after snapshot starts");
                    return partial;
                }
            }
            tokio::task::yield_now().await;
        }
    });

    let result = database.create_backup(&destination).await;
    let partial = timeout(Duration::from_secs(10), publish_blocker)
        .await
        .expect("partial snapshot should be observed")
        .expect("publication blocker task");

    assert!(result.is_err());
    assert!(destination.is_dir(), "the blocker must remain a directory");
    assert!(!destination.is_file(), "no backup database was published");
    assert!(!partial.exists(), "failed publication leaked {partial:?}");
}
