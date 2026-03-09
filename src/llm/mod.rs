use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

const OPENROUTER_API_URL: &str = "https://openrouter.ai/api/v1/chat/completions";

// ── Request types ─────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Clone)]
pub struct Message {
    pub role: String,
    pub content: MessageContent,
}

#[derive(Debug, Serialize, Clone)]
#[serde(untagged)]
pub enum MessageContent {
    Text(String),
    ToolResult {
        tool_call_id: String,
        content: String,
    },
}

impl Message {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: "system".to_string(),
            content: MessageContent::Text(content.into()),
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: "user".to_string(),
            content: MessageContent::Text(content.into()),
        }
    }

    pub fn tool_result(tool_call_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: "tool".to_string(),
            content: MessageContent::ToolResult {
                tool_call_id: tool_call_id.into(),
                content: content.into(),
            },
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

#[derive(Debug, Deserialize, Clone)]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub function: FunctionCall,
}

#[derive(Debug, Deserialize, Clone)]
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
    pub async fn chat(
        &self,
        messages: Vec<Message>,
        tools: Option<Vec<ToolDefinition>>,
        response_format: Option<Value>,
    ) -> Result<ResponseMessage> {
        let serialized_messages: Vec<Value> = messages
            .into_iter()
            .map(|m| serde_json::to_value(m).unwrap())
            .collect();

        let tool_choice = tools.as_ref().map(|_| "auto".to_string());

        let req = ChatRequest {
            model: self.model.clone(),
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

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!("OpenRouter API error {status}: {body}"));
        }

        let chat_resp: ChatResponse = resp.json().await?;
        chat_resp
            .choices
            .into_iter()
            .next()
            .map(|c| c.message)
            .ok_or_else(|| anyhow!("No choices returned from OpenRouter"))
    }

    /// Convenience: plain text completion (no tools).
    pub async fn complete(&self, system: &str, user: &str) -> Result<String> {
        let msg = self
            .chat(
                vec![Message::system(system), Message::user(user)],
                None,
                None,
            )
            .await?;
        msg.content
            .ok_or_else(|| anyhow!("LLM returned no text content"))
    }

    /// Structured output: returns parsed JSON value.
    pub async fn complete_structured(
        &self,
        system: &str,
        user: &str,
        schema_name: &str,
        schema: Value,
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
            )
            .await?;

        let text = msg
            .content
            .ok_or_else(|| anyhow!("LLM returned no text content for structured output"))?;

        serde_json::from_str(&text).map_err(|e| anyhow!("Failed to parse structured output: {e}\nRaw: {text}"))
    }
}
