#![cfg(windows)]

#[path = "fixtures/fake_pipe_server.rs"]
mod fake_pipe_server;

use std::time::Duration;

use fake_pipe_server::{FakePipeServer, next_uuid};
use fishmuse_domain::{ErrorCode, OperationId};
use fishmuse_playback::{
    PlaybackBackendKind, PlaybackStatus,
    foobar::{
        FoobarClient, FoobarConfig,
        protocol::{
            Capability, CommandAck, ErrorResponse, ErrorResponseCode, HandshakeResponse, Message,
            PlaybackEventKind, PlaybackEventPayload, StateSnapshot, WireCommand,
        },
    },
};
use tokio::time::{Instant, timeout};

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
        panic!("first client message must be handshake.request");
    };
    assert_eq!(handshake.supported_protocol_versions, vec![1]);
    peer.respond(
        &request,
        Message::HandshakeResponse(HandshakeResponse {
            plugin_version: "0.1.0-test".to_owned(),
            selected_protocol_version: 1,
            process_id: 42,
            nonce: handshake.nonce.clone(),
            session_id,
            capabilities: vec![Capability::Pause, Capability::GetState, Capability::Play],
        }),
    )
    .await;
}

async fn accept_initial_snapshot(
    peer: &mut fake_pipe_server::FakePipePeer,
    session_id: uuid::Uuid,
) {
    let request = peer.read().await;
    let Message::CommandRequest(command) = &request.message else {
        panic!("handshake must be followed by command.request");
    };
    assert_eq!(command.command, WireCommand::GetState);
    peer.respond(
        &request,
        Message::CommandAck(CommandAck {
            operation_id: command.operation_id,
            accepted: true,
            snapshot: snapshot(session_id, 1, PlaybackStatus::Stopped),
        }),
    )
    .await;
}

#[tokio::test]
async fn client_handshakes_correlates_ack_and_delivers_snapshot_and_event() {
    let server = FakePipeServer::bind();
    let pipe_name = server.pipe_name().to_owned();
    let session_id = next_uuid();
    let server_task = tokio::spawn(async move {
        let mut peer = server.accept().await;
        accept_handshake(&mut peer, session_id).await;
        accept_initial_snapshot(&mut peer, session_id).await;
        peer.event(
            2,
            Message::PlaybackEvent(PlaybackEventPayload {
                event: PlaybackEventKind::StateChanged,
                snapshot: snapshot(session_id, 2, PlaybackStatus::Playing),
                error_code: None,
            }),
        )
        .await;

        let request = peer.read().await;
        let Message::CommandRequest(command) = &request.message else {
            panic!("expected command.request");
        };
        assert_eq!(command.command, WireCommand::Pause);
        peer.respond(
            &request,
            Message::CommandAck(CommandAck {
                operation_id: command.operation_id,
                accepted: true,
                snapshot: snapshot(session_id, 3, PlaybackStatus::Paused),
            }),
        )
        .await;
    });

    let config = FoobarConfig::for_pipe(pipe_name).with_ack_timeout(Duration::from_millis(500));
    let client = FoobarClient::connect(config).await.expect("connect client");
    assert_eq!(client.session_id(), session_id);
    assert_eq!(client.snapshot().revision, 1);
    tokio::time::sleep(Duration::from_millis(20)).await;
    let mut events = client.subscribe();
    let started = Instant::now();
    let operation_id = OperationId::new();
    let ack = client
        .command(operation_id, WireCommand::Pause)
        .await
        .expect("pause ACK");
    assert!(started.elapsed() <= Duration::from_millis(500));
    assert_eq!(ack.operation_id, operation_id);
    assert_eq!(ack.snapshot.revision, 3);

    let event = timeout(Duration::from_millis(500), events.recv())
        .await
        .expect("event deadline")
        .expect("event channel");
    assert_eq!(event.sequence, 2);
    assert_eq!(event.snapshot.revision, 2);
    server_task.await.expect("fake server task");
}

#[tokio::test]
async fn client_maps_typed_remote_error_without_exposing_message_details() {
    let server = FakePipeServer::bind();
    let pipe_name = server.pipe_name().to_owned();
    let session_id = next_uuid();
    let server_task = tokio::spawn(async move {
        let mut peer = server.accept().await;
        accept_handshake(&mut peer, session_id).await;
        accept_initial_snapshot(&mut peer, session_id).await;
        let request = peer.read().await;
        peer.respond(
            &request,
            Message::ErrorResponse(ErrorResponse {
                code: ErrorResponseCode::BackendUnavailable,
                message: r"private path C:\music\secret.flac failed".to_owned(),
                retryable: true,
            }),
        )
        .await;
    });

    let client = FoobarClient::connect(FoobarConfig::for_pipe(pipe_name))
        .await
        .expect("connect client");
    let error = client
        .command(OperationId::new(), WireCommand::Pause)
        .await
        .expect_err("remote failure must map to AppError");
    assert_eq!(error.code, ErrorCode::BackendUnavailable);
    assert!(error.retryable);
    assert!(!error.user_message.contains("secret.flac"));
    assert!(
        !error
            .technical_context
            .as_deref()
            .unwrap_or_default()
            .contains("secret.flac")
    );
    server_task.await.expect("fake server task");
}

#[tokio::test]
async fn client_enforces_the_configured_ack_deadline() {
    let server = FakePipeServer::bind();
    let pipe_name = server.pipe_name().to_owned();
    let session_id = next_uuid();
    let server_task = tokio::spawn(async move {
        let mut peer = server.accept().await;
        accept_handshake(&mut peer, session_id).await;
        accept_initial_snapshot(&mut peer, session_id).await;
        let _request = peer.read().await;
        tokio::time::sleep(Duration::from_secs(2)).await;
    });

    let client = FoobarClient::connect(
        FoobarConfig::for_pipe(pipe_name).with_ack_timeout(Duration::from_millis(30)),
    )
    .await
    .expect("connect client");
    let error = client
        .command(OperationId::new(), WireCommand::Pause)
        .await
        .expect_err("missing ACK must time out");
    assert_eq!(error.code, ErrorCode::Unavailable);
    assert!(error.retryable);
    server_task.abort();
}
