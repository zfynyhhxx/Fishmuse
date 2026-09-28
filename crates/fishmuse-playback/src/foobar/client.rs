use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex as StdMutex, RwLock},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use fishmuse_domain::{AppError, AppResult, ErrorCategory, ErrorCode, OperationId};
use tokio::sync::{Mutex, broadcast, mpsc, oneshot, watch};
use uuid::Uuid;

use super::{
    backend::FoobarConfig,
    protocol::{
        Capability, CommandAck, CommandRequest, Envelope, ErrorResponse, ErrorResponseCode,
        HandshakeRequest, Message, PROTOCOL_VERSION, PlaybackEventPayload, StateSnapshot,
        WireCommand,
    },
    transport,
};

type PendingMap = Arc<Mutex<HashMap<Uuid, oneshot::Sender<AppResult<Envelope>>>>>;

#[derive(Clone, Debug, PartialEq)]
pub struct ClientEvent {
    pub session_id: Uuid,
    pub sequence: u64,
    pub snapshot: StateSnapshot,
    pub error_code: Option<String>,
}

pub struct FoobarClient {
    runtime: Arc<ClientRuntime>,
    session_id: Uuid,
    capabilities: HashSet<Capability>,
    snapshot: RwLock<StateSnapshot>,
    initial_events: StdMutex<Option<broadcast::Receiver<ClientEvent>>>,
}

impl FoobarClient {
    pub async fn connect(config: FoobarConfig) -> AppResult<Self> {
        let (reader, writer) =
            transport::connect(config.pipe_name()).map_err(|error| transport_error(&error))?;
        let (writer_tx, writer_rx) = mpsc::channel(config.writer_capacity());
        let (event_tx, initial_events) = broadcast::channel(config.event_capacity());
        let (disconnect_tx, _) = watch::channel(false);
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let pending = Arc::new(Mutex::new(HashMap::new()));

        tokio::spawn(writer_task(
            writer,
            writer_rx,
            pending.clone(),
            disconnect_tx.clone(),
            shutdown_rx.clone(),
        ));
        tokio::spawn(reader_task(
            reader,
            writer_tx.clone(),
            pending.clone(),
            event_tx.clone(),
            disconnect_tx.clone(),
            shutdown_rx,
        ));

        let runtime = Arc::new(ClientRuntime {
            writer: writer_tx,
            pending,
            events: event_tx,
            disconnected: disconnect_tx,
            shutdown: shutdown_tx,
            ack_timeout: config.ack_timeout(),
        });

        let nonce = Uuid::new_v4().simple().to_string();
        let handshake = runtime
            .request(
                Message::HandshakeRequest(HandshakeRequest {
                    app_version: env!("CARGO_PKG_VERSION").to_owned(),
                    supported_protocol_versions: vec![PROTOCOL_VERSION],
                    process_id: std::process::id(),
                    nonce: nonce.clone(),
                }),
                config.handshake_timeout(),
            )
            .await?;
        let Message::HandshakeResponse(response) = handshake.message else {
            return Err(protocol_app_error("unexpected_handshake_response"));
        };
        if response.nonce != nonce
            || response.selected_protocol_version != PROTOCOL_VERSION
            || !response.capabilities.contains(&Capability::GetState)
        {
            return Err(protocol_app_error("invalid_handshake_response"));
        }

        let snapshot_operation = OperationId::new();
        let initial = runtime
            .request(
                Message::CommandRequest(CommandRequest {
                    operation_id: snapshot_operation,
                    command: WireCommand::GetState,
                }),
                config.ack_timeout(),
            )
            .await?;
        let initial_snapshot = command_result(initial, snapshot_operation)?.snapshot;
        if initial_snapshot.session_id != response.session_id {
            return Err(protocol_app_error("snapshot_session_mismatch"));
        }

        Ok(Self {
            runtime,
            session_id: response.session_id,
            capabilities: response.capabilities.into_iter().collect(),
            snapshot: RwLock::new(initial_snapshot),
            initial_events: StdMutex::new(Some(initial_events)),
        })
    }

    #[must_use]
    pub const fn session_id(&self) -> Uuid {
        self.session_id
    }

    #[must_use]
    pub fn snapshot(&self) -> StateSnapshot {
        self.snapshot
            .read()
            .expect("foobar snapshot lock poisoned")
            .clone()
    }

    #[must_use]
    pub fn subscribe(&self) -> broadcast::Receiver<ClientEvent> {
        self.initial_events
            .lock()
            .expect("foobar initial event receiver lock poisoned")
            .take()
            .unwrap_or_else(|| self.runtime.events.subscribe())
    }

    #[must_use]
    pub fn disconnected(&self) -> watch::Receiver<bool> {
        self.runtime.disconnected.subscribe()
    }

