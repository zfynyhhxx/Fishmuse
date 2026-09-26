use std::{fmt, path::Path, time::Duration};

use fishmuse_domain::{AppError, AppResult, ErrorCategory, ErrorCode, UserId};
use sqlx::{
    Row, SqlitePool,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions},
};
use uuid::Uuid;

use crate::migrations;

const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

pub struct Database {
    pool: SqlitePool,
}

impl fmt::Debug for Database {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("Database").finish_non_exhaustive()
    }
}

impl Database {
    pub async fn open(path: &Path) -> AppResult<Self> {
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|error| storage_error("storage_failure", error))?;
        }

        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .foreign_keys(true)
            .journal_mode(SqliteJournalMode::Wal)
            .busy_timeout(BUSY_TIMEOUT);
        Self::open_with_options(options, 5).await
    }

    pub async fn open_in_memory() -> AppResult<Self> {
        let options = SqliteConnectOptions::new()
            .in_memory(true)
            .foreign_keys(true)
            .busy_timeout(BUSY_TIMEOUT);
        Self::open_with_options(options, 1).await
    }

    async fn open_with_options(
        options: SqliteConnectOptions,
        max_connections: u32,
    ) -> AppResult<Self> {
        let pool = match SqlitePoolOptions::new()
            .max_connections(max_connections)
            .connect_with(options)
            .await
        {
            Ok(pool) => pool,
            Err(error) => return Err(open_error(error)),
        };

        let integrity = sqlx::query("PRAGMA integrity_check")
            .fetch_one(&pool)
            .await
            .and_then(|row| row.try_get::<String, _>(0));
        match integrity {
            Ok(result) if result == "ok" => {}
            Ok(result) => {
                pool.close().await;
                return Err(storage_error("database_corrupt", result));
            }
            Err(error) => {
                pool.close().await;
                return Err(open_error(error));
            }
        }

        let migration_result = async {
            let mut transaction = pool
                .begin()
                .await
                .map_err(|error| storage_error("storage_failure", error))?;
            migrations::migrate(&mut transaction).await?;
            transaction
                .commit()
                .await
                .map_err(|error| storage_error("storage_failure", error))
        }
        .await;
        if let Err(error) = migration_result {
            pool.close().await;
            return Err(error);
        }

        Ok(Self { pool })
    }

    #[must_use]
    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    pub async fn close(self) {
        self.pool.close().await;
    }

    pub async fn ensure_local_user(&self) -> AppResult<UserId> {
        let candidate = UserId::new();
        sqlx::query(
            "INSERT OR IGNORE INTO users(user_id, local_slot, created_at) VALUES (?, 1, ?)",
        )
        .bind(candidate.as_uuid().to_string())
        .bind(time::OffsetDateTime::now_utc().unix_timestamp())
        .execute(&self.pool)
        .await
        .map_err(|error| storage_error("storage_failure", error))?;

        let stored: String = sqlx::query_scalar("SELECT user_id FROM users WHERE local_slot = 1")
            .fetch_one(&self.pool)
            .await
            .map_err(|error| storage_error("storage_failure", error))?;
        parse_user_id(&stored)
    }

    pub async fn create_backup(&self, destination: &Path) -> AppResult<()> {
        if destination.exists() {
            return Err(storage_error(
                "storage_failure",
                "backup destination already exists",
            ));
        }
        let parent = destination
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .ok_or_else(|| storage_error("storage_failure", "backup has no parent directory"))?;
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|error| storage_error("storage_failure", error))?;

        let file_name = destination
            .file_name()
            .ok_or_else(|| storage_error("storage_failure", "backup has no file name"))?
            .to_string_lossy();
        let partial = parent.join(format!(".{file_name}.{}.partial", Uuid::now_v7()));
        let partial_text = partial
            .to_str()
            .ok_or_else(|| storage_error("storage_failure", "backup path is not valid Unicode"))?;

        let backup_result = sqlx::query("VACUUM main INTO ?")
            .bind(partial_text)
            .execute(&self.pool)
            .await;
        if let Err(error) = backup_result {
            let _ = tokio::fs::remove_file(&partial).await;
            return Err(storage_error("storage_failure", error));
        }

        if let Err(error) = tokio::fs::rename(&partial, destination).await {
            let _ = tokio::fs::remove_file(&partial).await;
            return Err(storage_error("storage_failure", error));
        }

        Ok(())
    }
}

fn parse_user_id(value: &str) -> AppResult<UserId> {
    let uuid = Uuid::parse_str(value).map_err(|error| storage_error("storage_failure", error))?;
    UserId::try_from_uuid(uuid).map_err(|error| storage_error("storage_failure", error))
}

fn open_error(error: sqlx::Error) -> AppError {
    let context = error.to_string().to_ascii_lowercase();
    let message = if context.contains("not a database")
        || context.contains("database disk image is malformed")
        || context.contains("database corrupt")
    {
        "database_corrupt"
    } else {
        "storage_failure"
    };
    storage_error(message, error)
}

pub(crate) fn storage_error(message: &str, error: impl fmt::Display) -> AppError {
    AppError {
        code: ErrorCode::StorageFailure,
        category: ErrorCategory::Storage,
        user_message: message.to_owned(),
        retryable: false,
        suggested_action: None,
        technical_context: Some(error.to_string()),
    }
}
