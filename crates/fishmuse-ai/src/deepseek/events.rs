use std::collections::HashMap;

use serde::Deserialize;
use serde_json::Value;

use crate::{AIEvent, AIProviderError, AIUsage, ResponseId, ToolCall, ToolCallId};

const MAX_EVENT_BYTES: usize = 256 * 1024;

#[derive(Debug)]
struct PendingToolCall {
    id: ToolCallId,
    name: String,
    arguments: String,
}

#[derive(Debug, Default)]
pub struct DeepSeekEventDecoder {
    last_sequence: Option<u64>,
    pending_calls: HashMap<String, PendingToolCall>,
    completed: bool,
}

impl DeepSeekEventDecoder {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn decode_json_line(&mut self, line: &str) -> Result<Vec<AIEvent>, AIProviderError> {
        if line.len() > MAX_EVENT_BYTES {
            return Err(AIProviderError::ResponseTooLarge);
        }
        let value: Value =
            serde_json::from_str(line).map_err(|_| protocol("malformed JSON event"))?;
        let event = value
            .get("event")
            .or_else(|| value.get("type"))
            .and_then(Value::as_str)
            .ok_or_else(|| protocol("event type is missing"))?;
        let sequence_number = value
            .get("sequence_number")
            .and_then(Value::as_u64)
            .ok_or_else(|| protocol("sequence number is missing"))?;
        if self.completed {
            return Err(protocol("event received after completion"));
        }
        if self
            .last_sequence
            .is_some_and(|previous| sequence_number <= previous)
        {
            return Err(protocol("duplicate or out-of-order sequence"));
        }
        self.last_sequence = Some(sequence_number);

        match event {
            "response.output_text.delta" => {
                let event: TextDeltaEvent = from_value(value)?;
                Ok(vec![AIEvent::TextDelta(event.delta)])
            }
            "response.output_item.added" => self.output_item_added(value),
            "response.function_call_arguments.delta" => self.arguments_delta(value),
            "response.function_call_arguments.done" => self.arguments_done(value),
            "response.completed" => self.response_completed(value),
            "response.failed" | "response.incomplete" => Err(AIProviderError::StreamInterrupted),
            _ => Ok(Vec::new()),
        }
    }

    pub fn finish(self) -> Result<(), AIProviderError> {
        if self.completed && self.pending_calls.is_empty() {
            Ok(())
        } else {
            Err(AIProviderError::StreamInterrupted)
        }
    }

    fn output_item_added(&mut self, value: Value) -> Result<Vec<AIEvent>, AIProviderError> {
        let event: OutputItemAddedEvent = from_value(value)?;
        if event.item.kind != "function_call" {
            return Ok(Vec::new());
        }
        if event.item.id.is_empty()
            || event.item.name.is_empty()
            || self.pending_calls.contains_key(&event.item.id)
        {
            return Err(protocol("invalid or duplicate tool call"));
        }
        let id = ToolCallId::new(event.item.id.clone());
        let name = event.item.name;
        self.pending_calls.insert(
            event.item.id,
            PendingToolCall {
                id: id.clone(),
                name: name.clone(),
                arguments: event.item.arguments,
            },
        );
        Ok(vec![AIEvent::ToolCallStarted(id, name)])
    }

    fn arguments_delta(&mut self, value: Value) -> Result<Vec<AIEvent>, AIProviderError> {
        let event: ArgumentsDeltaEvent = from_value(value)?;
        let call = self
            .pending_calls
            .get_mut(&event.item_id)
            .ok_or_else(|| protocol("arguments for unknown tool call"))?;
        call.arguments.push_str(&event.delta);
        Ok(vec![AIEvent::ToolArgumentsDelta(
            call.id.clone(),
            event.delta,
        )])
    }

    fn arguments_done(&mut self, value: Value) -> Result<Vec<AIEvent>, AIProviderError> {
        let event: ArgumentsDoneEvent = from_value(value)?;
        let call = self
            .pending_calls
            .remove(&event.item_id)
            .ok_or_else(|| protocol("completion for unknown tool call"))?;
        if call.arguments != event.arguments {
            return Err(protocol(
                "final tool arguments differ from streamed arguments",
            ));
        }
        let arguments = serde_json::from_str(&event.arguments)
            .map_err(|_| protocol("tool arguments are not valid JSON"))?;
        Ok(vec![AIEvent::ToolCallCompleted(ToolCall {
            id: call.id,
            name: call.name,
            arguments,
        })])
    }

    fn response_completed(&mut self, value: Value) -> Result<Vec<AIEvent>, AIProviderError> {
        let event: CompletedEvent = from_value(value)?;
        if !self.pending_calls.is_empty() {
            return Err(protocol("response completed with unfinished tool calls"));
        }
        if event.response.usage.cached_input_tokens() > event.response.usage.input_tokens {
            return Err(protocol("cached token count exceeds input token count"));
        }
        self.completed = true;
        Ok(vec![
            AIEvent::Usage(AIUsage {
                input_tokens: event.response.usage.input_tokens,
                cached_input_tokens: event.response.usage.cached_input_tokens(),
                output_tokens: event.response.usage.output_tokens,
            }),
            AIEvent::Completed(ResponseId::new(event.response.id)),
        ])
    }
}

pub fn decode_sse_fixture(input: &str) -> Result<Vec<AIEvent>, AIProviderError> {
    let mut decoder = DeepSeekEventDecoder::new();
    let mut result = Vec::new();
    for line in input.lines().filter(|line| !line.trim().is_empty()) {
        result.extend(decoder.decode_json_line(line)?);
    }
    decoder.finish()?;
    Ok(result)
}

fn protocol(reason: impl Into<String>) -> AIProviderError {
    AIProviderError::Protocol {
        reason: reason.into(),
    }
}

fn from_value<T: for<'de> Deserialize<'de>>(value: Value) -> Result<T, AIProviderError> {
    serde_json::from_value(value)
        .map_err(|error| protocol(format!("malformed known event: {error}")))
}

#[derive(Deserialize)]
struct TextDeltaEvent {
    delta: String,
}

#[derive(Deserialize)]
struct OutputItemAddedEvent {
    item: OutputItem,
}

#[derive(Deserialize)]
struct OutputItem {
    id: String,
    #[serde(rename = "type")]
    kind: String,
    name: String,
    #[serde(default)]
    arguments: String,
}

#[derive(Deserialize)]
struct ArgumentsDeltaEvent {
    item_id: String,
    delta: String,
}

#[derive(Deserialize)]
struct ArgumentsDoneEvent {
    item_id: String,
    arguments: String,
}

#[derive(Deserialize)]
struct CompletedEvent {
    response: CompletedResponse,
}

#[derive(Deserialize)]
struct CompletedResponse {
    id: String,
    usage: Usage,
}

#[derive(Deserialize)]
struct Usage {
    input_tokens: u64,
    #[serde(default)]
    input_tokens_details: InputTokenDetails,
    output_tokens: u64,
}

#[derive(Default, Deserialize)]
struct InputTokenDetails {
    #[serde(default)]
    cached_tokens: u64,
}

impl Usage {
    fn cached_input_tokens(&self) -> u64 {
        self.input_tokens_details.cached_tokens
    }
}
