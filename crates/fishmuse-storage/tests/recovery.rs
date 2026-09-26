use fishmuse_storage::Database;
use sqlx::Row;
use tempfile::TempDir;

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
