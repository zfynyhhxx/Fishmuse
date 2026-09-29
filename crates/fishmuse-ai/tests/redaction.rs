use fishmuse_ai::redact_for_ai;
use serde_json::json;

#[test]
fn removes_paths_keys_technical_context_and_database_only_fields_recursively() {
    let input = json!({
        "title": "Safe title",
        "path": "C:\\Music\\secret.flac",
        "nested": {
            "api_key": "fixture-secret",
            "apiKey": "camel-secret",
            "technical_context": "SQL failed at C:\\Users\\name",
            "technicalContext": "camel technical detail",
            "normalized_path": [67, 58, 92],
            "original_path": [67, 58, 92],
            "raw_tags_json": {"private": true},
            "content_fingerprint": "internal-only",
            "safe": "visible"
        },
        "rows": [{"title": "ok", "database_path": "D:\\db.sqlite"}]
    });

    let output = redact_for_ai(input);
    let encoded = output.to_string();
    assert_eq!(output["title"], "Safe title");
    assert_eq!(output["nested"]["safe"], "visible");
    for forbidden in [
        "secret.flac",
        "fixture-secret",
        "camel-secret",
        "technical_context",
        "camel technical detail",
        "normalized_path",
        "raw_tags_json",
        "internal-only",
        "db.sqlite",
    ] {
        assert!(
            !encoded.contains(forbidden),
            "leaked {forbidden}: {encoded}"
        );
    }
}

#[test]
fn replaces_absolute_path_strings_even_under_unexpected_keys() {
    let output = redact_for_ai(json!({
        "note": "C:\\private\\track.wav",
        "unix": "/home/person/music.flac",
        "unc": "\\\\server\\share\\music.flac",
        "embedded": "failure while reading C:\\private\\embedded.flac",
        "relative": "artist/album/track"
    }));
    assert_eq!(output["note"], "[REDACTED]");
    assert_eq!(output["unix"], "[REDACTED]");
    assert_eq!(output["unc"], "[REDACTED]");
    assert_eq!(output["embedded"], "[REDACTED]");
    assert_eq!(output["relative"], "artist/album/track");
}
