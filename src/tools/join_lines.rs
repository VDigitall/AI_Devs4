use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde_json::{json, Value};

use super::{Tool, ToolContext};

pub struct JoinLinesTool;

#[async_trait]
impl Tool for JoinLinesTool {
    fn name(&self) -> &str {
        "join_lines"
    }

    fn description(&self) -> &str {
        "Join an array of text lines into a single string using '\\n' as the separator. \
         Use this when you have a list of lines and need to combine them into one block of text, \
         for example before writing to a file or passing to another tool."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "lines": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Array of text lines to join"
                }
            },
            "required": ["lines"],
            "additionalProperties": false
        })
    }

    async fn execute(&self, params: Value, ctx: &ToolContext) -> Result<Value> {
        let lines = params["lines"]
            .as_array()
            .ok_or_else(|| anyhow!("join_lines: missing or invalid 'lines' parameter"))?;

        let strings: Vec<&str> = lines
            .iter()
            .enumerate()
            .map(|(i, v)| {
                v.as_str()
                    .ok_or_else(|| anyhow!("join_lines: element at index {i} is not a string"))
            })
            .collect::<Result<_>>()?;

        let result = strings.join("\n");

        ctx.log(format!("join_lines: joined {} lines", strings.len())).await;

        Ok(json!({ "text": result }))
    }
}
