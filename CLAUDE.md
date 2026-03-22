# Conductor

Multi-agent AI orchestrator built in Rust. Decomposes complex coding tasks into parallel subtasks executed by independent Claude Code sessions in isolated git worktrees.

## Project Structure

Cargo workspace with 5 crates:

```
crates/
  conductor-types/    # Shared types, enums, events (no logic)
  conductor-bridge/   # Claude CLI spawning + NDJSON stream parsing
  conductor-core/     # Orchestra, Musician, Conductor agent, worktrees
  conductor-tui/      # Ratatui terminal UI rendering
  conductor-cli/      # Binary entry point + CLI arg parsing
```

## Commands

```bash
cargo check --workspace       # Type check all crates
cargo build --workspace       # Build all crates
cargo test --workspace        # Run all tests
cargo build --release         # Optimized release build
cargo check -p conductor-core # Check a single crate
```

## Before finishing any task

- [ ] `cargo check --workspace` passes with zero errors
- [ ] `cargo test --workspace` passes
- [ ] `cargo build --release && cp target/release/conductor ~/.cargo/bin/conductor` to install updated binary
- [ ] No `println!` in library crates — use `tracing` macros
- [ ] New public items have doc comments

## Architecture Decisions

1. **Async runtime**: tokio (full features)
2. **State management**: Channels, NOT Arc<Mutex>
   - `tokio::sync::watch` for state broadcasting (orchestra → TUI)
   - `tokio::sync::mpsc` for events (musicians → orchestra)
   - `tokio::sync::mpsc` for user input (TUI → orchestra)
3. **Error handling**: `thiserror` in library crates, `anyhow` only in conductor-cli
4. **TUI**: Ratatui + crossterm backend, immediate-mode rendering
5. **Serialization**: serde + serde_json for all types

## Key Rules

- All shared types live in `conductor-types`. Other crates import from it.
- Do NOT use `Arc<Mutex<>>` for OrchestraState. Orchestra is the single owner.
- Every function that does I/O must be async (tokio).
- Use `tracing` for logging, not `println!`.
- TUI render functions are pure: `fn render_xxx(f: &mut Frame, area: Rect, ...)` — no side effects.
- Musician execution is spawned on tokio tasks, communicating back via `mpsc` channels.

## Key Files

| File | Purpose |
|------|---------|
| `crates/conductor-cli/src/main.rs` | Binary entry, CLI parsing, wiring |
| `crates/conductor-core/src/orchestra.rs` | Main state machine + `select!` event loop |
| `crates/conductor-core/src/musician.rs` | Claude session wrapper, output buffering |
| `crates/conductor-core/src/conductor_agent.rs` | LLM planning, review, guidance |
| `crates/conductor-bridge/src/session.rs` | Claude CLI process management |
| `crates/conductor-types/src/state.rs` | All state types and enums |
| `crates/conductor-types/src/events.rs` | Event and action types |
| `crates/conductor-tui/src/app.rs` | TUI event loop |

## Phase State Machine

```
Init → Exploring → Analyzing → Decomposing → PlanReview
  → PhaseDetailing → PhaseExecuting → PhaseMerging → PhaseReviewing
  → (next phase or FinalReview) → Complete
```

## Channel Architecture

```
TUI ──action_tx──> Orchestra ──state_tx──> TUI (watch)
                       ^
Musicians ──event_tx───┘ (mpsc)
```

## Common Patterns

### Adding a new module to conductor-core
1. Create `crates/conductor-core/src/new_module.rs`
2. Add `pub mod new_module;` to `crates/conductor-core/src/lib.rs`
3. Import types from `conductor_types`
4. Use `CoreError` for error variants or extend it
5. Write tests as `#[cfg(test)] mod tests` at bottom of file

### Adding a new TUI component
1. Create `crates/conductor-tui/src/components/new_component.rs`
2. Add `pub mod new_component;` to `crates/conductor-tui/src/components/mod.rs`
3. Implement as `pub fn render_xxx(f: &mut Frame, area: Rect, ...)`
4. Call from `app.rs` render function

### Adding new event types
1. Add variant to relevant enum in `crates/conductor-types/src/events.rs`
2. Handle in orchestra's `handle_event()` or `handle_action()` match
3. Ensure it derives all required traits (Debug, Clone, Serialize, Deserialize)

## Documentation

- `README.md` — Project overview and quick start
- `docs/ARCHITECTURE.md` — Detailed architecture and crate responsibilities
