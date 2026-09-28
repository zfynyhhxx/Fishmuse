#![cfg(windows)]

#[path = "fixtures/fake_pipe_server.rs"]
mod fake_pipe_server;

use std::time::Duration;

use fake_pipe_server::{FakePipeServer, next_uuid};
use fishmuse_domain::{ErrorCode, OperationId};
use fishmuse_playback::{
    PlaybackBackend, PlaybackBackendKind, PlaybackCommand, PlaybackEvent, PlaybackStatus,
    foobar::{
        ConnectionState, FoobarBackend, FoobarConfig, ReconnectPolicy, StateReconciler,
        protocol::{
            Capability, CommandAck, HandshakeResponse, Message, PlaybackEventKind,
            PlaybackEventPayload, StateSnapshot, WireCommand,
        },
    },
};
use tokio::{sync::oneshot, time::timeout};

fn snapshot(session_id: uuid::Uuid, revision: u64, status: PlaybackStatus) -> StateSnapshot {
    StateSnapshot {
        session_id,
        revision,
        status,
        track_id: None,
        position_ms: 0,
        duration_ms: None,
        volume: 0.5,
        backend: PlaybackBackendKind::Foobar2000,
    }
}

async fn accept_handshake(peer: &mut fake_pipe_server::FakePipePeer, session_id: uuid::Uuid) {
    let request = peer.read().await;
    let Message::HandshakeRequest(handshake) = &request.message else {
        panic!("connection must begin with handshake.request");
    };
    peer.respond(
        &request,
        Message::HandshakeResponse(HandshakeResponse {
            plugin_version: "0.1.0-test".to_owned(),
            selected_protocol_version: 1,
            process_id: 42,
            nonce: handshake.nonce.clone(),
            session_id,
            capabilities: vec![Capability::Pause, Capability::GetState],
        }),
    )
    .await;
}

async fn accept_initial_snapshot(
    peer: &mut fake_pipe_server::FakePipePeer,
    session_id: uuid::Uuid,
    revision: u64,
) {
    let request = peer.read().await;
    let Message::CommandRequest(command) = &request.message else {
        panic!("handshake must be followed by snapshot request");
    };
    assert_eq!(command.command, WireCommand::GetState);
    peer.respond(
        &request,
        Message::CommandAck(CommandAck {
            operation_id: command.operation_id,
            accepted: true,
            snapshot: snapshot(session_id, revision, PlaybackStatus::Stopped),
        }),
    )
    .await;
}

async fn wait_for_state(
    receiver: &mut tokio::sync::watch::Receiver<ConnectionState>,
    predicate: impl Fn(&ConnectionState) -> bool,
) -> ConnectionState {
    timeout(Duration::from_secs(1), async {
        loop {
            let state = receiver.borrow().clone();
            if predicate(&state) {
                return state;
            }
            receiver.changed().await.expect("connection state sender");
        }
    })
    .await
    .expect("connection state deadline")
}

#[test]
fn reconnect_policy_is_exponential_capped_and_jittered() {
    let policy = ReconnectPolicy::new(Duration::from_millis(100), Duration::from_millis(800), 20);
    assert_eq!(policy.delay_for(0, 50), Duration::from_millis(100));
    assert_eq!(policy.delay_for(1, 50), Duration::from_millis(200));
    assert_eq!(policy.delay_for(2, 50), Duration::from_millis(400));
    assert_eq!(policy.delay_for(8, 50), Duration::from_millis(800));
    assert_eq!(policy.delay_for(0, 0), Duration::from_millis(80));
    assert_eq!(policy.delay_for(0, 100), Duration::from_millis(120));
}

#[tokio::test]
async fn absent_foobar_is_unavailable_and_manual_reconnect_bypasses_backoff() {
    let seed = FakePipeServer::bind();
    let pipe_name = seed.pipe_name().to_owned();
    drop(seed);
    let policy = ReconnectPolicy::new(Duration::from_secs(5), Duration::from_secs(5), 0);
    let backend = FoobarBackend::connect(
        FoobarConfig::for_pipe(pipe_name.clone()).with_reconnect_policy(policy),
    )
    .await
    .expect("backend construction must not fail when foobar is absent");
    let mut state = backend.connection_state();
    wait_for_state(&mut state, |value| {
        matches!(value, ConnectionState::Unavailable)
    })
    .await;
    let error = backend
        .snapshot()
        .await
        .expect_err("absent backend is unavailable");
    assert_eq!(error.code, ErrorCode::Unavailable);

    let server = FakePipeServer::bind_at(pipe_name);
    let session_id = next_uuid();
    let server_task = tokio::spawn(async move {
        let mut peer = server.accept().await;
        accept_handshake(&mut peer, session_id).await;
        accept_initial_snapshot(&mut peer, session_id, 0).await;
        tokio::time::sleep(Duration::from_millis(100)).await;
    });
    backend.reconnect_now();
    let connected = wait_for_state(&mut state, |value| {
        matches!(value, ConnectionState::Connected { .. })
    })
    .await;
    assert_eq!(connected, ConnectionState::Connected { session_id });
    server_task.await.expect("fake server task");
}

