use anyhow::Result;
use ratatui::widgets::ListState;
use std::path::Path;
use std::sync::Arc;
use tracing::{debug, info, warn};

use crate::agent::{Agent, AgentEvent, PlanStep, StepStatus};
use crate::config::Config;
use crate::event::EventHandler;
use crate::llm::LlmClient;
use crate::tools::default_registry;

const TASKS_DIR: &str = "tasks";
const LOG_MAX: usize = 500;

// ── State machine ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum AppState {
    Idle,
    Planning,
    Executing { current_step: usize },
    Done,
    Error(String),
}

// ── App ───────────────────────────────────────────────────────────────────────

pub struct App {
    pub state: AppState,
    pub task_names: Vec<String>,
    pub task_list_state: ListState,
    pub plan_steps: Vec<PlanStep>,
    pub current_step: Option<usize>,
    pub logs: Vec<String>,
    pub log_scroll: u16,
    pub should_quit: bool,
    /// Raw secret values that should be masked in the TUI log display.
    pub secrets: Vec<String>,
    /// Whether `{FLG:…}` patterns are revealed in the TUI log.
    pub reveal_flags: bool,

    llm: LlmClient,
}

impl App {
    pub fn new(config: Config) -> Result<Self> {
        let secrets = vec![
            config.ag3nts_api_key.clone(),
            config.openrouter_api_key.clone(),
        ]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect();

        let llm = LlmClient::new(
            config.openrouter_api_key.clone(),
            config.openrouter_model.clone(),
        );

        let task_names = load_task_names(TASKS_DIR);
        info!(tasks = task_names.len(), "loaded tasks from '{TASKS_DIR}/'");
        for name in &task_names {
            debug!(task = %name, "task available");
        }

        let mut task_list_state = ListState::default();
        if !task_names.is_empty() {
            task_list_state.select(Some(0));
        }

        Ok(Self {
            state: AppState::Idle,
            task_names,
            task_list_state,
            plan_steps: Vec::new(),
            current_step: None,
            logs: Vec::new(),
            log_scroll: 0,
            should_quit: false,
            secrets,
            reveal_flags: false,
            llm,
        })
    }

    pub fn toggle_reveal_flags(&mut self) {
        self.reveal_flags = !self.reveal_flags;
    }

    // ── Navigation ─────────────────────────────────────────────────────────────

    pub fn select_prev_task(&mut self) {
        if self.task_names.is_empty() {
            return;
        }
        let i = self
            .task_list_state
            .selected()
            .map(|i| if i == 0 { self.task_names.len() - 1 } else { i - 1 })
            .unwrap_or(0);
        self.task_list_state.select(Some(i));
    }

    pub fn select_next_task(&mut self) {
        if self.task_names.is_empty() {
            return;
        }
        let i = self
            .task_list_state
            .selected()
            .map(|i| (i + 1) % self.task_names.len())
            .unwrap_or(0);
        self.task_list_state.select(Some(i));
    }

    // ── Log scrolling ──────────────────────────────────────────────────────────

    pub fn scroll_log_down(&mut self, n: u16) {
        self.log_scroll = self.log_scroll.saturating_add(n);
    }

    pub fn scroll_log_up(&mut self, n: u16) {
        self.log_scroll = self.log_scroll.saturating_sub(n);
    }

    pub fn scroll_log_to_bottom(&mut self) {
        self.log_scroll = self.logs.len().saturating_sub(1) as u16;
    }

    // ── Error handling ─────────────────────────────────────────────────────────

    pub fn dismiss_error(&mut self) {
        if matches!(self.state, AppState::Error(_)) {
            self.state = AppState::Idle;
        }
    }

    // ── Task execution ─────────────────────────────────────────────────────────

