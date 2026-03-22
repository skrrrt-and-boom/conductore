# Conductor

Multi-agent AI orchestrator that decomposes complex coding tasks into parallel subtasks, each executed by an independent Claude Code session in isolated git worktrees.

## How It Works

1. **Conductor agent** (Opus) explores the codebase, analyzes the task, and decomposes it into a DAG of subtasks
2. **Musician agents** (Sonnet) execute subtasks in parallel, each in its own git worktree with a dedicated Claude Code session
3. **Orchestra** manages the state machine: scheduling tasks respecting dependencies, handling rate limits, merging branches, and running reviews
4. **TUI** renders real-time progress via Ratatui — musician output streams, task graph, insights, and controls

## Quick Start

```bash
# Build
cargo build --release

# Interactive mode (TUI)
./target/release/conductor -p /path/to/project

# Headless mode
./target/release/conductor run -p /path/to/project -t "Add retry logic to the API client"

# With options
./target/release/conductor run \
  -p /path/to/project \
  -t "Refactor auth module" \
  -m 4 \
  --conductor-model opus \
  --musician-model sonnet \
  --max-turns 30
```

## CLI Commands

| Command | Description |
|---------|-------------|
| `conductor` | Interactive TUI mode |
| `conductor run` | Headless mode with task |
| `conductor resume -s <id>` | Resume a paused session |
| `conductor list` | List all sessions |
| `conductor status -s <id>` | Show session status |
| `conductor clean --all` | Remove session data |

## Project Structure

Cargo workspace with 5 crates:

```
crates/
  conductor-types/    # Shared types, enums, events (no logic)
  conductor-bridge/   # Claude CLI spawning + NDJSON stream parsing
  conductor-core/     # Orchestra, Musician, Conductor agent, worktrees
  conductor-tui/      # Ratatui terminal UI
  conductor-cli/      # Binary entry point + CLI arg parsing
```

## Architecture

### Channel-Based Communication

```
TUI ──action_tx──> Orchestra ──state_tx──> TUI (watch channel)
                       ^
Musicians ──event_tx───┘ (mpsc channel)
```

- `watch` channel broadcasts `OrchestraState` from Orchestra to TUI
- `mpsc` channels carry `OrchestraEvent` from Musicians to Orchestra
- `mpsc` channel carries `UserAction` from TUI to Orchestra
- Orchestra is the single owner of state — no `Arc<Mutex<>>`

### Phase State Machine

```
Init → Exploring → Analyzing → Decomposing → PlanReview
  → PhaseDetailing → PhaseExecuting → PhaseMerging → PhaseReviewing
  → (next phase or FinalReview) → Complete
```

### Key Components

- **Orchestra** (`conductor-core/src/orchestra.rs`): Main `tokio::select!` event loop driving all phase transitions
- **Musician** (`conductor-core/src/musician.rs`): Wraps a `ClaudeSession`, streams events, manages output buffering
- **Conductor Agent** (`conductor-core/src/conductor_agent.rs`): LLM-powered planning, review, and guidance processing
- **Claude Bridge** (`conductor-bridge/src/session.rs`): Spawns `claude` CLI, streams NDJSON, bidirectional stdin/stdout
- **Worktree Manager** (`conductor-core/src/worktree_manager.rs`): Git worktree isolation per musician
- **TUI App** (`conductor-tui/src/app.rs`): Ratatui immediate-mode rendering with crossterm backend

## Development

```bash
cargo check --workspace       # Type check all crates
cargo build --workspace       # Build all crates
cargo test --workspace        # Run all tests
cargo build --release         # Optimized release build
cargo check -p conductor-core # Check a single crate
```

## Prerequisites

- Rust (edition 2024)
- Claude Code CLI (`npm install -g @anthropic-ai/claude-code`)
- Git (for worktree operations)

## License

MIT
