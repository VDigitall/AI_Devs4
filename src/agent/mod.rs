pub mod prompts;

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

use crate::config::Config;
use crate::llm::{LlmClient, Message};
use crate::tools::{SubAgentRunner, Tool, ToolContext, ToolRegistry};

// ── Plan step ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct PlanStep {
    pub tool_call_id: String,
    pub tool_name: String,
    pub arguments: String,
    pub status: StepStatus,
    pub result: Option<Value>,
    /// None = main agent step; Some(name) = subagent step (e.g. "proxy").
    pub label: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum StepStatus {
    Pending,
    Running,
    Done,
    Failed(String),
    MissingTool,
}

impl std::fmt::Display for StepStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StepStatus::Pending => write!(f, "pending"),
            StepStatus::Running => write!(f, "running"),
            StepStatus::Done => write!(f, "done"),
            StepStatus::Failed(e) => write!(f, "failed: {e}"),
            StepStatus::MissingTool => write!(f, "missing tool"),
        }
    }
}

// ── Agent messages (sent to TUI) ──────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum AgentEvent {
    Log(String),
    PlanReady(Vec<PlanStep>),
    StepStarted(usize),
    StepCompleted(usize, Value),
    StepFailed(usize, String),
    MissingTool(String),
    /// Replaces the subagent section of the plan panel with the latest snapshot.
    SubPlanReady(Vec<PlanStep>),
    /// Main loop finished; background services still running.
    Waiting,
    Done,
    Error(String),
}

// ── Agent ─────────────────────────────────────────────────────────────────────

pub struct Agent {
    llm: LlmClient,
    config: Config,
    registry: Arc<ToolRegistry>,
}

impl Agent {
    pub fn new(llm: LlmClient, config: Config, registry: Arc<ToolRegistry>) -> Self {
        Self { llm, config, registry }
    }

    /// Run the full plan-then-execute cycle for a task.
    /// All progress is sent through `event_tx`.
    pub async fn run(
        &self,
        task_content: &str,
        event_tx: mpsc::Sender<AgentEvent>,
    ) {
        if let Err(e) = self.run_inner(task_content, event_tx.clone()).await {
            let _ = event_tx.send(AgentEvent::Error(e.to_string())).await;
        }
    }

    async fn run_inner(
        &self,
        task_content: &str,
        event_tx: mpsc::Sender<AgentEvent>,
    ) -> Result<()> {
        const MAX_ITERATIONS: usize = 15;

        info!("Agent: starting iterative execution");
        let _ = event_tx.send(AgentEvent::Log("Planning...".into())).await;

        let (log_tx, mut log_rx) = mpsc::channel::<String>(64);
        let event_tx_log = event_tx.clone();
        tokio::spawn(async move {
            while let Some(msg) = log_rx.recv().await {
                let _ = event_tx_log.send(AgentEvent::Log(msg)).await;
            }
        });

        let sub_runner = Arc::new(SubAgentRunnerImpl {
            event_tx: event_tx.clone(),
        });
        let tool_ctx = ToolContext::new(
            self.llm.clone(),
            self.config.clone(),
            log_tx,
            sub_runner,
        );

        let tools = self.registry.tools_vec();
        let mut messages = vec![
            Message::system(prompts::PLANNING_SYSTEM),
            Message::user(task_content),
        ];

        run_tool_loop(
            &self.llm,
            &mut messages,
            &tools,
            &tool_ctx,
            MAX_ITERATIONS,
            &event_tx,
            None,
        )
        .await?;

        info!("Agent: all iterations completed");
        let _ = event_tx
            .send(AgentEvent::Log("All steps completed.".into()))
            .await;

        if !tool_ctx.background.is_empty().await {
            let _ = event_tx
                .send(AgentEvent::Log(
                    "Waiting for background services to finish...".into(),
                ))
                .await;
            let _ = event_tx.send(AgentEvent::Waiting).await;
            tool_ctx.background.join_all().await;
        }

        let _ = event_tx.send(AgentEvent::Done).await;
        Ok(())
    }
}

// ── SubAgentRunnerImpl ───────────────────────────────────────────────────────

struct SubAgentRunnerImpl {
    event_tx: mpsc::Sender<AgentEvent>,
}

#[async_trait]
impl SubAgentRunner for SubAgentRunnerImpl {
    async fn run(
        &self,
        name: &str,
        system_prompt: &str,
        messages: &mut Vec<Message>,
        tools: Vec<Arc<dyn Tool>>,
        max_iterations: usize,
        ctx: &ToolContext,
    ) -> Result<Option<String>> {
        if messages.is_empty() || messages[0].role != "system" {
            messages.insert(0, Message::system(system_prompt));
        }
        run_tool_loop(
            &ctx.llm,
            messages,
            &tools,
            ctx,
            max_iterations,
            &self.event_tx,
            Some(name),
        )
        .await
    }
}

