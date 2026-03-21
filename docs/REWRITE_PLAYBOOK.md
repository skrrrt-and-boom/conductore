# Conductor Rust Rewrite Playbook

All decisions are made. Workspace is scaffolded. Types are ported.
This document contains **copy-paste-ready prompts** for every phase.

---

## Pre-Work: Install Rust + Verify Skeleton

```bash
# Step 1: Install Rust (if not installed)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"

# Step 2: Init git repo
cd /Users/dominiknowak/code/conductor-rust
git init
git add -A
git commit -m "initial: workspace skeleton with types crate"

# Step 3: Verify it compiles
cargo check --workspace
```

If `cargo check` fails, fix the errors before proceeding. Every session below
depends on a green `cargo check --workspace` as the baseline.

---

## Phase 0: Types Crate Completion (Claude Code — single session)

The types crate is already scaffolded but may need refinements as we go.
Run this in a **single Claude Code session** (not conductor):

```bash
cd /Users/dominiknowak/code/conductor-rust
```

### Prompt (paste into Claude Code):

```
Review and complete the conductor-types crate at crates/conductor-types/.

The TypeScript source of truth is at /Users/dominiknowak/code/conductor/src/types.ts.
I've already ported all types to crates/conductor-types/src/state.rs and events to
crates/conductor-types/src/events.rs.

Tasks:
1. Read the TypeScript types.ts and compare with state.rs — ensure nothing is missing
2. Ensure all enums derive: Debug, Clone, Serialize, Deserialize, PartialEq, Eq
3. Ensure all structs derive: Debug, Clone, Serialize, Deserialize
4. Add Default impls where the TS code uses default values
5. Add a convenience impl for OrchestraState with a constructor that takes OrchestraConfig
6. Run: cargo check -p conductor-types
7. Run: cargo test -p conductor-types (add basic serialization round-trip tests)

Do NOT add any business logic. This crate is types-only.
```

---

## Phase 1: Pure Logic Ports (Conductor — 4 musicians)

### Run command:

```bash
cd /Users/dominiknowak/code/conductor-rust
conductor run \
  --project /Users/dominiknowak/code/conductor-rust \
  --task "$(cat <<'TASK'
Port conductor's pure logic modules from TypeScript to Rust.

## TypeScript Source (READ THESE FIRST)
All source files are at /Users/dominiknowak/code/conductor/src/. Read each one before porting.

## Target Crate
All output goes into: crates/conductor-core/src/
The crate's lib.rs already declares the modules: dag, rate_limiter, token_estimate, tool_summary, memory, task_store.

## What to Port

### 1. DAG cycle detection (dag.rs)
Source: /Users/dominiknowak/code/conductor/src/orchestra.ts — the function `detectDependencyCycles` (search for "Kahn's Algorithm").
- Port the full Kahn's algorithm implementation
- Port the cycle-finding DFS that follows it
- Use conductor_types::Task for the task type
- Add unit tests (port from orchestra.test.ts, search for "cycle")

### 2. Rate limiter (rate_limiter.rs)
Source: /Users/dominiknowak/code/conductor/src/rate-limiter.ts
- Port the RateLimiter struct with its state machine (ok → warning → limited → ok)
- The isRateLimitMessage() detection function
- Use tokio::time for probe interval timers
- Use tokio::sync::mpsc for sending rate limit events
- Use conductor_types::RateLimitState and RateLimitStatus
- Port tests from rate-limiter.test.ts

### 3. Token estimation (token_estimate.rs)
Source: /Users/dominiknowak/code/conductor/src/token-estimate.ts
- Port estimateTokens(), calibrateTokenEstimate(), getCalibrationRatio(), resetCalibration()
- Use a global static with std::sync::Mutex for calibration state (this is lightweight sync, fine for a counter)
- Port tests from token-estimate.test.ts

### 4. Tool summary (tool_summary.rs)
Source: /Users/dominiknowak/code/conductor/src/tool-summary.ts
- Port summarizeToolUse() — match on tool name and format a human-readable string
- Input type: tool_name: &str, tool_input: Option<&serde_json::Value>
- Port tests from tool-summary.test.ts

### 5. Shared memory (memory.rs)
Source: /Users/dominiknowak/code/conductor/src/memory.ts
- Port SharedMemory struct: init(), read(), append()
- Use tokio::fs for async file operations
- Use file-level locking with the fd-lock crate (add it to Cargo.toml if needed)
- Atomic writes: write to tmp file then rename

### 6. Task store (task_store.rs)
Source: /Users/dominiknowak/code/conductor/src/task-store.ts
- Port TaskStore struct: save/load session JSON, append logs, list sessions
- Sessions stored at ~/.conductor/sessions/{session_id}/
- Use tokio::fs for all I/O
- Use serde_json for serialization
- Port the resolveId() prefix-matching logic

## Rules
- Import all types from conductor_types (already in Cargo.toml as a dependency)
- Use thiserror for error types: create a CoreError enum in lib.rs
- Every public function needs a doc comment
- Run cargo check -p conductor-core after each module
- Run cargo test -p conductor-core to verify tests pass
TASK
)" \
  --musicians 4 \
  --conductor-model opus \
  --musician-model sonnet \
  --max-turns 30
```

