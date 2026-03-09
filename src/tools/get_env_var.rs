use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde_json::{json, Value};

use super::{Tool, ToolContext};

pub struct GetEnvVarTool;

#[async_trait]
impl Tool for GetEnvVarTool {
    fn name(&self) -> &str {
        "get_env_var"
    }

    fn description(&self) -> &str {
        "Read an environment variable by name and return its value as a string. \
         Use this to retrieve API keys or configuration values (e.g., AG3NTS_API_KEY)."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "The name of the environment variable to read"
                }
            },
            "required": ["name"],
            "additionalProperties": false
        })
    }

    async fn execute(&self, params: Value, ctx: &ToolContext) -> Result<Value> {
        let name = params["name"]
            .as_str()
            .ok_or_else(|| anyhow!("get_env_var: missing 'name' parameter"))?;

        let value = std::env::var(name)
            .map_err(|_| anyhow!("get_env_var: environment variable '{name}' not found"))?;

        ctx.log(format!("get_env_var: read {name}")).await;

        Ok(json!({ "value": value }))
    }
}
