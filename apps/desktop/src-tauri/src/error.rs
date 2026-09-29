use fishmuse_domain::{AppError, ErrorCategory, ErrorCode};
use serde::Serialize;

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CommandError {
    pub code: ErrorCode,
    pub category: ErrorCategory,
    pub user_message: String,
    pub retryable: bool,
    pub suggested_action: Option<String>,
}

impl From<AppError> for CommandError {
    fn from(error: AppError) -> Self {
        if let Some(context) = error.technical_context.as_deref() {
            let safe_context =
                fishmuse_ai::redact_for_ai(serde_json::Value::String(context.to_owned()));
            tracing::warn!(
                error_code = ?error.code,
                error_category = ?error.category,
                technical_context = %safe_context,
                "application command failed"
            );
        }
        Self {
            code: error.code,
            category: error.category,
            user_message: error.user_message,
            retryable: error.retryable,
            suggested_action: error.suggested_action,
        }
    }
}

pub fn invalid_input(message: &str) -> CommandError {
    CommandError {
        code: ErrorCode::InvalidInput,
        category: ErrorCategory::Validation,
        user_message: message.to_owned(),
        retryable: false,
        suggested_action: None,
    }
}

pub fn unavailable(category: ErrorCategory, message: &str) -> CommandError {
    CommandError {
        code: ErrorCode::Unavailable,
        category,
        user_message: message.to_owned(),
        retryable: true,
        suggested_action: None,
    }
}