### After Phase 1 — verify:
```bash
cargo check -p conductor-core
cargo test -p conductor-core
```

---

## Phase 2: Claude Bridge (Conductor — 2 musicians)

### Run command:

```bash
cd /Users/dominiknowak/code/conductor-rust
conductor run \
  --project /Users/dominiknowak/code/conductor-rust \
  --task "$(cat <<'TASK'
Port the Claude Code CLI bridge from TypeScript to Rust.

## TypeScript Source (READ FIRST)
/Users/dominiknowak/code/conductor/src/claude-bridge.ts — read the entire file.

## Target Crate
crates/conductor-bridge/src/

## What to Port

### 1. CLI Validation (validate.rs)
- validateClaudeCli(): spawn `claude --version`, check it exists (Command::new("claude"))
- validateModel(): check model name is in ["opus", "sonnet", "haiku"]
- VALID_MODELS constant

### 2. ClaudeSession (session.rs) — this is the critical piece
Source: The ClaudeSession class in claude-bridge.ts.

Architecture:
- ClaudeSession spawns `claude` CLI with these args:
  -p (prompt mode), --model <model>, --max-turns <n>,
  --input-format stream-json, --output-format stream-json,
  --append-system-prompt <text> (if provided),
  --allowedTools <tools> (if provided)
- Working directory set via Command::current_dir(cwd)
- stdin is kept open (bidirectional persistent session)
- stdout is read line-by-line as NDJSON

Implementation:
- Use tokio::process::Command to spawn the child process
- Use tokio::io::BufReader::new(stdout).lines() to stream NDJSON
- Parse each JSON line into conductor_types::ClaudeEvent using serde_json
- The ClaudeEvent JSON from Claude CLI uses "type" field with values like
  "assistant", "tool_use", "tool_result", "result", "error", "system".
  Map these to ClaudeEventType enum. The JSON field names use snake_case.
  Add #[serde(rename = "type")] for the type field.
- Forward parsed events through a tokio::sync::mpsc::Sender<ClaudeEvent>
  that is passed into ClaudeSession::new()
- sendMessage(text, images?): write JSON to stdin in stream-json format:
  {"type":"human","message":"<text>"} (plus images array if provided)
- start(prompt): send the initial message, then spawn a tokio task that
  reads stdout lines and sends events through the channel
- close(): kill the child process, drop stdin
- Implement Drop to ensure child process is killed on cleanup

Error handling:
- Create BridgeError enum with thiserror:
  - CliNotFound — claude command not on PATH
  - SpawnError(std::io::Error) — failed to start process
  - ParseError(serde_json::Error) — bad NDJSON line
  - SessionClosed — tried to send to closed session
  - UnexpectedExit(i32) — process exited with non-zero

Testing:
- Add a unit test that validates CLI detection (mock with a known command)
- Add a test for NDJSON parsing with sample Claude output lines
- DO NOT add integration tests that actually call Claude (too expensive)

## Rules
- All types come from conductor_types
- Use thiserror for BridgeError
- Use tracing::debug!/warn! for logging, not println!
- The session MUST handle the case where stdout closes before stdin
  (Claude crashes or rate-limits mid-response)
TASK
)" \
  --musicians 2 \
  --conductor-model opus \
  --musician-model sonnet \
  --max-turns 30
```

### After Phase 2 — verify:
```bash
cargo check -p conductor-bridge
cargo test -p conductor-bridge
```

---

