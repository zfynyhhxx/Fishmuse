use std::{fs, path::Path};

use fishmuse_playback::foobar::protocol::{ProtocolErrorCode, SequenceTracker, decode_json};
use serde_json::{Value, json};

const VECTOR_NAMES: &[&str] = &[
    "handshake.request.json",
    "handshake.response.json",
    "play.request.json",
    "play.ack.json",
    "state.snapshot.json",
    "error.response.json",
];

fn vector_text(name: &str) -> String {
    fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../protocol/foobar-v1/vectors")
            .join(name),
    )
    .expect("golden vector must be readable")
}

fn assert_invalid(document: impl AsRef<[u8]>) {
    let error = decode_json(document.as_ref()).expect_err("document must be rejected");
    assert_eq!(error.code(), ProtocolErrorCode::ProtocolInvalid);
    assert_eq!(error.code().as_str(), "protocol_invalid");
}

#[test]
fn protocol_golden_vectors_round_trip_without_semantic_changes() {
    for name in VECTOR_NAMES {
        let text = vector_text(name);
        let expected: Value = serde_json::from_str(&text).expect("vector is JSON");
        let decoded = decode_json(text.as_bytes()).expect("vector follows protocol v1");
        let actual = serde_json::to_value(decoded).expect("decoded envelope serializes");
        assert_eq!(actual, expected, "semantic mismatch in {name}");
    }
}

#[test]
fn protocol_rejects_unknown_kind_unsupported_version_missing_and_unknown_fields() {
    let base: Value = serde_json::from_str(&vector_text("handshake.request.json")).unwrap();

    let mut unknown_kind = base.clone();
    unknown_kind["kind"] = json!("admin.execute");
    assert_invalid(serde_json::to_vec(&unknown_kind).unwrap());

    let mut unsupported_version = base.clone();
    unsupported_version["protocolVersion"] = json!(2);
    let error = decode_json(&serde_json::to_vec(&unsupported_version).unwrap())
        .expect_err("unsupported version must be rejected separately");
    assert_eq!(error.code(), ProtocolErrorCode::ProtocolUnsupported);
    assert_eq!(error.code().as_str(), "protocol_unsupported");

    let mut missing_payload = base.clone();
    missing_payload.as_object_mut().unwrap().remove("payload");
    assert_invalid(serde_json::to_vec(&missing_payload).unwrap());

    let mut unknown_envelope_field = base;
    unknown_envelope_field["administrator"] = json!(true);
    assert_invalid(serde_json::to_vec(&unknown_envelope_field).unwrap());
}

#[test]
fn protocol_rejects_duplicate_fields_non_finite_numbers_invalid_uuid_and_invalid_utf8() {
    let duplicate = br#"{
        "protocolVersion":1,
        "messageId":"0199a1b2-c3d4-7001-8000-000000000001",
        "messageId":"0199a1b2-c3d4-7001-8000-000000000002",
        "correlationId":null,
        "sentAtUnixMs":0,
        "kind":"ping",
        "sequence":null,
        "payload":{"nonce":"0123456789abcdef0123456789abcdef"}
    }"#;
    assert_invalid(duplicate);

    let non_finite = vector_text("state.snapshot.json").replace("0.8", "NaN");
    assert_invalid(non_finite);

    let invalid_uuid = vector_text("handshake.request.json")
        .replace("0199a1b2-c3d4-7001-8000-000000000001", "not-a-uuid");
    assert_invalid(invalid_uuid);

    assert_invalid([0xff, 0xfe, 0xfd]);
}

#[test]
fn protocol_command_payload_cannot_gain_permissions_from_extra_json_fields() {
    let mut command: Value =
        serde_json::from_str(&vector_text("play.request.json")).expect("vector is JSON");
    command["payload"]["command"]["permission"] = json!("arbitrary_local_path");
    assert_invalid(serde_json::to_vec(&command).unwrap());
}

#[test]
fn protocol_sequence_tracker_rejects_duplicate_and_older_events() {
    let mut tracker = SequenceTracker::default();
    assert!(tracker.accept(7));
    assert!(!tracker.accept(7));
    assert!(!tracker.accept(6));
    assert!(tracker.accept(8));
    assert_eq!(tracker.last_applied(), Some(8));
}
