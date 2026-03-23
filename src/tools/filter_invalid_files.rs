use anyhow::{Context, Result};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::fs;
use tracing::info;

use super::{Tool, ToolContext};

pub struct FilterInvalidFilesTool;

#[async_trait]
impl Tool for FilterInvalidFilesTool {
    fn name(&self) -> &str {
        "filter_invalid_files"
    }

    fn description(&self) -> &str {
        "Reads the sensor evaluation artifact and the operator-notes evaluation artifact, \
         then returns only the filenames where data_is_valid is false OR op_is_valid is false. \
         Defaults to reading 'artifacts/sensor_static_eval.json' and 'artifacts/operator_notes_eval.json'. \
         The filtered list is also persisted to 'artifacts/invalid_files.json'."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "sensor_eval_file": {
                    "type": "string",
                    "description": "Path to the sensor evaluation JSON. Defaults to 'artifacts/sensor_static_eval.json'"
                },
                "operator_eval_file": {
                    "type": "string",
                    "description": "Path to the operator-notes evaluation JSON. Defaults to 'artifacts/operator_notes_eval.json'"
                },
                "output_file": {
                    "type": "string",
                    "description": "Path where the filtered filenames will be written. Defaults to 'artifacts/invalid_files.json'"
                }
            },
            "additionalProperties": false
        })
    }

    async fn execute(&self, params: Value, ctx: &ToolContext) -> Result<Value> {
        let sensor_file = params["sensor_eval_file"]
            .as_str()
            .unwrap_or("artifacts/sensor_static_eval.json")
            .to_string();

        let operator_file = params["operator_eval_file"]
            .as_str()
            .unwrap_or("artifacts/operator_notes_eval.json")
            .to_string();

        let output_file = params["output_file"]
            .as_str()
            .unwrap_or("artifacts/invalid_files.json")
            .to_string();

        ctx.log(format!(
            "filter_invalid_files: reading '{sensor_file}' and '{operator_file}'"
        ))
        .await;

        // ── Read sensor evaluation ───────────────────────────────────────
        let sensor_raw = fs::read_to_string(&sensor_file)
            .with_context(|| format!("filter_invalid_files: cannot read '{sensor_file}'"))?;
        let sensor_data: Vec<Value> = serde_json::from_str(&sensor_raw)
            .with_context(|| format!("filter_invalid_files: invalid JSON in '{sensor_file}'"))?;

        // ── Read operator-notes evaluation ───────────────────────────────
        let operator_raw = fs::read_to_string(&operator_file)
            .with_context(|| format!("filter_invalid_files: cannot read '{operator_file}'"))?;
        let operator_data: Vec<Value> = serde_json::from_str(&operator_raw)
            .with_context(|| format!("filter_invalid_files: invalid JSON in '{operator_file}'"))?;

        // ── Collect invalid filenames (sorted, deduplicated) ─────────────
        let mut invalid: BTreeSet<String> = BTreeSet::new();

        // Sensor: data_is_valid == false
        for entry in &sensor_data {
            let valid = entry["data_is_valid"].as_bool().unwrap_or(true);
            if !valid {
                if let Some(name) = entry["filename"].as_str() {
                    invalid.insert(name.to_string());
                }
            }
        }

        // Operator notes: op_is_valid == false
        for entry in &operator_data {
            let valid = entry["op_is_valid"].as_bool().unwrap_or(true);
            if !valid {
                if let Some(name) = entry["filename"].as_str() {
                    invalid.insert(name.to_string());
                }
            }
        }

        let result: Vec<Value> = invalid.iter().map(|n| Value::String(n.clone())).collect();

        // ── Persist ──────────────────────────────────────────────────────
        let output_json = serde_json::to_string_pretty(&result)?;
        fs::write(&output_file, &output_json)
            .with_context(|| format!("filter_invalid_files: cannot write '{output_file}'"))?;

        info!(
            total_invalid = result.len(),
            output = %output_file,
            "filter_invalid_files: done"
        );
        ctx.log(format!(
            "filter_invalid_files: found {} invalid files — written to '{output_file}'",
            result.len()
        ))
        .await;

        Ok(Value::Array(result))
    }
}
