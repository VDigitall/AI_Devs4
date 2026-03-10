use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde_json::{json, Value};
use tracing::{debug, info};

use super::{Tool, ToolContext};

pub struct ParseJsonTool;

#[async_trait]
impl Tool for ParseJsonTool {
    fn name(&self) -> &str {
        "parse_json"
    }

    fn description(&self) -> &str {
        "Parse JSON either from a raw string or from a file. \
         When reading from a file the path must be inside the 'artifacts/' directory. \
         Returns the parsed JSON value."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "json_string": {
                    "type": "string",
                    "description": "A raw JSON string to parse. Mutually exclusive with 'filepath'."
                },
                "filepath": {
                    "type": "string",
                    "description": "Path to a JSON file to read and parse. Must start with 'artifacts/' (e.g. 'artifacts/data.json')."
                }
            },
            "additionalProperties": false
        })
    }

    async fn execute(&self, params: Value, ctx: &ToolContext) -> Result<Value> {
        let json_string = params["json_string"].as_str();
        let filepath = params["filepath"].as_str();

        match (json_string, filepath) {
            (Some(_), Some(_)) => {
                Err(anyhow!("parse_json: provide either 'json_string' or 'filepath', not both"))
            }

            (Some(raw), None) => {
                debug!(len = raw.len(), "parse_json: parsing from string");
                ctx.log("parse_json: parsing JSON from string".to_string()).await;
                let value: Value = serde_json::from_str(raw)
                    .map_err(|e| anyhow!("parse_json: invalid JSON string – {e}"))?;
                info!("parse_json: parsed from string successfully");
                Ok(value)
            }

            (None, Some(path)) => {
                let canonical = std::path::Path::new(path)
                    .components()
                    .collect::<std::path::PathBuf>();

                let first_component = canonical
                    .components()
                    .next()
                    .and_then(|c| c.as_os_str().to_str())
                    .unwrap_or("");

                // Guard: only allow paths inside artifacts/
                if first_component != "artifacts" {
                    return Err(anyhow!(
                        "parse_json: file access is restricted to the 'artifacts/' directory, \
                         got '{path}'"
                    ));
                }

                // Guard: reject path traversal attempts
                if canonical.components().any(|c| {
                    matches!(c, std::path::Component::ParentDir)
                }) {
                    return Err(anyhow!(
                        "parse_json: path traversal is not allowed (got '{path}')"
                    ));
                }

                debug!(path = %path, "parse_json: reading from file");
                ctx.log(format!("parse_json: reading JSON from file '{path}'")).await;

                let contents = std::fs::read_to_string(&canonical)
                    .map_err(|e| anyhow!("parse_json: cannot read '{path}' – {e}"))?;

                let value: Value = serde_json::from_str(&contents)
                    .map_err(|e| anyhow!("parse_json: invalid JSON in '{path}' – {e}"))?;

                info!(path = %path, "parse_json: parsed from file successfully");
                ctx.log(format!("parse_json: parsed '{path}' successfully")).await;
                Ok(value)
            }

            (None, None) => {
                Err(anyhow!("parse_json: one of 'json_string' or 'filepath' is required"))
            }
        }
    }
}
