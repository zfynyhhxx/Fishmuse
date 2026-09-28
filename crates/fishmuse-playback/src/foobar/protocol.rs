use std::{collections::HashSet, fmt};

use fishmuse_domain::{OperationId, TrackId};
use serde::{Deserialize, Serialize, Serializer, ser::Error as _};
use serde_json::Value;
use uuid::Uuid;

use crate::{PlaybackBackendKind, PlaybackStatus};

pub const PROTOCOL_VERSION: u16 = 1;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProtocolErrorCode {
    ProtocolInvalid,
    ProtocolUnsupported,
    FrameTooLarge,
}

impl ProtocolErrorCode {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ProtocolInvalid => "protocol_invalid",
            Self::ProtocolUnsupported => "protocol_unsupported",
            Self::FrameTooLarge => "frame_too_large",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProtocolError {
    code: ProtocolErrorCode,
    detail: String,
}

impl ProtocolError {
    #[must_use]
    pub(crate) fn invalid(detail: impl Into<String>) -> Self {
        Self {
            code: ProtocolErrorCode::ProtocolInvalid,
            detail: detail.into(),
        }
    }

    #[must_use]
    pub(crate) fn unsupported(detail: impl Into<String>) -> Self {
        Self {
            code: ProtocolErrorCode::ProtocolUnsupported,
            detail: detail.into(),
        }
    }

    #[must_use]
    pub(crate) fn frame_too_large(detail: impl Into<String>) -> Self {
        Self {
            code: ProtocolErrorCode::FrameTooLarge,
            detail: detail.into(),
        }
    }

    #[must_use]
    pub const fn code(&self) -> ProtocolErrorCode {
        self.code
    }
}

impl fmt::Display for ProtocolError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.code.as_str(), self.detail)
    }
}

impl std::error::Error for ProtocolError {}

#[derive(Clone, Debug, PartialEq)]
pub struct Envelope {
    pub protocol_version: u16,
    pub message_id: Uuid,
    pub correlation_id: Option<Uuid>,
    pub sent_at_unix_ms: u64,
    pub sequence: Option<u64>,
    pub message: Message,
}

impl Serialize for Envelope {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let payload = self.message.payload().map_err(S::Error::custom)?;
        SerializableEnvelope {
            protocol_version: self.protocol_version,
            message_id: self.message_id,
            correlation_id: self.correlation_id,
            sent_at_unix_ms: self.sent_at_unix_ms,
            kind: self.message.kind(),
            sequence: self.sequence,
            payload,
        }
        .serialize(serializer)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Message {
    HandshakeRequest(HandshakeRequest),
    HandshakeResponse(HandshakeResponse),
    CommandRequest(CommandRequest),
    CommandAck(CommandAck),
    StateSnapshot(StateSnapshot),
    PlaybackEvent(PlaybackEventPayload),
    ErrorResponse(ErrorResponse),
    Ping(Heartbeat),
    Pong(Heartbeat),
}

impl Message {
    fn kind(&self) -> &'static str {
        match self {
            Self::HandshakeRequest(_) => "handshake.request",
            Self::HandshakeResponse(_) => "handshake.response",
            Self::CommandRequest(_) => "command.request",
            Self::CommandAck(_) => "command.ack",
            Self::StateSnapshot(_) => "state.snapshot",
            Self::PlaybackEvent(_) => "playback.event",
            Self::ErrorResponse(_) => "error.response",
            Self::Ping(_) => "ping",
            Self::Pong(_) => "pong",
        }
    }

