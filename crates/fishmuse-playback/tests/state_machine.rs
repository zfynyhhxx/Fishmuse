use fishmuse_domain::{MediaAssetId, OperationId, PlayableSource, TrackId};
use fishmuse_playback::{
    PlaybackBackendKind, PlaybackCommand, PlaybackEvent, PlaybackSnapshot, PlaybackStateMachine,
    PlaybackStatus,
};

fn snapshot(revision: u64, status: PlaybackStatus, position_ms: u64) -> PlaybackSnapshot {
    PlaybackSnapshot {
        revision,
        status,
        track_id: matches!(status, PlaybackStatus::Playing | PlaybackStatus::Paused)
            .then(TrackId::new),
        position_ms,
        duration_ms: Some(10_000),
        backend: PlaybackBackendKind::Foobar2000,
    }
}

#[test]
fn accepts_the_complete_valid_state_sequence() {
    let initial = snapshot(0, PlaybackStatus::Stopped, 0);
    let track = TrackId::new();
    let mut machine = PlaybackStateMachine::new(initial).expect("valid initial state");

    for (revision, status, position_ms) in [
        (1, PlaybackStatus::Loading, 0),
        (2, PlaybackStatus::Playing, 100),
        (3, PlaybackStatus::Paused, 200),
        (4, PlaybackStatus::Playing, 200),
        (5, PlaybackStatus::Stopped, 0),
        (6, PlaybackStatus::Unavailable, 0),
    ] {
        let mut next = snapshot(revision, status, position_ms);
        if matches!(status, PlaybackStatus::Playing | PlaybackStatus::Paused) {
            next.track_id = Some(track);
        }
        assert!(
            machine
                .apply(PlaybackEvent::Snapshot(next))
                .expect("transition")
        );
    }
    assert_eq!(machine.snapshot().status, PlaybackStatus::Unavailable);
}

#[test]
fn rejects_illegal_transition_and_out_of_bounds_seek() {
    let mut machine = PlaybackStateMachine::new(snapshot(0, PlaybackStatus::Stopped, 0))
        .expect("valid initial state");
    assert_eq!(
        machine
            .apply(PlaybackEvent::Snapshot(snapshot(
                1,
                PlaybackStatus::Paused,
                0,
            )))
            .expect_err("stopped cannot become paused")
            .user_message,
        "invalid_playback_transition"
    );

    let mut playing = snapshot(2, PlaybackStatus::Playing, 500);
    playing.track_id = Some(TrackId::new());
    let machine = PlaybackStateMachine::new(playing).expect("valid playing state");
    assert_eq!(
        machine
            .validate_command(
                &PlaybackCommand::Seek {
                    position_ms: 10_001,
                    operation_id: OperationId::new(),
                },
                true,
            )
            .expect_err("seek must remain inside duration")
            .user_message,
        "seek_out_of_bounds"
    );
}

#[test]
fn rejects_an_unavailable_playable_source() {
    let machine = PlaybackStateMachine::new(snapshot(0, PlaybackStatus::Stopped, 0))
        .expect("valid initial state");
    let command = PlaybackCommand::Play {
        source: PlayableSource {
            track_id: TrackId::new(),
            media_asset_id: MediaAssetId::new(),
            subsong_index: None,
            start_ms: None,
            end_ms: None,
        },
        operation_id: OperationId::new(),
    };
    assert_eq!(
        machine
            .validate_command(&command, false)
            .expect_err("missing source must be rejected")
            .user_message,
        "playback_source_unavailable"
    );
}

#[test]
fn ignores_duplicate_and_stale_revisions() {
    let mut current = snapshot(5, PlaybackStatus::Playing, 4_000);
    current.track_id = Some(TrackId::new());
    let mut machine = PlaybackStateMachine::new(current.clone()).expect("valid state");

    assert!(
        !machine
            .apply(PlaybackEvent::Snapshot(snapshot(
                5,
                PlaybackStatus::Stopped,
                0,
            )))
            .expect("duplicate is ignored")
    );
    assert!(
        !machine
            .apply(PlaybackEvent::Snapshot(snapshot(
                4,
                PlaybackStatus::Stopped,
                0,
            )))
            .expect("stale is ignored")
    );
    assert_eq!(machine.snapshot(), &current);
}

#[test]
fn paused_seek_can_transition_through_loading_back_to_paused() {
    let track_id = TrackId::new();
    let mut paused = snapshot(10, PlaybackStatus::Paused, 2_000);
    paused.track_id = Some(track_id);
    let mut machine = PlaybackStateMachine::new(paused).expect("paused state");
    let mut loading = snapshot(11, PlaybackStatus::Loading, 2_000);
    loading.track_id = Some(track_id);
    let mut paused_again = snapshot(12, PlaybackStatus::Paused, 3_000);
    paused_again.track_id = Some(track_id);

    assert!(
        machine
            .apply(PlaybackEvent::Snapshot(loading))
            .expect("paused to loading")
    );
    assert!(
        machine
            .apply(PlaybackEvent::Snapshot(paused_again))
            .expect("loading back to paused")
    );
    assert_eq!(machine.snapshot().revision, 12);
    assert_eq!(machine.snapshot().status, PlaybackStatus::Paused);
    assert_eq!(machine.snapshot().track_id, Some(track_id));
}
