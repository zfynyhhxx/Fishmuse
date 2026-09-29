use fishmuse_domain::{AppError, AppResult, ErrorCategory, ErrorCode, OperationId, UserId};
use fishmuse_library::LibraryQueryPort;
use fishmuse_playback::{PlaybackCommand, PlaybackManager};
use serde_json::Value;

use super::library::{internal, string, track_id};

pub(super) async fn execute(
    library: &dyn LibraryQueryPort,
    playback: &PlaybackManager,
    user_id: UserId,
    name: &str,
    arguments: &Value,
) -> AppResult<Value> {
    if name == "get_playback_state" {
        return serde_json::to_value(playback.snapshot().await?).map_err(serialization_error);
    }
    let operation_id = operation_id(arguments)?;
    let command = match name {
        "play_track" => {
            let track_id = track_id(arguments)?;
            let source = library
                .playable_source(user_id, track_id)
                .await?
                .ok_or_else(not_playable)?;
            PlaybackCommand::Play {
                source,
                operation_id,
            }
        }
        "pause_playback" => PlaybackCommand::Pause { operation_id },
        "resume_playback" => PlaybackCommand::Resume { operation_id },
        "seek_playback" => PlaybackCommand::Seek {
            position_ms: arguments
                .get("position_ms")
                .and_then(Value::as_u64)
                .ok_or_else(|| internal("validated seek position missing"))?,
            operation_id,
        },
        "skip_next" => PlaybackCommand::SkipNext { operation_id },
        _ => return Err(internal("invalid playback tool dispatch")),
    };
    serde_json::to_value(playback.execute(command).await?).map_err(serialization_error)
}

fn operation_id(arguments: &Value) -> AppResult<OperationId> {
    let value = string(arguments, "operation_id")?;
    let uuid =
        uuid::Uuid::parse_str(value).map_err(|_| internal("generated operation ID invalid"))?;
    OperationId::try_from_uuid(uuid).map_err(|_| internal("generated operation ID is not UUID v7"))
}

fn not_playable() -> AppError {
    AppError {
        code: ErrorCode::NotFound,
        category: ErrorCategory::Library,
        user_message: "This track is not currently playable.".to_owned(),
        retryable: false,
        suggested_action: Some("Rescan the folder or choose another track.".to_owned()),
        technical_context: None,
    }
}

fn serialization_error(error: serde_json::Error) -> AppError {
    internal(&format!("playback snapshot serialization failed: {error}"))
}
