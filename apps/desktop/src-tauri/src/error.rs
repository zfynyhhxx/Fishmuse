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
            #[cfg(feature = "live-e2e")]
            record_live_diagnostic(error.code, error.category, &safe_context);
            tracing::warn!(
                error_code = ?error.code,
                error_category = ?error.category,
                technical_context = %safe_context,
                "application command failed"
            );
        }
        let (user_message, suggested_action) = if error.category == ErrorCategory::Playback {
            let message = match error.code {
                ErrorCode::InvalidInput => "The playback request is invalid.",
                ErrorCode::NotFound => "The selected track is unavailable.",
                _ => "Playback control is unavailable. Please retry or open advanced diagnostics.",
            };
            let action = error.retryable.then(|| "retry_playback".to_owned());
            (message.to_owned(), action)
        } else {
            (error.user_message, error.suggested_action)
        };
        Self {
            code: error.code,
            category: error.category,
            user_message,
            retryable: error.retryable,
            suggested_action,
        }
    }
}

#[cfg(feature = "live-e2e")]
fn record_live_diagnostic(code: ErrorCode, category: ErrorCategory, context: &serde_json::Value) {
    use std::io::Write;

    let Ok(path) = std::env::var("FISHMUSE_LIVE_DIAGNOSTICS") else {
        return;
    };
    let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    else {
        return;
    };
    let diagnostic = serde_json::json!({
        "code": code,
        "category": category,
        "context": context,
    });
    let _ = writeln!(file, "{diagnostic}");
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

#[cfg(test)]
mod tests {
    use fishmuse_domain::{AppError, ErrorCategory, ErrorCode};

    use super::CommandError;

    #[test]
    fn playback_errors_use_closed_safe_copy_and_actions() {
        let error = CommandError::from(AppError {
            code: ErrorCode::BackendUnavailable,
            category: ErrorCategory::Playback,
            user_message: "foobar2000 failed at C:\\private\\track.flac".to_owned(),
            retryable: true,
            suggested_action: Some("restart_foobar2000".to_owned()),
            technical_context: Some("C:\\private\\track.flac".to_owned()),
        });

        let json = serde_json::to_string(&error).expect("serialize command error");
        assert_eq!(
            error.user_message,
            "Playback control is unavailable. Please retry or open advanced diagnostics."
        );
        assert_eq!(error.suggested_action.as_deref(), Some("retry_playback"));
        assert!(!json.to_ascii_lowercase().contains("foobar"));
        assert!(!json.contains("private"));
    }
}