## Phase 3: TUI Components (Conductor — 5 musicians)

### Run command:

```bash
cd /Users/dominiknowak/code/conductor-rust
conductor run \
  --project /Users/dominiknowak/code/conductor-rust \
  --task "$(cat <<'TASK'
Port conductor's TUI from Ink/React to Ratatui.

## TypeScript Source (READ THESE)
- /Users/dominiknowak/code/conductor/src/app.tsx — main app, read the full file
- /Users/dominiknowak/code/conductor/src/layout.ts — responsive layout logic
- /Users/dominiknowak/code/conductor/src/components/tui-utils.ts — colors, sparklines, formatting
- /Users/dominiknowak/code/conductor/src/components/ — all .tsx component files

## Target Crate
crates/conductor-tui/src/

## Architecture

Ratatui uses immediate-mode rendering. There are NO React components, NO hooks,
NO state in the UI layer. The TUI is a pure function:

  fn render(frame: &mut Frame, state: &OrchestraState, ui: &UiState)

Where UiState holds ONLY local UI concerns (focused panel index, scroll position,
help overlay visible, prompt input text). OrchestraState comes from conductor-types.

### File Structure
```
conductor-tui/src/
  lib.rs              — pub mod app, components, theme
  app.rs              — TuiApp struct with run() method (event loop)
  theme.rs            — color constants, status symbols (port tui-utils.ts)
  layout.rs           — responsive layout config (port layout.ts)
  components/
    mod.rs            — pub mod musician, insights, input, panels, status, header
    musician.rs       — render_musician_column(), render_musician_grid()
    insights.rs       — render_insight_panel(), render_task_graph()
    input.rs          — render_prompt_bar(), render_keyboard_help()
    panels.rs         — render_plan_review(), render_session_browser(), render_task_detail()
    status.rs         — render_status_line()
    header.rs         — render_header()
```

## What to Port

### 1. Theme + Utils (theme.rs)
Source: /Users/dominiknowak/code/conductor/src/components/tui-utils.ts

Port these as Rust constants and functions:
- Color palette: C.frame → Color::DarkGray, C.active → Color::Green, etc.
- PHASE map: phase name → (symbol, color)
- STATUS map: musician status → (color, label, dot char)
- TASK_VIZ map: task status → (dot char, color)
- sparkline(values, width) → String using unicode bar chars
- pbar(pct, width) → String using block chars
- waveform(values, width) → String
- elapsed(ms) → formatted string
- tokens(n, estimated) → formatted string
- All Ratatui colors use ratatui::style::Color enum

### 2. Layout (layout.rs)
Source: /Users/dominiknowak/code/conductor/src/layout.ts

Port:
- BREAKPOINTS: NARROW=80, WIDE=160, TALL=40
- LayoutConfig struct
- get_layout_config(width, height) → LayoutConfig

### 3. App Event Loop (app.rs)
This is NEW code (not a direct port — Ink handles this in React).

Create TuiApp struct:
```rust
pub struct TuiApp {
    state_rx: tokio::sync::watch::Receiver<OrchestraState>,
    action_tx: tokio::sync::mpsc::Sender<conductor_types::UserAction>,
}

