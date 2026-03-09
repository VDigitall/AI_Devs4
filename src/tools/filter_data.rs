use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde_json::{json, Value};

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
         'range' (numeric inclusive range with 'min' and 'max'). \
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
                            "type": { "type": "string", "enum": ["eq", "range"], "description": "Condition type" },
                            "value": { "type": "string", "description": "Value for 'eq' condition" },
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
                        _ => false,
                    }
                })
            })
            .cloned()
            .collect();

        ctx.log(format!(
            "filter_data: {} -> {} records after filtering",
            data.len(),
            filtered.len()
        ))
        .await;

        Ok(Value::Array(filtered))
    }
}
