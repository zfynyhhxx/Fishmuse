use async_trait::async_trait;
use fishmuse_domain::{
    AppResult, ConversationId, DiscNumber, LibraryItem, ListenId, ListenSummary, RecordingId,
    ReleaseId, ReleaseSummary, TrackId, TrackNumber, TrackSummary, UserId,
};
use serde_json::Value;
use sqlx::{Row, SqlitePool, sqlite::SqliteRow};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::database::storage_error;

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
        sqlx::query("INSERT INTO listening_events(listen_id, user_id, track_id, started_at, listened_ms, completed) VALUES (?, ?, ?, ?, ?, ?)")
            .bind(listen.id.as_uuid().to_string())
            .bind(self.user_id.as_uuid().to_string())
            .bind(listen.track_id.as_uuid().to_string())
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
        sqlx::query("INSERT INTO conversations(conversation_id, user_id, title, created_at) VALUES (?, ?, ?, ?)")
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
