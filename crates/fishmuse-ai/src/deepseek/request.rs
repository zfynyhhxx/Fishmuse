use serde::Serialize;

use crate::{AIMessage, AIRequest, AITool, AIToolOutput};

#[derive(Serialize)]
pub(super) struct DeepSeekRequest<'a> {
    model: &'a str,
    stream: bool,
    reasoning: Reasoning,
    #[serde(skip_serializing_if = "Option::is_none")]
    instructions: &'a Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    previous_response_id: &'a Option<String>,
    input: Vec<DeepSeekInput<'a>>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tools: Vec<DeepSeekTool<'a>>,
}

#[derive(Serialize)]
struct Reasoning {
    effort: &'static str,
}

#[derive(Serialize)]
struct DeepSeekTool<'a> {
    #[serde(rename = "type")]
    kind: &'static str,
    name: &'a str,
    description: &'a str,
    parameters: &'a serde_json::Value,
}

#[derive(Serialize)]
#[serde(untagged)]
enum DeepSeekInput<'a> {
    Message(&'a AIMessage),
    ToolOutput(DeepSeekToolOutput<'a>),
}

#[derive(Serialize)]
struct DeepSeekToolOutput<'a> {
    #[serde(rename = "type")]
    kind: &'static str,
    call_id: &'a str,
    output: &'a str,
}

impl<'a> DeepSeekRequest<'a> {
    pub(super) fn new(model: &'a str, request: &'a AIRequest) -> Self {
        Self {
            model,
            stream: true,
            reasoning: Reasoning { effort: "none" },
            instructions: &request.instructions,
            previous_response_id: &request.previous_response_id,
            input: if request.previous_response_id.is_some() {
                request
                    .tool_outputs
                    .iter()
                    .map(|output| DeepSeekInput::ToolOutput(DeepSeekToolOutput::from(output)))
                    .collect()
            } else {
                request
                    .messages
                    .iter()
                    .map(DeepSeekInput::Message)
                    .collect()
            },
            tools: request.tools.iter().map(DeepSeekTool::from).collect(),
        }
    }
}

impl<'a> From<&'a AIToolOutput> for DeepSeekToolOutput<'a> {
    fn from(value: &'a AIToolOutput) -> Self {
        Self {
            kind: "function_call_output",
            call_id: &value.call_id,
            output: &value.output,
        }
    }
}

impl<'a> From<&'a AITool> for DeepSeekTool<'a> {
    fn from(value: &'a AITool) -> Self {
        Self {
            kind: "function",
            name: &value.name,
            description: &value.description,
            parameters: &value.parameters,
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::DeepSeekRequest;
    use crate::{AIMessage, AIMessageRole, AIRequest, AITool};

    #[test]
    fn serializes_provider_defaults_and_flat_function_tools() {
        let request = AIRequest {
            instructions: Some("Keep answers concise.".to_owned()),
            messages: vec![AIMessage {
                role: AIMessageRole::User,
                content: "Find a track".to_owned(),
            }],
            tools: vec![AITool {
                name: "search_library".to_owned(),
                description: "Search local music".to_owned(),
                parameters: json!({"type": "object"}),
            }],
            previous_response_id: None,
            tool_outputs: Vec::new(),
        };

        let value = serde_json::to_value(DeepSeekRequest::new("deepseek-flash", &request))
            .expect("serialize");
        assert_eq!(value["model"], "deepseek-flash");
        assert_eq!(value["stream"], true);
        assert_eq!(value["reasoning"]["effort"], "none");
        assert_eq!(value["tools"][0]["type"], "function");
        assert_eq!(value["tools"][0]["name"], "search_library");
    }

    #[test]
    fn serializes_tool_outputs_with_previous_response_id() {
        let request = AIRequest {
            previous_response_id: Some("resp_previous".to_owned()),
            tool_outputs: vec![crate::AIToolOutput {
                call_id: "call_01".to_owned(),
                output: "{\"tracks\":[]}".to_owned(),
            }],
            ..AIRequest::default()
        };
        let value = serde_json::to_value(DeepSeekRequest::new("deepseek-flash", &request))
            .expect("serialize");
        assert_eq!(value["previous_response_id"], "resp_previous");
        assert_eq!(value["input"][0]["type"], "function_call_output");
        assert_eq!(value["input"][0]["call_id"], "call_01");
    }
}