impl TuiApp {
    pub async fn run(&mut self) -> anyhow::Result<()> {
        // 1. Setup terminal (crossterm::terminal::enable_raw_mode, etc.)
        // 2. Create ratatui::Terminal with CrosstermBackend
        // 3. Main loop:
        //    a. Poll state_rx for new OrchestraState
        //    b. terminal.draw(|f| render_all(f, &state, &ui_state))
        //    c. Poll crossterm events with 50ms timeout
        //    d. Map key events to UserAction, send through action_tx
        //    e. Break on 'q' or Ctrl+C
        // 4. Restore terminal on exit
    }
}
```

### 4. Musician Grid (components/musician.rs)
Source: /Users/dominiknowak/code/conductor/src/components/MusicianColumn.tsx + SplitDashboard.tsx

- render_musician_grid(f, area, musicians: &[MusicianState], focused_idx, layout_config)
  - Split area into N columns using Layout with Constraint::Ratio
  - For each musician, call render_musician_column()
- render_musician_column(f, area, musician: &MusicianState, is_focused)
  - Block with border, title = musician name + status dot
  - Border color: Cyan if focused, DarkGray otherwise
  - Inside: scrollable Paragraph with output lines
  - Output line colors based on content (see getOutputLineColor in tui-utils.ts):
    [USER] → Cyan, > → Gray, ERROR → Red, recent → White, old → DarkGray

### 5. Insight Panel (components/insights.rs)
Source: /Users/dominiknowak/code/conductor/src/components/InsightPanel.tsx + TaskGraph.tsx

- render_insight_panel(f, area, insights: &[Insight])
  - Block with "Insights" title
  - List of insight items with category icon + text
- render_task_graph(f, area, tasks: &[Task])
  - Vertical list of tasks with status dots and dependency arrows
  - Use TASK_VIZ for dot/color

### 6. Input Bar (components/input.rs)
Source: /Users/dominiknowak/code/conductor/src/components/PromptBar.tsx + KeyboardHelpOverlay.tsx

- render_prompt_bar(f, area, input_text: &str, phase: &OrchestraPhase)
  - Shows current input with cursor
  - Phase-dependent prompt label (e.g., "guidance>" during execution)
- render_keyboard_help(f, area)
  - Overlay showing keybindings (Tab, q, ?, Enter, etc.)
  - Render as a centered popup Block over the main content

### 7. Panels (components/panels.rs)
Source: /Users/dominiknowak/code/conductor/src/components/PlanReview.tsx + SessionBrowser.tsx + TaskDetailModal.tsx

- render_plan_review(f, area, plan: &Plan, refinement: &[PlanRefinementMessage])
  - Shows plan summary, task list, and refinement chat
  - Accept/reject/refine controls shown in footer
- render_session_browser(f, area, sessions: &[SessionData])
  - Table of past sessions with ID, phase, task count
- render_task_detail(f, area, task: &Task)
  - Modal overlay with full task info, acceptance criteria, result

### 8. Status + Header (components/status.rs, header.rs)
Source: /Users/dominiknowak/code/conductor/src/components/StatusLine.tsx + NoirHeader.tsx

- render_status_line(f, area, state: &OrchestraState)
  - Bottom bar: phase indicator, elapsed time, token count, cost
- render_header(f, area, state: &OrchestraState)
  - Top bar: "conductor" branding, session ID, musician count

## UiState struct (in app.rs)
```rust
pub struct UiState {
    pub focused_panel: usize,
    pub scroll_offset: u16,
    pub show_help: bool,
    pub show_task_detail: Option<usize>,  // task index
    pub prompt_input: String,
    pub prompt_cursor: usize,
}
```

## Rules
- All data comes from conductor_types::OrchestraState — read only, never mutate
- Use ratatui::{Frame, layout::*, widgets::*, style::*}
- Every render function signature: fn render_xxx(f: &mut Frame, area: Rect, ...)
- No business logic in TUI code — it's purely a visual representation
- Use the Ratatui Sparkline widget for token activity (don't hand-roll)
- Use Gauge widget for progress bars
- Use Paragraph with .scroll((offset, 0)) for scrollable output
TASK
)" \
  --musicians 5 \
  --conductor-model opus \
  --musician-model sonnet \
  --max-turns 30
```

### After Phase 3 — verify:
```bash
cargo check -p conductor-tui
```

---

## Phase 4: Orchestra Core (Conductor — 3 musicians)

This is the hardest phase. Budget for 2 attempts.

### Run command:

