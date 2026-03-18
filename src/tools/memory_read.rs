use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};
use tracing::info;

use super::{Tool, ToolContext};

pub struct MemoryReadTool;

#[async_trait]
impl Tool for MemoryReadTool {
    fn name(&self) -> &str {
        "memory_read"
    }

    fn description(&self) -> &str {
        "Read the memory file for a given task. Returns all previously saved notes \
         from 'memory/<task_name>.memory.md', or a message indicating the file does not exist yet."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "task_name": {
                    "type": "string",
                    "description": "The name of the task whose memory file should be read (e.g. 'failure')"
                }
            },
            "required": ["task_name"],
            "additionalProperties": false
        })
    }

    async fn execute(&self, params: Value, ctx: &ToolContext) -> Result<Value> {
        let task_name = params["task_name"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("memory_read: missing 'task_name' parameter"))?;

        let path = format!("memory/{task_name}.memory.md");

        ctx.log(format!("memory_read: reading '{path}'")).await;

        match std::fs::read_to_string(&path) {
            Ok(content) => {
                info!(path = %path, "memory_read: read successfully");
                ctx.log(format!("memory_read: read '{path}' successfully")).await;
                Ok(json!({ "content": content }))
            }
            Err(_) => {
                let msg = format!("Memory file for task '{task_name}' doesn't exists yet.");
                ctx.log(format!("memory_read: {msg}")).await;
                Ok(json!({ "content": msg }))
            }
        }
    }
}
