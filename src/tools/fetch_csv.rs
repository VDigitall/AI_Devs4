use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde_json::{json, Value};

use super::{Tool, ToolContext};

pub struct FetchCsvTool;

#[async_trait]
impl Tool for FetchCsvTool {
    fn name(&self) -> &str {
        "fetch_csv"
    }

    fn description(&self) -> &str {
        "Download a CSV file from a URL and return its rows as a JSON array of objects. \
         Each row becomes an object with keys taken from the CSV header."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "url": {
                    "type": "string",
                    "description": "The URL of the CSV file to download"
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

        ctx.log(format!("Fetching CSV from: {url}")).await;

        let response = ctx.http.get(url).send().await?;
        if !response.status().is_success() {
            return Err(anyhow!(
                "fetch_csv: HTTP {} for {url}",
                response.status()
            ));
        }

        let text = response.text().await?;
        let mut reader = csv::Reader::from_reader(text.as_bytes());

        let headers: Vec<String> = reader
            .headers()?
            .iter()
            .map(|h| h.trim().to_string())
            .collect();

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

        ctx.log(format!("Fetched {} rows from CSV", rows.len())).await;
        Ok(Value::Array(rows))
    }
}