// ── Shared tool-calling loop ─────────────────────────────────────────────────

/// The single tool-calling loop used by both the main agent and subagents.
///
/// `step_label`:
///   - `None`  → main agent: emits PlanReady / StepStarted / StepCompleted / StepFailed / MissingTool
///   - `Some(label)` → subagent: emits SubPlanReady snapshots, steps carry the given label
pub(crate) async fn run_tool_loop(
    llm: &LlmClient,
    messages: &mut Vec<Message>,
    tools: &[Arc<dyn Tool>],
    ctx: &ToolContext,
    max_iterations: usize,
    event_tx: &mpsc::Sender<AgentEvent>,
    step_label: Option<&str>,
) -> Result<Option<String>> {
    let is_main = step_label.is_none();
    let tool_defs: Vec<_> = tools.iter().map(|t| t.to_definition()).collect();
    info!(available_tools = tool_defs.len(), label = ?step_label, "run_tool_loop starting");
    for def in &tool_defs {
        debug!(tool = %def.function.name, "available tool");
    }

    let mut all_steps: Vec<PlanStep> = Vec::new();
    let mut step_results: Vec<Value> = Vec::new();

    for iteration in 0..max_iterations {
        debug!(iteration, "tool loop iteration");

        let response = llm
            .chat(messages.clone(), Some(tool_defs.clone()), None, None)
            .await
            .map_err(|e| anyhow!("LLM call failed (iteration {iteration}): {e}"))?;

        let tool_calls = match &response.tool_calls {
            Some(calls) if !calls.is_empty() => calls.clone(),
            _ => {
                let text = response.content.clone();
                if let Some(content) = &text {
                    info!(content = %content, "LLM finished with message");
                    let _ = event_tx
                        .send(AgentEvent::Log(format!("LLM: {content}")))
                        .await;
                }
                return Ok(text);
            }
        };

        info!(iteration, tool_calls = tool_calls.len(), "LLM returned tool calls");
        messages.push(Message::assistant_tool_calls(tool_calls.clone()));

        let base_idx = all_steps.len();
        let new_steps: Vec<PlanStep> = tool_calls
            .iter()
            .map(|tc| PlanStep {
                tool_call_id: tc.id.clone(),
                tool_name: tc.function.name.clone(),
                arguments: tc.function.arguments.clone(),
                status: StepStatus::Pending,
                result: None,
                label: step_label.map(|l| l.to_string()),
            })
            .collect();
        all_steps.extend(new_steps);

        if is_main {
            let _ = event_tx
                .send(AgentEvent::PlanReady(all_steps.clone()))
                .await;
        } else {
            let _ = event_tx
                .send(AgentEvent::SubPlanReady(all_steps.clone()))
                .await;
        }
        let _ = event_tx
            .send(AgentEvent::Log(format!(
                "Iteration {}: {} tool call(s)",
                iteration + 1,
                tool_calls.len()
            )))
            .await;

        for (j, tc) in tool_calls.iter().enumerate() {
            let step_idx = base_idx + j;
            let total = all_steps.len();
            let tool_name = all_steps[step_idx].tool_name.clone();

            let tool = match tools.iter().find(|t| t.name() == tc.function.name) {
                Some(t) => t,
                None => {
                    all_steps[step_idx].status = StepStatus::MissingTool;
                    warn!(tool = %tc.function.name, "tool not found");
                    if is_main {
                        let _ = event_tx
                            .send(AgentEvent::MissingTool(tc.function.name.clone()))
                            .await;
                    } else {
                        let _ = event_tx
                            .send(AgentEvent::SubPlanReady(all_steps.clone()))
                            .await;
                    }
                    let _ = event_tx
                        .send(AgentEvent::Log(format!(
                            "Missing tool '{}'. Please implement it and restart.",
                            tc.function.name
                        )))
                        .await;
                    return Ok(None);
                }
            };

            all_steps[step_idx].status = StepStatus::Running;
            if is_main {
                let _ = event_tx.send(AgentEvent::StepStarted(step_idx)).await;
            } else {
                let _ = event_tx
                    .send(AgentEvent::SubPlanReady(all_steps.clone()))
                    .await;
            }
            let _ = event_tx
                .send(AgentEvent::Log(format!(
                    "Step {}/{}: {}",
                    step_idx + 1,
                    total,
                    tool_name
                )))
                .await;

            let mut params: Value = serde_json::from_str(&tc.function.arguments)
                .unwrap_or(Value::Object(serde_json::Map::new()));

            inject_prev_data(&mut params, &step_results);

            info!(step = step_idx + 1, tool = %tool_name, "executing step");
            debug!(params = %params, "tool params");

            match tool.execute(params, ctx).await {
                Ok(result) => {
                    let summary = summarise_value(&result);
                    info!(step = step_idx + 1, tool = %tool_name, result_summary = %summary, "step completed");
                    all_steps[step_idx].status = StepStatus::Done;
                    all_steps[step_idx].result = Some(result.clone());
                    step_results.push(result.clone());
                    if is_main {
                        let _ = event_tx
                            .send(AgentEvent::StepCompleted(step_idx, result.clone()))
                            .await;
                    } else {
                        let _ = event_tx
                            .send(AgentEvent::SubPlanReady(all_steps.clone()))
                            .await;
                    }

                    let llm_summary = summarize_for_llm(&result);
                    messages.push(Message::tool_result(&tc.id, llm_summary));
                }
                Err(e) => {
                    let err_msg = e.to_string();
                    error!(step = step_idx + 1, tool = %tool_name, error = %err_msg, "step failed");
                    all_steps[step_idx].status = StepStatus::Failed(err_msg.clone());
                    if is_main {
                        let _ = event_tx
                            .send(AgentEvent::StepFailed(step_idx, err_msg.clone()))
                            .await;
                    } else {
                        let _ = event_tx
                            .send(AgentEvent::SubPlanReady(all_steps.clone()))
                            .await;
                    }
                    messages.push(Message::tool_result(
                        &tc.id,
                        format!("ERROR: {err_msg}. Please fix your parameters and try again."),
                    ));
                    step_results.push(Value::Null);
                }
            }
        }
    }

    Ok(None)
}

