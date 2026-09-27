use async_trait::async_trait;
use fishmuse_domain::{AppResult, ListenId, TrackId, UserId};
use fishmuse_playback::{ListeningEvent, ListeningSink};
use sqlx::{Row, SqlitePool};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::database::storage_error;

#[derive(Clone)]
pub struct SqliteListeningSink {
    pool: SqlitePool,
    user_id: UserId,
}

impl SqliteListeningSink {
    #[must_use]
    pub const fn new(pool: SqlitePool, user_id: UserId) -> Self {
        Self { pool, user_id }
    }

    pub async fn get_recent_listens(&self, limit: u32) -> AppResult<Vec<ListeningEvent>> {
        let limit = if limit == 0 { 20 } else { limit.min(100) };
        let rows = sqlx::query(
            "SELECT listen_id, track_id, started_at, ended_at, listened_ms, completed, interrupted FROM listening_events WHERE user_id = ? ORDER BY started_at DESC, listen_id DESC LIMIT ?",
        )
        .bind(self.user_text())
        .bind(i64::from(limit))
        .fetch_all(&self.pool)
        .await
        .map_err(db_error)?;
        rows.into_iter().map(event_from_row).collect()
    }

    fn user_text(&self) -> String {
        self.user_id.as_uuid().to_string()
    }
}

#[async_trait]
impl ListeningSink for SqliteListeningSink {
    async fn append(&self, event: ListeningEvent) -> AppResult<()> {
        let listened_ms = i64::try_from(event.listened_ms).map_err(db_error)?;
        let result = sqlx::query(
            "INSERT INTO listening_events(listen_id, user_id, track_id, started_at, ended_at, listened_ms, completed, interrupted) VALUES (?, ?, ?, ?, ?, ?, ?, ?) ON CONFLICT(listen_id) DO UPDATE SET ended_at = excluded.ended_at, listened_ms = excluded.listened_ms, completed = excluded.completed, interrupted = excluded.interrupted WHERE listening_events.user_id = excluded.user_id AND listening_events.track_id = excluded.track_id",
        )
        .bind(event.id.as_uuid().to_string())
        .bind(self.user_text())
        .bind(event.track_id.as_uuid().to_string())
        .bind(event.started_at.unix_timestamp())
        .bind(event.ended_at.map(OffsetDateTime::unix_timestamp))
        .bind(listened_ms)
        .bind(event.completed)
        .bind(event.interrupted)
        .execute(&self.pool)
        .await
        .map_err(db_error)?;
        if result.rows_affected() != 1 {
            return Err(storage_error(
                "storage_failure",
                "listen ownership conflict",
            ));
        }
        Ok(())
    }

    async fn recover_interrupted(&self, ended_at: OffsetDateTime) -> AppResult<()> {
        sqlx::query(
            "UPDATE listening_events SET ended_at = ?, interrupted = 1 WHERE user_id = ? AND ended_at IS NULL",
        )
        .bind(ended_at.unix_timestamp())
        .bind(self.user_text())
        .execute(&self.pool)
        .await
        .map_err(db_error)?;
        Ok(())
    }
}

fn event_from_row(row: sqlx::sqlite::SqliteRow) -> AppResult<ListeningEvent> {
    let listen_id: String = row.try_get("listen_id").map_err(db_error)?;
    let track_id: String = row.try_get("track_id").map_err(db_error)?;
    let listened_ms: i64 = row.try_get("listened_ms").map_err(db_error)?;
    Ok(ListeningEvent {
        id: ListenId::try_from_uuid(parse_uuid(&listen_id)?)
            .map_err(|error| storage_error("storage_failure", error))?,
        track_id: TrackId::try_from_uuid(parse_uuid(&track_id)?)
            .map_err(|error| storage_error("storage_failure", error))?,
        started_at: timestamp(row.try_get("started_at").map_err(db_error)?)?,
        ended_at: row
            .try_get::<Option<i64>, _>("ended_at")
            .map_err(db_error)?
            .map(timestamp)
            .transpose()?,
        listened_ms: u64::try_from(listened_ms).map_err(db_error)?,
        completed: row.try_get::<bool, _>("completed").map_err(db_error)?,
        interrupted: row.try_get::<bool, _>("interrupted").map_err(db_error)?,
    })
}

fn timestamp(value: i64) -> AppResult<OffsetDateTime> {
    OffsetDateTime::from_unix_timestamp(value).map_err(db_error)
}

fn parse_uuid(value: &str) -> AppResult<Uuid> {
    Uuid::parse_str(value).map_err(db_error)
}

fn db_error(error: impl std::fmt::Display) -> fishmuse_domain::AppError {
    storage_error("storage_failure", error)
}
