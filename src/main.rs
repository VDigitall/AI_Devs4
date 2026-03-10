#![allow(dead_code)]

mod agent;
mod app;
mod config;
mod event;
mod llm;
mod tools;
mod ui;

use anyhow::Result;
use crossterm::{
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io;
use tracing::info;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{fmt, EnvFilter};

use app::App;
use config::Config;
use event::EventHandler;

/// Initialise file-based tracing. Returns the guard that must be kept alive
/// for the duration of the program to flush the non-blocking writer on drop.
fn init_tracing() -> WorkerGuard {
    let file_appender = tracing_appender::rolling::never(".", "debug.log");
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("ai_devs4=debug,info"));

    fmt::Subscriber::builder()
        .with_env_filter(filter)
        .with_writer(non_blocking)
        .with_ansi(false)
        .with_target(true)
        .with_thread_ids(false)
        .init();

    guard
}

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install().ok();
    let _log_guard = init_tracing();

    let config = Config::from_env()?;
    info!("Config loaded. Model: {}", config.openrouter_model);

    // Set up terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = run(&mut terminal, config).await;

    // Always restore terminal, even on error
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    result
}

async fn run(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    config: Config,
) -> Result<()> {
    let mut app = App::new(config)?;
    let mut events = EventHandler::new(100); // 100ms tick

    loop {
        terminal.draw(|frame| ui::render(frame, &mut app))?;

        if app.should_quit {
            break;
        }

        let event = events.next().await?;

        // Handle the event through the app
        use event::AppEvent;
        match event {
            AppEvent::Key(key) => {
                use crossterm::event::KeyCode;
                use event::is_quit;

                if is_quit(&key) {
                    app.should_quit = true;
                    break;
                }

                match key.code {
                    KeyCode::Up => app.select_prev_task(),
                    KeyCode::Down => app.select_next_task(),
                    KeyCode::Enter => app.trigger_run_task(&events).await,
                    KeyCode::PageDown => app.scroll_log_down(5),
                    KeyCode::PageUp => app.scroll_log_up(5),
                    KeyCode::End => app.scroll_log_to_bottom(),
                    KeyCode::Esc => app.dismiss_error(),
                    KeyCode::Char('r') => app.toggle_reveal_flags(),
                    _ => {}
                }
            }
            AppEvent::Agent(agent_event) => {
                app.apply_agent_event(agent_event);
            }
            AppEvent::Tick => {}
            AppEvent::Resize(_, _) => {}
        }
    }

    Ok(())
}
