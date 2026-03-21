# conductor-rust

Rust rewrite of the conductor multi-agent orchestrator.

## Project Structure

This is a Cargo workspace with 5 crates:

```
crates/
  conductor-types/    # Shared types, enums, events (no logic)
  conductor-bridge/   # Claude CLI spawning + NDJSON stream parsing
  conductor-core/     # Orchestra, Musician, Conductor logic
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

## Architecture Decisions

1. **Async runtime**: tokio (full features)
2. **State management**: Channels, NOT Arc<Mutex>
   - `tokio::sync::watch` for state broadcasting (orchestra -> TUI)
   - `tokio::sync::mpsc` for events (musicians -> orchestra)
   - `tokio::sync::mpsc` for user input (TUI -> orchestra)
3. **Error handling**: `thiserror` in library crates, `anyhow` in conductor-cli
4. **TUI**: Ratatui + crossterm backend
5. **Serialization**: serde + serde_json for all types

## Key Rules

- All shared types live in `conductor-types`. Other crates import from it.
- Do NOT use `Arc<Mutex<>>` for OrchestraState. Orchestra is the single owner.
- Every function that does I/O must be async (tokio).
- Use `tracing` for logging, not `println!`.
- Port tests from the TypeScript source alongside the implementation.

## TypeScript Reference

The original TypeScript conductor is at `/Users/dominiknowak/code/conductor/`.
Key files to reference:
- `src/types.ts` — all type definitions (already ported to conductor-types)
- `src/orchestra.ts` — main state machine (2055 lines)
- `src/musician.ts` — musician agent (465 lines)
- `src/conductor.ts` — LLM prompt construction (1250 lines)
- `src/claude-bridge.ts` — Claude CLI session management (454 lines)
- `src/components/` — Ink/React TUI components
- `src/rate-limiter.ts` — rate limit detection
- `src/worktree-manager.ts` — git worktree isolation
- `src/memory.ts` — shared memory file coordination
- `src/task-store.ts` — session/task persistence
- `src/token-estimate.ts` — token count estimation
- `src/tool-summary.ts` — tool use display formatting
- `src/insights.ts` — insight extraction from tool use
- `src/layout.ts` — TUI layout breakpoint logic
- `src/caffeinate.ts` — macOS sleep prevention
