use std::time::Duration;

use async_trait::async_trait;
use fishmuse_domain::{AppError, AppResult, ErrorCategory, ErrorCode, OperationId, UserId};
use fishmuse_playback::{CommandFingerprint, OperationClaim, OperationStore, PlaybackSnapshot};
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};
use time::OffsetDateTime;

use crate::database::storage_error;

const DEFAULT_ORPHAN_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Clone)]
pub struct SqliteOperationStore {
    pool: SqlitePool,
    user_id: UserId,
    orphan_timeout: Duration,
}

impl SqliteOperationStore {
    #[must_use]
    pub const fn new(pool: SqlitePool, user_id: UserId) -> Self {
        Self::with_orphan_timeout(pool, user_id, DEFAULT_ORPHAN_TIMEOUT)
    }

    #[must_use]
    pub const fn with_orphan_timeout(
        pool: SqlitePool,
        user_id: UserId,
        orphan_timeout: Duration,
    ) -> Self {
        Self {
            pool,
            user_id,
            orphan_timeout,
        }
    }

    fn user_text(&self) -> String {
        self.user_id.as_uuid().to_string()
    }
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
enum StoredOperation {
    Pending {
        fingerprint: String,
    },
    Succeeded {
        fingerprint: String,
        snapshot: PlaybackSnapshot,
    },
    Failed {
        fingerprint: String,
        error: AppError,
    },
}

impl StoredOperation {
    fn fingerprint(&self) -> &str {
        match self {
            Self::Pending { fingerprint }
            | Self::Succeeded { fingerprint, .. }
            | Self::Failed { fingerprint, .. } => fingerprint,
        }
    }
}

#[async_trait]
impl OperationStore for SqliteOperationStore {
    async fn try_begin(
        &self,
        operation_id: OperationId,
        command_fingerprint: CommandFingerprint,
    ) -> AppResult<OperationClaim> {
        let fingerprint = command_fingerprint.to_hex();
        let pending = StoredOperation::Pending {
            fingerprint: fingerprint.clone(),
        };
        let pending_json = serde_json::to_string(&pending).map_err(db_error)?;
        let now = now_millis()?;
        let mut transaction = self.pool.begin().await.map_err(db_error)?;
        let inserted = sqlx::query(
            "INSERT OR IGNORE INTO applied_operations(operation_id, user_id, operation_kind, result_json, completed_at) VALUES (?, ?, 'playback', ?, ?)",
        )
        .bind(operation_id.as_uuid().to_string())
        .bind(self.user_text())
        .bind(&pending_json)
        .bind(now)
        .execute(&mut *transaction)
        .await
        .map_err(db_error)?;
        if inserted.rows_affected() == 1 {
            transaction.commit().await.map_err(db_error)?;
            return Ok(OperationClaim::Acquired);
        }

        let row = sqlx::query(
            "SELECT operation_kind, result_json, completed_at FROM applied_operations WHERE operation_id = ? AND user_id = ?",
        )
        .bind(operation_id.as_uuid().to_string())
        .bind(self.user_text())
        .fetch_optional(&mut *transaction)
        .await
        .map_err(db_error)?
        .ok_or_else(|| storage_error("storage_failure", "operation belongs to another user"))?;
        let kind: String = row.try_get("operation_kind").map_err(db_error)?;
        if kind != "playback" {
            transaction.commit().await.map_err(db_error)?;
            return Ok(OperationClaim::Conflict);
        }
        let result_json: String = row.try_get("result_json").map_err(db_error)?;
        let stored: StoredOperation = serde_json::from_str(&result_json).map_err(db_error)?;
        if stored.fingerprint() != fingerprint {
            transaction.commit().await.map_err(db_error)?;
            return Ok(OperationClaim::Conflict);
        }

        match stored {
            StoredOperation::Pending { .. } => {
                let claimed_at: i64 = row.try_get("completed_at").map_err(db_error)?;
                let timeout_ms = i64::try_from(self.orphan_timeout.as_millis()).unwrap_or(i64::MAX);
                if now.saturating_sub(claimed_at) < timeout_ms {
                    transaction.commit().await.map_err(db_error)?;
                    return Ok(OperationClaim::InFlight);
                }
                let error = interrupted_error();
                let failed_json = serde_json::to_string(&StoredOperation::Failed {
                    fingerprint,
                    error: error.clone(),
                })
                .map_err(db_error)?;
                sqlx::query(
                    "UPDATE applied_operations SET result_json = ?, completed_at = ? WHERE operation_id = ? AND user_id = ? AND result_json = ?",
                )
                .bind(failed_json)
                .bind(now)
                .bind(operation_id.as_uuid().to_string())
                .bind(self.user_text())
                .bind(result_json)
                .execute(&mut *transaction)
                .await
                .map_err(db_error)?;
                transaction.commit().await.map_err(db_error)?;
                Err(error)
            }
            StoredOperation::Succeeded { snapshot, .. } => {
                transaction.commit().await.map_err(db_error)?;
                Ok(OperationClaim::Completed(snapshot))
            }
            StoredOperation::Failed { error, .. } => {
                transaction.commit().await.map_err(db_error)?;
                Err(error)
            }
        }
    }

    async fn complete(
        &self,
        operation_id: OperationId,
        result: &AppResult<PlaybackSnapshot>,
    ) -> AppResult<()> {
        let current: String = sqlx::query_scalar(
            "SELECT result_json FROM applied_operations WHERE operation_id = ? AND user_id = ?",
        )
        .bind(operation_id.as_uuid().to_string())
        .bind(self.user_text())
        .fetch_one(&self.pool)
        .await
        .map_err(db_error)?;
        let stored: StoredOperation = serde_json::from_str(&current).map_err(db_error)?;
        let StoredOperation::Pending { fingerprint } = stored else {
            return Err(storage_error(
                "storage_failure",
                "operation is already terminal",
            ));
        };
        let completed = match result {
            Ok(snapshot) => StoredOperation::Succeeded {
                fingerprint,
                snapshot: snapshot.clone(),
            },
            Err(error) => StoredOperation::Failed {
                fingerprint,
                error: safe_error(error),
            },
        };
        let completed_json = serde_json::to_string(&completed).map_err(db_error)?;
        let updated = sqlx::query(
            "UPDATE applied_operations SET result_json = ?, completed_at = ? WHERE operation_id = ? AND user_id = ? AND result_json = ?",
        )
        .bind(completed_json)
        .bind(now_millis()?)
        .bind(operation_id.as_uuid().to_string())
        .bind(self.user_text())
        .bind(current)
        .execute(&self.pool)
        .await
        .map_err(db_error)?;
        if updated.rows_affected() != 1 {
            return Err(storage_error(
                "storage_failure",
                "operation completion race",
            ));
        }
        Ok(())
    }
}

fn safe_error(error: &AppError) -> AppError {
    AppError {
        code: error.code,
        category: error.category,
        user_message: error.user_message.clone(),
        retryable: error.retryable,
        suggested_action: error.suggested_action.clone(),
        technical_context: None,
    }
}

fn interrupted_error() -> AppError {
    AppError {
        code: ErrorCode::BackendUnavailable,
        category: ErrorCategory::Playback,
        user_message: "operation_interrupted".to_owned(),
        retryable: false,
        suggested_action: Some("refresh_playback_state".to_owned()),
        technical_context: None,
    }
}

fn now_millis() -> AppResult<i64> {
    i64::try_from(OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000_000)
        .map_err(|error| storage_error("storage_failure", error))
}

fn db_error(error: impl std::fmt::Display) -> AppError {
    storage_error("storage_failure", error)
}
