use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde_json::{json, Value};
use tracing::{debug, info};

use super::{Tool, ToolContext};

pub struct FilterDataTool;

#[async_trait]
impl Tool for FilterDataTool {
    fn name(&self) -> &str {
        "filter_data"
    }

    fn description(&self) -> &str {
        "Filter a JSON array of objects by one or more conditions. \
         Supported condition types: \
         'eq' (exact string match), \
         'range' (numeric inclusive range with 'min' and 'max'), \
         'contains' (check if an array field contains a value, or a string field contains a substring), \
         'gte' (greater than or equal — works with ISO dates like '1976-03-09' or numbers), \
         'lte' (less than or equal — works with ISO dates like '1986-03-09' or numbers). \
         For age-based queries use 'gte'+'lte' on the birthDate field with ISO date strings. \
         Returns the filtered array."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "data": {
                    "type": "array",
                    "description": "The array of objects to filter",
                    "items": { "type": "object" }
                },
                "conditions": {
                    "type": "array",
                    "description": "List of filter conditions to apply (all must match - AND logic)",
                    "items": {
                        "type": "object",
                        "properties": {
                            "field": { "type": "string", "description": "The field name to filter on" },
                            "type": { "type": "string", "enum": ["eq", "range", "contains", "gte", "lte"], "description": "Condition type" },
                            "value": { "type": "string", "description": "Value for 'eq', 'gte', or 'lte' conditions. ISO date strings (e.g. '1986-03-09') are supported for 'gte'/'lte'." },
                            "min": { "type": "number", "description": "Minimum value (inclusive) for 'range'" },
                            "max": { "type": "number", "description": "Maximum value (inclusive) for 'range'" }
                        },
                        "required": ["field", "type"],
                        "additionalProperties": false
                    }
                }
            },
            "required": ["data", "conditions"],
            "additionalProperties": false
        })
    }

    async fn execute(&self, params: Value, ctx: &ToolContext) -> Result<Value> {
        let data = params["data"]
            .as_array()
            .ok_or_else(|| anyhow!("filter_data: 'data' must be an array"))?;

        let conditions = params["conditions"]
            .as_array()
            .ok_or_else(|| anyhow!("filter_data: 'conditions' must be an array"))?;

        info!(input = data.len(), conditions = conditions.len(), "filter_data: filtering");
        for cond in conditions {
            debug!(condition = %cond, "filter_data: condition");
        }

        let filtered: Vec<Value> = data
            .iter()
            .filter(|row| {
                conditions.iter().all(|cond| {
                    let field = cond["field"].as_str().unwrap_or("");
                    let cond_type = cond["type"].as_str().unwrap_or("");
                    let row_val = &row[field];

                    match cond_type {
                        "eq" => {
                            let expected = cond["value"].as_str().unwrap_or("");
                            row_val.as_str().map(|v| v.eq_ignore_ascii_case(expected)).unwrap_or(false)
                        }
                        "range" => {
                            let min = cond["min"].as_f64().unwrap_or(f64::NEG_INFINITY);
                            let max = cond["max"].as_f64().unwrap_or(f64::INFINITY);
                            let num = row_val
                                .as_str()
                                .and_then(|s| s.parse::<f64>().ok())
                                .or_else(|| row_val.as_f64());
                            num.map(|n| n >= min && n <= max).unwrap_or(false)
                        }
                        "contains" => {
                            let expected = cond["value"].as_str().unwrap_or("");
                            match row_val {
                                Value::Array(arr) => arr.iter().any(|v| {
                                    v.as_str()
                                        .map(|s| s.eq_ignore_ascii_case(expected))
                                        .unwrap_or(false)
                                }),
                                Value::String(s) => {
                                    s.to_lowercase().contains(&expected.to_lowercase())
                                }
                                _ => false,
                            }
                        }
                        "gte" | "lte" => {
                            let threshold = cond["value"].as_str().unwrap_or("");
                            let row_str = row_val.as_str();
                            // Prefer lexicographic comparison (works for ISO dates YYYY-MM-DD
                            // and zero-padded strings); fall back to numeric if both parse.
                            if let Some(row_s) = row_str {
                                let num_cmp = row_s
                                    .parse::<f64>()
                                    .ok()
                                    .zip(threshold.parse::<f64>().ok())
                                    .map(|(r, t)| if cond_type == "gte" { r >= t } else { r <= t });

                                num_cmp.unwrap_or_else(|| {
                                    if cond_type == "gte" {
                                        row_s >= threshold
                                    } else {
                                        row_s <= threshold
                                    }
                                })
                            } else {
                                // Numeric JSON value
                                row_val
                                    .as_f64()
                                    .zip(threshold.parse::<f64>().ok())
                                    .map(|(r, t)| if cond_type == "gte" { r >= t } else { r <= t })
                                    .unwrap_or(false)
                            }
                        }
                        _ => false,
                    }
                })
            })
            .cloned()
            .collect();

        info!(
            input = data.len(),
            output = filtered.len(),
            "filter_data: done"
        );
        ctx.log(format!(
            "filter_data: {} -> {} records after filtering",
            data.len(),
            filtered.len()
        ))
        .await;

        Ok(Value::Array(filtered))
    }
}