#[tokio::test]
async fn explicit_shutdown_closes_the_transport_and_stops_the_supervisor() {
    let server = FakePipeServer::bind();
    let pipe_name = server.pipe_name().to_owned();
    let session_id = next_uuid();
    let (disconnected_tx, disconnected_rx) = oneshot::channel();
    let server_task = tokio::spawn(async move {
        let mut peer = server.accept().await;
        accept_handshake(&mut peer, session_id).await;
        accept_initial_snapshot(&mut peer, session_id, 0).await;
        peer.wait_for_disconnect().await;
        disconnected_tx.send(()).expect("report client shutdown");
    });
    let backend = FoobarBackend::connect(FoobarConfig::for_pipe(pipe_name))
        .await
        .expect("construct backend");
    let mut state = backend.connection_state();
    wait_for_state(&mut state, |value| {
        matches!(value, ConnectionState::Connected { .. })
    })
    .await;

    backend.shutdown().await;
    assert_eq!(*state.borrow(), ConnectionState::Disconnected);
    timeout(Duration::from_secs(1), disconnected_rx)
        .await
        .expect("pipe shutdown deadline")
        .expect("pipe shutdown signal");
    server_task.await.expect("fake server task");
}

#[tokio::test]
async fn lost_ack_reconnects_then_retries_once_with_the_same_operation_id() {
    let first_server = FakePipeServer::bind();
    let pipe_name = first_server.pipe_name().to_owned();
    let first_session = next_uuid();
    let (observed_tx, observed_rx) = oneshot::channel();
    let first_task = tokio::spawn(async move {
        let mut peer = first_server.accept().await;
        accept_handshake(&mut peer, first_session).await;
        accept_initial_snapshot(&mut peer, first_session, 4).await;
        let request = peer.read().await;
        let Message::CommandRequest(command) = request.message else {
            panic!("expected retriable command");
        };
        observed_tx
            .send(command.operation_id)
            .expect("send first operation id");
        // Drop the pipe before writing the ACK, modeling an unknown command outcome.
    });

    let backend = FoobarBackend::connect(
        FoobarConfig::for_pipe(pipe_name.clone()).with_reconnect_policy(ReconnectPolicy::new(
            Duration::from_millis(10),
            Duration::from_millis(20),
            0,
        )),
    )
    .await
    .expect("construct backend");
    let mut connection = backend.connection_state();
    wait_for_state(&mut connection, |value| {
        matches!(value, ConnectionState::Connected { session_id } if *session_id == first_session)
    })
    .await;
    let mut playback_events = backend.subscribe();

    let operation_id = OperationId::new();
    let execute = tokio::spawn({
        let backend = backend.clone();
        async move {
            backend
                .execute(PlaybackCommand::Pause { operation_id })
                .await
        }
    });
    assert_eq!(observed_rx.await.expect("first operation id"), operation_id);
    first_task.await.expect("first server task");

    let second_server = FakePipeServer::bind_at(pipe_name);
    let second_session = next_uuid();
    let second_task = tokio::spawn(async move {
        let mut peer = second_server.accept().await;
        accept_handshake(&mut peer, second_session).await;
        accept_initial_snapshot(&mut peer, second_session, 0).await;
        let request = peer.read().await;
        let Message::CommandRequest(command) = &request.message else {
            panic!("expected retried command after snapshot");
        };
        assert_eq!(command.operation_id, operation_id);
        assert_eq!(command.command, WireCommand::Pause);
        peer.respond(
            &request,
            Message::CommandAck(CommandAck {
                operation_id,
                accepted: true,
                snapshot: snapshot(second_session, 1, PlaybackStatus::Paused),
            }),
        )
        .await;
        peer.event(
            2,
            Message::PlaybackEvent(PlaybackEventPayload {
                event: PlaybackEventKind::StateChanged,
                snapshot: snapshot(second_session, 2, PlaybackStatus::Playing),
                error_code: None,
            }),
        )
        .await;
    });
    backend.reconnect_now();

    let result = timeout(Duration::from_secs(1), execute)
        .await
        .expect("retry deadline")
        .expect("execute task")
        .expect("retried command succeeds");
    assert!(result.revision > 4);
    assert_eq!(result.status, PlaybackStatus::Paused);
    let playing = timeout(Duration::from_secs(1), async {
        loop {
            match playback_events.recv().await.expect("playback event sender") {
                PlaybackEvent::Snapshot(snapshot) if snapshot.status == PlaybackStatus::Playing => {
                    break snapshot;
                }
                PlaybackEvent::Snapshot(_) | PlaybackEvent::Disconnected { .. } => {}
            }
        }
    })
    .await
    .expect("post-reconnect incremental event deadline");
    assert!(playing.revision > result.revision);
    second_task.await.expect("second server task");
}

#[test]
fn reconciliation_rejects_late_old_session_events_and_accepts_sequence_reset() {
    let old_session = next_uuid();
    let new_session = next_uuid();
    let mut reconcile = StateReconciler::default();
    let old_initial = reconcile
        .begin_session(snapshot(old_session, 10, PlaybackStatus::Playing))
        .expect("old authoritative snapshot");
    assert_eq!(old_initial.revision, 10);
    let old_incremental = reconcile
        .apply_event(snapshot(old_session, 11, PlaybackStatus::Paused))
        .expect("old event")
        .expect("old event accepted");

    let new_initial = reconcile
        .begin_session(snapshot(new_session, 0, PlaybackStatus::Stopped))
        .expect("new authoritative snapshot with reset sequence");
    assert!(new_initial.revision > old_incremental.revision);
    assert!(
        reconcile
            .apply_event(snapshot(old_session, 12, PlaybackStatus::Playing))
            .expect("late old event is not an error")
            .is_none()
    );
    assert!(
        reconcile
            .apply_event(snapshot(new_session, 0, PlaybackStatus::Playing))
            .expect("duplicate new event")
            .is_none()
    );
    let accepted = reconcile
        .apply_event(snapshot(new_session, 1, PlaybackStatus::Playing))
        .expect("new incremental event")
        .expect("new event accepted");
    assert!(accepted.revision > new_initial.revision);
}

#[allow(dead_code)]
fn assert_playback_event_shape(event: PlaybackEvent) {
    assert!(matches!(event, PlaybackEvent::Snapshot(_)));
}
