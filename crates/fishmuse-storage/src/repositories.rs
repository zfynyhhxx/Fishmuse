use async_trait::async_trait;
use fishmuse_domain::{
    AppResult, ConversationId, DiscNumber, LibraryItem, ListenId, ListenSummary, MediaAssetId,
    PlayableSource, RecordingId, ReleaseId, ReleaseSummary, ScanId, TrackId, TrackNumber,
    TrackSummary, UserId,
};
use serde_json::Value;
use sqlx::{Row, SqlitePool, sqlite::SqliteRow};
use time::OffsetDateTime;
use unicode_normalization::UnicodeNormalization;
use uuid::Uuid;

use crate::database::storage_error;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScanRunStatus {
    Completed,
    Cancelled,
    Failed,
}

impl ScanRunStatus {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::Cancelled => "cancelled",
            Self::Failed => "failed",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoredMediaAsset {
    pub media_asset_id: MediaAssetId,
    pub normalized_path: Vec<u8>,
    pub original_path: Vec<u8>,
    pub identity: Option<String>,
    pub availability: String,
    pub projected: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MediaAssetWrite {
    pub media_asset_id: MediaAssetId,
    pub normalized_path: Vec<u8>,
    pub original_path: Vec<u8>,
    pub identity: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScanDiagnosticWrite {
    pub path: Option<Vec<u8>>,
    pub code: String,
    pub message: String,
}

#[async_trait]
pub trait ScanRepository: Send + Sync {
    async fn upsert_root(&self, normalized_path: &[u8], original_path: &[u8]) -> AppResult<()>;
    async fn begin_scan(&self) -> AppResult<ScanId>;
    async fn list_assets(&self) -> AppResult<Vec<StoredMediaAsset>>;
    async fn commit_batch(
        &self,
        scan_id: ScanId,
        assets: &[MediaAssetWrite],
        diagnostics: &[ScanDiagnosticWrite],
    ) -> AppResult<()>;
    async fn mark_missing(&self, media_asset_ids: &[MediaAssetId]) -> AppResult<()>;
    async fn finish_scan(&self, scan_id: ScanId, status: ScanRunStatus) -> AppResult<()>;
}

#[derive(Clone)]
pub struct SqliteScanRepository {
    pool: SqlitePool,
    user_id: UserId,
}

impl SqliteScanRepository {
    #[must_use]
    pub const fn new(pool: SqlitePool, user_id: UserId) -> Self {
        Self { pool, user_id }
    }

    fn user_text(&self) -> String {
        self.user_id.as_uuid().to_string()
    }
}

#[async_trait]
impl ScanRepository for SqliteScanRepository {
    async fn upsert_root(&self, normalized_path: &[u8], original_path: &[u8]) -> AppResult<()> {
        sqlx::query(
            "INSERT INTO media_roots(media_root_id, user_id, normalized_path, original_path, enabled) VALUES (?, ?, ?, ?, 1) ON CONFLICT(user_id, normalized_path) DO UPDATE SET original_path = excluded.original_path, enabled = 1",
        )
        .bind(Uuid::now_v7().to_string())
        .bind(self.user_text())
        .bind(normalized_path)
        .bind(original_path)
        .execute(&self.pool)
        .await
        .map_err(db_error)?;
        Ok(())
    }

    async fn begin_scan(&self) -> AppResult<ScanId> {
        let scan_id = ScanId::new();
        sqlx::query(
            "INSERT INTO scan_runs(scan_run_id, user_id, media_root_id, status, started_at) VALUES (?, ?, NULL, 'running', ?)",
        )
        .bind(scan_id.as_uuid().to_string())
        .bind(self.user_text())
        .bind(OffsetDateTime::now_utc().unix_timestamp())
        .execute(&self.pool)
        .await
        .map_err(db_error)?;
        Ok(scan_id)
    }

    async fn list_assets(&self) -> AppResult<Vec<StoredMediaAsset>> {
        let rows = sqlx::query(
            "SELECT media_asset_id, normalized_path, original_path, content_fingerprint, availability, track_id IS NOT NULL AS projected FROM media_assets WHERE user_id = ?",
        )
        .bind(self.user_text())
        .fetch_all(&self.pool)
        .await
        .map_err(db_error)?;
        rows.into_iter()
            .map(|row| {
                let id: String = row.try_get("media_asset_id").map_err(db_error)?;
                Ok(StoredMediaAsset {
                    media_asset_id: MediaAssetId::try_from_uuid(parse_uuid(&id)?)
                        .map_err(|error| storage_error("storage_failure", error))?,
                    normalized_path: row.try_get("normalized_path").map_err(db_error)?,
                    original_path: row.try_get("original_path").map_err(db_error)?,
                    identity: row.try_get("content_fingerprint").map_err(db_error)?,
                    availability: row.try_get("availability").map_err(db_error)?,
                    projected: row.try_get::<i64, _>("projected").map_err(db_error)? != 0,
                })
            })
            .collect()
    }

    async fn commit_batch(
        &self,
        scan_id: ScanId,
        assets: &[MediaAssetWrite],
        diagnostics: &[ScanDiagnosticWrite],
    ) -> AppResult<()> {
        let mut transaction = self.pool.begin().await.map_err(db_error)?;
        for asset in assets {
            let updated = sqlx::query(
                "UPDATE media_assets SET normalized_path = ?, original_path = ?, content_fingerprint = ?, availability = 'available' WHERE media_asset_id = ? AND user_id = ?",
            )
            .bind(&asset.normalized_path)
            .bind(&asset.original_path)
            .bind(&asset.identity)
            .bind(asset.media_asset_id.as_uuid().to_string())
            .bind(self.user_text())
            .execute(&mut *transaction)
            .await
            .map_err(db_error)?;
            if updated.rows_affected() == 0 {
                sqlx::query(
                    "INSERT INTO media_assets(media_asset_id, user_id, track_id, normalized_path, original_path, content_fingerprint, availability) VALUES (?, ?, NULL, ?, ?, ?, 'available')",
                )
                .bind(asset.media_asset_id.as_uuid().to_string())
                .bind(self.user_text())
                .bind(&asset.normalized_path)
                .bind(&asset.original_path)
                .bind(&asset.identity)
                .execute(&mut *transaction)
                .await
                .map_err(db_error)?;
            }
        }
        for diagnostic in diagnostics {
            sqlx::query(
                "INSERT INTO scan_diagnostics(diagnostic_id, user_id, scan_run_id, path, code, message, created_at) VALUES (?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(Uuid::now_v7().to_string())
            .bind(self.user_text())
            .bind(scan_id.as_uuid().to_string())
            .bind(&diagnostic.path)
            .bind(&diagnostic.code)
            .bind(&diagnostic.message)
            .bind(OffsetDateTime::now_utc().unix_timestamp())
            .execute(&mut *transaction)
            .await
            .map_err(db_error)?;
        }
        transaction.commit().await.map_err(db_error)?;
        Ok(())
    }

    async fn mark_missing(&self, media_asset_ids: &[MediaAssetId]) -> AppResult<()> {
        let mut transaction = self.pool.begin().await.map_err(db_error)?;
        for media_asset_id in media_asset_ids {
            sqlx::query(
                "UPDATE media_assets SET availability = 'missing' WHERE media_asset_id = ? AND user_id = ?",
            )
            .bind(media_asset_id.as_uuid().to_string())
            .bind(self.user_text())
            .execute(&mut *transaction)
            .await
            .map_err(db_error)?;
        }
        transaction.commit().await.map_err(db_error)?;
        Ok(())
    }

    async fn finish_scan(&self, scan_id: ScanId, status: ScanRunStatus) -> AppResult<()> {
        let result = sqlx::query(
            "UPDATE scan_runs SET status = ?, completed_at = ? WHERE scan_run_id = ? AND user_id = ?",
        )
        .bind(status.as_str())
        .bind(OffsetDateTime::now_utc().unix_timestamp())
        .bind(scan_id.as_uuid().to_string())
        .bind(self.user_text())
        .execute(&self.pool)
        .await
        .map_err(db_error)?;
        if result.rows_affected() != 1 {
            return Err(storage_error("storage_failure", "scan run not found"));
        }
        Ok(())
    }
}

#[async_trait]
pub trait LibraryRepository: Send + Sync {
    async fn list_tracks(&self) -> AppResult<Vec<LibraryItem>>;
    async fn find_track(&self, track_id: TrackId) -> AppResult<Option<LibraryItem>>;
}

#[async_trait]
pub trait ListeningRepository: Send + Sync {
    async fn append(&self, listen: &ListenSummary) -> AppResult<()>;
    async fn recent(&self, limit: u32) -> AppResult<Vec<ListenSummary>>;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConversationMessage {
    pub id: Uuid,
    pub role: String,
    pub content: String,
    pub created_at: OffsetDateTime,
}

#[async_trait]
pub trait ConversationRepository: Send + Sync {
    async fn create(&self, id: ConversationId, title: Option<&str>) -> AppResult<()>;
    async fn append_message(
        &self,
        conversation_id: ConversationId,
        role: &str,
        content: &str,
    ) -> AppResult<Uuid>;
    async fn messages(
        &self,
        conversation_id: ConversationId,
    ) -> AppResult<Vec<ConversationMessage>>;
}

#[async_trait]
pub trait SettingsRepository: Send + Sync {
    async fn get(&self, key: &str) -> AppResult<Option<Value>>;
    async fn set(&self, key: &str, value: &Value) -> AppResult<()>;
}

#[derive(Clone)]
pub struct SqliteLibraryRepository {
    pool: SqlitePool,
    user_id: UserId,
}

impl SqliteLibraryRepository {
    #[must_use]
    pub const fn new(pool: SqlitePool, user_id: UserId) -> Self {
        Self { pool, user_id }
    }

    #[must_use]
    pub const fn user_id(&self) -> UserId {
        self.user_id
    }

    pub async fn search_tracks(
        &self,
        text: &str,
        artist: Option<&str>,
        release: Option<&str>,
        limit: u32,
        offset: u32,
    ) -> AppResult<Vec<TrackSummary>> {
        let limit = effective_limit(limit);
        let offset = offset.min(1_000_000);
        let normalized_text = normalize_search_text(text);
        let normalized_artist = artist.map(normalize_search_text);
        let normalized_release = release.map(normalize_search_text);
        let artist_pattern = normalized_artist.as_deref().map(like_contains_pattern);
        let release_pattern = normalized_release.as_deref().map(like_contains_pattern);
        let rows = if normalized_text.is_empty() {
            sqlx::query(
                "SELECT tracks.track_id, tracks.recording_id, tracks.release_id, tracks.title, tracks.duration_ms, tracks.disc_number, tracks.track_number, tracks.playable, releases.title AS release_title FROM tracks LEFT JOIN releases ON releases.release_id = tracks.release_id AND releases.user_id = tracks.user_id WHERE tracks.user_id = ? AND (? IS NULL OR EXISTS (SELECT 1 FROM track_artists JOIN artists ON artists.user_id = track_artists.user_id AND artists.artist_id = track_artists.artist_id WHERE track_artists.user_id = tracks.user_id AND track_artists.track_id = tracks.track_id AND artists.normalized_name LIKE ? ESCAPE '\\')) AND (? IS NULL OR releases.normalized_title LIKE ? ESCAPE '\\') ORDER BY tracks.imported_at DESC, tracks.track_id LIMIT ? OFFSET ?",
            )
            .bind(self.user_text())
            .bind(normalized_artist.as_deref())
            .bind(artist_pattern.as_deref())
            .bind(normalized_release.as_deref())
            .bind(release_pattern.as_deref())
            .bind(i64::from(limit))
            .bind(i64::from(offset))
            .fetch_all(&self.pool)
            .await
            .map_err(db_error)?
        } else {
            let Some(match_expression) = fts_match_expression(&normalized_text) else {
                return Ok(Vec::new());
            };
            let prefix_pattern = format!("{}%", escape_like(&normalized_text));
            if normalized_artist.is_none() && normalized_release.is_none() {
                sqlx::query(
                    "WITH ranked AS (SELECT library_fts.user_id, library_fts.track_id, library_fts.artist, library_fts.title, CASE WHEN library_fts.normalized_title = ? THEN 1 ELSE 0 END AS exact_title, CASE WHEN library_fts.normalized_title LIKE ? ESCAPE '\\' THEN 1 ELSE 0 END AS prefix_title, bm25(library_fts, 10.0, 5.0, 2.0) AS relevance FROM library_fts WHERE library_fts MATCH ? AND library_fts.user_id = ? ORDER BY exact_title DESC, prefix_title DESC, relevance, library_fts.artist COLLATE NOCASE, library_fts.title COLLATE NOCASE, library_fts.track_id LIMIT ? OFFSET ?) SELECT tracks.track_id, tracks.recording_id, tracks.release_id, tracks.title, tracks.duration_ms, tracks.disc_number, tracks.track_number, tracks.playable, releases.title AS release_title FROM ranked JOIN tracks ON tracks.user_id = ranked.user_id AND tracks.track_id = ranked.track_id LEFT JOIN releases ON releases.release_id = tracks.release_id AND releases.user_id = tracks.user_id ORDER BY ranked.exact_title DESC, ranked.prefix_title DESC, ranked.relevance, ranked.artist COLLATE NOCASE, ranked.title COLLATE NOCASE, ranked.track_id",
                )
                .bind(&normalized_text)
                .bind(prefix_pattern)
                .bind(match_expression)
                .bind(self.user_text())
                .bind(i64::from(limit))
                .bind(i64::from(offset))
                .fetch_all(&self.pool)
                .await
                .map_err(db_error)?
            } else {
                sqlx::query(
                    "SELECT tracks.track_id, tracks.recording_id, tracks.release_id, tracks.title, tracks.duration_ms, tracks.disc_number, tracks.track_number, tracks.playable, releases.title AS release_title FROM library_fts JOIN tracks ON tracks.user_id = library_fts.user_id AND tracks.track_id = library_fts.track_id LEFT JOIN releases ON releases.release_id = tracks.release_id AND releases.user_id = tracks.user_id WHERE library_fts MATCH ? AND library_fts.user_id = ? AND tracks.user_id = ? AND (? IS NULL OR EXISTS (SELECT 1 FROM track_artists JOIN artists ON artists.user_id = track_artists.user_id AND artists.artist_id = track_artists.artist_id WHERE track_artists.user_id = tracks.user_id AND track_artists.track_id = tracks.track_id AND artists.normalized_name LIKE ? ESCAPE '\\')) AND (? IS NULL OR releases.normalized_title LIKE ? ESCAPE '\\') ORDER BY CASE WHEN COALESCE(NULLIF(tracks.normalized_title, ''), lower(trim(tracks.title))) = ? THEN 1 ELSE 0 END DESC, CASE WHEN COALESCE(NULLIF(tracks.normalized_title, ''), lower(trim(tracks.title))) LIKE ? ESCAPE '\\' THEN 1 ELSE 0 END DESC, bm25(library_fts, 10.0, 5.0, 2.0), library_fts.artist COLLATE NOCASE, tracks.title COLLATE NOCASE, tracks.track_id LIMIT ? OFFSET ?",
                )
                .bind(match_expression)
                .bind(self.user_text())
                .bind(self.user_text())
                .bind(normalized_artist.as_deref())
                .bind(artist_pattern.as_deref())
                .bind(normalized_release.as_deref())
                .bind(release_pattern.as_deref())
                .bind(&normalized_text)
                .bind(prefix_pattern)
                .bind(i64::from(limit))
                .bind(i64::from(offset))
                .fetch_all(&self.pool)
                .await
                .map_err(db_error)?
            }
        };
        let mut tracks = Vec::with_capacity(rows.len());
        for row in rows {
            tracks.push(self.item_from_row(row).await?.track);
        }
        Ok(tracks)
    }

    pub async fn recent_listens(&self, limit: u32) -> AppResult<Vec<ListenSummary>> {
        SqliteListeningRepository::new(self.pool.clone(), self.user_id)
            .recent(effective_limit(limit))
            .await
    }

    async fn item_from_row(&self, row: SqliteRow) -> AppResult<LibraryItem> {
        let track_id = parse_track_id(row.try_get("track_id").map_err(db_error)?)?;
        let recording_id = parse_recording_id(row.try_get("recording_id").map_err(db_error)?)?;
        let release_id = row
            .try_get::<Option<String>, _>("release_id")
            .map_err(db_error)?
            .map(|value| parse_release_id(&value))
            .transpose()?;
        let release_title = row
            .try_get::<Option<String>, _>("release_title")
            .map_err(db_error)?;

        let artist_names = sqlx::query_scalar::<_, String>(
            "SELECT artists.name FROM artists JOIN track_artists ON track_artists.artist_id = artists.artist_id AND track_artists.user_id = artists.user_id WHERE track_artists.user_id = ? AND track_artists.track_id = ? ORDER BY track_artists.position, artists.name",
        )
        .bind(self.user_text())
        .bind(track_id.as_uuid().to_string())
        .fetch_all(&self.pool)
        .await
        .map_err(db_error)?;

        let duration_ms = optional_u64(&row, "duration_ms")?;
        let disc_number = optional_one_based(&row, "disc_number", DiscNumber::new)?;
        let track_number = optional_one_based(&row, "track_number", TrackNumber::new)?;
        let playable = row.try_get::<i64, _>("playable").map_err(db_error)? != 0;

        let track = TrackSummary {
            id: track_id,
            recording_id,
            title: row.try_get("title").map_err(db_error)?,
            artist_names: artist_names.clone(),
            release_title: release_title.clone(),
            duration_ms,
            disc_number,
            track_number,
            playable,
        };
        let release = match (release_id, release_title) {
            (Some(id), Some(title)) => Some(ReleaseSummary {
                id,
                title,
                artist_names,
            }),
            (None, None) => None,
            _ => {
                return Err(storage_error(
                    "storage_failure",
                    "inconsistent release projection",
                ));
            }
        };

        Ok(LibraryItem {
            track,
            release,
            provenance: vec![fishmuse_domain::SourceProvenance::Local],
        })
    }

    fn user_text(&self) -> String {
        self.user_id.as_uuid().to_string()
    }

    pub async fn playable_source(&self, track_id: TrackId) -> AppResult<Option<PlayableSource>> {
        let row = sqlx::query(
            "SELECT media_asset_id, subsong_index, start_ms, end_ms FROM media_assets WHERE user_id = ? AND track_id = ? AND availability = 'available' ORDER BY media_asset_id LIMIT 1",
        )
        .bind(self.user_text())
        .bind(track_id.as_uuid().to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(db_error)?;
        row.map(|row| {
            let media_asset_id: String = row.try_get("media_asset_id").map_err(db_error)?;
            let subsong_index: Option<i64> = row.try_get("subsong_index").map_err(db_error)?;
            let start_ms: Option<i64> = row.try_get("start_ms").map_err(db_error)?;
            let end_ms: Option<i64> = row.try_get("end_ms").map_err(db_error)?;
            Ok(PlayableSource {
                track_id,
                media_asset_id: MediaAssetId::try_from_uuid(parse_uuid(&media_asset_id)?)
                    .map_err(|error| storage_error("storage_failure", error))?,
                subsong_index: subsong_index
                    .map(u32::try_from)
                    .transpose()
                    .map_err(|error| storage_error("storage_failure", error))?,
                start_ms: start_ms
                    .map(u64::try_from)
                    .transpose()
                    .map_err(|error| storage_error("storage_failure", error))?,
                end_ms: end_ms
                    .map(u64::try_from)
                    .transpose()
                    .map_err(|error| storage_error("storage_failure", error))?,
            })
        })
        .transpose()
    }
}

const fn effective_limit(limit: u32) -> u32 {
    if limit == 0 {
        20
    } else if limit > 100 {
        100
    } else {
        limit
    }
}

fn normalize_search_text(value: &str) -> String {
    value
        .nfkc()
        .flat_map(char::to_lowercase)
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn fts_match_expression(value: &str) -> Option<String> {
    let terms: Vec<String> = value
        .split_whitespace()
        .filter(|term| term.chars().any(char::is_alphanumeric))
        .map(|term| format!("\"{}\"*", term.replace('"', "\"\"")))
        .collect();
    (!terms.is_empty()).then(|| terms.join(" "))
}

fn escape_like(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

fn like_contains_pattern(value: &str) -> String {
    format!("%{}%", escape_like(value))
}

#[async_trait]
impl LibraryRepository for SqliteLibraryRepository {
    async fn list_tracks(&self) -> AppResult<Vec<LibraryItem>> {
        let rows = sqlx::query(
            "SELECT tracks.track_id, tracks.recording_id, tracks.release_id, tracks.title, tracks.duration_ms, tracks.disc_number, tracks.track_number, tracks.playable, releases.title AS release_title FROM tracks LEFT JOIN releases ON releases.release_id = tracks.release_id AND releases.user_id = tracks.user_id WHERE tracks.user_id = ? ORDER BY tracks.title, tracks.track_id",
        )
        .bind(self.user_text())
        .fetch_all(&self.pool)
        .await
        .map_err(db_error)?;
        let mut items = Vec::with_capacity(rows.len());
        for row in rows {
            items.push(self.item_from_row(row).await?);
        }
        Ok(items)
    }

    async fn find_track(&self, track_id: TrackId) -> AppResult<Option<LibraryItem>> {
        let row = sqlx::query(
            "SELECT tracks.track_id, tracks.recording_id, tracks.release_id, tracks.title, tracks.duration_ms, tracks.disc_number, tracks.track_number, tracks.playable, releases.title AS release_title FROM tracks LEFT JOIN releases ON releases.release_id = tracks.release_id AND releases.user_id = tracks.user_id WHERE tracks.user_id = ? AND tracks.track_id = ?",
        )
        .bind(self.user_text())
        .bind(track_id.as_uuid().to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(db_error)?;
        match row {
            Some(row) => self.item_from_row(row).await.map(Some),
            None => Ok(None),
        }
    }
}

#[derive(Clone)]
pub struct SqliteListeningRepository {
    pool: SqlitePool,
    user_id: UserId,
}

impl SqliteListeningRepository {
    #[must_use]
    pub const fn new(pool: SqlitePool, user_id: UserId) -> Self {
        Self { pool, user_id }
    }
}

#[async_trait]
impl ListeningRepository for SqliteListeningRepository {
    async fn append(&self, listen: &ListenSummary) -> AppResult<()> {
        let listened_ms = i64::try_from(listen.listened_ms)
            .map_err(|error| storage_error("storage_failure", error))?;
        sqlx::query("INSERT INTO listening_events(listen_id, user_id, track_id, started_at, ended_at, listened_ms, completed, interrupted) VALUES (?, ?, ?, ?, ?, ?, ?, 0)")
            .bind(listen.id.as_uuid().to_string())
            .bind(self.user_id.as_uuid().to_string())
            .bind(listen.track_id.as_uuid().to_string())
            .bind(listen.started_at.unix_timestamp())
            .bind(listen.started_at.unix_timestamp())
            .bind(listened_ms)
            .bind(listen.completed)
            .execute(&self.pool)
            .await
            .map_err(db_error)?;
        Ok(())
    }

    async fn recent(&self, limit: u32) -> AppResult<Vec<ListenSummary>> {
        let rows = sqlx::query("SELECT listen_id, track_id, started_at, listened_ms, completed FROM listening_events WHERE user_id = ? ORDER BY started_at DESC, listen_id DESC LIMIT ?")
            .bind(self.user_id.as_uuid().to_string())
            .bind(i64::from(limit))
            .fetch_all(&self.pool)
            .await
            .map_err(db_error)?;
        rows.into_iter()
            .map(|row| {
                let listened_ms = row.try_get::<i64, _>("listened_ms").map_err(db_error)?;
                Ok(ListenSummary {
                    id: parse_listen_id(row.try_get("listen_id").map_err(db_error)?)?,
                    track_id: parse_track_id(row.try_get("track_id").map_err(db_error)?)?,
                    started_at: OffsetDateTime::from_unix_timestamp(
                        row.try_get("started_at").map_err(db_error)?,
                    )
                    .map_err(|error| storage_error("storage_failure", error))?,
                    listened_ms: u64::try_from(listened_ms)
                        .map_err(|error| storage_error("storage_failure", error))?,
                    completed: row.try_get::<i64, _>("completed").map_err(db_error)? != 0,
                })
            })
            .collect()
    }
}

#[derive(Clone)]
pub struct SqliteConversationRepository {
    pool: SqlitePool,
    user_id: UserId,
}

impl SqliteConversationRepository {
    #[must_use]
    pub const fn new(pool: SqlitePool, user_id: UserId) -> Self {
        Self { pool, user_id }
    }
}

#[async_trait]
impl ConversationRepository for SqliteConversationRepository {
    async fn create(&self, id: ConversationId, title: Option<&str>) -> AppResult<()> {
        sqlx::query(
            "INSERT INTO conversations(conversation_id, user_id, title, created_at) \
             VALUES (?, ?, ?, ?) \
             ON CONFLICT(conversation_id) DO NOTHING",
        )
        .bind(id.as_uuid().to_string())
        .bind(self.user_id.as_uuid().to_string())
        .bind(title)
        .bind(OffsetDateTime::now_utc().unix_timestamp())
        .execute(&self.pool)
        .await
        .map_err(db_error)?;
        Ok(())
    }

    async fn append_message(
        &self,
        conversation_id: ConversationId,
        role: &str,
        content: &str,
    ) -> AppResult<Uuid> {
        let id = Uuid::now_v7();
        sqlx::query("INSERT INTO conversation_messages(message_id, user_id, conversation_id, role, content, created_at) VALUES (?, ?, ?, ?, ?, ?)")
            .bind(id.to_string())
            .bind(self.user_id.as_uuid().to_string())
            .bind(conversation_id.as_uuid().to_string())
            .bind(role)
            .bind(content)
            .bind(OffsetDateTime::now_utc().unix_timestamp())
            .execute(&self.pool)
            .await
            .map_err(db_error)?;
        Ok(id)
    }

    async fn messages(
        &self,
        conversation_id: ConversationId,
    ) -> AppResult<Vec<ConversationMessage>> {
        let rows = sqlx::query("SELECT message_id, role, content, created_at FROM conversation_messages WHERE user_id = ? AND conversation_id = ? ORDER BY created_at, message_id")
            .bind(self.user_id.as_uuid().to_string())
            .bind(conversation_id.as_uuid().to_string())
            .fetch_all(&self.pool)
            .await
            .map_err(db_error)?;
        rows.into_iter()
            .map(|row| {
                let id = Uuid::parse_str(row.try_get("message_id").map_err(db_error)?)
                    .map_err(|error| storage_error("storage_failure", error))?;
                if id.get_version_num() != 7 {
                    return Err(storage_error(
                        "storage_failure",
                        "stored message ID is not UUID v7",
                    ));
                }
                Ok(ConversationMessage {
                    id,
                    role: row.try_get("role").map_err(db_error)?,
                    content: row.try_get("content").map_err(db_error)?,
                    created_at: OffsetDateTime::from_unix_timestamp(
                        row.try_get("created_at").map_err(db_error)?,
                    )
                    .map_err(|error| storage_error("storage_failure", error))?,
                })
            })
            .collect()
    }
}

#[derive(Clone)]
pub struct SqliteSettingsRepository {
    pool: SqlitePool,
    user_id: UserId,
}

impl SqliteSettingsRepository {
    #[must_use]
    pub const fn new(pool: SqlitePool, user_id: UserId) -> Self {
        Self { pool, user_id }
    }
}

#[async_trait]
impl SettingsRepository for SqliteSettingsRepository {
    async fn get(&self, key: &str) -> AppResult<Option<Value>> {
        let value = sqlx::query_scalar::<_, String>(
            "SELECT value_json FROM app_settings WHERE user_id = ? AND key = ?",
        )
        .bind(self.user_id.as_uuid().to_string())
        .bind(key)
        .fetch_optional(&self.pool)
        .await
        .map_err(db_error)?;
        value
            .map(|json| {
                serde_json::from_str(&json).map_err(|error| storage_error("storage_failure", error))
            })
            .transpose()
    }

    async fn set(&self, key: &str, value: &Value) -> AppResult<()> {
        let json = serde_json::to_string(value)
            .map_err(|error| storage_error("storage_failure", error))?;
        sqlx::query("INSERT INTO app_settings(user_id, key, value_json) VALUES (?, ?, ?) ON CONFLICT(user_id, key) DO UPDATE SET value_json = excluded.value_json")
            .bind(self.user_id.as_uuid().to_string())
            .bind(key)
            .bind(json)
            .execute(&self.pool)
            .await
            .map_err(db_error)?;
        Ok(())
    }
}

fn optional_u64(row: &SqliteRow, column: &str) -> AppResult<Option<u64>> {
    row.try_get::<Option<i64>, _>(column)
        .map_err(db_error)?
        .map(|value| u64::try_from(value).map_err(|error| storage_error("storage_failure", error)))
        .transpose()
}

fn optional_one_based<T>(
    row: &SqliteRow,
    column: &str,
    constructor: impl FnOnce(u32) -> Option<T>,
) -> AppResult<Option<T>> {
    let Some(value) = row.try_get::<Option<i64>, _>(column).map_err(db_error)? else {
        return Ok(None);
    };
    let value = u32::try_from(value).map_err(|error| storage_error("storage_failure", error))?;
    constructor(value)
        .map(Some)
        .ok_or_else(|| storage_error("storage_failure", "stored track number must be positive"))
}

fn parse_uuid(value: &str) -> AppResult<Uuid> {
    Uuid::parse_str(value).map_err(|error| storage_error("storage_failure", error))
}

fn parse_track_id(value: &str) -> AppResult<TrackId> {
    TrackId::try_from_uuid(parse_uuid(value)?)
        .map_err(|error| storage_error("storage_failure", error))
}

fn parse_recording_id(value: &str) -> AppResult<RecordingId> {
    RecordingId::try_from_uuid(parse_uuid(value)?)
        .map_err(|error| storage_error("storage_failure", error))
}

fn parse_release_id(value: &str) -> AppResult<ReleaseId> {
    ReleaseId::try_from_uuid(parse_uuid(value)?)
        .map_err(|error| storage_error("storage_failure", error))
}

fn parse_listen_id(value: &str) -> AppResult<ListenId> {
    ListenId::try_from_uuid(parse_uuid(value)?)
        .map_err(|error| storage_error("storage_failure", error))
}

fn db_error(error: sqlx::Error) -> fishmuse_domain::AppError {
    storage_error("storage_failure", error)
}