```bash
cd /Users/dominiknowak/code/conductor-rust
conductor run \
  --project /Users/dominiknowak/code/conductor-rust \
  --task "$(cat <<'TASK'
Port conductor's core orchestration logic from TypeScript to Rust.

## TypeScript Source (READ ALL OF THESE CAREFULLY)
- /Users/dominiknowak/code/conductor/src/musician.ts (465 lines) — read fully
- /Users/dominiknowak/code/conductor/src/conductor.ts (1250 lines) — read fully
- /Users/dominiknowak/code/conductor/src/orchestra.ts (2055 lines) — read fully
- /Users/dominiknowak/code/conductor/src/worktree-manager.ts (359 lines) — read fully
- /Users/dominiknowak/code/conductor/src/insights.ts (288 lines) — read fully
- /Users/dominiknowak/code/conductor/src/caffeinate.ts (62 lines) — read fully

## Target Crate
crates/conductor-core/src/ — add these new modules to lib.rs:
  pub mod orchestra;
  pub mod musician;
  pub mod conductor_agent;  // "conductor" is a reserved-ish name, use conductor_agent
  pub mod worktree;
  pub mod insights;
  pub mod caffeinate;

## Architecture Decisions (MANDATORY — do not deviate)

### Channels, NOT EventEmitter
The TypeScript code uses EventEmitter for everything. In Rust, use channels:

1. tokio::sync::watch for state broadcasting:
   - Orchestra OWNS the OrchestraState
   - Orchestra creates: let (state_tx, state_rx) = watch::channel(initial_state)
   - After any state mutation, call: state_tx.send(state.clone())
   - TUI receives state_rx (passed in from main.rs)

2. tokio::sync::mpsc for musician → orchestra events:
   - Type: conductor_types::OrchestraEvent
   - Orchestra creates ONE channel: let (event_tx, event_rx) = mpsc::channel(256)
   - Each Musician gets event_tx.clone()
   - Orchestra's main loop: event_rx.recv().await

3. tokio::sync::mpsc for user → orchestra actions:
   - Type: conductor_types::UserAction
   - TUI sends keyboard events through action_tx
   - Orchestra's main loop also selects on action_rx

### Orchestra Main Loop Pattern
```rust
loop {
    tokio::select! {
        // Musician event
        Some(event) = event_rx.recv() => {
            self.handle_event(event);
            state_tx.send(self.state.clone()).ok();
        }
        // User action
        Some(action) = action_rx.recv() => {
            self.handle_action(action);
            state_tx.send(self.state.clone()).ok();
        }
        // Periodic tick (rate limit probing, elapsed time updates)
        _ = tick_interval.tick() => {
            self.tick();
            state_tx.send(self.state.clone()).ok();
        }
    }
    if self.state.phase == OrchestraPhase::Complete
       || self.state.phase == OrchestraPhase::Failed {
        break;
    }
}
```

### Musician Execution
```rust
impl Musician {
    pub async fn execute(
        &mut self,
        task: Task,
        worktree_path: String,
        branch: String,
        event_tx: mpsc::Sender<OrchestraEvent>,
    ) -> TaskResult {
        // 1. Create ClaudeSession (from conductor-bridge)
        // 2. Create internal channel for session events
        // 3. Start session with prompt
        // 4. Loop: receive events from session, forward to orchestra via event_tx
        // 5. Return TaskResult on session close
    }
}
```

### Orchestra Does NOT Own Musicians Directly During Execution
When a musician starts executing, spawn it on a tokio task:
```rust
let event_tx = self.event_tx.clone();
let musician = self.musicians[idx].clone();  // or take ownership
tokio::spawn(async move {
    let result = musician.execute(task, path, branch, event_tx).await;
    // Result is sent back through event_tx as MusicianComplete
});
```

## What to Port

### 1. Musician (musician.rs)
Source: /Users/dominiknowak/code/conductor/src/musician.ts
- Musician struct with state, model, max_turns
- execute() method — creates ClaudeSession, processes events, returns TaskResult
- Output buffering (200 lines max, 100KB max)
- Token estimation during streaming (use conductor-core token_estimate module)
- Tool use tracking and file modification detection
- Checkpoint detection (git commit in Bash tool calls)
- injectPrompt() for mid-execution user messages

### 2. Conductor Agent (conductor_agent.rs)
Source: /Users/dominiknowak/code/conductor/src/conductor.ts
- Conductor struct — the LLM agent that plans and reviews
- Prompt construction: EXPLORATION_PROMPT, DECOMPOSITION_PROMPT, ANALYST_PROMPT, etc.
- loadProjectInstructions() — read CLAUDE.md with intelligent truncation
- extractJsonBlock() — find JSON in LLM markdown output
- sanitizeJson() — repair broken JSON from LLM output (use serde_json with fallback)
- plan() method — send planning prompt, parse response into Plan
- reviewPhase() method — send review prompt, parse PhaseReviewResult
- processGuidance() — parse user guidance into GuidanceActions

### 3. Orchestra (orchestra.rs)
Source: /Users/dominiknowak/code/conductor/src/orchestra.ts
This is the biggest piece. Port the state machine with all phase transitions:
- new() → initialize state, create channels
- run() → the main select! loop (see pattern above)
- Phase transitions: Init → Exploring → Analyzing → Decomposing → PlanReview →
  PhaseDetailing → PhaseExecuting → PhaseMerging → PhaseReviewing → (next phase or FinalReview) → Complete
- Task scheduling: find ready tasks, assign to idle musicians, respect dependencies
- Rate limit handling: pause musicians, start probing, resume
- Guidance processing: receive user messages, pass to Conductor agent
- Plan refinement: chat-style back-and-forth during PlanReview
- shutdown() — kill all musician sessions, cleanup worktrees

### 4. Worktree Manager (worktree.rs)
Source: /Users/dominiknowak/code/conductor/src/worktree-manager.ts
- WorktreeManager struct with project_path
- create_worktree(worker_id, branch) → path
- remove_worktree(worker_id)
- merge_branch(branch, into)
- cleanup_all()
- All git operations via tokio::process::Command("git", [...])

### 5. Insights (insights.rs)
Source: /Users/dominiknowak/code/conductor/src/insights.ts
- InsightGenerator struct with rules
- Pattern-matching on tool use events to generate educational insights
- No LLM calls — purely rule-based

### 6. Caffeinate (caffeinate.rs)
Source: /Users/dominiknowak/code/conductor/src/caffeinate.ts
- Caffeinate struct — spawns macOS `caffeinate` process
- start(), stop(), is_active()
- Platform check: only run on macOS (cfg!(target_os = "macos"))
- Implement Drop to ensure cleanup

## Rules
- Do NOT use Arc<Mutex<>> for OrchestraState. Orchestra is the single owner.
- Use conductor_types for ALL shared types
- Use conductor_bridge::ClaudeSession for all Claude interactions
- Use the modules from Phase 1 (dag, rate_limiter, token_estimate, etc.)
- Use thiserror for OrchestraError enum
- Use tracing for logging
- Port tests from conductor.test.ts and orchestra.test.ts
TASK
)" \
  --musicians 3 \
  --conductor-model opus \
  --musician-model opus \
  --max-turns 50
```

