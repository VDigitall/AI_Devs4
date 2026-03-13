use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::time::{sleep, Duration};

use super::{Tool, ToolContext};

pub struct SleepTool;

#[async_trait]
impl Tool for SleepTool {
    fn name(&self) -> &str {
        "sleep"
    }

    fn description(&self) -> &str {
        "Pause execution for a specified number of seconds before continuing to the next step. \
         Use this when you need to wait between retries, respect rate limits, or introduce \
         a deliberate delay between actions."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "seconds": {
                    "type": "number",
                    "description": "Number of seconds to sleep (may be fractional, e.g. 0.5)"
                }
            },
            "required": ["seconds"],
            "additionalProperties": false
        })
    }

    async fn execute(&self, params: Value, ctx: &ToolContext) -> Result<Value> {
        let seconds = params["seconds"]
            .as_f64()
            .ok_or_else(|| anyhow!("sleep: missing or invalid 'seconds' parameter"))?;

        if seconds < 0.0 {
            return Err(anyhow!("sleep: 'seconds' must be non-negative"));
        }

        ctx.log(format!("sleep: waiting {seconds}s")).await;

        sleep(Duration::from_secs_f64(seconds)).await;

        ctx.log(format!("sleep: done after {seconds}s")).await;

        Ok(json!({ "slept_seconds": seconds }))
    }
}
