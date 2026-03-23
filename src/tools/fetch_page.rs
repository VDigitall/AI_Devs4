use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde_json::{json, Value};
use tracing::{debug, info, warn};

use super::{Tool, ToolContext};

pub struct FetchPageTool;

#[async_trait]
impl Tool for FetchPageTool {
    fn name(&self) -> &str {
        "fetch_page"
    }

    fn description(&self) -> &str {
        "Fetch an HTML page from a URL, convert it to Markdown, save the result inside \
         the 'artifacts/' directory, and return the Markdown content. \
         Use this instead of 'http_get' when the target is a human-readable web page \
         (HTML) rather than a JSON/plain-text API endpoint."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "url": {
                    "type": "string",
                    "description": "HTTP/HTTPS URL of the HTML page to fetch."
                },
                "filename": {
                    "type": "string",
                    "description": "Optional filename (without extension) to use when saving \
                                    the Markdown artifact, e.g. 'homepage'. \
                                    Defaults to a name derived from the URL."
                }
            },
            "required": ["url"],
            "additionalProperties": false
        })
    }

    async fn execute(&self, params: Value, ctx: &ToolContext) -> Result<Value> {
        let url = params["url"]
            .as_str()
            .ok_or_else(|| anyhow!("fetch_page: missing 'url' parameter"))?;

        info!(url = %url, "fetch_page: fetching HTML");
        ctx.log(format!("fetch_page: GET {url}")).await;

        let response = ctx
            .http
            .get(url)
            .header("User-Agent", "Mozilla/5.0 (compatible; AI-Agent/1.0)")
            .send()
            .await?;

        let status = response.status();
        debug!(status = %status, "fetch_page: HTTP response");

        if !status.is_success() {
            warn!(status = %status, url = %url, "fetch_page: non-success response");
            return Err(anyhow!("fetch_page: HTTP {status} for {url}"));
        }

        let html = response.text().await?;
        info!(url = %url, bytes = html.len(), "fetch_page: HTML received");
        ctx.log(format!("fetch_page: received {} bytes of HTML", html.len())).await;

        let markdown = htmd::convert(&html)
            .map_err(|e| anyhow!("fetch_page: HTML-to-Markdown conversion failed - {e}"))?;

        let stem = if let Some(name) = params["filename"].as_str() {
            name.to_string()
        } else {
            url.trim_end_matches('/')
                .rsplit('/')
                .next()
                .and_then(|s| s.split('?').next())
                .and_then(|s| {
                    let p = std::path::Path::new(s);
                    p.file_stem()?.to_str().map(|v| v.to_string())
                })
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "page".to_string())
        };

        let artifact_path = format!("artifacts/{stem}.md");

        std::fs::create_dir_all("artifacts")
            .map_err(|e| anyhow!("fetch_page: cannot create 'artifacts/' - {e}"))?;

        std::fs::write(&artifact_path, &markdown)
            .map_err(|e| anyhow!("fetch_page: cannot write '{artifact_path}' - {e}"))?;

        info!(path = %artifact_path, "fetch_page: saved Markdown artifact");
        ctx.log(format!("fetch_page: saved to '{artifact_path}'")).await;

        Ok(json!({
            "artifact": artifact_path,
            "content": markdown
        }))
    }
}
