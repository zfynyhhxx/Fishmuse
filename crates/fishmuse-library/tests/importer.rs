use fishmuse_domain::{MediaAssetId, UserId};
use fishmuse_library::{LocalImport, LocalLibraryImporter, ParsedTags};
use fishmuse_storage::{Database, MediaAssetWrite, ScanRepository, SqliteScanRepository};

fn import(asset: MediaAssetId, path: &str, fingerprint: &str, title: &str) -> LocalImport {
    LocalImport {
        media_asset_id: asset,
        normalized_path: path.as_bytes().to_vec(),
        original_path: path.as_bytes().to_vec(),
        content_fingerprint: fingerprint.to_owned(),
        tags: ParsedTags {
            title: Some(title.to_owned()),
            artists: vec!["Artist A".to_owned()],
            album: Some("Album A".to_owned()),
            duration_ms: Some(180_000),
            disc_number: Some(1),
            track_number: Some(2),
        },
    }
}

async fn setup() -> (Database, UserId, LocalLibraryImporter) {
    let database = Database::open_in_memory().await.expect("database");
    let user = database.ensure_local_user().await.expect("local user");
    let importer = LocalLibraryImporter::new(database.pool().clone(), user);
    (database, user, importer)
}

#[tokio::test]
async fn rerun_is_idempotent_and_preserves_raw_normalized_and_provenance_data() {
    let (database, user, importer) = setup().await;
    let asset = MediaAssetId::new();
    let input = import(
        asset,
        "music/song.flac",
        "fingerprint-a",
        "  Cafe\u{301} Song  ",
    );

    let first = importer.import(input.clone()).await.expect("first import");
    let second = importer.import(input).await.expect("second import");

    assert_eq!(first.media_asset_id, asset);
    assert_eq!(first, second);
    assert!(!first.possible_match);
    for table in [
        "media_assets",
        "recordings",
        "tracks",
        "local_import_metadata",
    ] {
        let sql = format!("SELECT COUNT(*) FROM {table} WHERE user_id = ?");
        let count: i64 = sqlx::query_scalar(&sql)
            .bind(user.as_uuid().to_string())
            .fetch_one(database.pool())
            .await
            .expect("count");
        assert_eq!(count, 1, "rerun duplicated {table}");
    }
    let stored: (String, String, String) = sqlx::query_as(
        "SELECT raw_tags_json, normalized_title, provenance FROM local_import_metadata WHERE user_id = ? AND media_asset_id = ?",
    )
    .bind(user.as_uuid().to_string())
    .bind(asset.as_uuid().to_string())
    .fetch_one(database.pool())
    .await
    .expect("metadata");
    let raw: ParsedTags = serde_json::from_str(&stored.0).expect("stored raw tags");
    assert_eq!(raw.title.as_deref(), Some("  Cafe\u{301} Song  "));
    assert_eq!(stored.1, "caf\u{e9} song");
    assert_eq!(stored.2, "local_tags");
}

#[tokio::test]
async fn missing_asset_with_same_fingerprint_is_reused_as_a_move() {
    let (database, user, importer) = setup().await;
    let original_asset = MediaAssetId::new();
    let first = importer
        .import(import(
            original_asset,
            "old/song.flac",
            "fingerprint-a",
            "Song",
        ))
        .await
        .expect("initial import");
    sqlx::query(
        "UPDATE media_assets SET availability = 'missing' WHERE user_id = ? AND media_asset_id = ?",
    )
    .bind(user.as_uuid().to_string())
    .bind(original_asset.as_uuid().to_string())
    .execute(database.pool())
    .await
    .expect("mark missing");

    let moved = importer
        .import(import(
            MediaAssetId::new(),
            "new/song.flac",
            "fingerprint-a",
            "Song",
        ))
        .await
        .expect("moved import");

    assert_eq!(moved.media_asset_id, original_asset);
    assert_eq!(moved.track_id, first.track_id);
    assert_eq!(moved.recording_id, first.recording_id);
    let path: Vec<u8> = sqlx::query_scalar(
        "SELECT normalized_path FROM media_assets WHERE user_id = ? AND media_asset_id = ?",
    )
    .bind(user.as_uuid().to_string())
    .bind(original_asset.as_uuid().to_string())
    .fetch_one(database.pool())
    .await
    .expect("moved path");
    assert_eq!(path, b"new/song.flac");
}

