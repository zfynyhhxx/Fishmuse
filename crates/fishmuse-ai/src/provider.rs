use std::{fmt, pin::Pin, time::Duration};

use futures_core::Stream;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct ProviderId(String);

impl ProviderId {
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    #[must_use]
    pub fn deepseek() -> Self {
        Self::new("deepseek")
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToolCallId(String);

impl ToolCallId {
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResponseId(String);

impl ResponseId {
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AIUsage {
    pub input_tokens: u64,
    pub cached_input_tokens: u64,
    pub output_tokens: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ToolCall {
    pub id: ToolCallId,
    pub name: String,
    pub arguments: Value,
}

#[derive(Clone, Debug, PartialEq)]
pub enum AIEvent {
    TextDelta(String),
    ToolCallStarted(ToolCallId, String),
    ToolArgumentsDelta(ToolCallId, String),
    ToolCallCompleted(ToolCall),
    Usage(AIUsage),
    Completed(ResponseId),
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum AIMessageRole {
    User,
    Assistant,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AIMessage {
    pub role: AIMessageRole,
    pub content: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct AITool {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AIToolOutput {
    pub call_id: String,
    pub output: String,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct AIRequest {
    pub instructions: Option<String>,
    pub messages: Vec<AIMessage>,
    pub tools: Vec<AITool>,
    pub previous_response_id: Option<String>,
    pub tool_outputs: Vec<AIToolOutput>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AIProviderError {
    Unauthorized,
    RateLimited { retry_after: Option<Duration> },
    Server { status: u16 },
    Rejected { status: u16 },
    Transport,
    Timeout,
    ResponseTooLarge,
    Protocol { reason: String },
    StreamInterrupted,
    InvalidConfiguration,
}

impl fmt::Display for AIProviderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unauthorized => formatter.write_str("AI provider rejected the credential"),
            Self::RateLimited { .. } => formatter.write_str("AI provider rate limit reached"),
            Self::Server { status } => write!(formatter, "AI provider server error ({status})"),
            Self::Rejected { status } => {
                write!(formatter, "AI provider rejected the request ({status})")
            }
            Self::Transport => formatter.write_str("AI provider transport failed"),
            Self::Timeout => formatter.write_str("AI provider timed out"),
            Self::ResponseTooLarge => {
                formatter.write_str("AI provider response exceeded its limit")
            }
            Self::Protocol { .. } => formatter.write_str("AI provider returned an invalid stream"),
            Self::StreamInterrupted => formatter.write_str("AI provider stream was interrupted"),
            Self::InvalidConfiguration => {
                formatter.write_str("AI provider configuration is invalid")
            }
        }
    }
}

impl std::error::Error for AIProviderError {}

pub trait AIProvider: Send + Sync {
    fn stream(
        &self,
        request: AIRequest,
    ) -> Pin<Box<dyn Stream<Item = Result<AIEvent, AIProviderError>> + Send>>;
}
