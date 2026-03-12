use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde_json::{json, Value};
use tracing::{debug, info};

use super::{Tool, ToolContext};

pub struct ReadMarkdownTool;

#[async_trait]
impl Tool for ReadMarkdownTool {
    fn name(&self) -> &str {
        "read_markdown"
    }

    fn description(&self) -> &str {
        "Read Markdown content from a local file or a remote URL. \
         When reading from a file the path must be inside the 'artifacts/' directory. \
         Returns the raw Markdown text."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "filepath": {
                    "type": "string",
                    "description": "Path to a local Markdown file. Must start with 'artifacts/' (e.g. 'artifacts/notes.md'). Mutually exclusive with 'url'."
                },
                "url": {
                    "type": "string",
                    "description": "HTTP/HTTPS URL of a Markdown file to fetch. Mutually exclusive with 'filepath'."
                }
            },
            "additionalProperties": false
        })
    }

    async fn execute(&self, params: Value, ctx: &ToolContext) -> Result<Value> {
        let filepath = params["filepath"].as_str();
        let url = params["url"].as_str();

        match (filepath, url) {
            (Some(_), Some(_)) => Err(anyhow!(
                "read_markdown: provide either 'filepath' or 'url', not both"
            )),

            (Some(path), None) => {
                let canonical = std::path::Path::new(path)
                    .components()
                    .collect::<std::path::PathBuf>();

                let first_component = canonical
                    .components()
                    .next()
                    .and_then(|c| c.as_os_str().to_str())
                    .unwrap_or("");

                if first_component != "artifacts" {
                    return Err(anyhow!(
                        "read_markdown: file access is restricted to the 'artifacts/' directory, \
                         got '{path}'"
                    ));
                }

                if canonical
                    .components()
                    .any(|c| matches!(c, std::path::Component::ParentDir))
                {
                    return Err(anyhow!(
                        "read_markdown: path traversal is not allowed (got '{path}')"
                    ));
                }

                debug!(path = %path, "read_markdown: reading from file");
                ctx.log(format!("read_markdown: reading from file '{path}'")).await;

                let content = std::fs::read_to_string(&canonical)
                    .map_err(|e| anyhow!("read_markdown: cannot read '{path}' – {e}"))?;

                info!(path = %path, "read_markdown: read file successfully");
                ctx.log(format!("read_markdown: read '{path}' successfully")).await;
                Ok(json!({ "content": content }))
            }

            (None, Some(url)) => {
                info!(url = %url, "read_markdown: fetching from URL");
                ctx.log(format!("read_markdown: fetching from URL {url}")).await;

                let response = ctx.http.get(url).send().await?;
                let status = response.status();
                debug!(status = %status, "read_markdown: HTTP response");
                if !status.is_success() {
                    return Err(anyhow!("read_markdown: HTTP {status} for {url}"));
                }

                let content = response.text().await?;
                info!(url = %url, bytes = content.len(), "read_markdown: fetched successfully");
                ctx.log(format!("read_markdown: fetched {} bytes from {url}", content.len())).await;

                let stem = url
                    .rsplit('/')
                    .next()
                    .and_then(|s| s.split('?').next())
                    .and_then(|s| std::path::Path::new(s).file_stem()?.to_str())
                    .unwrap_or("unknown");
                let path = format!("artifacts/{stem}.md");
                std::fs::create_dir_all("artifacts")
                    .map_err(|e| anyhow!("read_markdown: cannot create 'artifacts/' – {e}"))?;
                std::fs::write(&path, &content)
                    .map_err(|e| anyhow!("read_markdown: cannot write '{path}' – {e}"))?;
                info!(path = %path, "read_markdown: saved to file");
                ctx.log(format!("read_markdown: saved to file '{path}'")).await;

                Ok(json!({ "content": content }))
            }

            (None, None) => Err(anyhow!(
                "read_markdown: one of 'filepath' or 'url' is required"
            )),
        }
    }
}
