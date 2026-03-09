use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde_json::{json, Value};
use tracing::{debug, info};

use super::{Tool, ToolContext};

pub struct FetchCsvTool;

#[async_trait]
impl Tool for FetchCsvTool {
    fn name(&self) -> &str {
        "fetch_csv"
    }

    fn description(&self) -> &str {
        "Download a CSV file from a URL and return its rows as a JSON array of objects. \
         Each row becomes an object with keys taken from the CSV header. \
         Optionally saves the raw CSV to disk as an artifact (parent directories are created automatically)."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "url": {
                    "type": "string",
                    "description": "The URL of the CSV file to download"
                },
                "filepath": {
                    "type": "string",
                    "description": "Optional path to save the raw CSV artifact (e.g. 'artifacts/people.csv'). Parent directories are created automatically."
                }
            },
            "required": ["url"],
            "additionalProperties": false
        })
    }

    async fn execute(&self, params: Value, ctx: &ToolContext) -> Result<Value> {
        let url = params["url"]
            .as_str()
            .ok_or_else(|| anyhow!("fetch_csv: missing 'url' parameter"))?;

        info!(url = %url, "fetch_csv: downloading");
        ctx.log(format!("Fetching CSV from: {url}")).await;

        let response = ctx.http.get(url).send().await?;
        let status = response.status();
        debug!(status = %status, "fetch_csv: HTTP response");
        if !status.is_success() {
            return Err(anyhow!("fetch_csv: HTTP {status} for {url}"));
        }

        let text = response.text().await?;
        debug!(bytes = text.len(), "fetch_csv: response body size");

        if let Some(filepath) = params["filepath"].as_str() {
            if let Some(parent) = std::path::Path::new(filepath).parent() {
                if !parent.as_os_str().is_empty() {
                    std::fs::create_dir_all(parent)
                        .map_err(|e| anyhow!("fetch_csv: failed to create directories for '{filepath}': {e}"))?;
                }
            }
            std::fs::write(filepath, &text)
                .map_err(|e| anyhow!("fetch_csv: failed to save artifact '{filepath}': {e}"))?;
            info!(filepath = %filepath, "fetch_csv: artifact saved");
            ctx.log(format!("Saved CSV artifact to: {filepath}")).await;
        }

        let mut reader = csv::Reader::from_reader(text.as_bytes());

        let headers: Vec<String> = reader
            .headers()?
            .iter()
            .map(|h| h.trim().to_string())
            .collect();
        debug!(headers = ?headers, "fetch_csv: CSV columns");

        let mut rows: Vec<Value> = Vec::new();
        for result in reader.records() {
            let record = result?;
            let mut obj = serde_json::Map::new();
            for (i, field) in record.iter().enumerate() {
                if let Some(key) = headers.get(i) {
                    obj.insert(key.clone(), Value::String(field.trim().to_string()));
                }
            }
            rows.push(Value::Object(obj));
        }

        info!(rows = rows.len(), "fetch_csv: done");
        ctx.log(format!("Fetched {} rows from CSV", rows.len())).await;
        Ok(Value::Array(rows))
    }
}
