use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde_json::{json, Value};
use tracing::{debug, info, warn};

use super::{Tool, ToolContext};

pub struct HttpGetTool;

#[async_trait]
impl Tool for HttpGetTool {
    fn name(&self) -> &str {
        "http_get"
    }

    fn description(&self) -> &str {
        "Send an HTTP GET request to a URL and return the response. \
         Use this to fetch data or content from external APIs and web pages."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "url": {
                    "type": "string",
                    "description": "The URL to GET"
                }
            },
            "required": ["url"],
            "additionalProperties": false
        })
    }

    async fn execute(&self, params: Value, ctx: &ToolContext) -> Result<Value> {
        let url = params["url"]
            .as_str()
            .ok_or_else(|| anyhow!("http_get: missing 'url' parameter"))?;

        info!(url = %url, "http_get: sending request");
        ctx.log(format!("http_get: GET {url}")).await;

        let response = ctx.http.get(url).send().await?;

        let status = response.status();
        let text = response.text().await.unwrap_or_default();

        info!(status = %status, "http_get: response received");
        debug!(body = %text, "http_get: response body");
        ctx.log(format!("http_get: response status {status}")).await;

        let parsed: Value = serde_json::from_str(&text).unwrap_or(Value::String(text.clone()));

        if !status.is_success() {
            warn!(status = %status, body = %text, "http_get: non-success response");
            ctx.log(format!("http_get: non-success response: {text}")).await;
        }

        Ok(json!({
            "status": status.as_u16(),
            "body": parsed
        }))
    }
}
