use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde_json::{json, Value};
use tracing::{debug, info};

use super::{Tool, ToolContext};

// ── CheckPackageTool ──────────────────────────────────────────────────────────

pub struct CheckPackageTool;

#[async_trait]
impl Tool for CheckPackageTool {
    fn name(&self) -> &str {
        "check_package"
    }

    fn description(&self) -> &str {
        "Check the current status and location of a package in the logistics system."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "packageid": {
                    "type": "string",
                    "description": "The package ID to check, e.g. PKG12345678"
                }
            },
            "required": ["packageid"],
            "additionalProperties": false
        })
    }

    async fn execute(&self, params: Value, ctx: &ToolContext) -> Result<Value> {
        let packageid = params["packageid"]
            .as_str()
            .ok_or_else(|| anyhow!("check_package: missing 'packageid'"))?;

        info!(packageid = %packageid, "check_package: checking status");
        ctx.log(format!("check_package: checking {packageid}")).await;

        let body = json!({
            "apikey": ctx.config.ag3nts_api_key,
            "action": "check",
            "packageid": packageid
        });

        let resp = ctx.http.post(&ctx.config.packages_api_url).json(&body).send().await?;
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();

        debug!(status = %status, body = %text, "check_package: response");
        ctx.log(format!("check_package: status {status}")).await;

        let parsed: Value =
            serde_json::from_str(&text).unwrap_or(Value::String(text));
        Ok(parsed)
    }
}

// ── RedirectPackageTool ───────────────────────────────────────────────────────

pub struct RedirectPackageTool;

#[async_trait]
impl Tool for RedirectPackageTool {
    fn name(&self) -> &str {
        "redirect_package"
    }

    fn description(&self) -> &str {
        "Redirect a package to a new destination. Requires the security code \
         provided by the operator. Returns a confirmation code on success."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "packageid": {
                    "type": "string",
                    "description": "The package ID to redirect"
                },
                "destination": {
                    "type": "string",
                    "description": "Destination facility code, e.g. PWR3847PL"
                },
                "code": {
                    "type": "string",
                    "description": "Security code provided by the operator"
                }
            },
            "required": ["packageid", "destination", "code"],
            "additionalProperties": false
        })
    }

    async fn execute(&self, params: Value, ctx: &ToolContext) -> Result<Value> {
        let packageid = params["packageid"]
            .as_str()
            .ok_or_else(|| anyhow!("redirect_package: missing 'packageid'"))?;
        let destination = params["destination"]
            .as_str()
            .ok_or_else(|| anyhow!("redirect_package: missing 'destination'"))?;
        let code = params["code"]
            .as_str()
            .ok_or_else(|| anyhow!("redirect_package: missing 'code'"))?;

        info!(packageid = %packageid, destination = %destination, "redirect_package: redirecting");
        ctx.log(format!("redirect_package: {packageid} → {destination}")).await;

        let body = json!({
            "apikey": ctx.config.ag3nts_api_key,
            "action": "redirect",
            "packageid": packageid,
            "destination": destination,
            "code": code
        });

        let resp = ctx.http.post(&ctx.config.packages_api_url).json(&body).send().await?;
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();

        debug!(status = %status, body = %text, "redirect_package: response");
        ctx.log(format!("redirect_package: status {status}")).await;

        let parsed: Value =
            serde_json::from_str(&text).unwrap_or(Value::String(text));
        Ok(parsed)
    }
}
