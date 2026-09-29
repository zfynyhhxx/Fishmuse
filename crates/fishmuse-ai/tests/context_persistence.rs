use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use fishmuse_ai::{
    AIUsage, AgentStore, AgentToolResult, AgentTurnRecord, AgentTurnStatus, ConversationAgentStore,
};
use fishmuse_domain::{AppResult, ConversationId, UserId};
use fishmuse_storage::{ConversationMessage, ConversationRepository};
use time::macros::datetime;
use uuid::Uuid;

#[derive(Default)]
struct FakeConversationRepository {
    messages: Mutex<Vec<(String, String)>>,
}

#[async_trait]
impl ConversationRepository for FakeConversationRepository {
    async fn create(&self, _id: ConversationId, _title: Option<&str>) -> AppResult<()> {
        Ok(())
    }

    async fn append_message(
        &self,
        _conversation_id: ConversationId,
        role: &str,
        content: &str,
    ) -> AppResult<Uuid> {
        self.messages
            .lock()
            .expect("messages")
            .push((role.to_owned(), content.to_owned()));
        Ok(Uuid::now_v7())
    }

    async fn messages(
        &self,
        _conversation_id: ConversationId,
    ) -> AppResult<Vec<ConversationMessage>> {
        Ok(self
            .messages
            .lock()
            .expect("messages")
            .iter()
            .enumerate()
            .map(|(index, (role, content))| ConversationMessage {
                id: Uuid::now_v7(),
                role: role.clone(),
                content: content.clone(),
                created_at: time::OffsetDateTime::from_unix_timestamp(index as i64)
                    .expect("timestamp"),
            })
            .collect())
    }
}

#[tokio::test]
async fn conversation_adapter_persists_safe_turn_metadata_and_estimated_cost() {
    let repository = Arc::new(FakeConversationRepository::default());
    let store = ConversationAgentStore::new(repository.clone());
    store
        .save_turn(AgentTurnRecord {
            user_id: UserId::new(),
            conversation_id: ConversationId::new(),
            user_text: "hello".to_owned(),
            assistant_text: "world".to_owned(),
            tool_results: vec![AgentToolResult {
                name: "search_library".to_owned(),
                result: serde_json::json!({
                    "title": "safe",
                    "path": "C:\\private\\track.flac",
                    "api_key": "fixture-secret"
                }),
            }],
            status: AgentTurnStatus::Completed,
            usage: Some(AIUsage {
                input_tokens: 10,
                cached_input_tokens: 2,
                output_tokens: 3,
            }),
            provider: "deepseek".to_owned(),
            model: "deepseek-flash".to_owned(),
            occurred_at: datetime!(2026-09-30 00:00 UTC),
        })
        .await
        .expect("persist");

    {
        let messages = repository.messages.lock().expect("messages");
        assert_eq!(messages[0], ("user".to_owned(), "hello".to_owned()));
        assert_eq!(messages[1].0, "tool");
        assert!(messages[1].1.contains("search_library"));
        assert!(messages[1].1.contains("safe"));
        assert!(!messages[1].1.contains("private"));
        assert!(!messages[1].1.contains("fixture-secret"));
        assert_eq!(messages[2].0, "assistant");
        let envelope: serde_json::Value =
            serde_json::from_str(&messages[2].1).expect("metadata envelope");
        assert_eq!(envelope["text"], "world");
        assert_eq!(envelope["provider"], "deepseek");
        assert_eq!(envelope["model"], "deepseek-flash");
        assert_eq!(envelope["status"], "completed");
        assert!(envelope["estimated_cost_microunits"].as_u64().is_some());
        assert!(!messages[2].1.contains("api_key"));
        assert!(!messages[2].1.contains("technical_context"));
    }

    for index in 0..21 {
        repository.messages.lock().expect("messages").push((
            "tool".to_owned(),
            serde_json::json!({"index": index, "note": "read C:\\private\\track.flac"}).to_string(),
        ));
    }
    let context = store
        .load_context(ConversationId::new())
        .await
        .expect("context");
    let tool_context: Vec<_> = context
        .iter()
        .filter(|message| message.content.contains("untrusted_historical_tool_data"))
        .collect();
    assert_eq!(tool_context.len(), 20);
    assert!(
        !tool_context
            .iter()
            .any(|message| message.content.contains("\"index\":0"))
    );
    assert!(
        !tool_context
            .iter()
            .any(|message| message.content.contains("private"))
    );
}
