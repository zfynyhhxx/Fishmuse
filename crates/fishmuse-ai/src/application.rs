use std::{fmt, pin::Pin, sync::Arc};

use fishmuse_domain::{AppResult, ArtistId, ConversationId, ReleaseId, TrackId, UserId};
use futures_core::Stream;
use futures_util::StreamExt;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::json;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::{
    AIMessage, AIMessageRole, AIProvider, AIUsage, AgentEvent, AgentRunner, AgentTurnRequest,
    TurnFailureReason, redact_for_ai,
};

pub const AI_APPLICATION_CONTRACT_VERSION: u16 = 1;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize)]
#[serde(transparent)]
pub struct AITurnId(Uuid);

impl AITurnId {
    #[must_use]
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        Self(Uuid::now_v7())
    }

    #[must_use]
    pub const fn as_uuid(self) -> Uuid {
        self.0
    }
}

impl<'de> Deserialize<'de> for AITurnId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Uuid::deserialize(deserializer)?;
        if value.get_version_num() == 7 {
            Ok(Self(value))
        } else {
            Err(serde::de::Error::custom("AI turn IDs must use UUID v7"))
        }
    }
}

impl fmt::Display for AITurnId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CurrentViewContext {
    Library,
    AskFishMuse,
    NowPlaying,
    Settings,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "entity_type", rename_all = "snake_case")]
