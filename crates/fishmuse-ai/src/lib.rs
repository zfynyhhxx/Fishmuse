//! Provider-neutral AI streaming, credentials, and budget controls.

mod cost;
mod credentials;
pub mod deepseek;
mod pricing;
mod provider;

pub use cost::{AtomicSpendLedger, BudgetDecision, CostPolicy, SpendOverflow};
#[cfg(windows)]
pub use credentials::WindowsCredentialStore;
pub use credentials::{CredentialStore, UnsupportedCredentialStore, credential_is_configured};
pub use deepseek::{
    DeepSeekClient, DeepSeekConfig, DeepSeekEventDecoder, classify_http_status, decode_sse_fixture,
};
pub use pricing::{
    CostEstimate, Currency, MicroYuan, PeakWindow, PriceSchedule, PricingError, TokenRates,
    deepseek_flash_cny_schedule, estimate_cost,
};
pub use provider::{
    AIEvent, AIMessage, AIMessageRole, AIProvider, AIProviderError, AIRequest, AITool, AIUsage,
    ProviderId, ResponseId, ToolCall, ToolCallId,
};
