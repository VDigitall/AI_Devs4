use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde_json::{json, Value};
use tracing::{debug, info, warn};

use super::{Tool, ToolContext};

pub struct HttpPostTool;

#[async_trait]
impl Tool for HttpPostTool {
    fn name(&self) -> &str {
        "http_post"
    }

    fn description(&self) -> &str {
        "Send an HTTP POST request with a JSON body to a URL and return the response as JSON. \
         Use this to submit answers or interact with external APIs."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "url": {
                    "type": "string",
                    "description": "The URL to POST to"
                },
                "body": {
                    "type": "object",
                    "description": "The JSON body to send"
                }
            },
            "required": ["url", "body"],
            "additionalProperties": false
        })
    }

    async fn execute(&self, params: Value, ctx: &ToolContext) -> Result<Value> {
        let url = params["url"]
            .as_str()
            .ok_or_else(|| anyhow!("http_post: missing 'url' parameter"))?;

        let body = &params["body"];

        info!(url = %url, "http_post: sending request");
        debug!(body = %body, "http_post: request body");
        ctx.log(format!("http_post: POST to {url}")).await;

        let response = ctx.http.post(url).json(body).send().await?;

        let status = response.status();
        let text = response.text().await.unwrap_or_default();

        info!(status = %status, "http_post: response received");
        debug!(body = %text, "http_post: response body");
        ctx.log(format!("http_post: response status {status}")).await;

        let parsed: Value = serde_json::from_str(&text).unwrap_or(Value::String(text.clone()));

        if !status.is_success() {
            warn!(status = %status, body = %text, "http_post: non-success response");
            ctx.log(format!("http_post: non-success response: {text}")).await;
        }

        Ok(json!({
            "status": status.as_u16(),
            "body": parsed
        }))
    }
}
