pub mod prompts;

use anyhow::{anyhow, Result};
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::mpsc;

use crate::llm::{LlmClient, Message};
use crate::tools::{ToolContext, ToolRegistry};

// ── Plan step ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct PlanStep {
    pub tool_call_id: String,
    pub tool_name: String,
    pub arguments: String,
    pub status: StepStatus,
    pub result: Option<Value>,
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
    Done,
    Error(String),
}

// ── Agent ─────────────────────────────────────────────────────────────────────

pub struct Agent {
    llm: LlmClient,
    registry: Arc<ToolRegistry>,
}

impl Agent {
    pub fn new(llm: LlmClient, registry: Arc<ToolRegistry>) -> Self {
        Self { llm, registry }
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
        // ── Phase 1: Planning ──────────────────────────────────────────────────
        let _ = event_tx.send(AgentEvent::Log("Planning...".to_string())).await;

        let tool_defs = self.registry.definitions();
        let messages = vec![
            Message::system(prompts::PLANNING_SYSTEM),
            Message::user(task_content),
        ];

        let response = self
            .llm
            .chat(messages, Some(tool_defs), None)
            .await
            .map_err(|e| anyhow!("Planning failed: {e}"))?;

        // Check if LLM returned tool calls (the plan) or plain text (missing tools / error)
        let tool_calls = match &response.tool_calls {
            Some(calls) if !calls.is_empty() => calls.clone(),
            _ => {
                let msg = response
                    .content
                    .unwrap_or_else(|| prompts::MISSING_TOOL_MSG.to_string());
                let _ = event_tx.send(AgentEvent::Error(msg)).await;
                return Ok(());
            }
        };

        // Convert tool calls to plan steps
        let mut plan: Vec<PlanStep> = tool_calls
            .iter()
            .map(|tc| PlanStep {
                tool_call_id: tc.id.clone(),
                tool_name: tc.function.name.clone(),
                arguments: tc.function.arguments.clone(),
                status: StepStatus::Pending,
                result: None,
            })
            .collect();

        let _ = event_tx
            .send(AgentEvent::Log(format!(
                "Plan ready: {} steps",
                plan.len()
            )))
            .await;
        let _ = event_tx.send(AgentEvent::PlanReady(plan.clone())).await;

        // ── Phase 2: Execution ─────────────────────────────────────────────────
        // Build a log channel that forwards to event_tx as AgentEvent::Log
        let (log_tx, mut log_rx) = mpsc::channel::<String>(64);
        let event_tx_log = event_tx.clone();
        tokio::spawn(async move {
            while let Some(msg) = log_rx.recv().await {
                let _ = event_tx_log.send(AgentEvent::Log(msg)).await;
            }
        });

        let tool_ctx = ToolContext::new(self.llm.clone(), log_tx);

        // Carry results between steps: last step's output becomes available for context
        let mut step_results: Vec<Value> = Vec::new();
        let total_steps = plan.len();

        for (i, step) in plan.iter_mut().enumerate() {
            let tool = match self.registry.get(&step.tool_name) {
                Some(t) => t,
                None => {
                    step.status = StepStatus::MissingTool;
                    let msg = format!(
                        "Missing tool '{}'. Please implement it and restart.",
                        step.tool_name
                    );
                    let _ = event_tx.send(AgentEvent::MissingTool(step.tool_name.clone())).await;
                    let _ = event_tx.send(AgentEvent::Log(msg)).await;
                    return Ok(());
                }
            };

            step.status = StepStatus::Running;
            let _ = event_tx.send(AgentEvent::StepStarted(i)).await;
            let _ = event_tx
                .send(AgentEvent::Log(format!(
                    "Step {}/{}: {}",
                    i + 1,
                    total_steps,
                    step.tool_name
                )))
                .await;

            // Parse arguments; inject previous result if arguments reference it
            let mut params: Value = serde_json::from_str(&step.arguments)
                .unwrap_or(Value::Object(serde_json::Map::new()));

            // Auto-inject: if a parameter named "data" is missing or null and we have a
            // previous result that is an array, use it automatically.
            if params.get("data").map(|v| v.is_null()).unwrap_or(false) {
                if let Some(last) = step_results.last() {
                    if last.is_array() {
                        params["data"] = last.clone();
                    }
                }
            }

            match tool.execute(params, &tool_ctx).await {
                Ok(result) => {
                    step.status = StepStatus::Done;
                    step.result = Some(result.clone());
                    step_results.push(result.clone());
                    let _ = event_tx.send(AgentEvent::StepCompleted(i, result)).await;
                }
                Err(e) => {
                    let err_msg = e.to_string();
                    step.status = StepStatus::Failed(err_msg.clone());
                    let _ = event_tx.send(AgentEvent::StepFailed(i, err_msg)).await;
                    return Ok(());
                }
            }
        }

        let _ = event_tx.send(AgentEvent::Log("All steps completed.".to_string())).await;
        let _ = event_tx.send(AgentEvent::Done).await;
        Ok(())
    }
}