### After Phase 4 — verify:
```bash
cargo check -p conductor-core
cargo test -p conductor-core
```

**If it fails**: This is expected. Run a Claude Code fix session:

```
cd /Users/dominiknowak/code/conductor-rust

Fix all cargo check errors in conductor-core. Run `cargo check -p conductor-core`
and fix each error. Common issues:
- Borrow checker: clone values before moving into async blocks
- Missing imports: add use statements
- Type mismatches: check conductor-types for correct types
- Lifetime errors in select!: clone receivers/state before the macro

Keep running cargo check until it passes with zero errors.
```

---

## Phase 5: CLI Entry Point + Wiring (Conductor — 2 musicians)

### Run command:

```bash
cd /Users/dominiknowak/code/conductor-rust
conductor run \
  --project /Users/dominiknowak/code/conductor-rust \
  --task "$(cat <<'TASK'
Wire conductor-rust into a working binary with CLI and TUI.

## TypeScript Source
/Users/dominiknowak/code/conductor/src/index.tsx — read the full file for CLI structure.

## Target
crates/conductor-cli/src/main.rs

## What to Build

### 1. CLI Argument Parsing (using clap derive)
Mirror the TypeScript yargs config. The CLI has these subcommands:

conductor (default)     → interactive TUI mode
conductor run           → headless mode with task
conductor resume        → resume a paused session
conductor list          → list all sessions
conductor status        → show session status
conductor clean         → remove session data

```rust
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "conductor", about = "Multi-agent coding orchestrator")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    Run {
        #[arg(short, long)]
        project: String,
        #[arg(short, long)]
        task: String,
        #[arg(short, long, default_value_t = 3)]
        musicians: usize,
        #[arg(long, default_value = "opus")]
        conductor_model: String,
        #[arg(long, default_value = "sonnet")]
        musician_model: String,
        #[arg(long, default_value_t = 30)]
        max_turns: u32,
        #[arg(long, default_value_t = false)]
        dry_run: bool,
        #[arg(long, name = "ref")]
        reference: Option<String>,
    },
    Resume {
        #[arg(short, long)]
        session: String,
    },
    List,
    Status {
        #[arg(short, long)]
        session: String,
    },
    Clean {
        #[arg(short, long)]
        session: Option<String>,
        #[arg(long, default_value_t = false)]
        all: bool,
        #[arg(long)]
        older_than: Option<u64>,
        #[arg(long)]
        keep: Option<usize>,
    },
}
```

### 2. Main Function — Wire Everything Together

```rust
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // 1. Init tracing
    tracing_subscriber::init();

    // 2. Parse CLI
    let cli = Cli::parse();

    // 3. Match command
    match cli.command {
        None => run_interactive().await,
        Some(Commands::Run { .. }) => run_headless(config).await,
        Some(Commands::List) => list_sessions().await,
        // etc
    }
}
```

### 3. run_interactive() — The Full Stack Wiring

This is the key function that connects everything:

```rust
async fn run_interactive() -> anyhow::Result<()> {
    // a. Validate Claude CLI
    conductor_bridge::validate_cli()?;

    // b. Create channels
    let initial_state = OrchestraState::new(config);
    let (state_tx, state_rx) = tokio::sync::watch::channel(initial_state);
    let (event_tx, event_rx) = tokio::sync::mpsc::channel::<OrchestraEvent>(256);
    let (action_tx, action_rx) = tokio::sync::mpsc::channel::<UserAction>(64);

    // c. Start caffeinate
    let caffeinate = conductor_core::Caffeinate::new();
    caffeinate.start();

    // d. Spawn orchestra on a background task
    let orchestra_handle = tokio::spawn(async move {
        let mut orchestra = Orchestra::new(config, state_tx, event_tx, event_rx, action_rx);
        orchestra.run().await
    });

    // e. Run TUI on main thread (blocks until quit)
    let mut tui = conductor_tui::TuiApp::new(state_rx, action_tx);
    tui.run().await?;

    // f. Cleanup
    caffeinate.stop();
    orchestra_handle.abort();
    Ok(())
}
```

### 4. run_headless() — Simplified (no TUI)

Similar to run_interactive but without TUI:
- Orchestra runs directly
- Print progress to stdout using tracing
- Auto-approve plans (no user interaction)

### 5. list_sessions(), status(), clean() — Simple CLI commands

These just read from ~/.conductor/sessions/ and print to stdout.
Use conductor_core::TaskStore for all session operations.

## Rules
- Use anyhow::Result for all error handling in this crate
- All async operations use tokio
- Signal handling: tokio::signal::ctrl_c() for graceful shutdown
- The binary name is "conductor" (set in Cargo.toml [[bin]])
TASK
)" \
  --musicians 2 \
  --conductor-model opus \
  --musician-model sonnet \
  --max-turns 30
```

