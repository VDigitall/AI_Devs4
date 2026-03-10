# AI_Devs4

A Rust terminal UI application for solving [AI_Devs 4](https://aidevs.pl) course tasks. Each task is driven by an LLM-powered agent with a toolbox (HTTP calls, CSV/JSON parsing, geocoding, data filtering, etc.) and rendered in an interactive TUI built with [ratatui](https://github.com/ratatui-org/ratatui).

## Requirements

- Rust (stable, 2021 edition) — install via [rustup](https://rustup.rs)
- API keys for [OpenRouter](https://openrouter.ai) and AG3NTS

## Setup

```bash
cp .env.example .env
# Fill in your keys in .env
```

`.env` variables:

| Variable | Description |
|---|---|
| `AG3NTS_API_KEY` | AG3NTS platform API key |
| `OPENROUTER_API_KEY` | OpenRouter API key |
| `OPENROUTER_MODEL` | Model to use (e.g. `openai/gpt-4o-mini`) |

## Running

```bash
cargo run
```

- `↑ / ↓` — select a task
- `Enter` — run selected task
- `PageUp / PageDown` — scroll log
- `End` — jump to bottom of log
- `r` — toggle flag reveal
- `Esc` — dismiss error
- `q` / `Ctrl+C` — quit

Debug output is written to `<task>.debug.log` in the project root.

## Project Structure

```
src/
  main.rs          # Entry point, terminal setup
  app.rs           # App state and event handling
  config.rs        # Config loaded from .env
  event.rs         # Async event loop
  agent/           # LLM agent loop and prompts
  llm/             # OpenRouter client
  tools/           # Agent tools (HTTP, CSV, JSON, geocoding, …)
  ui/              # Ratatui widgets (task list, log, plan, status bar)
tasks/             # Task definitions (Markdown)
artifacts/         # Task inputs/outputs
```

## Development

```bash
cargo build          # compile
cargo clippy         # lint
RUST_LOG=debug cargo run   # verbose logging
```

Add a new task by creating a `.md` file in `tasks/` and wiring it up in `app.rs`.