pub enum SelectedEntityContext {
    Artist {
        artist_id: ArtistId,
        display_name: String,
    },
    Release {
        release_id: ReleaseId,
        display_title: String,
    },
    Track {
        track_id: TrackId,
        display_title: String,
    },
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NowPlayingStatus {
    Stopped,
    Loading,
    Playing,
    Paused,
    Unavailable,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NowPlayingContext {
    pub track_id: Option<TrackId>,
    pub status: NowPlayingStatus,
    pub position_ms: u64,
    pub duration_ms: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ContextEnvelope {
    pub contract_version: u16,
    pub current_view: Option<CurrentViewContext>,
    pub selected_entity: Option<SelectedEntityContext>,
    pub selected_text: Option<String>,
    pub now_playing: Option<NowPlayingContext>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ContextEnvelopeWire {
    contract_version: u16,
    current_view: Option<CurrentViewContext>,
    selected_entity: Option<SelectedEntityContext>,
    selected_text: Option<String>,
    now_playing: Option<NowPlayingContext>,
}

impl<'de> Deserialize<'de> for ContextEnvelope {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = ContextEnvelopeWire::deserialize(deserializer)?;
        if wire.contract_version != AI_APPLICATION_CONTRACT_VERSION {
            return Err(serde::de::Error::custom(
                "unsupported AI context contract version",
            ));
        }
        Ok(Self {
            contract_version: wire.contract_version,
            current_view: wire.current_view,
            selected_entity: wire.selected_entity,
            selected_text: wire.selected_text,
            now_playing: wire.now_playing,
        })
    }
}

impl ContextEnvelope {
    pub(crate) fn into_untrusted_message(self) -> AIMessage {
        let value = serde_json::to_value(self).unwrap_or(serde_json::Value::Null);
        AIMessage {
            role: AIMessageRole::User,
            content: json!({
                "type": "untrusted_fishmuse_context",
                "context": redact_for_ai(value),
            })
            .to_string(),
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AIServiceStatus {
    Ready,
    NotConfigured,
    Unavailable,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ServiceImplementation {
    pub id: String,
    pub display_name: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AIServiceState {
    pub status: AIServiceStatus,
    pub implementation: Option<ServiceImplementation>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AIApplicationFailureReason {
    Provider,
    Tool,
    ToolLimit,
    Cancelled,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "event_type", content = "payload", rename_all = "snake_case")]
pub enum AIApplicationEvent {
    TurnStarted,
    TextDelta {
        delta: String,
    },
    ToolStarted {
        id: String,
        name: String,
    },
    ToolFinished {
        id: String,
        name: String,
        result: serde_json::Value,
    },
    Usage {
        input_tokens: u64,
        cached_input_tokens: u64,
        output_tokens: u64,
    },
    TurnCompleted,
    TurnFailed {
        reason: AIApplicationFailureReason,
    },
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct AIEventEnvelope {
    pub contract_version: u16,
    pub turn_id: AITurnId,
    pub sequence: u64,
    #[serde(flatten)]
    pub event: AIApplicationEvent,
}

pub struct AIServiceTurnRequest {
    pub user_id: UserId,
    pub conversation_id: ConversationId,
    pub user_text: String,
    pub context: Option<ContextEnvelope>,
    pub cancellation: CancellationToken,
}

pub type AIApplicationEventStream = Pin<Box<dyn Stream<Item = AppResult<AIEventEnvelope>> + Send>>;

pub struct AIServiceTurn {
    pub turn_id: AITurnId,
    pub events: AIApplicationEventStream,
}

pub trait AIService: Send + Sync {
    fn state(&self) -> AIServiceState;
    fn start_turn(&self, request: AIServiceTurnRequest) -> AIServiceTurn;
}

pub struct AgentAIService<P: AIProvider> {
    runner: Arc<AgentRunner<P>>,
    state: AIServiceState,
}

impl<P: AIProvider + 'static> AgentAIService<P> {
    #[must_use]
    pub fn new(runner: AgentRunner<P>, state: AIServiceState) -> Self {
        Self {
            runner: Arc::new(runner),
            state,
        }
    }
}

impl<P: AIProvider + 'static> AIService for AgentAIService<P> {
    fn state(&self) -> AIServiceState {
        self.state.clone()
    }

    fn start_turn(&self, request: AIServiceTurnRequest) -> AIServiceTurn {
        let turn_id = AITurnId::new();
        let mut sequence = 0_u64;
        let events = self.runner.run_turn(AgentTurnRequest {
            user_id: request.user_id,
            conversation_id: request.conversation_id,
            user_text: request.user_text,
            context: request.context,
            cancellation: request.cancellation,
        });
        let events = events.map(move |event| {
            sequence = sequence.saturating_add(1);
            event.map(|event| AIEventEnvelope {
                contract_version: AI_APPLICATION_CONTRACT_VERSION,
                turn_id,
                sequence,
                event: map_agent_event(event),
            })
        });
        AIServiceTurn {
            turn_id,
            events: Box::pin(events),
        }
    }
}

fn map_agent_event(event: AgentEvent) -> AIApplicationEvent {
    match event {
        AgentEvent::TurnStarted => AIApplicationEvent::TurnStarted,
        AgentEvent::TextDelta(delta) => AIApplicationEvent::TextDelta { delta },
        AgentEvent::ToolStarted { id, name } => AIApplicationEvent::ToolStarted {
            id: id.as_str().to_owned(),
            name,
        },
        AgentEvent::ToolFinished { id, name, result } => AIApplicationEvent::ToolFinished {
            id: id.as_str().to_owned(),
            name,
            result,
        },
        AgentEvent::Usage(AIUsage {
            input_tokens,
            cached_input_tokens,
            output_tokens,
        }) => AIApplicationEvent::Usage {
            input_tokens,
            cached_input_tokens,
            output_tokens,
        },
        AgentEvent::TurnCompleted => AIApplicationEvent::TurnCompleted,
        AgentEvent::TurnFailed { reason } => AIApplicationEvent::TurnFailed {
            reason: match reason {
                TurnFailureReason::Provider => AIApplicationFailureReason::Provider,
                TurnFailureReason::Tool => AIApplicationFailureReason::Tool,
                TurnFailureReason::ToolLimit => AIApplicationFailureReason::ToolLimit,
                TurnFailureReason::Cancelled => AIApplicationFailureReason::Cancelled,
            },
        },
    }
}