    fn payload(&self) -> Result<Value, serde_json::Error> {
        match self {
            Self::HandshakeRequest(value) => serde_json::to_value(value),
            Self::HandshakeResponse(value) => serde_json::to_value(value),
            Self::CommandRequest(value) => serde_json::to_value(value),
            Self::CommandAck(value) => serde_json::to_value(value),
            Self::StateSnapshot(value) => serde_json::to_value(value),
            Self::PlaybackEvent(value) => serde_json::to_value(value),
            Self::ErrorResponse(value) => serde_json::to_value(value),
            Self::Ping(value) | Self::Pong(value) => serde_json::to_value(value),
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HandshakeRequest {
    pub app_version: String,
    pub supported_protocol_versions: Vec<u16>,
    pub process_id: u32,
    pub nonce: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    Play,
    Pause,
    Resume,
    Stop,
    Seek,
    SkipNext,
    SetVolume,
    GetState,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HandshakeResponse {
    pub plugin_version: String,
    pub selected_protocol_version: u16,
    pub process_id: u32,
    pub nonce: String,
    pub session_id: Uuid,
    pub capabilities: Vec<Capability>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CommandRequest {
    pub operation_id: OperationId,
    pub command: WireCommand,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(
    tag = "name",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum WireCommand {
    Play {
        track_id: TrackId,
        path: String,
        subsong_index: Option<u32>,
        start_ms: Option<u64>,
        end_ms: Option<u64>,
    },
    Pause,
    Resume,
    Stop,
    Seek {
        position_ms: u64,
    },
    SkipNext,
    SetVolume {
        volume: f64,
    },
    GetState,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CommandAck {
    pub operation_id: OperationId,
    pub accepted: bool,
    pub snapshot: StateSnapshot,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StateSnapshot {
    pub session_id: Uuid,
    pub revision: u64,
    pub status: PlaybackStatus,
    pub track_id: Option<TrackId>,
    pub position_ms: u64,
    pub duration_ms: Option<u64>,
    pub volume: f64,
    pub backend: PlaybackBackendKind,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PlaybackEventKind {
    StateChanged,
    TrackChanged,
    Position,
    VolumeChanged,
    PlaybackError,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlaybackEventPayload {
    pub event: PlaybackEventKind,
    pub snapshot: StateSnapshot,
    pub error_code: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorResponseCode {
    ProtocolInvalid,
    ProtocolUnsupported,
    FrameTooLarge,
    HandshakeRequired,
    Unauthorized,
    OperationConflict,
    BackendUnavailable,
    PlaybackFailed,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ErrorResponse {
    pub code: ErrorResponseCode,
    pub message: String,
    pub retryable: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Heartbeat {
    pub nonce: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RawEnvelope {
    protocol_version: u16,
    message_id: Uuid,
    correlation_id: Option<Uuid>,
    sent_at_unix_ms: u64,
    kind: String,
    sequence: Option<u64>,
    payload: Value,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SerializableEnvelope<'a> {
    protocol_version: u16,
    message_id: Uuid,
    correlation_id: Option<Uuid>,
    sent_at_unix_ms: u64,
    kind: &'a str,
    sequence: Option<u64>,
    payload: Value,
}

pub fn decode_json(input: &[u8]) -> Result<Envelope, ProtocolError> {
    let raw: RawEnvelope =
        serde_json::from_slice(input).map_err(|error| ProtocolError::invalid(error.to_string()))?;
    if raw.protocol_version != PROTOCOL_VERSION {
        return Err(ProtocolError::unsupported(format!(
            "unsupported protocol version {}",
            raw.protocol_version
        )));
    }

    let message = match raw.kind.as_str() {
        "handshake.request" => Message::HandshakeRequest(payload(raw.payload)?),
        "handshake.response" => Message::HandshakeResponse(payload(raw.payload)?),
        "command.request" => Message::CommandRequest(payload(raw.payload)?),
        "command.ack" => Message::CommandAck(payload(raw.payload)?),
        "state.snapshot" => Message::StateSnapshot(payload(raw.payload)?),
        "playback.event" => Message::PlaybackEvent(payload(raw.payload)?),
        "error.response" => Message::ErrorResponse(payload(raw.payload)?),
        "ping" => Message::Ping(payload(raw.payload)?),
        "pong" => Message::Pong(payload(raw.payload)?),
        _ => return Err(ProtocolError::invalid("unknown message kind")),
    };

    let envelope = Envelope {
        protocol_version: raw.protocol_version,
        message_id: raw.message_id,
        correlation_id: raw.correlation_id,
        sent_at_unix_ms: raw.sent_at_unix_ms,
        sequence: raw.sequence,
        message,
    };
    validate(&envelope)?;
    Ok(envelope)
}

fn payload<T>(value: Value) -> Result<T, ProtocolError>
where
    T: for<'de> Deserialize<'de>,
{
    serde_json::from_value(value).map_err(|error| ProtocolError::invalid(error.to_string()))
}

fn validate(envelope: &Envelope) -> Result<(), ProtocolError> {
    match &envelope.message {
        Message::HandshakeRequest(request) => {
            require_request(envelope)?;
            require_text(&request.app_version, 128, "appVersion")?;
            if request.process_id == 0
                || request.supported_protocol_versions.is_empty()
                || !request
                    .supported_protocol_versions
                    .contains(&PROTOCOL_VERSION)
                || request
                    .supported_protocol_versions
                    .iter()
                    .copied()
                    .collect::<HashSet<_>>()
                    .len()
                    != request.supported_protocol_versions.len()
            {
                return Err(ProtocolError::invalid("invalid handshake request"));
            }
            require_nonce(&request.nonce)?;
        }
        Message::HandshakeResponse(response) => {
            require_response(envelope)?;
            require_text(&response.plugin_version, 128, "pluginVersion")?;
            if response.selected_protocol_version != PROTOCOL_VERSION
                || response.process_id == 0
                || response.capabilities.is_empty()
                || response
                    .capabilities
                    .iter()
                    .copied()
                    .collect::<HashSet<_>>()
                    .len()
                    != response.capabilities.len()
            {
                return Err(ProtocolError::invalid("invalid handshake response"));
            }
            require_nonce(&response.nonce)?;
        }
        Message::CommandRequest(request) => {
            require_request(envelope)?;
            validate_command(&request.command)?;
        }
        Message::CommandAck(ack) => {
            require_response(envelope)?;
            if !ack.accepted {
                return Err(ProtocolError::invalid(
                    "a rejected command must use error.response",
                ));
            }
            validate_snapshot(&ack.snapshot)?;
        }
        Message::StateSnapshot(snapshot) => {
            require_sequence(envelope, snapshot.revision)?;
            validate_snapshot(snapshot)?;
        }
        Message::PlaybackEvent(event) => {
            require_sequence(envelope, event.snapshot.revision)?;
            validate_snapshot(&event.snapshot)?;
            if (event.event == PlaybackEventKind::PlaybackError) != event.error_code.is_some() {
                return Err(ProtocolError::invalid("invalid playback event errorCode"));
            }
        }
        Message::ErrorResponse(error) => {
            require_response(envelope)?;
            require_text(&error.message, 512, "message")?;
        }
        Message::Ping(heartbeat) => {
            require_request(envelope)?;
            require_nonce(&heartbeat.nonce)?;
        }
        Message::Pong(heartbeat) => {
            require_response(envelope)?;
            require_nonce(&heartbeat.nonce)?;
        }
    }
    Ok(())
}

fn require_request(envelope: &Envelope) -> Result<(), ProtocolError> {
    if envelope.correlation_id.is_some() || envelope.sequence.is_some() {
        return Err(ProtocolError::invalid("request metadata is invalid"));
    }
    Ok(())
}

fn require_response(envelope: &Envelope) -> Result<(), ProtocolError> {
    if envelope.correlation_id.is_none() || envelope.sequence.is_some() {
        return Err(ProtocolError::invalid("response metadata is invalid"));
    }
    Ok(())
}

fn require_sequence(envelope: &Envelope, revision: u64) -> Result<(), ProtocolError> {
    if envelope.sequence != Some(revision) {
        return Err(ProtocolError::invalid(
            "event sequence must match snapshot revision",
        ));
    }
    Ok(())
}

fn require_nonce(nonce: &str) -> Result<(), ProtocolError> {
    if nonce.len() != 32
        || !nonce
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(ProtocolError::invalid(
            "nonce must be 32 lowercase hex digits",
        ));
    }
    Ok(())
}

fn require_text(value: &str, maximum: usize, field: &str) -> Result<(), ProtocolError> {
    if value.is_empty() || value.len() > maximum {
        return Err(ProtocolError::invalid(format!("invalid {field}")));
    }
    Ok(())
}

fn validate_command(command: &WireCommand) -> Result<(), ProtocolError> {
    match command {
        WireCommand::Play {
            path,
            start_ms,
            end_ms,
            ..
        } => {
            require_text(path, 32_767, "path")?;
            if matches!((start_ms, end_ms), (Some(start), Some(end)) if start >= end) {
                return Err(ProtocolError::invalid("invalid play range"));
            }
        }
        WireCommand::SetVolume { volume }
            if !volume.is_finite() || !(0.0..=1.0).contains(volume) =>
        {
            return Err(ProtocolError::invalid("invalid volume"));
        }
        _ => {}
    }
    Ok(())
}

fn validate_snapshot(snapshot: &StateSnapshot) -> Result<(), ProtocolError> {
    if !snapshot.volume.is_finite()
        || !(0.0..=1.0).contains(&snapshot.volume)
        || snapshot
            .duration_ms
            .is_some_and(|duration| snapshot.position_ms > duration)
        || (matches!(
            snapshot.status,
            PlaybackStatus::Playing | PlaybackStatus::Paused
        ) && snapshot.track_id.is_none())
    {
        return Err(ProtocolError::invalid("invalid playback snapshot"));
    }
    Ok(())
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SequenceTracker {
    last_applied: Option<u64>,
}

impl SequenceTracker {
    pub fn accept(&mut self, sequence: u64) -> bool {
        if self.last_applied.is_some_and(|last| sequence <= last) {
            return false;
        }
        self.last_applied = Some(sequence);
        true
    }

    #[must_use]
    pub const fn last_applied(&self) -> Option<u64> {
        self.last_applied
    }

    pub fn reset(&mut self) {
        self.last_applied = None;
    }
}
