# Architecture

## Crate Dependency Graph

```
conductor-cli
  ├── conductor-tui
  │     └── conductor-types
  ├── conductor-core
  │     ├── conductor-types
  │     └── conductor-bridge
  │           └── conductor-types
  └── conductor-types
```

## conductor-types

Pure data definitions — no logic. All other crates import from here.

**Key types:**
- `OrchestraState` — full snapshot of orchestra state, broadcast via `watch` channel
- `OrchestraConfig` — session configuration (project path, models, musician count)
- `OrchestraPhase` — state machine phase enum
- `MusicianState` — per-musician status, output buffer, token counts
- `Task` / `TaskStatus` — DAG task nodes with dependencies
- `ClaudeEvent` / `ClaudeEventType` — parsed NDJSON events from Claude CLI
- `OrchestraEvent` — musician → orchestra event envelope
- `UserAction` — TUI → orchestra user input

**Rules:**
- All enums derive `Debug, Clone, Serialize, Deserialize, PartialEq, Eq`
- All structs derive `Debug, Clone, Serialize, Deserialize`
- No business logic — only types and Default impls

## conductor-bridge

Manages Claude Code CLI process lifecycle and NDJSON stream parsing.

**Components:**
- `ClaudeSession` — spawns `claude` CLI with `--input-format stream-json --output-format stream-json`, maintains stdin/stdout pipe
- `validate` — checks CLI availability and model name validity
- `parse` — parses NDJSON lines into `ClaudeEvent`, detects rate limit messages

**Session lifecycle:**
1. `ClaudeSession::new()` — spawns child process with configured args
2. `start(prompt)` — sends initial message via stdin, spawns stdout reader task
3. Reader task parses each stdout line → `ClaudeEvent` → sends through `mpsc` channel
4. `send_message(text)` — writes JSON to stdin for follow-up messages
5. `close()` / `Drop` — kills child process

**Error type:** `BridgeError` (thiserror)

## conductor-core

All orchestration logic.

### Orchestra (`orchestra.rs`)

The central state machine. Owns `OrchestraState` directly (no Arc/Mutex).

**Main loop pattern:**
```rust
loop {
    tokio::select! {
        Some(event) = event_rx.recv() => { handle_event(event); }
        Some(action) = action_rx.recv() => { handle_action(action); }
        _ = tick_interval.tick() => { tick(); }
    }
    state_tx.send(state.clone()).ok();
    if terminal_phase { break; }
}
```

**Responsibilities:**
- Phase transitions through the full state machine
- Task scheduling: find ready tasks, assign to idle musicians, respect DAG dependencies
- Rate limit handling: pause/probe/resume musicians
- Plan refinement: chat-style back-and-forth during PlanReview phase
- Branch merging and review orchestration

### Musician (`musician.rs`)

Wraps a `ClaudeSession` for task execution. Spawned on tokio tasks during execution.

- Output buffering (200 lines max, 100KB max)
- Token estimation during streaming
- Tool use tracking and file modification detection
- Checkpoint detection (git commits in Bash tool calls)
- Mid-execution prompt injection for user guidance

### Conductor Agent (`conductor_agent.rs`)

The LLM-powered planning brain. Uses Claude to:
- Explore the codebase and analyze the task
- Decompose into a DAG of subtasks with dependencies
- Review completed phases and plan next ones
- Process user guidance into actionable changes

### Supporting Modules

| Module | Purpose |
|--------|---------|
| `dag.rs` | Kahn's algorithm cycle detection on task DAGs |
| `rate_limiter.rs` | Rate limit state machine (ok → warning → limited → ok) |
| `token_estimate.rs` | Token count estimation with calibration |
| `tool_summary.rs` | Human-readable formatting of Claude tool use |
| `memory.rs` | Shared memory file coordination between musicians |
| `task_store.rs` | Session persistence to `~/.conductor/sessions/` |
| `worktree_manager.rs` | Git worktree creation/removal/merging per musician |
| `insights.rs` | Rule-based insight extraction from tool use patterns |
| `caffeinate.rs` | macOS sleep prevention (spawns `caffeinate` process) |

## conductor-tui

Immediate-mode Ratatui rendering. Pure display — no business logic.

**Signature pattern:** `fn render_xxx(f: &mut Frame, area: Rect, ...)`

**Components:**
- `app.rs` — `TuiApp` with event loop (crossterm key events + state watch)
- `theme.rs` — colors, status symbols, sparkline/progress bar rendering
- `layout.rs` — responsive breakpoints (NARROW=80, WIDE=160, TALL=40)
- `components/` — musician grid, insight panel, input bar, panels, status line, header

**UiState** holds only local UI concerns: focused panel, scroll offset, help overlay visibility, prompt input text.

## conductor-cli

Binary entry point. Wires everything together:
- CLI parsing via `clap` derive
- Channel creation and thread spawning
- Interactive mode: Orchestra on main task, TUI spawned
- Headless mode: Orchestra only, progress via tracing
- Session management commands (list, status, resume, clean)

**Error handling:** `anyhow` (only crate that uses it — all library crates use `thiserror`)
