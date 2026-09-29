use fishmuse_domain::{AppError, AppResult, ErrorCategory, ErrorCode, TrackId, UserId};
use fishmuse_library::{LibraryQueryPort, SearchQuery};
use serde_json::Value;

pub(super) async fn execute(
    library: &dyn LibraryQueryPort,
    user_id: UserId,
    name: &str,
    arguments: &Value,
) -> AppResult<Value> {
    match name {
        "search_library" => {
            let query = SearchQuery {
                text: string(arguments, "query")?.to_owned(),
                artist: optional_string(arguments, "artist")?.map(str::to_owned),
                release: optional_string(arguments, "release")?.map(str::to_owned),
                limit: u32_value(arguments, "limit")?,
                offset: 0,
            };
            serde_json::to_value(library.search(user_id, query).await?).map_err(serialization_error)
        }
        "get_library_item" => {
            let track_id = track_id(arguments)?;
            serde_json::to_value(library.get_item(user_id, track_id).await?)
                .map_err(serialization_error)
        }
        "get_recent_listens" => serde_json::to_value(
            library
                .recent_listens(user_id, u32_value(arguments, "limit")?)
                .await?,
        )
        .map_err(serialization_error),
        _ => Err(internal("invalid library tool dispatch")),
    }
}

pub(super) fn string<'a>(arguments: &'a Value, key: &str) -> AppResult<&'a str> {
    arguments
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| internal("validated string argument missing"))
}

fn optional_string<'a>(arguments: &'a Value, key: &str) -> AppResult<Option<&'a str>> {
    arguments
        .get(key)
        .map(|value| {
            value
                .as_str()
                .ok_or_else(|| internal("validated string argument invalid"))
        })
        .transpose()
}

fn u32_value(arguments: &Value, key: &str) -> AppResult<u32> {
    arguments
        .get(key)
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .ok_or_else(|| internal("validated integer argument missing"))
}

pub(super) fn track_id(arguments: &Value) -> AppResult<TrackId> {
    let value = string(arguments, "track_id")?;
    let uuid = uuid::Uuid::parse_str(value).map_err(|_| internal("validated track ID invalid"))?;
    TrackId::try_from_uuid(uuid).map_err(|_| internal("validated track ID is not UUID v7"))
}

pub(super) fn internal(context: &str) -> AppError {
    AppError {
        code: ErrorCode::Internal,
        category: ErrorCategory::Ai,
        user_message: "The music action could not be completed.".to_owned(),
        retryable: false,
        suggested_action: None,
        technical_context: Some(context.to_owned()),
    }
}

fn serialization_error(error: serde_json::Error) -> AppError {
    internal(&format!("safe tool result serialization failed: {error}"))
}
