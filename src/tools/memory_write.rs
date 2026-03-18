use anyhow::{anyhow, Result};
use async_trait::async_trait;
use chrono::Local;
use serde_json::{json, Value};
use tracing::info;

use super::{Tool, ToolContext};

pub struct MemoryWriteTool;

#[async_trait]
impl Tool for MemoryWriteTool {
    fn name(&self) -> &str {
        "memory_write"
    }

    fn description(&self) -> &str {
        "Append a note to the memory file for the current task. \
         The note is stored in 'memory/<task_name>.memory.md' with a timestamp header. \
         Use this to persist important findings, intermediate results, or context \
         that should survive across agent iterations."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "task_name": {
                    "type": "string",
                    "description": "The name of the task (used as the memory file name, e.g. 'failure')"
                },
                "note": {
                    "type": "string",
                    "description": "The text to append to the memory file"
                }
            },
            "required": ["task_name", "note"],
            "additionalProperties": false
        })
    }

    async fn execute(&self, params: Value, ctx: &ToolContext) -> Result<Value> {
        let task_name = params["task_name"]
            .as_str()
            .ok_or_else(|| anyhow!("memory_write: missing 'task_name' parameter"))?;

        let note = params["note"]
            .as_str()
            .ok_or_else(|| anyhow!("memory_write: missing 'note' parameter"))?;

        let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
        let entry = format!("#{timestamp}:\n {note}\n\n");

        std::fs::create_dir_all("memory")
            .map_err(|e| anyhow!("memory_write: cannot create 'memory/' directory – {e}"))?;

        let path = format!("memory/{task_name}.memory.md");

        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|e| anyhow!("memory_write: cannot open '{path}' – {e}"))?;

        use std::io::Write;
        file.write_all(entry.as_bytes())
            .map_err(|e| anyhow!("memory_write: cannot write to '{path}' – {e}"))?;

        info!(path = %path, "memory_write: appended note");
        ctx.log(format!("memory_write: appended note to '{path}'")).await;

        Ok(json!({ "path": path, "timestamp": timestamp }))
    }
}
