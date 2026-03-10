pub mod fetch_csv;
pub mod filter_data;
pub mod get_env_var;
pub mod http_post;
pub mod parse_json;
pub mod tag_with_llm;

use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc;

use crate::llm::LlmClient;

// ── ToolContext ───────────────────────────────────────────────────────────────

/// Shared context passed to every tool during execution.
#[derive(Clone)]
pub struct ToolContext {
    pub http: reqwest::Client,
    pub llm: LlmClient,
    pub log_tx: mpsc::Sender<String>,
}

impl ToolContext {
    pub fn new(llm: LlmClient, log_tx: mpsc::Sender<String>) -> Self {
        Self {
            http: reqwest::Client::new(),
            llm,
            log_tx,
        }
    }

    pub async fn log(&self, msg: impl Into<String>) {
        let _ = self.log_tx.send(msg.into()).await;
    }
}

// ── Tool trait ────────────────────────────────────────────────────────────────

#[async_trait]
pub trait Tool: Send + Sync {
    /// Unique snake_case name used in function calling.
    fn name(&self) -> &str;

    /// Human-readable description sent to the LLM.
    fn description(&self) -> &str;

    /// JSON Schema for the tool's parameters (OpenAI function calling format).
    fn parameters_schema(&self) -> Value;

    /// Execute the tool with the given parameters.
    async fn execute(&self, params: Value, ctx: &ToolContext) -> Result<Value>;

    /// Produce the full OpenRouter/OpenAI tool definition object.
    fn to_definition(&self) -> crate::llm::ToolDefinition {
        crate::llm::ToolDefinition::new(
            self.name(),
            self.description(),
            self.parameters_schema(),
        )
    }
}

// ── ToolRegistry ──────────────────────────────────────────────────────────────

pub struct ToolRegistry {
    tools: HashMap<String, Arc<dyn Tool>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
        }
    }

    pub fn register(&mut self, tool: impl Tool + 'static) {
        let name = tool.name().to_string();
        self.tools.insert(name, Arc::new(tool));
    }

    pub fn get(&self, name: &str) -> Option<Arc<dyn Tool>> {
        self.tools.get(name).cloned()
    }

    /// Returns all tool definitions for the LLM's `tools` parameter.
    pub fn definitions(&self) -> Vec<crate::llm::ToolDefinition> {
        let mut defs: Vec<_> = self.tools.values().map(|t| t.to_definition()).collect();
        defs.sort_by(|a, b| a.function.name.cmp(&b.function.name));
        defs
    }

    pub fn names(&self) -> Vec<String> {
        let mut names: Vec<_> = self.tools.keys().cloned().collect();
        names.sort();
        names
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// ── Helper: build default registry ───────────────────────────────────────────

pub fn default_registry() -> ToolRegistry {
    let mut reg = ToolRegistry::new();
    reg.register(fetch_csv::FetchCsvTool);
    reg.register(filter_data::FilterDataTool);
    reg.register(parse_json::ParseJsonTool);
    reg.register(tag_with_llm::TagWithLlmTool);
    reg.register(http_post::HttpPostTool);
    reg.register(get_env_var::GetEnvVarTool);
    reg
}
