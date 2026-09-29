use std::sync::Arc;

use fishmuse_ai::{
    AIEvent, AIProviderError, AIUsage, AgentEvent, AgentLimits, AgentRunner, AgentTurnRequest,
    AgentTurnStatus, FakeAIProvider, FakeToolExecutor, MemoryAgentStore, ResponseId, ToolCall,
    ToolCallId, ToolRegistry, TurnFailureReason,
};
use fishmuse_domain::{ConversationId, UserId};
use futures_util::StreamExt;
use serde_json::json;
use tokio_util::sync::CancellationToken;

fn usage() -> AIUsage {
    AIUsage {
        input_tokens: 10,
        cached_input_tokens: 2,
        output_tokens: 3,
    }
}

fn text_response(id: &str, text: &str) -> Vec<Result<AIEvent, AIProviderError>> {
    vec![
        Ok(AIEvent::TextDelta(text.to_owned())),
        Ok(AIEvent::Usage(usage())),
        Ok(AIEvent::Completed(ResponseId::new(id))),
    ]
}

fn tool_response(
    id: &str,
    name: &str,
    arguments: serde_json::Value,
) -> Vec<Result<AIEvent, AIProviderError>> {
    let call_id = ToolCallId::new(id);
    let encoded = arguments.to_string();
    vec![
        Ok(AIEvent::ToolCallStarted(call_id.clone(), name.to_owned())),
        Ok(AIEvent::ToolArgumentsDelta(call_id.clone(), encoded)),
        Ok(AIEvent::ToolCallCompleted(ToolCall {
            id: call_id,
            name: name.to_owned(),
            arguments,
        })),
        Ok(AIEvent::Usage(usage())),
        Ok(AIEvent::Completed(ResponseId::new(format!(
            "response-{id}"
        )))),
    ]
}

fn request(cancellation: CancellationToken) -> AgentTurnRequest {
    AgentTurnRequest {
        user_id: UserId::new(),
        conversation_id: ConversationId::new(),
        user_text: "播放一些音乐".to_owned(),
        context: None,
        cancellation,
    }
}

fn runner(
    scripts: Vec<Vec<Result<AIEvent, AIProviderError>>>,
    executor: Arc<FakeToolExecutor>,
) -> (AgentRunner<FakeAIProvider>, Arc<MemoryAgentStore>) {
    let store = Arc::new(MemoryAgentStore::default());
    let registry = ToolRegistry::new(executor);
    (
        AgentRunner::new(
            FakeAIProvider::scripted(scripts),
            registry,
            store.clone(),
            AgentLimits::default(),
        ),
        store,
    )
}

async fn collect(
    runner: &AgentRunner<FakeAIProvider>,
    request: AgentTurnRequest,
) -> Vec<AgentEvent> {
    runner
        .run_turn(request)
        .map(|event| event.expect("agent event"))
        .collect()
        .await
}

#[tokio::test]
async fn streams_a_text_only_answer_and_persists_completion() {
    let executor = Arc::new(FakeToolExecutor::default());
    let (runner, store) = runner(vec![text_response("one", "你好")], executor);
    let events = collect(&runner, request(CancellationToken::new())).await;

    assert!(matches!(events.first(), Some(AgentEvent::TurnStarted)));
    assert!(
        events
            .iter()
            .any(|event| matches!(event, AgentEvent::TextDelta(value) if value == "你好"))
    );
    assert!(matches!(events.last(), Some(AgentEvent::TurnCompleted)));
    let turns = store.turns();
    assert_eq!(turns.len(), 1);
    assert_eq!(turns[0].status, AgentTurnStatus::Completed);
    assert_eq!(turns[0].assistant_text, "你好");
}

#[tokio::test]
async fn executes_search_then_continues_with_the_tool_result() {
    let executor = Arc::new(FakeToolExecutor::default());
    executor.set_result(
        "search_library",
        json!({"tracks": [{"id": "safe-id", "title": "Jóga"}]}),
    );
    let scripts = vec![
        tool_response(
            "search",
            "search_library",
            json!({"query": "Björk", "limit": 5}),
        ),
        text_response("answer", "找到了 Jóga"),
    ];
    let (runner, store) = runner(scripts, executor.clone());
    let events = collect(&runner, request(CancellationToken::new())).await;

    assert_eq!(executor.call_names(), vec!["search_library"]);
    assert!(events.iter().any(
        |event| matches!(event, AgentEvent::ToolFinished { name, .. } if name == "search_library")
    ));
    assert!(
        events
            .iter()
            .any(|event| matches!(event, AgentEvent::TextDelta(value) if value == "找到了 Jóga"))
    );
    let turns = store.turns();
    assert_eq!(turns[0].tool_results.len(), 1);
    assert_eq!(turns[0].tool_results[0].name, "search_library");
    assert_eq!(
        turns[0].tool_results[0].result["tracks"][0]["title"],
        "Jóga"
    );
}