    pub async fn trigger_run_task(&mut self, events: &EventHandler) {
        if !matches!(self.state, AppState::Idle | AppState::Done | AppState::Error(_)) {
            warn!("trigger_run_task called while state is not idle/done/error — ignoring");
            self.push_log("A task is already running. Wait for it to finish.");
            return;
        }

        let selected = match self.task_list_state.selected() {
            Some(i) => i,
            None => {
                self.push_log("No task selected.");
                return;
            }
        };

        let task_name = match self.task_names.get(selected) {
            Some(n) => n.clone(),
            None => return,
        };

        let task_path = format!("{TASKS_DIR}/{task_name}.md");
        let content = match std::fs::read_to_string(&task_path) {
            Ok(c) => c,
            Err(e) => {
                warn!(path = %task_path, error = %e, "failed to read task file");
                self.state = AppState::Error(format!("Cannot read {task_path}: {e}"));
                return;
            }
        };

        info!(task = %task_name, chars = content.len(), "starting task");

        // Reset state for new run
        self.plan_steps.clear();
        self.current_step = None;
        self.logs.clear();
        self.log_scroll = 0;
        self.state = AppState::Planning;
        debug!("state → Planning");

        self.push_log(format!("Starting task: {task_name}"));

        let registry = Arc::new(default_registry());
        let agent = Agent::new(self.llm.clone(), registry);
        let agent_tx = events.agent_tx.clone();

        tokio::spawn(async move {
            agent.run(&content, agent_tx).await;
        });
    }

    // ── Agent event handling ───────────────────────────────────────────────────

    pub fn apply_agent_event(&mut self, event: AgentEvent) {
        match event {
            AgentEvent::Log(msg) => {
                debug!(log = %msg, "agent log");
                self.push_log(msg);
                self.scroll_log_to_bottom();
            }
            AgentEvent::PlanReady(steps) => {
                let first_pending = steps
                    .iter()
                    .position(|s| s.status == StepStatus::Pending)
                    .unwrap_or(0);
                info!(steps = steps.len(), first_pending, "plan updated → state Executing");
                self.plan_steps = steps;
                self.state = AppState::Executing { current_step: first_pending };
            }
            AgentEvent::StepStarted(i) => {
                let tool = self.plan_steps.get(i).map(|s| s.tool_name.as_str()).unwrap_or("?");
                debug!(step = i + 1, tool = %tool, "step started");
                self.current_step = Some(i);
                if let Some(step) = self.plan_steps.get_mut(i) {
                    step.status = StepStatus::Running;
                }
                self.state = AppState::Executing { current_step: i };
            }
            AgentEvent::StepCompleted(i, _result) => {
                let tool = self.plan_steps.get(i).map(|s| s.tool_name.as_str()).unwrap_or("?");
                debug!(step = i + 1, tool = %tool, "step completed");
                if let Some(step) = self.plan_steps.get_mut(i) {
                    step.status = StepStatus::Done;
                }
            }
            AgentEvent::StepFailed(i, err) => {
                let tool = self.plan_steps.get(i).map(|s| s.tool_name.as_str()).unwrap_or("?");
                warn!(step = i + 1, tool = %tool, error = %err, "step failed → state Error");
                if let Some(step) = self.plan_steps.get_mut(i) {
                    step.status = StepStatus::Failed(err.clone());
                }
                self.state = AppState::Error(format!("Step {} failed: {err}", i + 1));
            }
            AgentEvent::MissingTool(name) => {
                warn!(tool = %name, "missing tool → state Error");
                self.state = AppState::Error(format!("Missing tool: {name}"));
            }
            AgentEvent::Done => {
                info!("agent done → state Done");
                self.state = AppState::Done;
                self.current_step = None;
            }
            AgentEvent::Error(e) => {
                warn!(error = %e, "agent error → state Error");
                self.state = AppState::Error(e);
            }
        }
    }

    // ── Internal helpers ───────────────────────────────────────────────────────

    fn push_log(&mut self, msg: impl Into<String>) {
        self.logs.push(msg.into());
        if self.logs.len() > LOG_MAX {
            self.logs.drain(0..self.logs.len() - LOG_MAX);
        }
    }
}

// ── File helpers ──────────────────────────────────────────────────────────────

fn load_task_names(dir: &str) -> Vec<String> {
    let path = Path::new(dir);
    if !path.is_dir() {
        return Vec::new();
    }

    let mut names: Vec<String> = std::fs::read_dir(path)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let p = entry.path();
            if p.extension().and_then(|e| e.to_str()) == Some("md") {
                p.file_stem()
                    .and_then(|s| s.to_str())
                    .map(|s| s.to_string())
            } else {
                None
            }
        })
        .collect();

    names.sort();
    names
}