    pub async fn command(
        &self,
        operation_id: OperationId,
        command: WireCommand,
    ) -> AppResult<CommandAck> {
        let capability = command_capability(&command);
        if !self.capabilities.contains(&capability) {
            return Err(AppError {
                code: ErrorCode::Unavailable,
                category: ErrorCategory::Playback,
                user_message: "The foobar2000 bridge does not support this command.".to_owned(),
                retryable: false,
                suggested_action: Some("Update the FishMuse foobar2000 component.".to_owned()),
                technical_context: Some("missing_capability".to_owned()),
            });
        }
        let response = self
            .runtime
            .request(
                Message::CommandRequest(CommandRequest {
                    operation_id,
                    command,
                }),
                self.runtime.ack_timeout,
            )
            .await?;
        let ack = command_result(response, operation_id)?;
        if ack.snapshot.session_id != self.session_id {
            return Err(protocol_app_error("command_snapshot_session_mismatch"));
        }
        *self
            .snapshot
            .write()
            .expect("foobar snapshot lock poisoned") = ack.snapshot.clone();
        Ok(ack)
    }
}

struct ClientRuntime {
    writer: mpsc::Sender<Envelope>,
    pending: PendingMap,
    events: broadcast::Sender<ClientEvent>,
    disconnected: watch::Sender<bool>,
    shutdown: watch::Sender<bool>,
    ack_timeout: Duration,
}

impl ClientRuntime {
    async fn request(&self, message: Message, deadline: Duration) -> AppResult<Envelope> {
        if *self.disconnected.borrow() {
            return Err(unavailable("pipe_disconnected"));
        }
        let message_id = Uuid::new_v4();
        let envelope = Envelope {
            protocol_version: PROTOCOL_VERSION,
            message_id,
            correlation_id: None,
            sent_at_unix_ms: unix_millis(),
            sequence: None,
            message,
        };
        let (response_tx, response_rx) = oneshot::channel();
        self.pending.lock().await.insert(message_id, response_tx);
        let expires_at = tokio::time::Instant::now() + deadline;
        match tokio::time::timeout_at(expires_at, self.writer.send(envelope)).await {
            Ok(Ok(())) => {}
            Ok(Err(_)) => {
                self.pending.lock().await.remove(&message_id);
                return Err(unavailable("pipe_writer_closed"));
            }
            Err(_) => {
                self.pending.lock().await.remove(&message_id);
                return Err(unavailable("writer_queue_timeout"));
            }
        }

        match tokio::time::timeout_at(expires_at, response_rx).await {
            Ok(Ok(response)) => response,
            Ok(Err(_)) => Err(unavailable("pipe_response_closed")),
            Err(_) => {
                self.pending.lock().await.remove(&message_id);
                Err(unavailable("ack_timeout"))
            }
        }
    }
}

impl Drop for ClientRuntime {
    fn drop(&mut self) {
        let _ = self.shutdown.send(true);
    }
}

async fn writer_task<W>(
    mut writer: transport::FramedWriter<W>,
    mut outgoing: mpsc::Receiver<Envelope>,
    pending: PendingMap,
    disconnected: watch::Sender<bool>,
    mut shutdown: watch::Receiver<bool>,
) where
    W: tokio::io::AsyncWrite + Unpin,
{
    loop {
        tokio::select! {
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() {
                    break;
                }
            }
            envelope = outgoing.recv() => {
                let Some(envelope) = envelope else { break; };
                if let Err(error) = writer.write(&envelope).await {
                    signal_disconnect(&pending, &disconnected, transport_error(&error)).await;
                    break;
                }
            }
        }
    }
}

async fn reader_task<R>(
    mut reader: transport::FramedReader<R>,
    writer: mpsc::Sender<Envelope>,
    pending: PendingMap,
    events: broadcast::Sender<ClientEvent>,
    disconnected: watch::Sender<bool>,
    mut shutdown: watch::Receiver<bool>,
) where
    R: tokio::io::AsyncRead + Unpin,
{
    loop {
        let envelope = tokio::select! {
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() {
                    break;
                }
                continue;
            }
            result = reader.read() => match result {
                Ok(envelope) => envelope,
                Err(error) => {
                    signal_disconnect(&pending, &disconnected, transport_error(&error)).await;
                    break;
                }
            }
        };

        if let Some(correlation_id) = envelope.correlation_id {
            if let Some(response) = pending.lock().await.remove(&correlation_id) {
                let _ = response.send(match envelope.message {
                    Message::ErrorResponse(error) => Err(remote_error(error)),
                    _ => Ok(envelope),
                });
            }
            continue;
        }

        match envelope.message {
            Message::StateSnapshot(snapshot) => {
                if let Some(sequence) = envelope.sequence {
                    let _ = events.send(ClientEvent {
                        session_id: snapshot.session_id,
                        sequence,
                        snapshot,
                        error_code: None,
                    });
                }
            }
            Message::PlaybackEvent(PlaybackEventPayload {
                snapshot,
                error_code,
                ..
            }) => {
                if let Some(sequence) = envelope.sequence {
                    let _ = events.send(ClientEvent {
                        session_id: snapshot.session_id,
                        sequence,
                        snapshot,
                        error_code,
                    });
                }
            }
            Message::Ping(heartbeat) => {
                let response = Envelope {
                    protocol_version: PROTOCOL_VERSION,
                    message_id: Uuid::new_v4(),
                    correlation_id: Some(envelope.message_id),
                    sent_at_unix_ms: unix_millis(),
                    sequence: None,
                    message: Message::Pong(heartbeat),
                };
                if writer.send(response).await.is_err() {
                    signal_disconnect(&pending, &disconnected, unavailable("pipe_writer_closed"))
                        .await;
                    break;
                }
            }
            _ => {
                signal_disconnect(
                    &pending,
                    &disconnected,
                    protocol_app_error("unexpected_unsolicited_message"),
                )
                .await;
                break;
            }
        }
    }
}

