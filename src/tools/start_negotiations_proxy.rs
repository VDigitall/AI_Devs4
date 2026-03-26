use anyhow::Result;
use async_trait::async_trait;
use axum::{Json, Router, extract::State, routing::post};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::Arc;
use tracing::{info, warn};

use super::{Tool, ToolContext};
use super::negotiations_assistant::NegotiationsAssistant;

// ── Axum shared state ─────────────────────────────────────────────────────────

#[derive(Clone)]
struct SearchState {
    assistant: Arc<NegotiationsAssistant>,
    ctx: ToolContext,
}

// ── Request / response types ──────────────────────────────────────────────────

#[derive(Deserialize)]
struct SearchRequest {
    params: String,
}

#[derive(Serialize)]
struct SearchResponse {
    output: String,
}

// ── Axum POST handler ─────────────────────────────────────────────────────────

async fn handle_search(
    State(state): State<SearchState>,
    Json(req): Json<SearchRequest>,
) -> Json<SearchResponse> {
    info!(query = %req.params, "negotiations proxy: received search request");
    state.ctx.log(format!("negotiations proxy ← request: {:?}", req.params)).await;

    let output = state
        .assistant
        .search(&req.params, &state.ctx)
        .await
        .unwrap_or_else(|e| {
            warn!(error = %e, "negotiations proxy: search error");
            "Search error, please try again.".to_string()
        });

    state.ctx.log(format!("negotiations proxy → response: {:?}", output)).await;
    Json(SearchResponse { output })
}

// ── Tool implementation ───────────────────────────────────────────────────────

pub struct StartNegotiationsProxyTool;

#[async_trait]
impl Tool for StartNegotiationsProxyTool {
    fn name(&self) -> &str {
        "start_negotiations_proxy"
    }

    fn description(&self) -> &str {
        "Start a local HTTP server for the 'negotiations' task. The server exposes a single \
         POST /api/search endpoint that the ag3nts.org agent will call to find which Polish \
         cities sell a given electronic component. The server loads data from artifacts/items.csv, \
         artifacts/cities.csv, and artifacts/connections.csv. Returns immediately after spawning \
         the server in the background. Use port 3001 by default to avoid conflicts."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "port": {
                    "type": "number",
                    "description": "Port to listen on (default: 3001)"
                }
            },
            "additionalProperties": false
        })
    }

    async fn execute(&self, params: Value, ctx: &ToolContext) -> Result<Value> {
        let port = params["port"].as_u64().unwrap_or(3001) as u16;

        ctx.log("start_negotiations_proxy: loading CSV data...".to_string()).await;
        let assistant = NegotiationsAssistant::load()?;
        ctx.log(format!("start_negotiations_proxy: data loaded, starting server on port {port}")).await;

        let state = SearchState {
            assistant: Arc::new(assistant),
            ctx: ctx.clone(),
        };

        let app = Router::new()
            .route("/api/search", post(handle_search))
            .with_state(state);

        let addr = format!("0.0.0.0:{port}");
        let listener = tokio::net::TcpListener::bind(&addr)
            .await
            .map_err(|e| anyhow::anyhow!("start_negotiations_proxy: cannot bind to {addr}: {e}"))?;

        info!(addr = %addr, "negotiations proxy server bound");
        ctx.log(format!("start_negotiations_proxy: listening on {addr}")).await;

        let mut shutdown_rx = ctx.background.shutdown_rx.clone();
        let handle = tokio::spawn(async move {
            let shutdown = async move {
                loop {
                    if shutdown_rx.changed().await.is_err() {
                        break;
                    }
                    if *shutdown_rx.borrow() {
                        break;
                    }
                }
            };
            if let Err(e) = axum::serve(listener, app).with_graceful_shutdown(shutdown).await {
                warn!(error = %e, "negotiations proxy server stopped");
            }
            info!("negotiations proxy server shut down");
        });

        ctx.background.push(handle).await;
        ctx.log(format!("start_negotiations_proxy: server running on port {port}")).await;

        Ok(json!({
            "status": "running",
            "port": port,
            "local_address": format!("http://0.0.0.0:{port}"),
            "endpoint": format!("http://0.0.0.0:{port}/api/search"),
            "note": "Expose this port publicly (e.g. via ngrok) and register the public URL with /verify. The external agent will POST {\"params\": \"<item query>\"} to /api/search and expects {\"output\": \"...\"}.",
            "tool_description_for_agent": "Accepts a natural language description of an electronic component (in Polish or English) and returns the list of Polish cities where that item is available for purchase. POST request body: {\"params\": \"description of the item\"}. Response: {\"output\": \"Item: <name>. Cities: city1, city2, ...\"}."
        }))
    }
}
