use std::{
    collections::{HashMap, HashSet, VecDeque},
    io,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};

use fishmuse_domain::{
    AppError, AppResult, ErrorCategory, ErrorCode, MediaAssetId, ScanId, UserId,
};
use fishmuse_storage::{
    MediaAssetWrite, ScanDiagnosticWrite, ScanRepository, ScanRunStatus, SqliteScanRepository,
    StoredMediaAsset,
};
use futures::{StreamExt, stream};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use tokio::sync::{
    Semaphore,
    mpsc::{self, Sender, UnboundedReceiver, error::TrySendError},
};
use tokio::{task::JoinHandle, time::Interval};
use tokio_util::sync::CancellationToken;

use crate::file_identity::{canonicalize_roots, visit_files_from_canonical_roots};
use crate::{
    DiagnosticCode, ExtensionDisposition, FileIdentity, TagReader, classify_extension,
    normalize_path_bytes,
};

const DEFAULT_BATCH_SIZE: usize = 32;
const PROGRESS_ITEM_THRESHOLD: u64 = 100;
const PROGRESS_TIME_THRESHOLD: Duration = Duration::from_millis(250);

#[derive(Clone, Debug)]
pub struct ScanRequest {
    pub user_id: UserId,
    pub roots: Vec<PathBuf>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ScanStatus {
    Completed,
    Cancelled,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ScanProgress {
    pub scan_id: ScanId,
    pub discovered: u64,
    pub parsed: u64,
    pub unchanged: u64,
    pub failed: u64,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ScanSummary {
    pub scan_id: ScanId,
    pub status: ScanStatus,
    pub discovered: u64,
    pub parsed: u64,
    pub unchanged: u64,
    pub failed: u64,
}

pub struct LibraryScanner<R: TagReader> {
    pool: SqlitePool,
    tag_reader: Arc<R>,
    concurrency_limit: usize,
    batch_size: usize,
}

impl<R: TagReader> LibraryScanner<R> {
    #[must_use]
    pub fn new(pool: SqlitePool, tag_reader: R) -> Self {
        let logical_cpus = std::thread::available_parallelism().map_or(1, usize::from);
        Self {
            pool,
            tag_reader: Arc::new(tag_reader),
            concurrency_limit: logical_cpus.min(4),
            batch_size: DEFAULT_BATCH_SIZE,
        }
    }

    #[must_use]
    pub fn with_concurrency_limit(mut self, limit: usize) -> Self {
        self.concurrency_limit = limit.max(1);
        self
    }

    #[must_use]
    pub fn with_batch_size(mut self, batch_size: usize) -> Self {
        self.batch_size = batch_size.max(1);
        self
    }

    pub async fn scan(
        &self,
        request: ScanRequest,
        cancellation: CancellationToken,
        progress: Sender<ScanProgress>,
    ) -> AppResult<ScanSummary> {
        let repository = SqliteScanRepository::new(self.pool.clone(), request.user_id);
        let scan_id = repository.begin_scan().await?;
        if request.roots.is_empty() {
            repository
                .finish_scan(scan_id, ScanRunStatus::Failed)
                .await?;
            return Err(library_error(
                "No media folder was selected.",
                "Choose at least one media folder and try again.",
                "scan request contained no roots",
            ));
        }
        if request.roots.iter().any(|root| !root.is_absolute()) {
            repository
                .finish_scan(scan_id, ScanRunStatus::Failed)
                .await?;
            return Err(library_error(
                "A media folder path is invalid.",
                "Choose the media folder again and retry the scan.",
                "relative media root rejected",
            ));
        }
        let mut state = ScanState::new(scan_id);
        let mut publisher = ProgressPublisher::new(progress, 0);
        let mut progress_interval = tokio::time::interval(PROGRESS_TIME_THRESHOLD);
        progress_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        progress_interval.tick().await;
        let requested_roots = request.roots;
        let mut canonicalization =
            tokio::task::spawn_blocking(move || canonicalize_roots(&requested_roots));
        let roots = loop {
            tokio::select! {
                () = cancellation.cancelled() => {
                    repository.finish_scan(scan_id, ScanRunStatus::Cancelled).await?;
                    publisher.publish_terminal(state.progress()).await;
                    return Ok(state.summary(ScanStatus::Cancelled));
                }
                _ = progress_interval.tick() => publisher.publish_latest(state.progress()),
                result = &mut canonicalization => match result {
                    Ok(Ok(roots)) => break roots,
                    Ok(Err(error)) => {
                        repository.finish_scan(scan_id, ScanRunStatus::Failed).await?;
                        return Err(library_error(
                            "A media folder could not be read.",
                            "Check that the folder exists and that FishMuse has permission to read it.",
                            error,
                        ));
                    }
                    Err(error) => {
                        repository.finish_scan(scan_id, ScanRunStatus::Failed).await?;
                        return Err(library_error(
                            "The media scan could not start.",
                            "Try the scan again.",
                            error,
                        ));
                    }
                }
            }
        };
        for root in &roots {
            repository
                .upsert_root(&normalize_path_bytes(root), &native_path_bytes(root))
                .await?;
        }
        let normalized_roots: Vec<Vec<u8>> = roots
            .iter()
            .map(|root| normalize_path_bytes(root))
            .collect();

        let (discovery_tx, discovery_rx) = mpsc::unbounded_channel();
        let discovery = tokio::task::spawn_blocking(move || {
            visit_files_from_canonical_roots(&roots, |path| {
                discovery_tx.send(path).map_err(|_| {
                    io::Error::new(io::ErrorKind::Interrupted, "media discovery cancelled")
                })
            })
        });
        let mut candidate_paths = Vec::new();
        let mut diagnostics = Vec::new();
        match receive_discovery(
            discovery_rx,
            discovery,
            &cancellation,
            &mut progress_interval,
            &mut publisher,
            &mut state,
            &mut candidate_paths,
            &mut diagnostics,
        )
        .await
        {
            DiscoveryOutcome::Completed => {}
            DiscoveryOutcome::Cancelled => {
                repository
                    .finish_scan(scan_id, ScanRunStatus::Cancelled)
                    .await?;
                publisher.publish_terminal(state.progress()).await;
                return Ok(state.summary(ScanStatus::Cancelled));
            }
            DiscoveryOutcome::IoError(error) => {
                repository
                    .finish_scan(scan_id, ScanRunStatus::Failed)
                    .await?;
                return Err(library_error(
                    "A media folder could not be read.",
                    "Check that the folder exists and that FishMuse has permission to read it.",
                    error,
                ));
            }
            DiscoveryOutcome::TaskError(error) => {
                repository
                    .finish_scan(scan_id, ScanRunStatus::Failed)
                    .await?;
                return Err(library_error(
                    "The media scan could not start.",
                    "Try the scan again.",
                    error,
                ));
            }
        }
        candidate_paths.sort_by_cached_key(|path| normalize_path_bytes(path));

        let discovered_normalized_paths: HashSet<Vec<u8>> = candidate_paths
            .iter()
            .map(|path| normalize_path_bytes(path))
            .collect();
        let existing = ExistingAssets::new(
            repository.list_assets().await?,
            &discovered_normalized_paths,
            &normalized_roots,
        );
        let semaphore = Arc::new(Semaphore::new(self.concurrency_limit));
        let tag_reader = self.tag_reader.clone();
        let mut work = stream::iter(candidate_paths.into_iter().map(|path| {
            let tag_reader = tag_reader.clone();
            let semaphore = semaphore.clone();
            let existing = existing.clone();
            async move {
                let _permit = semaphore
                    .acquire_owned()
                    .await
                    .expect("scanner owns the semaphore for the duration of the scan");
                process_file(tag_reader.as_ref(), path, &existing).await
            }
        }))
        .buffer_unordered(self.concurrency_limit);

        let mut assets = Vec::new();
        let mut seen_asset_ids = HashSet::new();
        let mut cancelled = false;
        loop {
            let next = tokio::select! {
                () = cancellation.cancelled() => {
                    cancelled = true;
                    None
                }
                _ = progress_interval.tick() => {
                    publisher.publish_latest(state.progress());
                    continue;
                }
                next = work.next() => next,
            };
            let Some(result) = next else {
                break;
            };
            match result {
                ProcessedFile::Parsed(asset) => {
                    state.parsed += 1;
                    seen_asset_ids.insert(asset.media_asset_id);
                    assets.push(asset);
                }
                ProcessedFile::Unchanged {
                    media_asset_id,
                    write,
                } => {
                    state.unchanged += 1;
                    seen_asset_ids.insert(media_asset_id);
                    if let Some(asset) = write {
                        assets.push(asset);
                    }
                }
                ProcessedFile::Failed {
                    diagnostic: failure,
                    existing_media_asset_id,
                } => {
                    state.failed += 1;
                    if let Some(media_asset_id) = existing_media_asset_id {
                        seen_asset_ids.insert(media_asset_id);
                    }
                    diagnostics.push(failure);
                }
            }
            publisher.record_completed(state.progress());
            if assets.len() + diagnostics.len() >= self.batch_size {
                commit_pending(&repository, scan_id, &mut assets, &mut diagnostics).await?;
            }
        }

        commit_pending(&repository, scan_id, &mut assets, &mut diagnostics).await?;
        let status = if cancelled {
            repository
                .finish_scan(scan_id, ScanRunStatus::Cancelled)
                .await?;
            ScanStatus::Cancelled
        } else {
            let missing: Vec<MediaAssetId> = existing
                .all
                .iter()
                .filter(|asset| {
                    normalized_roots
                        .iter()
                        .any(|root| normalized_path_is_within_root(&asset.normalized_path, root))
                        && !seen_asset_ids.contains(&asset.media_asset_id)
                })
                .map(|asset| asset.media_asset_id)
                .collect();
            repository.mark_missing(&missing).await?;
            repository
                .finish_scan(scan_id, ScanRunStatus::Completed)
                .await?;
            ScanStatus::Completed
        };
        publisher.publish_terminal(state.progress()).await;
        Ok(state.summary(status))
    }
}

enum DiscoveryOutcome {
    Completed,
    Cancelled,
    IoError(io::Error),
    TaskError(tokio::task::JoinError),
}

#[allow(clippy::too_many_arguments)]
async fn receive_discovery(
    mut paths: UnboundedReceiver<PathBuf>,
    mut discovery: JoinHandle<io::Result<()>>,
    cancellation: &CancellationToken,
    progress_interval: &mut Interval,
    publisher: &mut ProgressPublisher,
    state: &mut ScanState,
    candidate_paths: &mut Vec<PathBuf>,
    diagnostics: &mut Vec<ScanDiagnosticWrite>,
) -> DiscoveryOutcome {
    let mut discovery_finished = false;
    let mut paths_closed = false;
    loop {
        if discovery_finished && paths_closed {
            return DiscoveryOutcome::Completed;
        }
        tokio::select! {
            () = cancellation.cancelled() => return DiscoveryOutcome::Cancelled,
            _ = progress_interval.tick() => publisher.publish_latest(state.progress()),
            path = paths.recv(), if !paths_closed => match path {
                Some(path) => match classify_extension(&path) {
                    ExtensionDisposition::Supported => {
                        state.discovered += 1;
                        candidate_paths.push(path);
                    }
                    ExtensionDisposition::Diagnostic(code) => {
                        state.discovered += 1;
                        state.failed += 1;
                        diagnostics.push(diagnostic(&path, code, code.as_str()));
                        publisher.record_completed(state.progress());
                    }
                    ExtensionDisposition::Ignored => {}
                },
                None => paths_closed = true,
            },
            result = &mut discovery, if !discovery_finished => {
                discovery_finished = true;
                match result {
                    Ok(Ok(())) => {}
                    Ok(Err(error)) => return DiscoveryOutcome::IoError(error),
                    Err(error) => return DiscoveryOutcome::TaskError(error),
                }
            }
        }
    }
}

struct ProgressPublisher {
    sender: Sender<ScanProgress>,
    completed_since_delivery: u64,
    pending: Option<ScanProgress>,
}

impl ProgressPublisher {
    const fn new(sender: Sender<ScanProgress>, completed_since_delivery: u64) -> Self {
        Self {
            sender,
            completed_since_delivery,
            pending: None,
        }
    }

    fn record_completed(&mut self, progress: ScanProgress) {
        self.completed_since_delivery += 1;
        if self.completed_since_delivery >= PROGRESS_ITEM_THRESHOLD {
            self.publish_latest(progress);
        }
    }

    fn publish_latest(&mut self, progress: ScanProgress) {
        self.pending = Some(progress);
        self.try_flush();
    }

    fn try_flush(&mut self) {
        let Some(progress) = self.pending.take() else {
            return;
        };
        match self.sender.try_send(progress) {
            Ok(()) => self.completed_since_delivery = 0,
            Err(TrySendError::Full(progress)) => self.pending = Some(progress),
            Err(TrySendError::Closed(_)) => self.completed_since_delivery = 0,
        }
    }

    async fn publish_terminal(self, progress: ScanProgress) {
        let _ = self.sender.send(progress).await;
    }
}

#[derive(Clone)]
struct ExistingAsset {
    stored: StoredMediaAsset,
    quick: Option<String>,
    full: Option<String>,
}

#[derive(Clone, Default)]
struct ExistingAssets {
    by_path: Arc<HashMap<Vec<u8>, ExistingAsset>>,
    move_candidates_by_full: Arc<Mutex<HashMap<String, VecDeque<ExistingAsset>>>>,
    all: Arc<Vec<StoredMediaAsset>>,
}

impl ExistingAssets {
    fn new(
        assets: Vec<StoredMediaAsset>,
        discovered_paths: &HashSet<Vec<u8>>,
        normalized_roots: &[Vec<u8>],
    ) -> Self {
        let mut by_path = HashMap::new();
        let mut move_candidates_by_full: HashMap<String, VecDeque<ExistingAsset>> = HashMap::new();
        for stored in &assets {
            let parsed = stored.identity.as_deref().and_then(parse_identity);
            let asset = ExistingAsset {
                stored: stored.clone(),
                quick: parsed.map(|(quick, _)| quick.to_owned()),
                full: parsed.map(|(_, full)| full.to_owned()),
            };
            let is_disappeared_in_scanned_root = normalized_roots
                .iter()
                .any(|root| normalized_path_is_within_root(&asset.stored.normalized_path, root))
                && !discovered_paths.contains(&asset.stored.normalized_path);
            if is_disappeared_in_scanned_root && let Some(full) = &asset.full {
                move_candidates_by_full
                    .entry(full.clone())
                    .or_default()
                    .push_back(asset.clone());
            }
            by_path.insert(asset.stored.normalized_path.clone(), asset);
        }
        Self {
            by_path: Arc::new(by_path),
            move_candidates_by_full: Arc::new(Mutex::new(move_candidates_by_full)),
            all: Arc::new(assets),
        }
    }

    fn take_move_candidate(&self, full_fingerprint: &str) -> Option<ExistingAsset> {
        self.move_candidates_by_full
            .lock()
            .expect("move candidates lock")
            .get_mut(full_fingerprint)
            .and_then(VecDeque::pop_front)
    }
}

enum ProcessedFile {
    Parsed(MediaAssetWrite),
    Unchanged {
        media_asset_id: MediaAssetId,
        write: Option<MediaAssetWrite>,
    },
    Failed {
        diagnostic: ScanDiagnosticWrite,
        existing_media_asset_id: Option<MediaAssetId>,
    },
}

async fn process_file<R: TagReader>(
    tag_reader: &R,
    path: PathBuf,
    existing: &ExistingAssets,
) -> ProcessedFile {
    let normalized_path = normalize_path_bytes(&path);
    let same_path = existing.by_path.get(&normalized_path);
    let existing_media_asset_id = same_path.map(|asset| asset.stored.media_asset_id);
    let full_path = path.clone();
    let identity = match tokio::task::spawn_blocking(move || FileIdentity::read(&full_path)).await {
        Ok(Ok(identity)) => identity,
        Ok(Err(error)) => {
            return failed_file(
                diagnostic_with_context(
                    &path,
                    DiagnosticCode::Unreadable,
                    "media file content could not be read",
                    error,
                ),
                existing_media_asset_id,
            );
        }
        Err(error) => {
            return failed_file(
                diagnostic_with_context(
                    &path,
                    DiagnosticCode::Unreadable,
                    "media fingerprint task failed",
                    error,
                ),
                existing_media_asset_id,
            );
        }
    };
    if let Some(asset) = same_path
        .filter(|asset| asset.full.as_deref() == Some(identity.content_fingerprint.as_str()))
    {
        let write = (asset.stored.availability != "available"
            || asset.quick.as_deref() != Some(identity.quick_fingerprint.as_str()))
        .then(|| asset_write(asset.stored.media_asset_id, &path, &identity));
        return ProcessedFile::Unchanged {
            media_asset_id: asset.stored.media_asset_id,
            write,
        };
    }
    if same_path.is_none()
        && let Some(moved) = existing.take_move_candidate(&identity.content_fingerprint)
    {
        return ProcessedFile::Unchanged {
            media_asset_id: moved.stored.media_asset_id,
            write: Some(asset_write(moved.stored.media_asset_id, &path, &identity)),
        };
    }

    match tag_reader.read(&path).await {
        Ok(_tags) => ProcessedFile::Parsed(asset_write(
            same_path.map_or_else(MediaAssetId::new, |asset| asset.stored.media_asset_id),
            &path,
            &identity,
        )),
        Err(failure) => failed_file(
            ScanDiagnosticWrite {
                path: Some(native_path_bytes(&path)),
                code: failure.code.as_str().to_owned(),
                message: failure.message,
            },
            existing_media_asset_id,
        ),
    }
}

fn asset_write(id: MediaAssetId, path: &Path, identity: &FileIdentity) -> MediaAssetWrite {
    asset_write_parts(
        id,
        path,
        &identity.quick_fingerprint,
        &identity.content_fingerprint,
    )
}

fn asset_write_parts(
    id: MediaAssetId,
    path: &Path,
    quick_fingerprint: &str,
    content_fingerprint: &str,
) -> MediaAssetWrite {
    MediaAssetWrite {
        media_asset_id: id,
        normalized_path: normalize_path_bytes(path),
        original_path: native_path_bytes(path),
        identity: format!("q:{quick_fingerprint}|f:{content_fingerprint}"),
    }
}

const fn failed_file(
    diagnostic: ScanDiagnosticWrite,
    existing_media_asset_id: Option<MediaAssetId>,
) -> ProcessedFile {
    ProcessedFile::Failed {
        diagnostic,
        existing_media_asset_id,
    }
}

fn parse_identity(value: &str) -> Option<(&str, &str)> {
    let (quick, full) = value.strip_prefix("q:")?.split_once("|f:")?;
    (!quick.is_empty() && !full.is_empty()).then_some((quick, full))
}

fn normalized_path_is_within_root(path: &[u8], root: &[u8]) -> bool {
    if path == root {
        return true;
    }
    let Some(suffix) = path.strip_prefix(root) else {
        return false;
    };
    root.ends_with(normalized_separator()) || suffix.starts_with(normalized_separator())
}

#[cfg(windows)]
const fn normalized_separator() -> &'static [u8] {
    &[b'\\', 0]
}

#[cfg(not(windows))]
const fn normalized_separator() -> &'static [u8] {
    &[b'/']
}

async fn commit_pending(
    repository: &SqliteScanRepository,
    scan_id: ScanId,
    assets: &mut Vec<MediaAssetWrite>,
    diagnostics: &mut Vec<ScanDiagnosticWrite>,
) -> AppResult<()> {
    if assets.is_empty() && diagnostics.is_empty() {
        return Ok(());
    }
    repository
        .commit_batch(scan_id, assets, diagnostics)
        .await?;
    assets.clear();
    diagnostics.clear();
    Ok(())
}

struct ScanState {
    scan_id: ScanId,
    discovered: u64,
    parsed: u64,
    unchanged: u64,
    failed: u64,
}

impl ScanState {
    const fn new(scan_id: ScanId) -> Self {
        Self {
            scan_id,
            discovered: 0,
            parsed: 0,
            unchanged: 0,
            failed: 0,
        }
    }

