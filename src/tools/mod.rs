pub mod describe_image;
pub mod download_file;
pub mod extract_links;
pub mod fetch_csv;
pub mod filter_data;
pub mod geocode_reverse;
pub mod get_env_var;
pub mod http_get;
pub mod http_post;
pub mod logistics_assistant;
pub mod packages;
pub mod parse_json;
pub mod read_markdown;
pub mod sleep;
pub mod start_proxy;
pub mod tag_with_llm;

use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, watch, Mutex};
use tokio::task::JoinHandle;

use crate::config::Config;
use crate::llm::{LlmClient, Message};

// ── SubAgentRunner trait ──────────────────────────────────────────────────────

/// Allows tools to spawn subagent loops that share the same TUI log pipeline.
#[async_trait]
pub trait SubAgentRunner: Send + Sync {
    /// `name` is the short label shown in the plan panel (e.g. "proxy").
    async fn run(
        &self,
        name: &str,
        system_prompt: &str,
        messages: &mut Vec<Message>,
        tools: Vec<Arc<dyn Tool>>,
        max_iterations: usize,
        ctx: &ToolContext,
    ) -> Result<Option<String>>;
}

// ── Background task tracker ───────────────────────────────────────────────────

/// Collects `JoinHandle`s from tools that spawn long-running background work
/// (e.g. proxy servers). The agent waits for all of them before reporting Done.
/// Also carries a watch-channel shutdown signal that tools can listen to.
#[derive(Clone)]
pub struct BackgroundTasks {
    handles: Arc<Mutex<Vec<JoinHandle<()>>>>,
    shutdown_tx: Arc<watch::Sender<bool>>,
    pub shutdown_rx: watch::Receiver<bool>,
}

impl Default for BackgroundTasks {
    fn default() -> Self {
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        Self {
            handles: Arc::new(Mutex::new(Vec::new())),
            shutdown_tx: Arc::new(shutdown_tx),
            shutdown_rx,
        }
    }
}

impl BackgroundTasks {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a background task handle. The agent will wait for it.
    pub async fn push(&self, handle: JoinHandle<()>) {
        self.handles.lock().await.push(handle);
    }

    /// Wait for all registered tasks to complete.
    pub async fn join_all(&self) {
        let handles: Vec<_> = {
            let mut guard = self.handles.lock().await;
            guard.drain(..).collect()
        };
        for h in handles {
            let _ = h.await;
        }
    }

    pub async fn is_empty(&self) -> bool {
        self.handles.lock().await.is_empty()
    }

    /// Signal all background services to shut down gracefully.
    pub fn notify_shutdown(&self) {
        let _ = self.shutdown_tx.send(true);
    }
}

// ── ToolContext ───────────────────────────────────────────────────────────────

/// Shared context passed to every tool during execution.
#[derive(Clone)]
pub struct ToolContext {
    pub http: reqwest::Client,
    pub llm: LlmClient,
    pub config: Config,
    pub log_tx: mpsc::Sender<String>,
    pub sub_agent: Arc<dyn SubAgentRunner>,
    pub background: BackgroundTasks,
}

impl ToolContext {
    pub fn new(
        llm: LlmClient,
        config: Config,
        log_tx: mpsc::Sender<String>,
        sub_agent: Arc<dyn SubAgentRunner>,
    ) -> Self {
        Self {
            http: reqwest::Client::new(),
            llm,
            config,
            log_tx,
            sub_agent,
            background: BackgroundTasks::new(),
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

    pub fn tools_vec(&self) -> Vec<Arc<dyn Tool>> {
        self.tools.values().cloned().collect()
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
    reg.register(describe_image::DescribeImageTool);
    reg.register(download_file::DownloadFileTool);
    reg.register(extract_links::ExtractLinksTool);
    reg.register(fetch_csv::FetchCsvTool);
    reg.register(filter_data::FilterDataTool);
    reg.register(geocode_reverse::GeocodeReverseTool);
    reg.register(parse_json::ParseJsonTool);
    reg.register(read_markdown::ReadMarkdownTool);
    reg.register(tag_with_llm::TagWithLlmTool);
    reg.register(http_get::HttpGetTool);
    reg.register(http_post::HttpPostTool);
    reg.register(get_env_var::GetEnvVarTool);
    reg.register(packages::CheckPackageTool);
    reg.register(packages::RedirectPackageTool);
    reg.register(sleep::SleepTool);
    reg.register(start_proxy::StartProxyTool);
    reg
}
