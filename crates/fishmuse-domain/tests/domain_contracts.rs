use fishmuse_domain::{
    AppError, ArtistId, DiscNumber, ErrorCategory, ErrorCode, LibraryItem, MediaAssetId,
    PlayableSource, RecordingId, ReleaseId, ScanId, SourceProvenance, TrackId, TrackNumber,
    TrackSummary, UserId,
};
use uuid::Uuid;

fn track_summary() -> TrackSummary {
    TrackSummary {
        id: TrackId::new(),
        recording_id: RecordingId::new(),
        title: "From the Start".to_owned(),
        artist_names: vec!["Laufey".to_owned()],
        release_title: Some("Bewitched".to_owned()),
        duration_ms: Some(u64::MAX),
        disc_number: Some(DiscNumber::new(1).expect("disc numbers are one-based")),
        track_number: Some(TrackNumber::new(u32::MAX).expect("u32::MAX is valid")),
        playable: true,
    }
}

#[test]
fn domain_ids_round_trip_as_uuid_v7_json_and_remain_distinct_types() {
    fn accepts_track_id(_: TrackId) {}

    let user_id = UserId::new();
    let serialized = serde_json::to_string(&user_id).expect("UserId serializes to JSON");
    let restored: UserId = serde_json::from_str(&serialized).expect("UserId deserializes");

    assert_eq!(restored, user_id);
    assert_eq!(user_id.as_uuid().get_version_num(), 7);
    accepts_track_id(TrackId::new());
    let _artist_id = ArtistId::new();
    let _release_id = ReleaseId::new();
    let scan_id = ScanId::new();
    assert_eq!(scan_id.as_uuid().get_version_num(), 7);
}

#[test]
fn domain_ids_reject_non_v7_construction_and_json_payloads() {
    let version_four = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000")
        .expect("fixture is a valid UUID v4");

    assert!(UserId::try_from_uuid(version_four).is_err());
    assert!(serde_json::from_str::<UserId>(r#""550e8400-e29b-41d4-a716-446655440000""#).is_err());
    assert!(serde_json::from_str::<TrackId>(r#""00000000-0000-0000-0000-000000000000""#).is_err());
}

#[test]
fn track_summary_serializes_full_duration_and_one_based_disc_and_track_positions() {
    let summary = track_summary();
    let serialized = serde_json::to_value(&summary).expect("TrackSummary serializes");

    assert_eq!(serialized["duration_ms"], serde_json::json!(u64::MAX));
    assert_eq!(serialized["disc_number"], serde_json::json!(1));
    assert_eq!(serialized["track_number"], serde_json::json!(u32::MAX));
    assert_eq!(DiscNumber::new(0), None);
    assert_eq!(TrackNumber::new(0), None);
}

#[test]
fn app_error_serialization_keeps_technical_context_out_of_user_payloads() {
    let error = AppError {
        code: ErrorCode::BackendUnavailable,
        category: ErrorCategory::Playback,
        user_message: "The playback backend is unavailable.".to_owned(),
        retryable: true,
        suggested_action: Some("Start foobar2000 and try again.".to_owned()),
        technical_context: Some(r"named pipe timeout at \\.\pipe\fishmuse".to_owned()),
    };

    let serialized = serde_json::to_value(error).expect("AppError serializes");

    assert_eq!(
        serialized["user_message"],
        serde_json::json!("The playback backend is unavailable.")
    );
    assert_eq!(serialized["retryable"], serde_json::json!(true));
    assert!(serialized.get("technical_context").is_none());
}

#[test]
fn playable_source_payload_uses_logical_identifiers_without_backend_selection() {
    let item = LibraryItem {
        track: track_summary(),
        release: None,
        provenance: vec![SourceProvenance::Local],
    };
    let source = PlayableSource {
        track_id: item.track.id,
        media_asset_id: MediaAssetId::new(),
        subsong_index: None,
        start_ms: None,
        end_ms: None,
    };

    let item_json = serde_json::to_string(&item).expect("LibraryItem serializes");
    let source_json = serde_json::to_string(&source).expect("PlayableSource serializes");

    assert!(!item_json.contains(r"C:\\Users\\fish\\Music"));
    assert!(!source_json.contains(r"C:\\Users\\fish\\Music"));
    assert!(source_json.contains("media_asset_id"));
    assert!(!source_json.contains("backend"));
    assert!(!source_json.contains("path"));
}

#[test]
fn provenance_serializes_each_supported_source() {
    let provenance = vec![
        SourceProvenance::Local,
        SourceProvenance::MusicBrainz,
        SourceProvenance::AppleMusic,
        SourceProvenance::NetEase,
    ];

    let serialized = serde_json::to_value(provenance).expect("provenance serializes");

    assert_eq!(
        serialized,
        serde_json::json!(["local", "music_brainz", "apple_music", "net_ease"])
    );
}
