use std::time::Duration;

use fishmuse_ai::{
    AIEvent, AIProviderError, DeepSeekEventDecoder, classify_http_status, decode_sse_fixture,
};

fn fixture(name: &str) -> &'static str {
    match name {
        "text" => include_str!("fixtures/responses/text.jsonl"),
        "tool" => include_str!("fixtures/responses/tool.jsonl"),
        "unknown" => include_str!("fixtures/responses/unknown.jsonl"),
        "duplicate" => include_str!("fixtures/responses/duplicate.jsonl"),
        "malformed" => include_str!("fixtures/responses/malformed.jsonl"),
        _ => panic!("unknown fixture"),
    }
}

#[test]
fn decodes_text_usage_and_completion() {
    let events = decode_sse_fixture(fixture("text")).expect("valid fixture");

    assert!(matches!(&events[0], AIEvent::TextDelta(value) if value == "Hello "));
    assert!(matches!(&events[1], AIEvent::TextDelta(value) if value == "FishMuse"));
    assert!(matches!(&events[2], AIEvent::Usage(usage)
        if usage.input_tokens == 12 && usage.cached_input_tokens == 4 && usage.output_tokens == 7));
    assert!(matches!(&events[3], AIEvent::Completed(id) if id.as_str() == "resp_01"));
}

#[test]
fn only_releases_a_tool_call_after_arguments_done() {
    let mut decoder = DeepSeekEventDecoder::new();
    let mut emitted = Vec::new();

    for line in fixture("tool")
        .lines()
        .filter(|line| !line.trim().is_empty())
        .take(3)
    {
        emitted.extend(decoder.decode_json_line(line).expect("valid event"));
    }
    assert!(emitted.iter().any(
        |event| matches!(event, AIEvent::ToolCallStarted(_, name) if name == "search_library")
    ));
    assert!(
        emitted
            .iter()
            .any(|event| matches!(event, AIEvent::ToolArgumentsDelta(_, _)))
    );
    assert!(
        !emitted
            .iter()
            .any(|event| matches!(event, AIEvent::ToolCallCompleted(_)))
    );

    for line in fixture("tool")
        .lines()
        .filter(|line| !line.trim().is_empty())
        .skip(3)
    {
        emitted.extend(decoder.decode_json_line(line).expect("valid event"));
    }
    let completed = emitted
        .iter()
        .find_map(|event| match event {
            AIEvent::ToolCallCompleted(call) => Some(call),
            _ => None,
        })
        .expect("completed tool call");
    assert_eq!(completed.name, "search_library");
    assert_eq!(completed.arguments, serde_json::json!({"query": "Björk"}));
}

#[test]
fn ignores_unknown_forward_compatible_events() {
    let events = decode_sse_fixture(fixture("unknown")).expect("unknown event is skippable");
    assert_eq!(events.len(), 3);
    assert!(matches!(events[0], AIEvent::TextDelta(_)));
    assert!(matches!(events[1], AIEvent::Usage(_)));
    assert!(matches!(events[2], AIEvent::Completed(_)));
}

#[test]
fn rejects_duplicate_or_out_of_order_sequences() {
    let error = decode_sse_fixture(fixture("duplicate")).expect_err("duplicate must fail");
    assert!(matches!(error, AIProviderError::Protocol { .. }));
}

#[test]
fn rejects_malformed_known_event_json() {
    let error = decode_sse_fixture(fixture("malformed")).expect_err("malformed must fail");
    assert!(matches!(error, AIProviderError::Protocol { .. }));
}

#[test]
fn reports_an_interrupted_stream_with_pending_tool_arguments() {
    let mut decoder = DeepSeekEventDecoder::new();
    for line in fixture("tool")
        .lines()
        .filter(|line| !line.trim().is_empty())
        .take(3)
    {
        decoder.decode_json_line(line).expect("valid event");
    }
    let error = decoder
        .finish()
        .expect_err("pending call means interruption");
    assert!(matches!(error, AIProviderError::StreamInterrupted));
}

#[test]
fn classifies_http_failures_without_leaking_response_bodies() {
    assert!(matches!(
        classify_http_status(401, None),
        AIProviderError::Unauthorized
    ));
    assert!(
        matches!(classify_http_status(429, Some("3")), AIProviderError::RateLimited { retry_after: Some(value) } if value == Duration::from_secs(3))
    );
    assert!(matches!(
        classify_http_status(503, None),
        AIProviderError::Server { status: 503 }
    ));
}
