use std::fmt;

use serde::{Deserialize, Serialize};

pub type AppResult<T> = Result<T, AppError>;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    InvalidInput,
    NotFound,
    BackendUnavailable,
    StorageFailure,
    Unauthorized,
    Unavailable,
    Internal,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCategory {
    Validation,
    Library,
    Storage,
    Playback,
    Ai,
    Configuration,
    Internal,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AppError {
    pub code: ErrorCode,
    pub category: ErrorCategory,
    pub user_message: String,
    pub retryable: bool,
    pub suggested_action: Option<String>,
    #[serde(skip_serializing, skip_deserializing)]
    pub technical_context: Option<String>,
}

impl fmt::Display for AppError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.user_message)
    }
}

impl std::error::Error for AppError {}
