use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tracing::{debug, warn};

const OPENROUTER_API_URL: &str = "https://openrouter.ai/api/v1/chat/completions";

// ── Request types ─────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Clone)]
pub struct Message {
    pub role: String,
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

impl Message {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: "system".into(),
            content: Some(content.into()),
            tool_calls: None,
            tool_call_id: None,
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: "user".into(),
            content: Some(content.into()),
            tool_calls: None,
            tool_call_id: None,
        }
    }

    pub fn assistant_tool_calls(tool_calls: Vec<ToolCall>) -> Self {
        Self {
            role: "assistant".into(),
            content: None,
            tool_calls: Some(tool_calls),
            tool_call_id: None,
        }
    }

    pub fn tool_result(tool_call_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: "tool".into(),
            content: Some(content.into()),
            tool_calls: None,
            tool_call_id: Some(tool_call_id.into()),
        }
    }
}

#[derive(Debug, Serialize, Clone)]
pub struct ToolDefinition {
    #[serde(rename = "type")]
    pub kind: String,
    pub function: FunctionDefinition,
}

#[derive(Debug, Serialize, Clone)]
pub struct FunctionDefinition {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

impl ToolDefinition {
    pub fn new(name: impl Into<String>, description: impl Into<String>, parameters: Value) -> Self {
        Self {
            kind: "function".to_string(),
            function: FunctionDefinition {
                name: name.into(),
                description: description.into(),
                parameters,
            },
        }
    }
}

#[derive(Debug, Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<ToolDefinition>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    response_format: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_choice: Option<String>,
}

// ── Response types ────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct ChatResponse {
    pub choices: Vec<Choice>,
}

#[derive(Debug, Deserialize)]
pub struct Choice {
    pub message: ResponseMessage,
    pub finish_reason: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ResponseMessage {
    pub role: String,
    pub content: Option<String>,
    pub tool_calls: Option<Vec<ToolCall>>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub function: FunctionCall,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct FunctionCall {
    pub name: String,
    pub arguments: String,
}

// ── Client ────────────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct LlmClient {
    http: reqwest::Client,
    api_key: String,
    pub model: String,
}

impl LlmClient {
    pub fn new(api_key: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            http: reqwest::Client::new(),
            api_key: api_key.into(),
            model: model.into(),
        }
    }

    /// Send a chat completion request with optional tools and structured output.
    /// Pass `model: Some("…")` to override the client's default model for this call only.
    pub async fn chat(
        &self,
        messages: Vec<Message>,
        tools: Option<Vec<ToolDefinition>>,
        response_format: Option<Value>,
        model: Option<&str>,
    ) -> Result<ResponseMessage> {
        let serialized_messages: Vec<Value> = messages
            .into_iter()
            .map(|m| serde_json::to_value(m).unwrap())
            .collect();

        let tool_choice = tools.as_ref().map(|_| "auto".to_string());
        let n_tools = tools.as_ref().map(|t| t.len()).unwrap_or(0);
        let n_messages = serialized_messages.len();
        let effective_model = model.unwrap_or(&self.model);

        debug!(
            model = %effective_model,
            messages = n_messages,
            tools = n_tools,
            has_response_format = response_format.is_some(),
            "→ LLM request"
        );

        let req = ChatRequest {
            model: effective_model.to_string(),
            messages: serialized_messages,
            tools,
            response_format,
            tool_choice,
        };

        let resp = self
            .http
            .post(OPENROUTER_API_URL)
            .bearer_auth(&self.api_key)
            .header("HTTP-Referer", "https://github.com/ai-devs4")
            .header("X-Title", "AI Devs 4 Agent")
            .json(&req)
            .send()
            .await?;

        let status = resp.status();
        debug!(status = %status, "← LLM response received");

        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            warn!(status = %status, body = %body, "LLM API error");
            return Err(anyhow!("OpenRouter API error {status}: {body}"));
        }

        let chat_resp: ChatResponse = resp.json().await?;
        let message = chat_resp
            .choices
            .into_iter()
            .next()
            .map(|c| c.message)
            .ok_or_else(|| anyhow!("No choices returned from OpenRouter"))?;

        let has_content = message.content.is_some();
        let n_tool_calls = message.tool_calls.as_ref().map(|t| t.len()).unwrap_or(0);
        debug!(
            has_content,
            tool_calls = n_tool_calls,
            "← LLM response parsed"
        );
        if n_tool_calls > 0 {
            if let Some(calls) = &message.tool_calls {
                for tc in calls {
                    debug!(
                        id = %tc.id,
                        tool = %tc.function.name,
                        args = %tc.function.arguments,
                        "  tool_call"
                    );
                }
            }
        }

        Ok(message)
    }

    /// Convenience: plain text completion (no tools).
    /// Pass `model: Some("…")` to override the client's default model for this call only.
    pub async fn complete(&self, system: &str, user: &str, model: Option<&str>) -> Result<String> {
        let msg = self
            .chat(
                vec![Message::system(system), Message::user(user)],
                None,
                None,
                model,
            )
            .await?;
        msg.content
            .ok_or_else(|| anyhow!("LLM returned no text content"))
    }

    /// Structured output: returns parsed JSON value.
    /// Pass `model: Some("…")` to override the client's default model for this call only.
    pub async fn complete_structured(
        &self,
        system: &str,
        user: &str,
        schema_name: &str,
        schema: Value,
        model: Option<&str>,
    ) -> Result<Value> {
        let response_format = serde_json::json!({
            "type": "json_schema",
            "json_schema": {
                "name": schema_name,
                "strict": true,
                "schema": schema
            }
        });

        let msg = self
            .chat(
                vec![Message::system(system), Message::user(user)],
                None,
                Some(response_format),
                model,
            )
            .await?;

        let text = msg
            .content
            .ok_or_else(|| anyhow!("LLM returned no text content for structured output"))?;

        debug!(schema = %schema_name, raw = %text, "structured output raw");
        serde_json::from_str(&text).map_err(|e| anyhow!("Failed to parse structured output: {e}\nRaw: {text}"))
    }
}
