#![cfg(windows)]

use std::{env, fs, path::PathBuf};

use fishmuse_ai::{
    AIEvent, AIMessage, AIMessageRole, AIProvider, AIRequest, AITool, CostEstimate,
    CredentialStore, DeepSeekClient, DeepSeekConfig, ProviderId, WindowsCredentialStore,
    deepseek_flash_cny_schedule, estimate_cost,
};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::json;
use time::OffsetDateTime;

#[derive(Default, Deserialize, Serialize)]
struct Ledger {
    spent_microunits: u64,
}

const WARNING_AT: u64 = 10_000_000;
const HARD_STOP_AT: u64 = 20_000_000;

fn ledger_path() -> PathBuf {
    PathBuf::from(env::var_os("LOCALAPPDATA").expect("Windows LOCALAPPDATA"))
        .join("FishMuse")
        .join("live-tests")
        .join("deepseek-budget.json")
}

fn read_ledger(path: &PathBuf) -> Ledger {
    let bytes = fs::read(path).expect("live budget ledger must exist");
    let json = bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(&bytes);
    serde_json::from_slice(json).expect("live budget ledger must be valid JSON")
}

#[test]
fn live_budget_ledger_accepts_utf8_bom_from_windows_powershell() {
    let path = env::temp_dir().join(format!(
        "fishmuse-deepseek-ledger-{}-{}.json",
        std::process::id(),
        OffsetDateTime::now_utc().unix_timestamp_nanos()
    ));
    fs::write(&path, b"\xef\xbb\xbf{\"spent_microunits\":0}")
        .expect("write PowerShell-style ledger fixture");

    let ledger = read_ledger(&path);

    fs::remove_file(path).expect("remove ledger fixture");
    assert_eq!(ledger.spent_microunits, 0);
}

fn assert_live_gate(path: &PathBuf) {
    assert_eq!(
        env::var("FISHMUSE_LIVE_DEEPSEEK_APPROVAL").as_deref(),
        Ok("interactive-approved"),
        "run the guarded PowerShell live-test script"
    );
    let spent = read_ledger(path).spent_microunits;
    assert!(spent < HARD_STOP_AT, "DeepSeek live-test hard stop reached");
    assert!(
        spent < WARNING_AT
            || env::var("FISHMUSE_LIVE_DEEPSEEK_WARNING_APPROVAL").as_deref()
                == Ok("interactive-approved"),
        "warning threshold requires a fresh interactive confirmation"
    );
}

async fn request(client: &DeepSeekClient, request: AIRequest) -> Vec<AIEvent> {
    client
        .stream(request)
        .map(|event| event.expect("DeepSeek live stream event"))
        .collect()
        .await
}

fn record_usage(events: &[AIEvent], ledger_path: &PathBuf) {
    let mut ledger = read_ledger(ledger_path);
    let schedule = deepseek_flash_cny_schedule();
    for usage in events.iter().filter_map(|event| match event {
        AIEvent::Usage(usage) => Some(usage),
        _ => None,
    }) {
        let CostEstimate::Known(cost) =
            estimate_cost(&schedule, Some(usage), OffsetDateTime::now_utc())
                .expect("DeepSeek usage must be billable")
        else {
            panic!("DeepSeek usage cost must be known");
        };
        ledger.spent_microunits = ledger
            .spent_microunits
            .checked_add(cost.0)
            .expect("live budget ledger overflow");
    }
    fs::write(
        ledger_path,
        serde_json::to_vec_pretty(&ledger).expect("serialize live budget ledger"),
    )
    .expect("persist live budget ledger");
}

#[tokio::test]
#[ignore = "spends real DeepSeek credit; run scripts/test-live-deepseek.ps1 explicitly"]
async fn deepseek_text_stream_and_usage_live() {
    let ledger_path = ledger_path();
    assert_live_gate(&ledger_path);
    let key = WindowsCredentialStore::new()
        .load_api_key(ProviderId::deepseek())
        .await
        .expect("read DeepSeek credential")
        .expect("DeepSeek key is not configured in Windows Credential Manager");
    let client = DeepSeekClient::new(key, DeepSeekConfig::default()).expect("DeepSeek client");

    let text_events = request(
        &client,
        AIRequest {
            messages: vec![AIMessage {
                role: AIMessageRole::User,
                content: "Reply with exactly: FishMuse live stream OK".to_owned(),
            }],
            ..AIRequest::default()
        },
    )
    .await;
    assert!(
        text_events
            .iter()
            .any(|event| matches!(event, AIEvent::TextDelta(_)))
    );
    assert!(
        text_events
            .iter()
            .any(|event| matches!(event, AIEvent::Usage(_)))
    );
    assert!(
        text_events
            .iter()
            .any(|event| matches!(event, AIEvent::Completed(_)))
    );
    record_usage(&text_events, &ledger_path);
}

#[tokio::test]
#[ignore = "spends real DeepSeek credit; run scripts/test-live-deepseek.ps1 explicitly"]
async fn deepseek_search_tool_and_usage_live() {
    let ledger_path = ledger_path();
    assert_live_gate(&ledger_path);
    let key = WindowsCredentialStore::new()
        .load_api_key(ProviderId::deepseek())
        .await
        .expect("read DeepSeek credential")
        .expect("DeepSeek key is not configured in Windows Credential Manager");
    let client = DeepSeekClient::new(key, DeepSeekConfig::default()).expect("DeepSeek client");
    let tool_events = request(
        &client,
        AIRequest {
            instructions: Some(
                "Call search_library exactly once for the user's request. Do not answer before calling it."
                    .to_owned(),
            ),
            messages: vec![AIMessage {
                role: AIMessageRole::User,
                content: "Find Miles Davis in my library.".to_owned(),
            }],
            tools: vec![AITool {
                name: "search_library".to_owned(),
                description: "Search the user's local music library.".to_owned(),
                parameters: json!({
                    "type": "object",
                    "properties": { "query": { "type": "string" } },
                    "required": ["query"],
                    "additionalProperties": false
                }),
            }],
            ..AIRequest::default()
        },
    )
    .await;
    assert!(tool_events.iter().any(|event| {
        matches!(event, AIEvent::ToolCallCompleted(call) if call.name == "search_library")
    }));
    assert!(
        tool_events
            .iter()
            .any(|event| matches!(event, AIEvent::Usage(_)))
    );
    record_usage(&tool_events, &ledger_path);
}
