use fishmuse_playback::{PlaybackServiceState, PlaybackServiceStatus, ServiceImplementation};
use serde_json::json;

#[test]
fn playback_service_state_is_backend_neutral_and_diagnostics_are_optional() {
    let unavailable = PlaybackServiceState {
        status: PlaybackServiceStatus::Unavailable,
        implementation: None,
    };
    let disconnected = PlaybackServiceState {
        status: PlaybackServiceStatus::Disconnected,
        implementation: Some(ServiceImplementation {
            id: "foobar2000".to_owned(),
            display_name: "foobar2000".to_owned(),
        }),
    };

    assert_eq!(
        serde_json::to_value(unavailable).expect("unavailable JSON"),
        json!({"status": "unavailable", "implementation": null})
    );
    assert_eq!(
        serde_json::to_value(disconnected).expect("disconnected JSON"),
        json!({
            "status": "disconnected",
            "implementation": {
                "id": "foobar2000",
                "display_name": "foobar2000"
            }
        })
    );
}
