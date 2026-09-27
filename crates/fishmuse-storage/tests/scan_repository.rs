use fishmuse_domain::{MediaAssetId, UserId};
use fishmuse_storage::{
    Database, MediaAssetWrite, ScanDiagnosticWrite, ScanRepository, ScanRunStatus,
    SqliteScanRepository,
};

async fn insert_user(database: &Database) -> UserId {
    let user = UserId::new();
    sqlx::query("INSERT INTO users(user_id, local_slot, created_at) VALUES (?, NULL, ?)")
        .bind(user.as_uuid().to_string())
        .bind(1_800_000_000_i64)
        .execute(database.pool())
        .await
        .expect("second user");
    user
}

#[tokio::test]
async fn scan_repository_persists_user_scoped_runs_assets_and_diagnostics_atomically() {
    let database = Database::open_in_memory().await.expect("database");
    let local_user = database.ensure_local_user().await.expect("local user");
    let other_user = insert_user(&database).await;
    let local = SqliteScanRepository::new(database.pool().clone(), local_user);
    let other = SqliteScanRepository::new(database.pool().clone(), other_user);
    local
        .upsert_root(b"normalized-root", b"original-root")
        .await
        .expect("root");
    let scan_id = local.begin_scan().await.expect("scan run");
    let asset_id = MediaAssetId::new();

    local
        .commit_batch(
            scan_id,
            &[MediaAssetWrite {
                media_asset_id: asset_id,
                normalized_path: b"normalized-track".to_vec(),
                original_path: b"original-track".to_vec(),
                identity: "q:quick|f:full".to_owned(),
            }],
            &[ScanDiagnosticWrite {
                path: Some(b"technical-path".to_vec()),
                code: "invalid_tags".to_owned(),
                message: "invalid embedded tags".to_owned(),
            }],
        )
        .await
        .expect("atomic batch");
    local
        .finish_scan(scan_id, ScanRunStatus::Cancelled)
        .await
        .expect("finish scan");

    let local_assets = local.list_assets().await.expect("local assets");
    assert_eq!(local_assets.len(), 1);
    assert_eq!(local_assets[0].media_asset_id, asset_id);
    assert!(other.list_assets().await.expect("other assets").is_empty());
    let status: String =
        sqlx::query_scalar("SELECT status FROM scan_runs WHERE user_id = ? AND scan_run_id = ?")
            .bind(local_user.as_uuid().to_string())
            .bind(scan_id.as_uuid().to_string())
            .fetch_one(database.pool())
            .await
            .expect("status");
    assert_eq!(status, "cancelled");
    let diagnostic_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM scan_diagnostics WHERE user_id = ? AND scan_run_id = ?",
    )
    .bind(local_user.as_uuid().to_string())
    .bind(scan_id.as_uuid().to_string())
    .fetch_one(database.pool())
    .await
    .expect("diagnostic count");
    assert_eq!(diagnostic_count, 1);
}
