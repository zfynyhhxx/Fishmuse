use fishmuse_domain::{MediaAssetId, UserId};
use fishmuse_library::{LocalImport, LocalLibraryImporter, ParsedTags};
use fishmuse_storage::{Database, MediaAssetWrite, ScanRepository, SqliteScanRepository};

fn import(asset: MediaAssetId, path: &str, fingerprint: &str, title: &str) -> LocalImport {
    LocalImport {
        media_asset_id: asset,
        normalized_path: path.as_bytes().to_vec(),
        original_path: path.as_bytes().to_vec(),
        content_fingerprint: fingerprint.to_owned(),
        fallback_title: path
            .rsplit('/')
            .next()
            .unwrap_or(path)
            .rsplit_once('.')
            .map_or_else(|| path.to_owned(), |(stem, _)| stem.to_owned()),
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

fn tagless(
    asset: MediaAssetId,
    path: &str,
    fingerprint: &str,
    fallback_title: &str,
) -> LocalImport {
    let mut input = import(asset, path, fingerprint, "ignored");
    input.tags.title = None;
    input.fallback_title = fallback_title.to_owned();
    input
}

async fn setup() -> (Database, UserId, LocalLibraryImporter) {
    let database = Database::open_in_memory().await.expect("database");
    let user = database.ensure_local_user().await.expect("local user");
    let importer = LocalLibraryImporter::new(database.pool().clone(), user);
    (database, user, importer)
}

async fn projected_titles(
    database: &Database,
    track_id: fishmuse_domain::TrackId,
) -> (String, String) {
    sqlx::query_as("SELECT tracks.title, recordings.title FROM tracks JOIN recordings ON recordings.user_id = tracks.user_id AND recordings.recording_id = tracks.recording_id WHERE tracks.track_id = ?")
        .bind(track_id.as_uuid().to_string())
        .fetch_one(database.pool())
        .await
        .expect("projected titles")
}

#[tokio::test]
async fn tagless_import_uses_fallback_and_repairs_only_confirmed_legacy_placeholders() {
    let (database, _user, importer) = setup().await;

    let fresh_asset = MediaAssetId::new();
    let fresh = importer
        .import(tagless(
            fresh_asset,
            "music/Quiet River.flac",
            "fresh-fingerprint",
            "Quiet River",
        ))
        .await
        .expect("fresh tagless import");
    assert_eq!(
        projected_titles(&database, fresh.track_id).await,
        ("Quiet River".to_owned(), "Quiet River".to_owned())
    );

    let legacy_asset = MediaAssetId::new();
    let legacy = importer
        .import(tagless(
            legacy_asset,
            "music/Recovered Name.flac",
            "legacy-fingerprint",
            "Unknown title",
        ))
        .await
        .expect("legacy placeholder");
    let repaired = importer
        .import(tagless(
            legacy_asset,
            "music/Recovered Name.flac",
            "legacy-fingerprint",
            "Recovered Name",
        ))
        .await
        .expect("repair placeholder");
    assert_eq!(repaired.track_id, legacy.track_id);
    assert_eq!(repaired.recording_id, legacy.recording_id);
    assert_eq!(
        projected_titles(&database, repaired.track_id).await,
        ("Recovered Name".to_owned(), "Recovered Name".to_owned())
    );

    let literal_asset = MediaAssetId::new();
    let literal = importer
        .import(import(
            literal_asset,
            "music/Literal.flac",
            "literal-fingerprint",
            "Unknown title",
        ))
        .await
        .expect("literal title");
    let mut literal_rescan = import(
        literal_asset,
        "music/Literal.flac",
        "literal-fingerprint",
        "Unknown title",
    );
    literal_rescan.fallback_title = "Literal".to_owned();
    importer
        .import(literal_rescan)
        .await
        .expect("literal rescan");
    assert_eq!(
        projected_titles(&database, literal.track_id).await,
        ("Unknown title".to_owned(), "Unknown title".to_owned())
    );

    let named_asset = MediaAssetId::new();
    let named = importer
        .import(import(
            named_asset,
            "music/Original.flac",
            "named-fingerprint",
            "Original title",
        ))
        .await
        .expect("named title");
    let mut named_rescan = import(
        named_asset,
        "music/Original.flac",
        "named-fingerprint",
        "Original title",
    );
    named_rescan.fallback_title = "Replacement attempt".to_owned();
    importer.import(named_rescan).await.expect("named rescan");
    assert_eq!(
        projected_titles(&database, named.track_id).await,
        ("Original title".to_owned(), "Original title".to_owned())
    );
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

#[tokio::test]
async fn same_path_with_changed_fingerprint_gets_a_new_projection_and_preserves_history() {
    let (database, user, importer) = setup().await;
    let asset = MediaAssetId::new();
    let first = importer
        .import(import(
            asset,
            "replace/song.flac",
            "fingerprint-original",
            "Song",
        ))
        .await
        .expect("original import");
    sqlx::query("INSERT INTO listening_events(listen_id, user_id, track_id, started_at, listened_ms, completed) VALUES (?, ?, ?, ?, ?, ?)")
        .bind(fishmuse_domain::ListenId::new().as_uuid().to_string())
        .bind(user.as_uuid().to_string())
        .bind(first.track_id.as_uuid().to_string())
        .bind(1_800_000_000_i64)
        .bind(1_000_i64)
        .bind(true)
        .execute(database.pool())
        .await
        .expect("history");
    let mut replacement = import(
        asset,
        "replace/song.flac",
        "fingerprint-replacement",
        "Song",
    );
    replacement.tags.album = Some("Replacement Album".to_owned());
    replacement.tags.track_number = Some(9);

    let second = importer
        .import(replacement)
        .await
        .expect("replacement import");

    assert_eq!(second.media_asset_id, asset);
    assert_ne!(second.track_id, first.track_id);
    assert_ne!(second.recording_id, first.recording_id);
    assert!(second.possible_match);
    let asset_projection: (String, String) = sqlx::query_as(
        "SELECT track_id, content_fingerprint FROM media_assets WHERE user_id = ? AND media_asset_id = ?",
    )
    .bind(user.as_uuid().to_string())
    .bind(asset.as_uuid().to_string())
    .fetch_one(database.pool())
    .await
    .expect("asset projection");
    assert_eq!(asset_projection.0, second.track_id.as_uuid().to_string());
    assert_eq!(asset_projection.1, "fingerprint-replacement");
    let canonical_counts: (i64, i64, i64) = sqlx::query_as(
        "SELECT (SELECT COUNT(*) FROM recordings WHERE user_id = ?), (SELECT COUNT(*) FROM tracks WHERE user_id = ?), (SELECT COUNT(*) FROM listening_events WHERE user_id = ? AND track_id = ?)",
    )
    .bind(user.as_uuid().to_string())
    .bind(user.as_uuid().to_string())
    .bind(user.as_uuid().to_string())
    .bind(first.track_id.as_uuid().to_string())
    .fetch_one(database.pool())
    .await
    .expect("preserved canonical rows and history");
    assert_eq!(canonical_counts, (2, 2, 1));
    let old_playable: i64 =
        sqlx::query_scalar("SELECT playable FROM tracks WHERE user_id = ? AND track_id = ?")
            .bind(user.as_uuid().to_string())
            .bind(first.track_id.as_uuid().to_string())
            .fetch_one(database.pool())
            .await
            .expect("historical track state");
    assert_eq!(old_playable, 0, "superseded track has no playable asset");
    let refreshed: (String, i64, String) = sqlx::query_as(
        "SELECT releases.title, tracks.track_number, local_import_metadata.raw_tags_json FROM tracks JOIN releases ON releases.user_id = tracks.user_id AND releases.release_id = tracks.release_id JOIN media_assets ON media_assets.user_id = tracks.user_id AND media_assets.track_id = tracks.track_id JOIN local_import_metadata ON local_import_metadata.user_id = media_assets.user_id AND local_import_metadata.media_asset_id = media_assets.media_asset_id WHERE tracks.user_id = ? AND tracks.track_id = ?",
    )
    .bind(user.as_uuid().to_string())
    .bind(second.track_id.as_uuid().to_string())
    .fetch_one(database.pool())
    .await
    .expect("refreshed projection");
    assert_eq!(refreshed.0, "Replacement Album");
    assert_eq!(refreshed.1, 9);
    let raw: ParsedTags = serde_json::from_str(&refreshed.2).expect("raw replacement tags");
    assert_eq!(raw.album.as_deref(), Some("Replacement Album"));
    let possible_match: (String, String) = sqlx::query_as(
        "SELECT recording_id, candidate_recording_id FROM recording_possible_matches WHERE user_id = ?",
    )
    .bind(user.as_uuid().to_string())
    .fetch_one(database.pool())
    .await
    .expect("replacement possible match");
    assert_eq!(possible_match.0, second.recording_id.as_uuid().to_string());
    assert_eq!(possible_match.1, first.recording_id.as_uuid().to_string());
}
