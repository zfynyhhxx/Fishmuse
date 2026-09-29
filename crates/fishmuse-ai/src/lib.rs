//! Provider-neutral AI streaming, credentials, and budget controls.

mod agent;
mod context;
mod cost;
mod credentials;
pub mod deepseek;
mod fake;
mod pricing;
mod provider;
mod redaction;
pub mod tools;

pub use agent::{AgentEvent, AgentLimits, AgentRunner, AgentTurnRequest, TurnFailureReason};
pub use context::{
    AgentStore, AgentToolResult, AgentTurnRecord, AgentTurnStatus, ConversationAgentStore,
};
pub use cost::{AtomicSpendLedger, BudgetDecision, CostPolicy, SpendOverflow};
#[cfg(windows)]
pub use credentials::WindowsCredentialStore;
pub use credentials::{CredentialStore, UnsupportedCredentialStore, credential_is_configured};
pub use deepseek::{
    DeepSeekClient, DeepSeekConfig, DeepSeekEventDecoder, classify_http_status, decode_sse_fixture,
};
pub use fake::{FakeAIProvider, FakeToolExecutor, MemoryAgentStore};
pub use pricing::{
    CostEstimate, Currency, MicroYuan, PeakWindow, PriceSchedule, PricingError, TokenRates,
    deepseek_flash_cny_schedule, estimate_cost,
};
pub use provider::{
    AIEvent, AIMessage, AIMessageRole, AIProvider, AIProviderError, AIRequest, AITool,
    AIToolOutput, AIUsage, ProviderId, ResponseId, ToolCall, ToolCallId,
};
pub use redaction::redact_for_ai;
pub use tools::{MusicToolExecutor, ToolExecutor, ToolRegistry};
