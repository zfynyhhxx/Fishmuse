use fishmuse_domain::AppResult;
use sqlx::{Executor, Sqlite, Transaction};

use crate::database::storage_error;

const SCHEMA_VERSION: i64 = 1;
const INITIAL_SCHEMA: &str = include_str!("../migrations/0001_initial.sql");

pub(crate) async fn migrate(transaction: &mut Transaction<'_, Sqlite>) -> AppResult<()> {
    let version: i64 = sqlx::query_scalar("PRAGMA user_version")
        .fetch_one(&mut **transaction)
        .await
        .map_err(|error| storage_error("storage_failure", error))?;

    match version {
        0 => {
            sqlx::raw_sql(INITIAL_SCHEMA)
                .execute(&mut **transaction)
                .await
                .map_err(|error| storage_error("storage_failure", error))?;
            transaction
                .execute("PRAGMA user_version = 1")
                .await
                .map_err(|error| storage_error("storage_failure", error))?;
        }
        SCHEMA_VERSION => {}
        unsupported => {
            return Err(storage_error(
                "storage_failure",
                format!("unsupported schema version {unsupported}"),
            ));
        }
    }

    Ok(())
}