async fn signal_disconnect(
    pending: &PendingMap,
    disconnected: &watch::Sender<bool>,
    error: AppError,
) {
    if !*disconnected.borrow() {
        let _ = disconnected.send(true);
    }
    let entries = std::mem::take(&mut *pending.lock().await);
    for (_, response) in entries {
        let _ = response.send(Err(error.clone()));
    }
}

fn command_result(envelope: Envelope, operation_id: OperationId) -> AppResult<CommandAck> {
    let Message::CommandAck(ack) = envelope.message else {
        return Err(protocol_app_error("unexpected_command_response"));
    };
    if ack.operation_id != operation_id {
        return Err(protocol_app_error("operation_id_mismatch"));
    }
    Ok(ack)
}

fn command_capability(command: &WireCommand) -> Capability {
    match command {
        WireCommand::Play { .. } => Capability::Play,
        WireCommand::Pause => Capability::Pause,
        WireCommand::Resume => Capability::Resume,
        WireCommand::Stop => Capability::Stop,
        WireCommand::Seek { .. } => Capability::Seek,
        WireCommand::SkipNext => Capability::SkipNext,
        WireCommand::SetVolume { .. } => Capability::SetVolume,
        WireCommand::GetState => Capability::GetState,
    }
}

fn remote_error(error: ErrorResponse) -> AppError {
    let code = match error.code {
        ErrorResponseCode::BackendUnavailable => ErrorCode::BackendUnavailable,
        ErrorResponseCode::Unauthorized => ErrorCode::Unauthorized,
        ErrorResponseCode::OperationConflict
        | ErrorResponseCode::ProtocolInvalid
        | ErrorResponseCode::ProtocolUnsupported
        | ErrorResponseCode::FrameTooLarge
        | ErrorResponseCode::HandshakeRequired => ErrorCode::InvalidInput,
        ErrorResponseCode::PlaybackFailed => ErrorCode::Unavailable,
    };
    AppError {
        code,
        category: ErrorCategory::Playback,
        user_message: match error.code {
            ErrorResponseCode::BackendUnavailable => {
                "The foobar2000 playback backend is unavailable."
            }
            ErrorResponseCode::Unauthorized => "The foobar2000 bridge rejected this connection.",
            ErrorResponseCode::PlaybackFailed => "foobar2000 could not complete playback.",
            _ => "The foobar2000 bridge rejected an invalid request.",
        }
        .to_owned(),
        retryable: error.retryable,
        suggested_action: error
            .retryable
            .then(|| "Retry after the foobar2000 bridge reconnects.".to_owned()),
        technical_context: Some(format!("remote_error:{:?}", error.code)),
    }
}

pub(crate) fn unavailable(context: &str) -> AppError {
    AppError {
        code: ErrorCode::Unavailable,
        category: ErrorCategory::Playback,
        user_message: "The foobar2000 playback bridge is unavailable.".to_owned(),
        retryable: true,
        suggested_action: Some("Start foobar2000 or reconnect the playback bridge.".to_owned()),
        technical_context: Some(context.to_owned()),
    }
}

fn protocol_app_error(context: &str) -> AppError {
    AppError {
        code: ErrorCode::InvalidInput,
        category: ErrorCategory::Playback,
        user_message: "The foobar2000 bridge returned an invalid protocol message.".to_owned(),
        retryable: false,
        suggested_action: Some("Update the FishMuse foobar2000 component.".to_owned()),
        technical_context: Some(context.to_owned()),
    }
}

fn transport_error(error: &transport::TransportError) -> AppError {
    match error {
        transport::TransportError::Io(kind) => unavailable(&format!("pipe_io:{kind:?}")),
        transport::TransportError::Protocol(protocol) => {
            protocol_app_error(protocol.code().as_str())
        }
    }
}

fn unix_millis() -> u64 {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    u64::try_from(millis).unwrap_or(u64::MAX)
}
