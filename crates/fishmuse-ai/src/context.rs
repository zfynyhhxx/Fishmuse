use std::sync::Arc;

use async_trait::async_trait;
use fishmuse_domain::{AppResult, ConversationId, UserId};
use fishmuse_storage::ConversationRepository;
use serde_json::json;
use time::OffsetDateTime;

use crate::{
    AIMessage, AIMessageRole, AIUsage, CostEstimate, deepseek_flash_cny_schedule, estimate_cost,
    redact_for_ai,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentToolResult {
    pub name: String,
    pub result: serde_json::Value,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AgentTurnStatus {
    Completed,
    Failed,
    Cancelled,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentTurnRecord {
    pub user_id: UserId,
    pub conversation_id: ConversationId,
    pub user_text: String,
    pub assistant_text: String,
    pub tool_results: Vec<AgentToolResult>,
    pub status: AgentTurnStatus,
    pub usage: Option<AIUsage>,
    pub provider: String,
    pub model: String,
    pub occurred_at: OffsetDateTime,
}

#[async_trait]
pub trait AgentStore: Send + Sync {
    async fn load_context(&self, _conversation_id: ConversationId) -> AppResult<Vec<AIMessage>> {
        Ok(Vec::new())
    }

    async fn save_turn(&self, record: AgentTurnRecord) -> AppResult<()>;
}

pub struct ConversationAgentStore<R: ConversationRepository> {
    repository: Arc<R>,
}

impl<R: ConversationRepository> ConversationAgentStore<R> {
    #[must_use]
    pub fn new(repository: Arc<R>) -> Self {
        Self { repository }
    }
}

#[async_trait]
impl<R: ConversationRepository> AgentStore for ConversationAgentStore<R> {
    async fn load_context(&self, conversation_id: ConversationId) -> AppResult<Vec<AIMessage>> {
        let messages = self.repository.messages(conversation_id).await?;
        let tool_count = messages
            .iter()
            .filter(|message| message.role == "tool")
            .count();
        let mut tools_to_skip = tool_count.saturating_sub(20);
        let mut context = Vec::new();
        for message in messages {
            match message.role.as_str() {
                "user" => context.push(AIMessage {
                    role: AIMessageRole::User,
                    content: safe_text(&message.content),
                }),
                "assistant" => context.push(AIMessage {
                    role: AIMessageRole::Assistant,
                    content: assistant_text(&message.content),
                }),
                "tool" if tools_to_skip > 0 => tools_to_skip -= 1,
                "tool" => {
                    let value = serde_json::from_str(&message.content)
                        .unwrap_or(serde_json::Value::String(message.content));
                    context.push(AIMessage {
                        role: AIMessageRole::User,
                        content: json!({
                            "type": "untrusted_historical_tool_data",
                            "result": redact_for_ai(value),
                        })
                        .to_string(),
                    });
                }
                _ => {}
            }
        }
        Ok(context)
    }

    async fn save_turn(&self, record: AgentTurnRecord) -> AppResult<()> {
        self.repository
            .append_message(record.conversation_id, "user", &record.user_text)
            .await?;
        for tool_result in &record.tool_results {
            let content = json!({
                "name": tool_result.name,
                "result": redact_for_ai(tool_result.result.clone()),
            });
            self.repository
                .append_message(record.conversation_id, "tool", &content.to_string())
                .await?;
        }
        let estimated_cost = match estimate_cost(
            &deepseek_flash_cny_schedule(),
            record.usage.as_ref(),
            record.occurred_at,
        ) {
            Ok(CostEstimate::Known(value)) => Some(value.0),
            Ok(CostEstimate::Unknown) | Err(_) => None,
        };
        let status = match record.status {
            AgentTurnStatus::Completed => "completed",
            AgentTurnStatus::Failed => "failed",
            AgentTurnStatus::Cancelled => "cancelled",
        };
        let envelope = json!({
            "text": record.assistant_text,
            "provider": record.provider,
            "model": record.model,
            "status": status,
            "usage": record.usage.map(|usage| json!({
                "input_tokens": usage.input_tokens,
                "cached_input_tokens": usage.cached_input_tokens,
                "output_tokens": usage.output_tokens,
            })),
            "estimated_cost_microunits": estimated_cost,
        });
        self.repository
            .append_message(record.conversation_id, "assistant", &envelope.to_string())
            .await?;
        Ok(())
    }
}

fn assistant_text(content: &str) -> String {
    serde_json::from_str::<serde_json::Value>(content)
        .ok()
        .and_then(|value| {
            value
                .get("text")
                .and_then(serde_json::Value::as_str)
                .map(safe_text)
        })
        .unwrap_or_else(|| safe_text(content))
}

fn safe_text(content: &str) -> String {
    match redact_for_ai(serde_json::Value::String(content.to_owned())) {
        serde_json::Value::String(value) => value,
        _ => "[REDACTED]".to_owned(),
    }
}
