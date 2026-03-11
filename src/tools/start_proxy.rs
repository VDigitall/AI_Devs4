use anyhow::Result;
use async_trait::async_trait;
use axum::{Json, Router, extract::State, routing::post};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::Arc;
use tracing::{info, warn};

use crate::config::Config;
use super::{Tool, ToolContext};
use super::logistics_assistant::LogisticsAssistant;

// ── Axum shared state ─────────────────────────────────────────────────────────

#[derive(Clone)]
struct ProxyState {
    assistant: Arc<LogisticsAssistant>,
    llm: crate::llm::LlmClient,
    http: reqwest::Client,
    config: Config,
}

// ── Request / response types ──────────────────────────────────────────────────

#[derive(Deserialize)]
struct OperatorRequest {
    #[serde(rename = "sessionID")]
    session_id: String,
    msg: String,
}

#[derive(Serialize)]
struct OperatorResponse {
    msg: String,
}

// ── Axum POST handler ─────────────────────────────────────────────────────────

async fn handle_operator(
    State(state): State<ProxyState>,
    Json(req): Json<OperatorRequest>,
) -> Json<OperatorResponse> {
    let msg = state
        .assistant
        .handle(&req.session_id, &req.msg, &state.llm, &state.http, &state.config)
        .await
        .unwrap_or_else(|e| {
            warn!(error = %e, session = %req.session_id, "error handling operator message");
            "I'm sorry, I'm experiencing technical difficulties. Please try again.".to_string()
        });

    Json(OperatorResponse { msg })
}

// ── Tool implementation ───────────────────────────────────────────────────────

pub struct StartProxyTool;

#[async_trait]
impl Tool for StartProxyTool {
    fn name(&self) -> &str {
        "start_proxy"
    }

    fn description(&self) -> &str {
        "Start a local HTTP proxy server that acts as an intelligent logistics assistant \
         with per-session conversation memory. Listens for POST requests with \
         { sessionID, msg } and responds with { msg }. Returns immediately after \
         spawning the server in the background."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "port": {
                    "type": "number",
                    "description": "Port to listen on (default: 3000)"
                }
            },
            "additionalProperties": false
        })
    }

    async fn execute(&self, params: Value, ctx: &ToolContext) -> Result<Value> {
        let port = params["port"].as_u64().unwrap_or(3000) as u16;

        ctx.log(format!("start_proxy: starting server on port {port}")).await;
        info!(port, "starting proxy server");

        let state = ProxyState {
            assistant: Arc::new(LogisticsAssistant::new()),
            llm: ctx.llm.clone(),
            http: reqwest::Client::new(),
            config: ctx.config.clone(),
        };

        let app = Router::new()
            .route("/", post(handle_operator))
            .with_state(state);

        let addr = format!("0.0.0.0:{port}");
        let listener = tokio::net::TcpListener::bind(&addr)
            .await
            .map_err(|e| anyhow::anyhow!("start_proxy: cannot bind to {addr}: {e}"))?;

        info!(addr = %addr, "proxy server bound");
        ctx.log(format!("start_proxy: listening on {addr}")).await;

        tokio::spawn(async move {
            if let Err(e) = axum::serve(listener, app).await {
                warn!(error = %e, "proxy server stopped");
            }
        });

        ctx.log(format!("start_proxy: server running on port {port}")).await;

        Ok(json!({
            "status": "running",
            "port": port,
            "address": format!("http://0.0.0.0:{port}")
        }))
    }
}
