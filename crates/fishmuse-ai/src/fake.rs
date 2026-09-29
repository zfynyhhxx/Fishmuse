use std::{
    collections::{HashMap, VecDeque},
    pin::Pin,
    sync::{Arc, Mutex},
};

use async_trait::async_trait;
use fishmuse_domain::AppResult;
use futures_core::Stream;
use serde_json::Value;
use tokio_util::sync::CancellationToken;

use crate::{
    AIEvent, AIProvider, AIProviderError, AIRequest, AgentStore, AgentTurnRecord, ToolExecutor,
};

type ProviderScript = Vec<Result<AIEvent, AIProviderError>>;
type ProviderScripts = Arc<Mutex<VecDeque<ProviderScript>>>;

#[derive(Clone)]
pub struct FakeAIProvider {
    scripts: ProviderScripts,
    requests: Arc<Mutex<Vec<AIRequest>>>,
}

impl FakeAIProvider {
    #[must_use]
    pub fn scripted(scripts: Vec<Vec<Result<AIEvent, AIProviderError>>>) -> Self {
        Self {
            scripts: Arc::new(Mutex::new(scripts.into())),
            requests: Arc::new(Mutex::new(Vec::new())),
        }
    }

    #[must_use]
    pub fn requests(&self) -> Vec<AIRequest> {
        self.requests.lock().expect("fake requests lock").clone()
    }
}

impl AIProvider for FakeAIProvider {
    fn stream(
        &self,
        request: AIRequest,
    ) -> Pin<Box<dyn Stream<Item = Result<AIEvent, AIProviderError>> + Send>> {
        self.requests
            .lock()
            .expect("fake requests lock")
            .push(request);
        let script = self
            .scripts
            .lock()
            .expect("fake provider lock")
            .pop_front()
            .unwrap_or_else(|| vec![Err(AIProviderError::StreamInterrupted)]);
        Box::pin(futures_util::stream::iter(script))
    }
}

#[derive(Default)]
pub struct FakeToolExecutor {
    results: Mutex<HashMap<String, Value>>,
    calls: Mutex<Vec<(String, Value)>>,
    cancellation: Mutex<Option<(CancellationToken, usize)>>,
}

impl FakeToolExecutor {
    pub fn set_result(&self, name: &str, result: Value) {
        self.results
            .lock()
            .expect("fake result lock")
            .insert(name.to_owned(), result);
    }

    #[must_use]
    pub fn call_names(&self) -> Vec<String> {
        self.calls
            .lock()
            .expect("fake calls lock")
            .iter()
            .map(|(name, _)| name.clone())
            .collect()
    }

    #[must_use]
    pub fn arguments_for(&self, name: &str) -> Vec<Value> {
        self.calls
            .lock()
            .expect("fake calls lock")
            .iter()
            .filter(|(called, _)| called == name)
            .map(|(_, arguments)| arguments.clone())
            .collect()
    }

    pub fn cancel_after_call(&self, token: CancellationToken, count: usize) {
        *self.cancellation.lock().expect("fake cancellation lock") = Some((token, count));
    }
}

#[async_trait]
impl ToolExecutor for FakeToolExecutor {
    async fn execute(&self, name: &str, arguments: &Value) -> AppResult<Value> {
        let call_count = {
            let mut calls = self.calls.lock().expect("fake calls lock");
            calls.push((name.to_owned(), arguments.clone()));
            calls.len()
        };
        if let Some((token, count)) = &*self.cancellation.lock().expect("fake cancellation lock")
            && call_count >= *count
        {
            token.cancel();
        }
        Ok(self
            .results
            .lock()
            .expect("fake result lock")
            .get(name)
            .cloned()
            .unwrap_or(Value::Null))
    }
}

#[derive(Default)]
pub struct MemoryAgentStore {
    turns: Mutex<Vec<AgentTurnRecord>>,
}

impl MemoryAgentStore {
    #[must_use]
    pub fn turns(&self) -> Vec<AgentTurnRecord> {
        self.turns.lock().expect("agent store lock").clone()
    }
}

#[async_trait]
impl AgentStore for MemoryAgentStore {
    async fn save_turn(&self, record: AgentTurnRecord) -> AppResult<()> {
        self.turns.lock().expect("agent store lock").push(record);
        Ok(())
    }
}
