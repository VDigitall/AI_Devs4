use anyhow::Result;
use crossterm::event::{Event as CrosstermEvent, EventStream, KeyCode, KeyEvent, KeyModifiers};
use futures::StreamExt;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::interval;

use crate::agent::AgentEvent;

/// All events the main app loop handles.
#[derive(Debug)]
pub enum AppEvent {
    /// A keyboard event from the terminal.
    Key(KeyEvent),
    /// A periodic tick for UI refresh.
    Tick,
    /// An event from the running agent.
    Agent(AgentEvent),
    /// Terminal resize.
    Resize(u16, u16),
}

pub struct EventHandler {
    rx: mpsc::Receiver<AppEvent>,
    /// Public sender so the agent can push AgentEvents into the same channel.
    pub agent_tx: mpsc::Sender<AgentEvent>,
}

impl EventHandler {
    pub fn new(tick_rate_ms: u64) -> Self {
        let (tx, rx) = mpsc::channel::<AppEvent>(256);
        let (agent_tx, mut agent_rx) = mpsc::channel::<AgentEvent>(256);

        // Forward agent events into the main event channel
        let tx_agent_fwd = tx.clone();
        tokio::spawn(async move {
            while let Some(ev) = agent_rx.recv().await {
                if tx_agent_fwd.send(AppEvent::Agent(ev)).await.is_err() {
                    break;
                }
            }
        });

        // Crossterm event stream + tick loop
        tokio::spawn(async move {
            let mut reader = EventStream::new();
            let mut tick = interval(Duration::from_millis(tick_rate_ms));

            loop {
                tokio::select! {
                    _ = tick.tick() => {
                        if tx.send(AppEvent::Tick).await.is_err() { break; }
                    }
                    maybe_event = reader.next() => {
                        match maybe_event {
                            Some(Ok(CrosstermEvent::Key(key))) => {
                                if tx.send(AppEvent::Key(key)).await.is_err() { break; }
                            }
                            Some(Ok(CrosstermEvent::Resize(w, h))) => {
                                if tx.send(AppEvent::Resize(w, h)).await.is_err() { break; }
                            }
                            Some(Err(_)) | None => break,
                            _ => {}
                        }
                    }
                }
            }
        });

        Self { rx, agent_tx }
    }

    /// Receive the next event (blocks until one is available).
    pub async fn next(&mut self) -> Result<AppEvent> {
        self.rx
            .recv()
            .await
            .ok_or_else(|| anyhow::anyhow!("Event channel closed"))
    }
}

/// Convenience: check if a key event is quit (q or Ctrl-C).
pub fn is_quit(key: &KeyEvent) -> bool {
    matches!(key.code, KeyCode::Char('q'))
        || (key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL))
}