#[tokio::test]
async fn simultaneous_copy_and_same_metadata_different_content_get_independent_recordings() {
    let (database, user, importer) = setup().await;
    let first = importer
        .import(import(
            MediaAssetId::new(),
            "one/song.flac",
            "fingerprint-a",
            "Song",
        ))
        .await
        .expect("first import");
    let copy = importer
        .import(import(
            MediaAssetId::new(),
            "copy/song.flac",
            "fingerprint-a",
            "Song",
        ))
        .await
        .expect("simultaneous copy");
    let different_content = importer
        .import(import(
            MediaAssetId::new(),
            "other/song.flac",
            "fingerprint-b",
            "Song",
        ))
        .await
        .expect("different content");

    assert_ne!(copy.media_asset_id, first.media_asset_id);
    assert_ne!(copy.recording_id, first.recording_id);
    assert_ne!(different_content.recording_id, first.recording_id);
    assert!(copy.possible_match);
    assert!(different_content.possible_match);
    let matches: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM recording_possible_matches WHERE user_id = ?")
            .bind(user.as_uuid().to_string())
            .fetch_one(database.pool())
            .await
            .expect("possible matches");
    assert_eq!(matches, 2);
}

#[tokio::test]
async fn artist_credit_and_recording_variants_never_merge_without_authoritative_id() {
    let (_database, _user, importer) = setup().await;
    let mut outcomes = Vec::new();
    for (index, (title, artist)) in [
        ("Song (Original)", "Artist A"),
        ("Song (Remaster)", "Artist A"),
        ("Song (Live)", "Artist A"),
        ("Song (Mono)", "Artist A"),
        ("Song (Stereo)", "Artist A"),
        ("Song", "Artist A feat. B"),
        ("Song", "Artist A"),
    ]
    .into_iter()
    .enumerate()
    {
        let mut input = import(
            MediaAssetId::new(),
            &format!("variant/{index}.flac"),
            &format!("fingerprint-{index}"),
            title,
        );
        input.tags.artists = vec![artist.to_owned()];
        outcomes.push(importer.import(input).await.expect("variant import"));
    }

    for (index, outcome) in outcomes.iter().enumerate() {
        assert!(
            !outcomes[..index]
                .iter()
                .any(|earlier| earlier.recording_id == outcome.recording_id),
            "variant {index} was incorrectly merged"
        );
    }
}

#[tokio::test]
async fn scanned_asset_without_a_projection_is_attached_in_place() {
    let (database, user, importer) = setup().await;
    let asset = MediaAssetId::new();
    let scan_repository = SqliteScanRepository::new(database.pool().clone(), user);
    let scan = scan_repository.begin_scan().await.expect("scan");
    scan_repository
        .commit_batch(
            scan,
            &[MediaAssetWrite {
                media_asset_id: asset,
                normalized_path: b"scan/song.flac".to_vec(),
                original_path: b"Scan/Song.flac".to_vec(),
                identity: "fingerprint-scan".to_owned(),
            }],
            &[],
        )
        .await
        .expect("scanned asset");

    let outcome = importer
        .import(import(
            asset,
            "scan/song.flac",
            "fingerprint-scan",
            "Scanned Song",
        ))
        .await
        .expect("projection attaches to scanned asset");

    assert_eq!(outcome.media_asset_id, asset);
    let stored_track: String = sqlx::query_scalar(
        "SELECT track_id FROM media_assets WHERE user_id = ? AND media_asset_id = ?",
    )
    .bind(user.as_uuid().to_string())
    .bind(asset.as_uuid().to_string())
    .fetch_one(database.pool())
    .await
    .expect("attached track");
    assert_eq!(stored_track, outcome.track_id.as_uuid().to_string());
}