#[tokio::test]
async fn can_query_then_play_a_track_using_only_its_logical_id() {
    let executor = Arc::new(FakeToolExecutor::default());
    let track_id = fishmuse_domain::TrackId::new().as_uuid().to_string();
    executor.set_result(
        "search_library",
        json!({"tracks": [{"id": track_id, "title": "Track"}]}),
    );
    executor.set_result("play_track", json!({"status": "playing"}));
    let scripts = vec![
        tool_response("search", "search_library", json!({"query": "Track"})),
        tool_response("play", "play_track", json!({"track_id": track_id})),
        text_response("done", "开始播放"),
    ];
    let (runner, _) = runner(scripts, executor.clone());
    collect(&runner, request(CancellationToken::new())).await;

    assert_eq!(executor.call_names(), vec!["search_library", "play_track"]);
    let calls = executor.arguments_for("play_track");
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0]["track_id"], track_id);
}

#[tokio::test]
async fn executes_multiple_tools_in_sequence() {
    let executor = Arc::new(FakeToolExecutor::default());
    executor.set_result("get_playback_state", json!({"status": "playing"}));
    executor.set_result("pause_playback", json!({"status": "paused"}));
    let scripts = vec![
        tool_response("state", "get_playback_state", json!({})),
        tool_response("pause", "pause_playback", json!({})),
        text_response("done", "已暂停"),
    ];
    let (runner, _) = runner(scripts, executor.clone());
    collect(&runner, request(CancellationToken::new())).await;
    assert_eq!(
        executor.call_names(),
        vec!["get_playback_state", "pause_playback"]
    );
}

#[tokio::test]
async fn refuses_the_seventh_tool_call_and_finishes_safely() {
    let executor = Arc::new(FakeToolExecutor::default());
    executor.set_result("get_playback_state", json!({"status": "stopped"}));
    let scripts = (0..7)
        .map(|index| tool_response(&format!("call-{index}"), "get_playback_state", json!({})))
        .collect();
    let (runner, store) = runner(scripts, executor.clone());
    let events = collect(&runner, request(CancellationToken::new())).await;

    assert_eq!(executor.call_names().len(), 6);
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, AgentEvent::ToolStarted { .. }))
            .count(),
        6
    );
    assert!(matches!(
        events.last(),
        Some(AgentEvent::TurnFailed {
            reason: TurnFailureReason::ToolLimit
        })
    ));
    assert_eq!(store.turns()[0].status, AgentTurnStatus::Failed);
}

#[tokio::test]
async fn provider_interruption_persists_partial_failed_state() {
    let executor = Arc::new(FakeToolExecutor::default());
    let scripts = vec![vec![
        Ok(AIEvent::TextDelta("部分回答".to_owned())),
        Err(AIProviderError::StreamInterrupted),
    ]];
    let (runner, store) = runner(scripts, executor);
    let events = collect(&runner, request(CancellationToken::new())).await;

    assert!(matches!(
        events.last(),
        Some(AgentEvent::TurnFailed {
            reason: TurnFailureReason::Provider
        })
    ));
    assert_eq!(store.turns()[0].status, AgentTurnStatus::Failed);
    assert_eq!(store.turns()[0].assistant_text, "部分回答");
}

#[tokio::test]
async fn provider_auth_and_rate_limit_failures_remain_actionable() {
    for (error, expected) in [
        (
            AIProviderError::Unauthorized,
            TurnFailureReason::ProviderUnauthorized,
        ),
        (
            AIProviderError::RateLimited { retry_after: None },
            TurnFailureReason::ProviderRateLimited,
        ),
    ] {
        let executor = Arc::new(FakeToolExecutor::default());
        let (runner, _) = runner(vec![vec![Err(error)]], executor);
        let events = collect(&runner, request(CancellationToken::new())).await;

        assert!(matches!(
            events.last(),
            Some(AgentEvent::TurnFailed { reason }) if *reason == expected
        ));
    }
}

#[tokio::test]
async fn cancellation_prevents_tools_that_have_not_started() {
    let cancellation = CancellationToken::new();
    let executor = Arc::new(FakeToolExecutor::default());
    executor.set_result("get_playback_state", json!({"status": "playing"}));
    executor.cancel_after_call(cancellation.clone(), 1);
    let scripts = vec![
        tool_response("state", "get_playback_state", json!({})),
        tool_response("pause", "pause_playback", json!({})),
    ];
    let (runner, store) = runner(scripts, executor.clone());
    let events = collect(&runner, request(cancellation)).await;

    assert_eq!(executor.call_names(), vec!["get_playback_state"]);
    assert!(matches!(
        events.last(),
        Some(AgentEvent::TurnFailed {
            reason: TurnFailureReason::Cancelled
        })
    ));
    assert_eq!(store.turns()[0].status, AgentTurnStatus::Cancelled);
}
