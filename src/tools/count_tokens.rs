use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde_json::{json, Value};
use tiktoken_rs::cl100k_base;
use tracing::info;

use super::{Tool, ToolContext};

/// cl100k_base is a good universal approximation for any modern LLM
/// (GPT-4, GPT-3.5, Gemini, Claude, Llama, Mistral, etc.).
fn count_tokens(text: &str) -> Result<usize> {
    Ok(cl100k_base()?.encode_with_special_tokens(text).len())
}

pub struct CountTokensTool;

#[async_trait]
impl Tool for CountTokensTool {
    fn name(&self) -> &str {
        "count_tokens"
    }

    fn description(&self) -> &str {
        "Count the number of tokens in a text string or a list of chat messages. \
         Uses the cl100k_base tokenizer as a universal approximation compatible with \
         any modern LLM (GPT-4, GPT-4o, o1, o3, Gemini, Claude, Llama, Mistral, etc.). \
         Useful for estimating API costs and checking whether a prompt fits within a \
         context window. Returns the total token count and, for message lists, a \
         per-message breakdown."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "text": {
                    "type": "string",
                    "description": "A single text string to tokenize and count."
                },
                "messages": {
                    "type": "array",
                    "description": "A list of chat messages to tokenize. Each message's 'role' and 'content' fields are concatenated and counted. Provide either 'text' or 'messages', not both.",
                    "items": {
                        "type": "object",
                        "properties": {
                            "role": {
                                "type": "string",
                                "description": "Message role (e.g. 'system', 'user', 'assistant')"
                            },
                            "content": {
                                "type": "string",
                                "description": "Message content"
                            }
                        },
                        "required": ["role", "content"],
                        "additionalProperties": false
                    }
                },
            },
            "additionalProperties": false
        })
    }

    async fn execute(&self, params: Value, ctx: &ToolContext) -> Result<Value> {
        let (total_tokens, breakdown) = if let Some(text) = params["text"].as_str() {
            if params["messages"].is_array() {
                return Err(anyhow!(
                    "count_tokens: provide either 'text' or 'messages', not both"
                ));
            }
            let count = count_tokens(text)?;
            info!(tokens = count, "count_tokens: counted text tokens");
            ctx.log(format!("count_tokens: {count} tokens in text")).await;
            (count, None)
        } else if let Some(messages) = params["messages"].as_array() {
            if messages.is_empty() {
                return Err(anyhow!("count_tokens: 'messages' array must not be empty"));
            }

            let mut total = 0usize;
            let mut per_message: Vec<Value> = Vec::with_capacity(messages.len());

            for (i, msg) in messages.iter().enumerate() {
                let role = msg["role"]
                    .as_str()
                    .ok_or_else(|| anyhow!("count_tokens: message[{i}] missing 'role'"))?;
                let content = msg["content"]
                    .as_str()
                    .ok_or_else(|| anyhow!("count_tokens: message[{i}] missing 'content'"))?;

                // 4-token overhead per message matches the OpenAI chat completion convention
                // and is a reasonable approximation for other model families too.
                let message_tokens = count_tokens(&format!("{role}\n{content}"))? + 4;
                total += message_tokens;

                per_message.push(json!({
                    "role": role,
                    "tokens": message_tokens
                }));
            }

            // 3-token primer for the assistant reply turn.
            total += 3;

            info!(tokens = total, messages = messages.len(), "count_tokens: counted message tokens");
            ctx.log(format!(
                "count_tokens: {total} tokens across {} messages",
                messages.len()
            ))
            .await;

            (total, Some(per_message))
        } else {
            return Err(anyhow!(
                "count_tokens: provide either a 'text' string or a 'messages' array"
            ));
        };

        let mut result = json!({
            "token_count": total_tokens,
            "tokenizer": "cl100k_base"
        });

        if let Some(breakdown) = breakdown {
            result["per_message"] = Value::Array(breakdown);
        }

        Ok(result)
    }
}
