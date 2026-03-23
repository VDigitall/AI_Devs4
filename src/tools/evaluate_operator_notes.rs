use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde_json::{json, Value};
use tracing::{info, warn};

use super::{Tool, ToolContext};

pub struct EvaluateOperatorNotesTool;

#[async_trait]
impl Tool for EvaluateOperatorNotesTool {
    fn name(&self) -> &str {
        "evaluate_operator_notes"
    }

    fn description(&self) -> &str {
        "Classifies operator notes for a list of sensor files using an LLM. \
         Each entry maps a filename to its operator_notes text. \
         The LLM determines whether the operator considers the sensor behavior OK (true) or not OK (false). \
         Input: [{\"<filename>\": \"<operator_notes>\"}, ...] \
         Output: [{\"filename\": \"<filename>\", \"op_is_valid\": true/false}, ...] \
         Results are also persisted to 'artifacts/operator_notes_eval.json'."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "data": {
                    "type": "array",
                    "description": "Array of objects, each mapping a filename to its operator_notes string. E.g. [{\"9384.json\": \"All measured channels stay in tolerance.\"}]",
                    "items": { "type": "object" }
                }
            },
            "required": ["data"],
            "additionalProperties": false
        })
    }

    async fn execute(&self, params: Value, ctx: &ToolContext) -> Result<Value> {
        let data = params["data"]
            .as_array()
            .ok_or_else(|| anyhow!("evaluate_operator_notes: 'data' must be an array"))?
            .clone();

        if data.is_empty() {
            return Ok(Value::Array(vec![]));
        }

        info!(count = data.len(), "evaluate_operator_notes: start");
        ctx.log(format!(
            "evaluate_operator_notes: classifying operator notes for {} files",
            data.len()
        ))
        .await;

        let system = "You are a sensor operations analyst. \
            You will receive a JSON array where each element is an object mapping a filename to the operator's notes for that sensor reading. \
            Your task is to determine, for each entry, whether the operator considers the sensor behavior OK (true) or NOT OK (false). \
            Return a single JSON object where each key is the filename and each value is a boolean: \
            true if the operator notes indicate behavior is acceptable/OK, false if they indicate a problem, anomaly, or intervention is needed. \
            Only output the JSON object — no explanation.";

        let user = serde_json::to_string(&data)?;

        let schema = json!({
            "type": "object",
            "additionalProperties": { "type": "boolean" }
        });

        let result = ctx
            .llm
            .complete_structured(
                system,
                &user,
                "operator_notes_classification",
                schema,
                Some("google/gemini-3.1-flash-lite-preview"),
            )
            .await;

        let classifications = match result {
            Ok(v) => {
                if let Some(map) = v.as_object() {
                    map.clone()
                } else {
                    warn!("evaluate_operator_notes: LLM returned non-object, defaulting all to false");
                    ctx.log("evaluate_operator_notes: unexpected LLM response shape, defaulting all to false".to_string()).await;
                    serde_json::Map::new()
                }
            }
            Err(e) => {
                warn!(error = %e, "evaluate_operator_notes: LLM error");
                ctx.log(format!("evaluate_operator_notes: LLM error: {e}")).await;
                return Err(e);
            }
        };

        let output: Vec<Value> = data
            .iter()
            .filter_map(|item| {
                item.as_object()
                    .and_then(|obj| obj.keys().next().cloned())
                    .map(|filename| {
                        let is_ok = classifications
                            .get(&filename)
                            .and_then(|v| v.as_bool())
                            .unwrap_or(false);
                        json!({
                            "filename": filename,
                            "op_is_valid": is_ok
                        })
                    })
            })
            .collect();

        info!(classified = output.len(), "evaluate_operator_notes: done");
        ctx.log(format!(
            "evaluate_operator_notes: classified {} files",
            output.len()
        ))
        .await;

        // Store results in file
        let output_file = "artifacts/operator_notes_eval.json";
        let file = std::fs::File::create(output_file).unwrap();
        serde_json::to_writer_pretty(file, &output).unwrap();

        Ok(Value::Array(output))
    }
}
