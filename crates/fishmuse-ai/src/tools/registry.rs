use std::sync::Arc;

use async_trait::async_trait;
use fishmuse_domain::{AppError, AppResult, ErrorCategory, ErrorCode, OperationId, TrackId};
use serde_json::{Value, json};

use crate::AITool;

pub const TOOL_NAMES: [&str; 9] = [
    "search_library",
    "get_library_item",
    "get_recent_listens",
    "get_playback_state",
    "play_track",
    "pause_playback",
    "resume_playback",
    "seek_playback",
    "skip_next",
];

#[async_trait]
pub trait ToolExecutor: Send + Sync {
    async fn execute(&self, name: &str, arguments: &Value) -> AppResult<Value>;
}

#[derive(Clone)]
pub struct ToolRegistry {
    executor: Arc<dyn ToolExecutor>,
    max_search_results: u32,
}

impl ToolRegistry {
    #[must_use]
    pub fn new<E>(executor: Arc<E>) -> Self
    where
        E: ToolExecutor + 'static,
    {
        Self {
            executor,
            max_search_results: 20,
        }
    }

    #[must_use]
    pub fn with_max_search_results(mut self, limit: u32) -> Self {
        self.max_search_results = limit.clamp(1, 20);
        self
    }

    #[must_use]
    pub fn definitions(&self) -> Vec<AITool> {
        TOOL_NAMES
            .iter()
            .map(|name| definition(name, self.max_search_results))
            .collect()
    }

    #[must_use]
    pub fn allows(&self, name: &str) -> bool {
        TOOL_NAMES.contains(&name)
    }

    pub async fn execute(&self, name: &str, arguments: &Value) -> AppResult<Value> {
        if !self.allows(name) {
            return Err(tool_error("unknown tool"));
        }
        let arguments = validate_and_prepare(name, arguments, self.max_search_results)?;
        self.executor.execute(name, &arguments).await
    }
}

fn definition(name: &str, max_search_results: u32) -> AITool {
    let (properties, required) = match name {
        "search_library" => (
            json!({
                "query": {"type": "string"},
                "artist": {"type": "string"},
                "release": {"type": "string"},
                "limit": {"type": "integer", "minimum": 1, "maximum": max_search_results}
            }),
            json!(["query"]),
        ),
        "get_library_item" | "play_track" => (
            json!({"track_id": {"type": "string", "format": "uuid"}}),
            json!(["track_id"]),
        ),
        "get_recent_listens" => (
            json!({"limit": {"type": "integer", "minimum": 1, "maximum": max_search_results}}),
            json!([]),
        ),
        "seek_playback" => (
            json!({"position_ms": {"type": "integer", "minimum": 0}}),
            json!(["position_ms"]),
        ),
        _ => (json!({}), json!([])),
    };
    AITool {
        name: name.to_owned(),
        description: format!("FishMuse music tool: {name}"),
        parameters: json!({
            "type": "object",
            "properties": properties,
            "required": required,
            "additionalProperties": false
        }),
    }
}

fn validate_and_prepare(
    name: &str,
    arguments: &Value,
    max_search_results: u32,
) -> AppResult<Value> {
    let object = arguments
        .as_object()
        .ok_or_else(|| tool_error("tool arguments must be an object"))?;
    let allowed: &[&str] = match name {
        "search_library" => &["query", "artist", "release", "limit"],
        "get_library_item" | "play_track" => &["track_id"],
        "get_recent_listens" => &["limit"],
        "seek_playback" => &["position_ms"],
        _ => &[],
    };
    if object.keys().any(|key| !allowed.contains(&key.as_str())) {
        return Err(tool_error("tool arguments contain an unknown property"));
    }
    let mut prepared = object.clone();
    match name {
        "search_library" => {
            required_string(object, "query")?;
            optional_string(object, "artist")?;
            optional_string(object, "release")?;
            let limit = optional_positive_u32(object, "limit")?
                .unwrap_or(max_search_results)
                .min(max_search_results);
            prepared.insert("limit".to_owned(), Value::from(limit));
        }
        "get_library_item" | "play_track" => validate_track_id(object)?,
        "get_recent_listens" => {
            let limit = optional_positive_u32(object, "limit")?
                .unwrap_or(max_search_results)
                .min(max_search_results);
            prepared.insert("limit".to_owned(), Value::from(limit));
        }
        "seek_playback" => {
            object
                .get("position_ms")
                .and_then(Value::as_u64)
                .ok_or_else(|| tool_error("position_ms must be an unsigned integer"))?;
        }
        _ => {}
    }
    if matches!(
        name,
        "play_track" | "pause_playback" | "resume_playback" | "seek_playback" | "skip_next"
    ) {
        prepared.insert(
            "operation_id".to_owned(),
            Value::String(OperationId::new().as_uuid().to_string()),
        );
    }
    Ok(Value::Object(prepared))
}

fn required_string<'a>(
    object: &'a serde_json::Map<String, Value>,
    key: &str,
) -> AppResult<&'a str> {
    object
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| tool_error("required string argument is missing"))
}

fn optional_string(object: &serde_json::Map<String, Value>, key: &str) -> AppResult<()> {
    if object
        .get(key)
        .is_some_and(|value| value.as_str().is_none())
    {
        return Err(tool_error("optional string argument has the wrong type"));
    }
    Ok(())
}

fn optional_positive_u32(
    object: &serde_json::Map<String, Value>,
    key: &str,
) -> AppResult<Option<u32>> {
    let Some(value) = object.get(key) else {
        return Ok(None);
    };
    let value = value
        .as_u64()
        .and_then(|value| u32::try_from(value).ok())
        .filter(|value| *value > 0)
        .ok_or_else(|| tool_error("limit must be a positive integer"))?;
    Ok(Some(value))
}

fn validate_track_id(object: &serde_json::Map<String, Value>) -> AppResult<()> {
    let value = required_string(object, "track_id")?;
    let uuid = uuid::Uuid::parse_str(value).map_err(|_| tool_error("track_id must be a UUID"))?;
    TrackId::try_from_uuid(uuid).map_err(|_| tool_error("track_id must be UUID v7"))?;
    Ok(())
}

fn tool_error(context: &str) -> AppError {
    AppError {
        code: ErrorCode::InvalidInput,
        category: ErrorCategory::Ai,
        user_message: "The requested music action is not allowed.".to_owned(),
        retryable: false,
        suggested_action: None,
        technical_context: Some(context.to_owned()),
    }
}