/// Produce a short human-readable summary of a JSON value for log lines.
fn summarise_value(v: &Value) -> String {
    match v {
        Value::Array(arr) => format!("array[{}]", arr.len()),
        Value::Object(map) => {
            let keys: Vec<&str> = map.keys().map(|k| k.as_str()).take(4).collect();
            format!("object{{{}}}", keys.join(", "))
        }
        Value::String(s) if s.len() > 80 => format!("\"{}…\"", &s[..80]),
        other => other.to_string(),
    }
}

/// Produce a summary of a tool result suitable for sending back to the LLM.
/// Small results are sent verbatim; large arrays are summarized to save tokens.
fn summarize_for_llm(value: &Value) -> String {
    let full = serde_json::to_string(value).unwrap_or_default();
    if full.len() <= 4000 {
        return full;
    }
    match value {
        Value::Array(arr) => {
            let sample: Vec<&Value> = arr.iter().take(3).collect();
            let sample_str = serde_json::to_string_pretty(&sample).unwrap_or_default();
            format!(
                "Array of {} items. First 3 items:\n{}\n\n\
                 (Full data available — use \"data\": null in tool parameters to reference this array automatically)",
                arr.len(),
                sample_str,
            )
        }
        _ => {
            let truncated = if full.len() > 3000 { &full[..3000] } else { &full };
            format!("{truncated}...(truncated, {} chars total)", full.len())
        }
    }
}

/// Recursively inject the most recent array result wherever `null` or `"$PREV"` appears.
fn inject_prev_data(params: &mut Value, step_results: &[Value]) {
    if let Some(data) = params.get("data") {
        if data.is_null() {
            if let Some(last_array) = step_results.iter().rev().find(|v| v.is_array()) {
                let len = last_array.as_array().map(|a| a.len()).unwrap_or(0);
                debug!(injected_rows = len, "auto-injected previous array result into 'data'");
                params["data"] = last_array.clone();
            }
        }
    }
    replace_prev_placeholder(params, step_results);
    // After placeholder substitution, if `data` is still not an array (e.g. the LLM sent
    // an object like {"__v":"$PREV","step":1} as a step-reference), fall back to auto-inject.
    if let Some(data) = params.get("data") {
        if !data.is_array() && !data.is_null() {
            if let Some(last_array) = step_results.iter().rev().find(|v| v.is_array()) {
                let len = last_array.as_array().map(|a| a.len()).unwrap_or(0);
                debug!(injected_rows = len, "auto-injected previous array (data was non-array value)");
                params["data"] = last_array.clone();
            }
        }
    }
}

fn replace_prev_placeholder(value: &mut Value, step_results: &[Value]) {
    match value {
        Value::String(s) if s == "$PREV" => {
            if let Some(last_array) = step_results.iter().rev().find(|v| v.is_array()) {
                *value = last_array.clone();
            }
        }
        Value::Object(map) => {
            for v in map.values_mut() {
                replace_prev_placeholder(v, step_results);
            }
        }
        Value::Array(arr) => {
            for v in arr.iter_mut() {
                replace_prev_placeholder(v, step_results);
            }
        }
        _ => {}
    }
}
