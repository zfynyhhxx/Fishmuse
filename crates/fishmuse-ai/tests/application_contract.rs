use std::sync::Arc;

use fishmuse_ai::{
    AIApplicationEvent, AIEvent, AIService, AIServiceState, AIServiceStatus, AIServiceTurnRequest,
    AgentAIService, AgentLimits, AgentRunner, ContextEnvelope, CurrentViewContext, FakeAIProvider,
    FakeToolExecutor, MemoryAgentStore, NowPlayingContext, NowPlayingStatus, ResponseId,
    ServiceImplementation, ToolRegistry,
};
use fishmuse_domain::{ConversationId, TrackId, UserId};
use futures_util::StreamExt;
use serde_json::json;
use tokio_util::sync::CancellationToken;

fn service(provider: FakeAIProvider) -> (AgentAIService<FakeAIProvider>, FakeAIProvider) {
    let observed_provider = provider.clone();
    let runner = AgentRunner::new(
        provider,
        ToolRegistry::new(Arc::new(FakeToolExecutor::default())),
        Arc::new(MemoryAgentStore::default()),
        AgentLimits::default(),
    );
    let state = AIServiceState {
        status: AIServiceStatus::Ready,
        implementation: Some(ServiceImplementation {
            id: "deepseek".to_owned(),
            display_name: "DeepSeek".to_owned(),
        }),
    };
    (AgentAIService::new(runner, state), observed_provider)
}

fn request(context: Option<ContextEnvelope>) -> AIServiceTurnRequest {
    AIServiceTurnRequest {
        user_id: UserId::new(),
        conversation_id: ConversationId::new(),
        user_text: "What is selected?".to_owned(),
        context,
        cancellation: CancellationToken::new(),
    }
}

#[tokio::test]
async fn context_is_optional_structured_redacted_and_untrusted() {
    let provider = FakeAIProvider::scripted(vec![vec![Ok(AIEvent::Completed(ResponseId::new(
        "response-one",
    )))]]);
    let (service, observed_provider) = service(provider);
    let context = ContextEnvelope {
        contract_version: 1,
        current_view: Some(CurrentViewContext::Library),
        selected_entity: None,
        selected_text: Some(r#"C:\Users\alice\Music\private.flac"#.to_owned()),
        now_playing: Some(NowPlayingContext {
            track_id: Some(TrackId::new()),
            status: NowPlayingStatus::Playing,
            position_ms: 1_250,
            duration_ms: Some(180_000),
        }),
    };

    let turn = service.start_turn(request(Some(context)));
    let _events: Vec<_> = turn.events.collect().await;

    let requests = observed_provider.requests();
    let context_message = requests[0]
        .messages
        .iter()
        .find(|message| message.content.contains("untrusted_fishmuse_context"))
        .expect("structured context message");
    let value: serde_json::Value =
        serde_json::from_str(&context_message.content).expect("context JSON");
    assert_eq!(value["type"], "untrusted_fishmuse_context");
    assert_eq!(value["context"]["contract_version"], 1);
    assert_eq!(value["context"]["current_view"], "library");
    assert_eq!(value["context"]["selected_text"], "[REDACTED]");
    let encoded = context_message.content.to_ascii_lowercase();
    assert!(!encoded.contains("c:\\users"));
    assert!(!encoded.contains("api_key"));
    assert!(!encoded.contains("technical_context"));
}

#[tokio::test]
async fn events_are_versioned_turn_scoped_and_strictly_sequenced() {
    let provider = FakeAIProvider::scripted(vec![vec![
        Ok(AIEvent::TextDelta("hello".to_owned())),
        Ok(AIEvent::Completed(ResponseId::new("provider-response"))),
    ]]);
    let (service, _) = service(provider);

    let turn = service.start_turn(request(None));
    let turn_id = turn.turn_id;
    let events: Vec<_> = turn
        .events
        .map(|event| event.expect("application event"))
        .collect()
        .await;

    assert_eq!(turn_id.as_uuid().get_version_num(), 7);
    assert_eq!(events.len(), 3);
    assert_eq!(
        events
            .iter()
            .map(|event| event.sequence)
            .collect::<Vec<_>>(),
        vec![1, 2, 3]
    );
    assert!(
        events
            .iter()
            .all(|event| { event.contract_version == 1 && event.turn_id == turn_id })
    );
    assert!(matches!(
        &events[1].event,
        AIApplicationEvent::TextDelta { delta } if delta == "hello"
    ));
    let encoded = serde_json::to_string(&events).expect("event JSON");
    assert!(!encoded.contains("provider-response"));
    assert!(!encoded.contains("deepseek"));
    let text_envelope = serde_json::to_value(&events[1]).expect("text envelope JSON");
    assert_eq!(text_envelope["event_type"], "text_delta");
    assert_eq!(text_envelope["payload"], json!({"delta": "hello"}));
    assert!(text_envelope.get("event").is_none());
    let decoded: fishmuse_ai::AIEventEnvelope =
        serde_json::from_value(text_envelope).expect("event envelope round trip");
    assert_eq!(decoded, events[1]);
    assert_eq!(
        serde_json::to_value(&events[2].event).expect("completed JSON"),
        json!({"event_type": "turn_completed"})
    );
}

#[tokio::test]
async fn fake_provider_substitutes_without_changing_the_application_contract() {
    let provider = FakeAIProvider::scripted(vec![vec![Ok(AIEvent::Completed(ResponseId::new(
        "fake-response",
    )))]]);
    let (service, _) = service(provider);
    let service: Arc<dyn AIService> = Arc::new(service);

    let turn = service.start_turn(request(None));
    let events: Vec<_> = turn
        .events
        .map(|event| event.expect("application event"))
        .collect()
        .await;

    assert_eq!(events.len(), 2);
    assert!(matches!(events[0].event, AIApplicationEvent::TurnStarted));
    assert!(matches!(events[1].event, AIApplicationEvent::TurnCompleted));
}

#[test]
fn ai_service_state_is_generic_and_serialization_safe() {
    let state = AIServiceState {
        status: AIServiceStatus::NotConfigured,
        implementation: None,
    };

    assert_eq!(
        serde_json::to_value(state).expect("state JSON"),
        json!({"status": "not_configured", "implementation": null})
    );
}

#[test]
fn context_rejects_unsupported_contract_versions() {
    let result = serde_json::from_value::<ContextEnvelope>(json!({
        "contract_version": 2,
        "current_view": "library",
        "selected_entity": null,
        "selected_text": null,
        "now_playing": null
    }));

    assert!(result.is_err());
}
