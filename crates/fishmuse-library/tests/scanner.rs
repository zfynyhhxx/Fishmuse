use std::{
    collections::HashSet,
    fs,
    io::{Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use async_trait::async_trait;
use filetime::FileTime;
use fishmuse_domain::{ErrorCategory, MediaAssetId, ScanId};
use fishmuse_library::{
    DiagnosticCode, LibraryScanner, ParsedTags, QuickFileIdentity, ScanFailure, ScanProgress,
    ScanRequest, ScanStatus, TagReader,
};
use fishmuse_storage::Database;
use tempfile::tempdir;
use tokio::sync::{mpsc, watch};
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
struct FakeTagReader {
    calls: Arc<Mutex<Vec<PathBuf>>>,
    failures: Arc<HashSet<String>>,
    block_on: Arc<Option<String>>,
    block_started: watch::Sender<bool>,
}

impl FakeTagReader {
    fn successful() -> Self {
        let (block_started, _) = watch::channel(false);
        Self {
            calls: Arc::new(Mutex::new(Vec::new())),
            failures: Arc::new(HashSet::new()),
            block_on: Arc::new(None),
            block_started,
        }
    }

    fn failing(file_names: &[&str]) -> Self {
        let mut reader = Self::successful();
        reader.failures = Arc::new(file_names.iter().map(ToString::to_string).collect());
        reader
    }

    fn blocking(file_name: &str) -> Self {
        let mut reader = Self::successful();
        reader.block_on = Arc::new(Some(file_name.to_owned()));
        reader
    }

    fn call_count(&self) -> usize {
        self.calls.lock().expect("calls lock").len()
    }

    fn called_file_names(&self) -> Vec<String> {
        self.calls
            .lock()
            .expect("calls lock")
            .iter()
            .map(|path| {
                path.file_name()
                    .expect("file name")
                    .to_string_lossy()
                    .into_owned()
            })
            .collect()
    }

    async fn wait_until_blocked(&self) {
        let mut receiver = self.block_started.subscribe();
        if !*receiver.borrow() {
            receiver
                .changed()
                .await
                .expect("blocking reader still alive");
        }
    }
}

#[async_trait]
impl TagReader for FakeTagReader {
    async fn read(&self, path: &Path) -> Result<ParsedTags, ScanFailure> {
        self.calls
            .lock()
            .expect("calls lock")
            .push(path.to_path_buf());
        let file_name = path
            .file_name()
            .expect("file name")
            .to_string_lossy()
            .into_owned();
        if self.block_on.as_deref() == Some(file_name.as_str()) {
            self.block_started.send_replace(true);
            std::future::pending::<()>().await;
        }
        if self.failures.contains(&file_name) {
            return Err(ScanFailure::new(
                DiagnosticCode::InvalidTags,
                "invalid embedded tags",
            ));
        }
        Ok(ParsedTags {
            title: Some(file_name),
            artists: vec!["Fixture Artist".to_owned()],
            album: Some("Fixture Album".to_owned()),
            duration_ms: Some(1_000),
            disc_number: Some(1),
            track_number: Some(1),
        })
    }
}

async fn setup() -> (Database, fishmuse_domain::UserId) {
    let database = Database::open_in_memory().await.expect("database");
    let user_id = database.ensure_local_user().await.expect("local user");
    (database, user_id)
}

fn fixture(path: &Path, contents: &[u8]) {
    fs::write(path, contents).expect("self-generated media fixture");
}

fn progress_channel() -> (
    mpsc::Sender<fishmuse_library::ScanProgress>,
    mpsc::Receiver<fishmuse_library::ScanProgress>,
) {
    mpsc::channel(32)
}

#[tokio::test]
async fn first_scan_persists_assets_cue_diagnostics_and_ignores_unknown_extensions() {
    let (database, user_id) = setup().await;
    let directory = tempdir().expect("temporary directory");
    fixture(&directory.path().join("song.MP3"), b"generated mp3 fixture");
    fixture(&directory.path().join("album.cue"), b"FILE song.mp3 WAVE");
    fixture(
        &directory.path().join("cover.jpg"),
        b"generated image fixture",
    );
    let reader = FakeTagReader::successful();
    let scanner = LibraryScanner::new(database.pool().clone(), reader.clone());
    let (progress, _receiver) = progress_channel();

    let summary = scanner
        .scan(
            ScanRequest {
                user_id,
                roots: vec![directory.path().to_path_buf()],
            },
            CancellationToken::new(),
            progress,
        )
        .await
        .expect("scan");

    assert_eq!(summary.status, ScanStatus::Completed);
    assert_eq!(summary.parsed, 1);
    assert_eq!(summary.failed, 1);
    assert_eq!(reader.called_file_names(), vec!["song.MP3"]);
    let asset_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM media_assets WHERE user_id = ? AND availability = 'available'",
    )
    .bind(user_id.as_uuid().to_string())
    .fetch_one(database.pool())
    .await
    .expect("asset count");
    assert_eq!(asset_count, 1);
    let diagnostic_code: String = sqlx::query_scalar(
        "SELECT code FROM scan_diagnostics WHERE user_id = ? ORDER BY created_at LIMIT 1",
    )
    .bind(user_id.as_uuid().to_string())
    .fetch_one(database.pool())
    .await
    .expect("cue diagnostic");
    assert_eq!(diagnostic_code, "unsupported_cue");
}

#[tokio::test]
async fn unchanged_rescan_does_not_parse_tags_again() {
    let (database, user_id) = setup().await;
    let directory = tempdir().expect("temporary directory");
    fixture(
        &directory.path().join("song.flac"),
        b"generated flac fixture",
    );
    let reader = FakeTagReader::successful();
    let scanner = LibraryScanner::new(database.pool().clone(), reader.clone());

    for _ in 0..2 {
        let (progress, _receiver) = progress_channel();
        scanner
            .scan(
                ScanRequest {
                    user_id,
                    roots: vec![directory.path().to_path_buf()],
                },
                CancellationToken::new(),
                progress,
            )
            .await
            .expect("scan");
    }

    assert_eq!(reader.call_count(), 1);
    let asset_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM media_assets WHERE user_id = ?")
            .bind(user_id.as_uuid().to_string())
            .fetch_one(database.pool())
            .await
            .expect("asset count");
    assert_eq!(asset_count, 1);
}

#[tokio::test]
async fn middle_only_change_with_same_size_mtime_and_stable_identity_is_reparsed() {
    let (database, user_id) = setup().await;
    let directory = tempdir().expect("temporary directory");
    let path = directory.path().join("large.flac");
    let mut contents = vec![b'a'; 256 * 1024];
    contents[128 * 1024..128 * 1024 + 8].copy_from_slice(b"before!!");
    fixture(&path, &contents);
    let fixed_time = FileTime::from_unix_time(1_800_000_000, 0);
    filetime::set_file_mtime(&path, fixed_time).expect("fixed mtime");
    let reader = FakeTagReader::successful();
    let scanner = LibraryScanner::new(database.pool().clone(), reader.clone());

    let (progress, _receiver) = progress_channel();
    scanner
        .scan(
            ScanRequest {
                user_id,
                roots: vec![directory.path().to_path_buf()],
            },
            CancellationToken::new(),
            progress,
        )
        .await
        .expect("first scan");
    let fingerprint_before: String =
        sqlx::query_scalar("SELECT content_fingerprint FROM media_assets WHERE user_id = ?")
            .bind(user_id.as_uuid().to_string())
            .fetch_one(database.pool())
            .await
            .expect("first fingerprint");
    let quick_before = QuickFileIdentity::read(&path).expect("quick identity before");

    let mut file = fs::OpenOptions::new()
        .write(true)
        .open(&path)
        .expect("open fixture for middle mutation");
    file.seek(SeekFrom::Start(128 * 1024))
        .expect("seek to middle");
    file.write_all(b"after!!!").expect("mutate middle");
    file.sync_all().expect("flush mutation");
    filetime::set_file_mtime(&path, fixed_time).expect("restore mtime");
    let quick_after = QuickFileIdentity::read(&path).expect("quick identity after");
    assert_eq!(
        quick_before.quick_fingerprint,
        quick_after.quick_fingerprint
    );

    let (progress, _receiver) = progress_channel();
    scanner
        .scan(
            ScanRequest {
                user_id,
                roots: vec![directory.path().to_path_buf()],
            },
            CancellationToken::new(),
            progress,
        )
        .await
        .expect("second scan");

    let fingerprint_after: String =
        sqlx::query_scalar("SELECT content_fingerprint FROM media_assets WHERE user_id = ?")
            .bind(user_id.as_uuid().to_string())
            .fetch_one(database.pool())
            .await
            .expect("second fingerprint");
    assert_ne!(fingerprint_before, fingerprint_after);
    assert_eq!(reader.call_count(), 2);
}

#[tokio::test]
async fn uncertain_existing_identity_is_reparsed_and_updated_in_place() {
    let (database, user_id) = setup().await;
    let directory = tempdir().expect("temporary directory");
    let path = directory.path().join("uncertain.flac");
    fixture(&path, b"generated uncertain fixture");
    let media_asset_id = fishmuse_domain::MediaAssetId::new();
    sqlx::query(
        "INSERT INTO media_assets(media_asset_id, user_id, normalized_path, original_path, content_fingerprint, availability) VALUES (?, ?, ?, ?, NULL, 'available')",
    )
    .bind(media_asset_id.as_uuid().to_string())
    .bind(user_id.as_uuid().to_string())
    .bind(fishmuse_library::normalize_path_bytes(&path))
    .bind(b"technical-original-path".as_slice())
    .execute(database.pool())
    .await
    .expect("uncertain asset");
    let reader = FakeTagReader::successful();
    let scanner = LibraryScanner::new(database.pool().clone(), reader.clone());
    let (progress, _receiver) = progress_channel();

    let summary = scanner
        .scan(
            ScanRequest {
                user_id,
                roots: vec![directory.path().to_path_buf()],
            },
            CancellationToken::new(),
            progress,
        )
        .await
        .expect("scan uncertain asset");

    assert_eq!(summary.parsed, 1);
    assert_eq!(reader.call_count(), 1);
    let row: (String, Option<String>) = sqlx::query_as(
        "SELECT media_asset_id, content_fingerprint FROM media_assets WHERE user_id = ?",
    )
    .bind(user_id.as_uuid().to_string())
    .fetch_one(database.pool())
    .await
    .expect("updated asset");
    assert_eq!(row.0, media_asset_id.as_uuid().to_string());
    assert!(row.1.is_some());
}

#[tokio::test]
async fn moved_identical_file_updates_one_asset_after_full_hash_confirmation() {
    let (database, user_id) = setup().await;
    let directory = tempdir().expect("temporary directory");
    let original = directory.path().join("before.flac");
    let moved = directory.path().join("after.flac");
    fixture(&original, b"generated move fixture");
    let reader = FakeTagReader::successful();
    let scanner = LibraryScanner::new(database.pool().clone(), reader.clone());
    let (progress, _receiver) = progress_channel();
    scanner
        .scan(
            ScanRequest {
                user_id,
                roots: vec![directory.path().to_path_buf()],
            },
            CancellationToken::new(),
            progress,
        )
        .await
        .expect("first scan");
    fs::rename(&original, &moved).expect("move fixture");

    let (progress, _receiver) = progress_channel();
    let second = scanner
        .scan(
            ScanRequest {
                user_id,
                roots: vec![directory.path().to_path_buf()],
            },
            CancellationToken::new(),
            progress,
        )
        .await
        .expect("second scan");

    assert_eq!(second.unchanged, 1);
    assert_eq!(reader.call_count(), 1);
    let asset_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM media_assets WHERE user_id = ?")
            .bind(user_id.as_uuid().to_string())
            .fetch_one(database.pool())
            .await
            .expect("asset count");
    assert_eq!(asset_count, 1);
}

#[tokio::test]
async fn identical_copy_beside_original_creates_a_distinct_media_asset() {
    let (database, user_id) = setup().await;
    let directory = tempdir().expect("temporary directory");
    let original = directory.path().join("original.flac");
    let copy = directory.path().join("copy.flac");
    fixture(&original, b"generated duplicate-content fixture");
    let reader = FakeTagReader::successful();
    let scanner = LibraryScanner::new(database.pool().clone(), reader.clone());

    let (progress, _receiver) = progress_channel();
    scanner
        .scan(
            ScanRequest {
                user_id,
                roots: vec![directory.path().to_path_buf()],
            },
            CancellationToken::new(),
            progress,
        )
        .await
        .expect("first scan");
    fs::copy(&original, &copy).expect("copy fixture");

    let (progress, _receiver) = progress_channel();
    scanner
        .scan(
            ScanRequest {
                user_id,
                roots: vec![directory.path().to_path_buf()],
            },
            CancellationToken::new(),
            progress,
        )
        .await
        .expect("second scan");

    let assets: Vec<(String, Vec<u8>)> = sqlx::query_as(
        "SELECT media_asset_id, normalized_path FROM media_assets WHERE user_id = ? ORDER BY normalized_path",
    )
    .bind(user_id.as_uuid().to_string())
    .fetch_all(database.pool())
    .await
    .expect("assets");
    assert_eq!(assets.len(), 2);
    assert_ne!(assets[0].0, assets[1].0);
    assert_eq!(
        reader.call_count(),
        2,
        "the new copy needs its own tag parse"
    );
}

#[tokio::test]
async fn identical_replacements_map_sorted_old_ids_to_sorted_new_paths_deterministically() {
    let (database, user_id) = setup().await;
    let directory = tempdir().expect("temporary directory");
    let contents = vec![b'x'; 512 * 1024];
    let new_paths: Vec<PathBuf> = ["new-a.flac", "new-b.flac", "new-c.flac", "new-d.flac"]
        .into_iter()
        .map(|name| directory.path().join(name))
        .collect();
    for path in &new_paths {
        fixture(path, &contents);
    }
    let identity = fishmuse_library::FileIdentity::read(&new_paths[0]).expect("file identity");
    let stored_identity = format!(
        "q:{}|f:{}",
        identity.quick_fingerprint, identity.content_fingerprint
    );
    let old_paths: Vec<PathBuf> = ["old-a.flac", "old-b.flac", "old-c.flac", "old-d.flac"]
        .into_iter()
        .map(|name| directory.path().join(name))
        .collect();
    let old_ids: Vec<MediaAssetId> = (0..old_paths.len()).map(|_| MediaAssetId::new()).collect();
    for (old_path, old_id) in old_paths.iter().zip(&old_ids).rev() {
        let normalized = fishmuse_library::normalize_path_bytes(old_path);
        sqlx::query(
            "INSERT INTO media_assets(media_asset_id, user_id, normalized_path, original_path, content_fingerprint, availability) VALUES (?, ?, ?, ?, ?, 'available')",
        )
        .bind(old_id.as_uuid().to_string())
        .bind(user_id.as_uuid().to_string())
        .bind(&normalized)
        .bind(&normalized)
        .bind(&stored_identity)
        .execute(database.pool())
        .await
        .expect("seed disappeared asset");
    }
    let reader = FakeTagReader::successful();
    let scanner = LibraryScanner::new(database.pool().clone(), reader.clone())
        .with_concurrency_limit(new_paths.len());
    let (progress, _receiver) = progress_channel();

    let summary = scanner
        .scan(
            ScanRequest {
                user_id,
                roots: vec![directory.path().to_path_buf()],
            },
            CancellationToken::new(),
            progress,
        )
        .await
        .expect("replacement scan");

    assert_eq!(summary.unchanged, 4);
    assert_eq!(reader.call_count(), 0);
    for (old_id, new_path) in old_ids.iter().zip(&new_paths) {
        let stored_path: Vec<u8> = sqlx::query_scalar(
            "SELECT normalized_path FROM media_assets WHERE user_id = ? AND media_asset_id = ?",
        )
        .bind(user_id.as_uuid().to_string())
        .bind(old_id.as_uuid().to_string())
        .fetch_one(database.pool())
        .await
        .expect("mapped asset path");
        assert_eq!(
            stored_path,
            fishmuse_library::normalize_path_bytes(new_path),
            "lexically ordered old paths must map to lexically ordered replacements"
        );
    }
}

#[tokio::test]
async fn completed_rescan_marks_a_disappeared_file_unavailable_without_deleting_it() {
    let (database, user_id) = setup().await;
    let directory = tempdir().expect("temporary directory");
    let path = directory.path().join("temporary.flac");
    fixture(&path, b"generated temporary fixture");
    let scanner = LibraryScanner::new(database.pool().clone(), FakeTagReader::successful());
    let (progress, _receiver) = progress_channel();
    scanner
        .scan(
            ScanRequest {
                user_id,
                roots: vec![directory.path().to_path_buf()],
            },
            CancellationToken::new(),
            progress,
        )
        .await
        .expect("first scan");
    fs::remove_file(&path).expect("remove fixture");

    let (progress, _receiver) = progress_channel();
    scanner
        .scan(
            ScanRequest {
                user_id,
                roots: vec![directory.path().to_path_buf()],
            },
            CancellationToken::new(),
            progress,
        )
        .await
        .expect("rescan");

    let availability: String =
        sqlx::query_scalar("SELECT availability FROM media_assets WHERE user_id = ?")
            .bind(user_id.as_uuid().to_string())
            .fetch_one(database.pool())
            .await
            .expect("availability");
    assert_eq!(availability, "missing");
}

#[tokio::test]
async fn root_dot_dot_alias_is_canonicalized_before_persistence_and_missing_detection() {
    let (database, user_id) = setup().await;
    let directory = tempdir().expect("temporary directory");
    fs::create_dir(directory.path().join("child")).expect("alias child directory");
    let canonical_root = directory.path().to_path_buf();
    let mut alias_spelling = canonical_root.as_os_str().to_os_string();
    alias_spelling.push(format!(
        "{}child{}..",
        std::path::MAIN_SEPARATOR,
        std::path::MAIN_SEPARATOR
    ));
    let alias_root = PathBuf::from(alias_spelling);
    assert_ne!(
        fishmuse_library::normalize_path_bytes(&canonical_root),
        fishmuse_library::normalize_path_bytes(&alias_root),
        "fixture must exercise a non-canonical spelling"
    );
    let path = canonical_root.join("temporary.flac");
    fixture(&path, b"generated alias fixture");
    let scanner = LibraryScanner::new(database.pool().clone(), FakeTagReader::successful());

    let (progress, _receiver) = progress_channel();
    scanner
        .scan(
            ScanRequest {
                user_id,
                roots: vec![canonical_root],
            },
            CancellationToken::new(),
            progress,
        )
        .await
        .expect("canonical scan");
    fs::remove_file(&path).expect("remove fixture");

    let (progress, _receiver) = progress_channel();
    scanner
        .scan(
            ScanRequest {
                user_id,
                roots: vec![alias_root],
            },
            CancellationToken::new(),
            progress,
        )
        .await
        .expect("alias rescan");

    let root_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM media_roots WHERE user_id = ?")
        .bind(user_id.as_uuid().to_string())
        .fetch_one(database.pool())
        .await
        .expect("root count");
    let availability: String =
        sqlx::query_scalar("SELECT availability FROM media_assets WHERE user_id = ?")
            .bind(user_id.as_uuid().to_string())
            .fetch_one(database.pool())
            .await
            .expect("availability");
    assert_eq!(root_count, 1);
    assert_eq!(availability, "missing");
}

#[tokio::test]
async fn one_bad_file_records_a_diagnostic_without_stopping_the_scan() {
    let (database, user_id) = setup().await;
    let directory = tempdir().expect("temporary directory");
    fixture(
        &directory.path().join("good.flac"),
        b"generated good fixture",
    );
    fixture(
        &directory.path().join("bad.flac"),
        b"generated broken fixture",
    );
    let reader = FakeTagReader::failing(&["bad.flac"]);
    let scanner = LibraryScanner::new(database.pool().clone(), reader);
    let (progress, _receiver) = progress_channel();

    let summary = scanner
        .scan(
            ScanRequest {
                user_id,
                roots: vec![directory.path().to_path_buf()],
            },
            CancellationToken::new(),
            progress,
        )
        .await
        .expect("scan continues");

    assert_eq!(summary.status, ScanStatus::Completed);
    assert_eq!(summary.parsed, 1);
    assert_eq!(summary.failed, 1);
    let codes: Vec<String> =
        sqlx::query_scalar("SELECT code FROM scan_diagnostics WHERE user_id = ? ORDER BY code")
            .bind(user_id.as_uuid().to_string())
            .fetch_all(database.pool())
            .await
            .expect("diagnostics");
    assert_eq!(codes, vec!["invalid_tags"]);
}

#[tokio::test]
async fn cancellation_marks_the_run_cancelled_and_keeps_committed_assets_consistent() {
    let (database, user_id) = setup().await;
    let directory = tempdir().expect("temporary directory");
    fixture(
        &directory.path().join("a-good.flac"),
        b"generated good fixture",
    );
    fixture(
        &directory.path().join("z-slow.flac"),
        b"generated slow fixture",
    );
    let reader = FakeTagReader::blocking("z-slow.flac");
    let scanner = LibraryScanner::new(database.pool().clone(), reader.clone())
        .with_concurrency_limit(1)
        .with_batch_size(1);
    let cancellation = CancellationToken::new();
    let cancel_for_scan = cancellation.clone();
    let (progress, _receiver) = progress_channel();
    let task = tokio::spawn(async move {
        scanner
            .scan(
                ScanRequest {
                    user_id,
                    roots: vec![directory.path().to_path_buf()],
                },
                cancel_for_scan,
                progress,
            )
            .await
    });
    reader.wait_until_blocked().await;

    cancellation.cancel();
    let summary = task.await.expect("scan task").expect("cancelled summary");

    assert_eq!(summary.status, ScanStatus::Cancelled);
    let asset_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM media_assets WHERE user_id = ?")
            .bind(user_id.as_uuid().to_string())
            .fetch_one(database.pool())
            .await
            .expect("asset count");
    assert_eq!(asset_count, 1);
    let run_status: String = sqlx::query_scalar(
        "SELECT status FROM scan_runs WHERE user_id = ? ORDER BY started_at DESC LIMIT 1",
    )
    .bind(user_id.as_uuid().to_string())
    .fetch_one(database.pool())
    .await
    .expect("run status");
    assert_eq!(run_status, "cancelled");
}

#[tokio::test]
async fn inaccessible_root_returns_actionable_safe_error_and_persists_failed_run() {
    let (database, user_id) = setup().await;
    let missing = PathBuf::from(r"Z:\fishmuse-test-root-that-does-not-exist\private");
    let reader = FakeTagReader::successful();
    let scanner = LibraryScanner::new(database.pool().clone(), reader);
    let (progress, _receiver) = progress_channel();

    let error = scanner
        .scan(
            ScanRequest {
                user_id,
                roots: vec![missing.clone()],
            },
            CancellationToken::new(),
            progress,
        )
        .await
        .expect_err("missing root");

    assert_eq!(error.category, ErrorCategory::Library);
    assert!(error.suggested_action.is_some());
    let payload = serde_json::to_string(&error).expect("safe error payload");
    assert!(!payload.contains("fishmuse-test-root-that-does-not-exist"));
    assert!(!payload.contains("Z:\\\\"));
    let status: String = sqlx::query_scalar(
        "SELECT status FROM scan_runs WHERE user_id = ? ORDER BY started_at DESC LIMIT 1",
    )
    .bind(user_id.as_uuid().to_string())
    .fetch_one(database.pool())
    .await
    .expect("failed run");
    assert_eq!(status, "failed");
}

#[tokio::test]
async fn relative_root_is_rejected_before_any_path_is_persisted() {
    let (database, user_id) = setup().await;
    let relative = PathBuf::from("relative-private-media-root");
    let scanner = LibraryScanner::new(database.pool().clone(), FakeTagReader::successful());
    let (progress, _receiver) = progress_channel();

    let error = scanner
        .scan(
            ScanRequest {
                user_id,
                roots: vec![relative],
            },
            CancellationToken::new(),
            progress,
        )
        .await
        .expect_err("relative roots are not accepted");

    assert_eq!(error.category, ErrorCategory::Library);
    let root_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM media_roots WHERE user_id = ?")
        .bind(user_id.as_uuid().to_string())
        .fetch_one(database.pool())
        .await
        .expect("root count");
    assert_eq!(root_count, 0);
}

#[tokio::test]
async fn scan_never_writes_source_file_bytes_or_mtime() {
    let (database, user_id) = setup().await;
    let directory = tempdir().expect("temporary directory");
    let path = directory.path().join("readonly.wav");
    fixture(&path, b"generated read-only fixture");
    let fixed_time = FileTime::from_unix_time(1_800_000_000, 0);
    filetime::set_file_mtime(&path, fixed_time).expect("fixed mtime");
    let bytes_before = blake3::hash(&fs::read(&path).expect("bytes before"));
    let mtime_before = fs::metadata(&path)
        .expect("metadata before")
        .modified()
        .expect("mtime");
    let scanner = LibraryScanner::new(database.pool().clone(), FakeTagReader::successful());
    let (progress, _receiver) = progress_channel();

    scanner
        .scan(
            ScanRequest {
                user_id,
                roots: vec![directory.path().to_path_buf()],
            },
            CancellationToken::new(),
            progress,
        )
        .await
        .expect("scan");

    let bytes_after = blake3::hash(&fs::read(&path).expect("bytes after"));
    let mtime_after = fs::metadata(&path)
        .expect("metadata after")
        .modified()
        .expect("mtime");
    assert_eq!(bytes_before, bytes_after);
    assert_eq!(mtime_before, mtime_after);
}

#[derive(Clone)]
struct CountingTagReader {
    in_flight: Arc<AtomicUsize>,
    maximum: Arc<AtomicUsize>,
}

#[async_trait]
impl TagReader for CountingTagReader {
    async fn read(&self, _path: &Path) -> Result<ParsedTags, ScanFailure> {
        let current = self.in_flight.fetch_add(1, Ordering::SeqCst) + 1;
        self.maximum.fetch_max(current, Ordering::SeqCst);
        tokio::time::sleep(Duration::from_millis(40)).await;
        self.in_flight.fetch_sub(1, Ordering::SeqCst);
        Ok(ParsedTags::default())
    }
}

#[tokio::test]
async fn configurable_semaphore_bounds_parallel_tag_reads() {
    let (database, user_id) = setup().await;
    let directory = tempdir().expect("temporary directory");
    for index in 0..6 {
        fixture(
            &directory.path().join(format!("track-{index}.ogg")),
            format!("generated fixture {index}").as_bytes(),
        );
    }
    let reader = CountingTagReader {
        in_flight: Arc::new(AtomicUsize::new(0)),
        maximum: Arc::new(AtomicUsize::new(0)),
    };
    let maximum = reader.maximum.clone();
    let scanner = LibraryScanner::new(database.pool().clone(), reader).with_concurrency_limit(2);
    let (progress, _receiver) = progress_channel();

    scanner
        .scan(
            ScanRequest {
                user_id,
                roots: vec![directory.path().to_path_buf()],
            },
            CancellationToken::new(),
            progress,
        )
        .await
        .expect("scan");

    assert_eq!(maximum.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn progress_is_published_after_250_ms_while_a_tag_read_is_still_pending() {
    let (database, user_id) = setup().await;
    let directory = tempdir().expect("temporary directory");
    fixture(
        &directory.path().join("slow.flac"),
        b"generated slow fixture",
    );
    let reader = FakeTagReader::blocking("slow.flac");
    let scanner =
        LibraryScanner::new(database.pool().clone(), reader.clone()).with_concurrency_limit(1);
    let cancellation = CancellationToken::new();
    let cancel_for_scan = cancellation.clone();
    let (progress, mut receiver) = progress_channel();
    let task = tokio::spawn(async move {
        scanner
            .scan(
                ScanRequest {
                    user_id,
                    roots: vec![directory.path().to_path_buf()],
                },
                cancel_for_scan,
                progress,
            )
            .await
    });
    reader.wait_until_blocked().await;

    let interim = tokio::time::timeout(Duration::from_millis(500), receiver.recv()).await;
    cancellation.cancel();
    let summary = task.await.expect("scan task").expect("cancelled summary");
    let interim = interim
        .expect("progress within 250 ms plus scheduling margin")
        .expect("progress");

    assert_eq!(interim.discovered, 1);
    assert_eq!(interim.parsed, 0);
    assert_eq!(summary.status, ScanStatus::Cancelled);
}

#[tokio::test]
async fn progress_is_published_at_the_100_item_threshold_without_per_file_events() {
    let (database, user_id) = setup().await;
    let directory = tempdir().expect("temporary directory");
    for index in 0..101 {
        fixture(
            &directory.path().join(format!("track-{index:03}.mp3")),
            format!("generated fixture {index}").as_bytes(),
        );
    }
    let scanner = LibraryScanner::new(database.pool().clone(), FakeTagReader::successful());
    let (progress, mut receiver) = progress_channel();

    scanner
        .scan(
            ScanRequest {
                user_id,
                roots: vec![directory.path().to_path_buf()],
            },
            CancellationToken::new(),
            progress,
        )
        .await
        .expect("scan");

    let mut events = Vec::new();
    while let Ok(event) = receiver.try_recv() {
        events.push(event);
    }
    assert_eq!(events.len(), 2, "one threshold event and one final event");
    assert!(events[0].parsed >= 100);
    assert_eq!(events[1].parsed, 101);
}

#[tokio::test]
async fn terminal_progress_waits_for_capacity_and_is_guaranteed_to_an_open_receiver() {
    let (database, user_id) = setup().await;
    let directory = tempdir().expect("temporary directory");
    fixture(&directory.path().join("one.flac"), b"generated fixture");
    let scanner = LibraryScanner::new(database.pool().clone(), FakeTagReader::successful());
    let (progress, mut receiver) = mpsc::channel(1);
    progress
        .try_send(ScanProgress {
            scan_id: ScanId::new(),
            discovered: 999,
            parsed: 999,
            unchanged: 0,
            failed: 0,
        })
        .expect("prefill bounded channel");
    let root = directory.path().to_path_buf();
    let task = tokio::spawn(async move {
        scanner
            .scan(
                ScanRequest {
                    user_id,
                    roots: vec![root],
                },
                CancellationToken::new(),
                progress,
            )
            .await
    });

    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(
        !task.is_finished(),
        "an open but full receiver must apply backpressure to terminal delivery"
    );
    let prefilled = receiver.recv().await.expect("prefilled progress");
    assert_eq!(prefilled.discovered, 999);
    let terminal = tokio::time::timeout(Duration::from_secs(1), receiver.recv())
        .await
        .expect("terminal progress timeout")
        .expect("terminal progress");
    let summary = task.await.expect("scan task").expect("scan");

    assert_eq!(terminal.scan_id, summary.scan_id);
    assert_eq!(terminal.discovered, summary.discovered);
    assert_eq!(terminal.parsed, summary.parsed);
    assert_eq!(terminal.unchanged, summary.unchanged);
    assert_eq!(terminal.failed, summary.failed);
}