    const fn progress(&self) -> ScanProgress {
        ScanProgress {
            scan_id: self.scan_id,
            discovered: self.discovered,
            parsed: self.parsed,
            unchanged: self.unchanged,
            failed: self.failed,
        }
    }

    const fn summary(&self, status: ScanStatus) -> ScanSummary {
        ScanSummary {
            scan_id: self.scan_id,
            status,
            discovered: self.discovered,
            parsed: self.parsed,
            unchanged: self.unchanged,
            failed: self.failed,
        }
    }
}

fn diagnostic(path: &Path, code: DiagnosticCode, message: &str) -> ScanDiagnosticWrite {
    ScanDiagnosticWrite {
        path: Some(native_path_bytes(path)),
        code: code.as_str().to_owned(),
        message: message.to_owned(),
    }
}

fn diagnostic_with_context(
    path: &Path,
    code: DiagnosticCode,
    message: &str,
    _context: impl std::fmt::Display,
) -> ScanDiagnosticWrite {
    diagnostic(path, code, message)
}

#[cfg(windows)]
fn native_path_bytes(path: &Path) -> Vec<u8> {
    use std::os::windows::ffi::OsStrExt;

    path.as_os_str()
        .encode_wide()
        .flat_map(u16::to_le_bytes)
        .collect()
}

#[cfg(unix)]
fn native_path_bytes(path: &Path) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt;