### After Phase 5 — verify:
```bash
cargo build --workspace
cargo build --release
./target/release/conductor --help
```

---

## Phase 6: Fix + Polish (Claude Code — single session)

If there are compilation errors after all phases, run a single Claude Code session:

### Prompt:

```
cd /Users/dominiknowak/code/conductor-rust

Run `cargo build --workspace 2>&1` and fix ALL errors. Then run `cargo test --workspace`
and fix any test failures.

For each error:
1. Read the error message carefully
2. Understand what Rust is complaining about
3. Fix it with the minimum change needed

Common patterns to fix:
- "moved value" → add .clone() before the move
- "borrowed value does not live long enough" → clone before entering async block
- "mismatched types" → check conductor-types for correct type
- "cannot find" → add missing use/mod statements
- "doesn't implement trait" → add missing derive macros

After all errors are fixed, run:
  cargo build --release
  ./target/release/conductor --help

Confirm the binary works.
```

---

## Quick Reference: Run Order

```
1. Install Rust           → curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
2. git init + commit      → cd conductor-rust && git init && git add -A && git commit -m "init"
3. Phase 0 (types)        → Claude Code session in conductor-rust/
4. cargo check            → verify skeleton compiles
5. Phase 1 (pure logic)   → conductor run ... (4 musicians)
6. cargo check + test     → verify
7. Phase 2 (bridge)       → conductor run ... (2 musicians)
8. cargo check + test     → verify
9. Phase 3 (TUI)          → conductor run ... (5 musicians)
10. cargo check           → verify
11. Phase 4 (orchestra)   → conductor run ... (3 musicians, opus)
12. cargo check           → verify (expect errors — run Phase 6 fix)
13. Phase 5 (CLI wiring)  → conductor run ... (2 musicians)
14. cargo build --release → verify
15. Phase 6 (fix/polish)  → Claude Code session if needed
16. Test manually          → ./target/release/conductor --help
```
