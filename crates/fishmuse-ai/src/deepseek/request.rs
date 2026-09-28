use serde::Serialize;

use crate::{AIRequest, AITool};

#[derive(Serialize)]
pub(super) struct DeepSeekRequest<'a> {
    model: &'a str,
    stream: bool,
    reasoning: Reasoning,
    #[serde(skip_serializing_if = "Option::is_none")]
    instructions: &'a Option<String>,
    input: &'a [crate::AIMessage],
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

impl<'a> DeepSeekRequest<'a> {
    pub(super) fn new(model: &'a str, request: &'a AIRequest) -> Self {
        Self {
            model,
            stream: true,
            reasoning: Reasoning { effort: "none" },
            instructions: &request.instructions,
            input: &request.messages,
            tools: request.tools.iter().map(DeepSeekTool::from).collect(),
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
        };

        let value = serde_json::to_value(DeepSeekRequest::new("deepseek-flash", &request))
            .expect("serialize");
        assert_eq!(value["model"], "deepseek-flash");
        assert_eq!(value["stream"], true);
        assert_eq!(value["reasoning"]["effort"], "none");
        assert_eq!(value["tools"][0]["type"], "function");
        assert_eq!(value["tools"][0]["name"], "search_library");
    }
}