    path.as_os_str().as_bytes().to_vec()
}

#[cfg(not(any(unix, windows)))]
fn native_path_bytes(path: &Path) -> Vec<u8> {
    path.as_os_str().to_string_lossy().as_bytes().to_vec()
}

fn library_error(
    user_message: &str,
    suggested_action: &str,
    technical_context: impl std::fmt::Display,
) -> AppError {
    AppError {
        code: ErrorCode::Unavailable,
        category: ErrorCategory::Library,
        user_message: user_message.to_owned(),
        retryable: true,
        suggested_action: Some(suggested_action.to_owned()),
        technical_context: Some(technical_context.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn progress_is_published_when_discovery_exceeds_250_ms() {
        let scan_id = ScanId::new();
        let (progress_tx, mut progress_rx) = mpsc::channel(4);
        let (path_tx, path_rx) = mpsc::unbounded_channel();
        let discovery = tokio::task::spawn_blocking(move || {
            path_tx
                .send(PathBuf::from("slow.flac"))
                .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "receiver closed"))?;
            std::thread::sleep(Duration::from_millis(600));
            Ok(())
        });
        let task = tokio::spawn(async move {
            let cancellation = CancellationToken::new();
            let mut interval = tokio::time::interval(PROGRESS_TIME_THRESHOLD);
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            interval.tick().await;
            let mut publisher = ProgressPublisher::new(progress_tx, 0);
            let mut state = ScanState::new(scan_id);
            let mut candidates = Vec::new();
            let mut diagnostics = Vec::new();
            let outcome = receive_discovery(
                path_rx,
                discovery,
                &cancellation,
                &mut interval,
                &mut publisher,
                &mut state,
                &mut candidates,
                &mut diagnostics,
            )
            .await;
            (outcome, state, candidates)
        });

        let interim = tokio::time::timeout(Duration::from_millis(450), progress_rx.recv())
            .await
            .expect("progress during discovery")
            .expect("progress sender open");
        assert_eq!(interim.discovered, 1);
        assert!(!task.is_finished(), "discovery must still be running");
        let (outcome, state, candidates) = task.await.expect("discovery task");
        assert!(matches!(outcome, DiscoveryOutcome::Completed));
        assert_eq!(state.discovered, 1);
        assert_eq!(candidates, vec![PathBuf::from("slow.flac")]);
    }

    #[tokio::test]
    async fn full_progress_channel_keeps_item_threshold_pending_until_delivery() {
        let scan_id = ScanId::new();
        let (sender, mut receiver) = mpsc::channel(1);
        sender
            .try_send(ScanProgress {
                scan_id,
                discovered: 0,
                parsed: 99,
                unchanged: 0,
                failed: 0,
            })
            .expect("prefill progress channel");
        let mut publisher = ProgressPublisher::new(sender, PROGRESS_ITEM_THRESHOLD - 1);
        let at_threshold = ScanProgress {
            scan_id,
            discovered: 100,
            parsed: 100,
            unchanged: 0,
            failed: 0,
        };
        publisher.record_completed(at_threshold);
        assert_eq!(publisher.completed_since_delivery, PROGRESS_ITEM_THRESHOLD);
        assert!(publisher.pending.is_some());

        receiver.recv().await.expect("remove prefilled event");
        publisher.record_completed(ScanProgress {
            discovered: 101,
            parsed: 101,
            ..at_threshold
        });
        let delivered = receiver.recv().await.expect("pending threshold progress");
        assert_eq!(delivered.parsed, 101);
        assert_eq!(publisher.completed_since_delivery, 0);
        assert!(publisher.pending.is_none());
    }
}
