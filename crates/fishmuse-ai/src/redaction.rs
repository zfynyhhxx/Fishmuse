use serde_json::{Map, Value};

const BLOCKED_KEYS: [&str; 10] = [
    "path",
    "normalizedpath",
    "originalpath",
    "databasepath",
    "apikey",
    "credential",
    "technicalcontext",
    "rawtagsjson",
    "contentfingerprint",
    "sqliterow",
];

#[must_use]
pub fn redact_for_ai(value: Value) -> Value {
    match value {
        Value::Object(object) => Value::Object(redact_object(object)),
        Value::Array(values) => Value::Array(values.into_iter().map(redact_for_ai).collect()),
        Value::String(value) if looks_like_absolute_path(&value) => {
            Value::String("[REDACTED]".to_owned())
        }
        other => other,
    }
}

fn redact_object(object: Map<String, Value>) -> Map<String, Value> {
    object
        .into_iter()
        .filter(|(key, _)| {
            let normalized: String = key
                .chars()
                .filter(|character| character.is_ascii_alphanumeric())
                .map(|character| character.to_ascii_lowercase())
                .collect();
            !BLOCKED_KEYS.contains(&normalized.as_str())
        })
        .map(|(key, value)| (key, redact_for_ai(value)))
        .collect()
}

fn looks_like_absolute_path(value: &str) -> bool {
    let bytes = value.as_bytes();
    let boundary = |index: usize| {
        index == 0
            || bytes[index - 1].is_ascii_whitespace()
            || matches!(bytes[index - 1], b'\'' | b'"' | b'(')
    };
    let windows_drive = (0..bytes.len().saturating_sub(2)).any(|index| {
        boundary(index)
            && bytes[index].is_ascii_alphabetic()
            && bytes[index + 1] == b':'
            && matches!(bytes[index + 2], b'\\' | b'/')
    });
    let unc = (0..bytes.len().saturating_sub(1))
        .any(|index| boundary(index) && bytes[index] == b'\\' && bytes[index + 1] == b'\\');
    let unix = (0..bytes.len()).any(|index| {
        boundary(index)
            && bytes[index] == b'/'
            && bytes.get(index + 1).is_some_and(|next| *next != b'/')
    });
    windows_drive || unc || unix
}
