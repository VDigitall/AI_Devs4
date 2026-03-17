use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde_json::{json, Value};
use tracing::{debug, info};

use super::{Tool, ToolContext};

pub struct DownloadFileTool;

#[async_trait]
impl Tool for DownloadFileTool {
    fn name(&self) -> &str {
        "download_file"
    }

    fn description(&self) -> &str {
        "Download a file from a URL and save it to the artifacts directory. \
         Returns the path to the saved file. \
         If no filename is provided, the filename is inferred from the URL. \
         Parent directories are created automatically."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "url": {
                    "type": "string",
                    "description": "The URL of the file to download"
                },
                "filename": {
                    "type": "string",
                    "description": "Optional filename to save the file as inside the artifacts directory (e.g. 'data.zip'). If omitted, the filename is derived from the URL."
                }
            },
            "required": ["url"],
            "additionalProperties": false
        })
    }

    async fn execute(&self, params: Value, ctx: &ToolContext) -> Result<Value> {
        let url = params["url"]
            .as_str()
            .ok_or_else(|| anyhow!("download_file: missing 'url' parameter"))?;

        let filename = if let Some(name) = params["filename"].as_str() {
            name.to_string()
        } else {
            url.split('/')
                .last()
                .filter(|s| !s.is_empty())
                .unwrap_or("downloaded_file")
                .to_string()
        };

        let filepath = format!("artifacts/{filename}");

        info!(url = %url, filepath = %filepath, "download_file: downloading");
        ctx.log(format!("Downloading file from: {url}")).await;

        let response = ctx.http.get(url).send().await?;
        let status = response.status();
        debug!(status = %status, "download_file: HTTP response");
        if !status.is_success() {
            return Err(anyhow!("download_file: HTTP {status} for {url}"));
        }

        let bytes = response.bytes().await?;
        debug!(bytes = bytes.len(), "download_file: response body size");

        std::fs::create_dir_all("artifacts")
            .map_err(|e| anyhow!("download_file: failed to create artifacts directory: {e}"))?;

        std::fs::write(&filepath, &bytes)
            .map_err(|e| anyhow!("download_file: failed to save file '{filepath}': {e}"))?;

        info!(filepath = %filepath, bytes = bytes.len(), "download_file: file saved");
        ctx.log(format!("File saved to: {filepath} ({} bytes)", bytes.len())).await;

        Ok(json!({ "path": filepath }))
    }
}
