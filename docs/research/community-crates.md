# Ratatui Community Crates Survey — Conductor TUI

> **Research date:** 2026-03-22
> **Target ratatui version:** 0.30 (as specified in workspace `Cargo.toml`)
> **Scope:** Evaluate community widgets to replace/enhance hand-rolled code in `conductor-tui`

---

## TL;DR / Priority Picks

| Priority | Crate | Replaces | Compatibility |
|----------|-------|----------|---------------|
| ★★★ | `ansi-to-tui` | Raw ANSI in musician output panels | ✅ ratatui 0.30 (official org) |
| ★★★ | `tui-textarea` | `app.rs:209-287` hand-rolled input | ✅ ratatui >=0.23,<1 covers 0.30 |
| ★★ | `tui-popup` | `centered_rect + Clear` pattern | ✅ ratatui 0.30 (via tui-widgets) |
| ★★ | `tui-widget-list` | Session browser / task list | ✅ ratatui 0.30 compatible |
| ★★ | `tachyonfx` | No current code — adds polish | ✅ ratatui 0.30 (ratatui org) |
| ★ | `tui-big-text` | Logo/branding in header | ✅ ratatui 0.30 (via tui-widgets) |
| ★ | `tui-scrollview` | Manual scroll_offset in panels | ✅ ratatui 0.30 (via tui-widgets) |
| ★ | `tui-tree-widget` | Task dependency graph | ✅ ratatui 0.30 (v0.24.0, Feb 2026) |
| ℹ️ | `tui-input` | Simpler single-line alt to tui-textarea | ✅ Active, lighter than tui-textarea |
| ℹ️ | `tui-prompts` | Plan refinement dialog | ✅ Active, in tui-widgets |
| ⚠️ | `tui-logger` | Conductor internal log view | ⚠️ Pinned to ratatui 0.29 — verify before use |
| ⚠️ | `ratatui-image` | Image display in task context | ⚠️ Complex protocol requirements |
| ℹ️ | `tui-term` | Musician terminal emulator | ⚠️ Experimental / architectural mismatch |

---

## ratatui 0.30 Breaking Changes (affects community crate compat)

These are the breaking changes in 0.30 most relevant to third-party widget crates:

1. **`Alignment` → `HorizontalAlignment`**: A type alias is provided for backward
   compat, so old code still compiles but emits deprecation warnings. Low risk for
   widget consumers.

2. **`WidgetRef` no longer blanket-implements `Widget`**: Crates implementing custom
   widgets via `WidgetRef` must update their trait bounds. Symptom on 0.30: compile
   error `the trait Widget is not satisfied for &MyWidget`. Check a crate's GitHub
   issues for "0.30 compatibility" if you see this.

3. **Custom `Backend` trait requires `Error` associated type + `clear_region()`**:
   Only relevant to backend crates, not widget-only crates like those surveyed here.

4. **`ratatui` split into workspace sub-crates**: Community crates pinning
   `ratatui = "0.29"` will conflict at the semver level. Crates using `>=0.23, <1`
   or `>=0.28, <1` are forward-compatible through 0.30.

---

## Detailed Crate Evaluations

### 1. `tui-textarea`

**What it provides:** A full multi-line text editor widget with cursor movement,
text selection, undo/redo (50-step history), word-jump (Alt+Arrow), cut/copy/paste,
bracket matching, search, line numbers, syntax highlighting hooks, and mouse support.
Renders as a ratatui `Widget`. Single-line mode via `set_max_lines(1)`.

| Field | Value |
|-------|-------|
| Crate | `tui-textarea` |
| Latest version | 0.7.0 |
| Author | rhysd (Hiroshi Shinji) |
| GitHub | github.com/rhysd/tui-textarea |
| Stars | ~1,800+ |
| Last updated | Active (0.7.0 released 2024/2025) |
| License | MIT |

**Ratatui 0.30 compatibility:**
> ✅ **Compatible.** `tui-textarea` 0.7.0 specifies `ratatui = ">=0.23.0, <1"` — this
> range explicitly covers 0.30. The `WidgetRef` change in 0.30 is handled internally.
> No fork required.

**Conductor-TUI replacement target:**

**`app.rs:209-287`** — The 80-line hand-rolled prompt input handler. This block
manually tracks `prompt_input: String` and `prompt_cursor: usize`, implementing
backspace, word-delete (Alt+Backspace), left/right, word-jump (Alt+Arrow), and
character insertion. It has a real correctness bug: the word-jump and word-delete
logic uses `ui.prompt_input.as_bytes()[cursor - 1] == b' '` (byte indexing) which
is incorrect for multi-byte UTF-8 characters (e.g. emoji, accented letters) — the
cursor will corrupt such text.

**`src/components/input.rs:21-49`** — `render_prompt_bar()` would be replaced by
rendering the `TextArea` widget directly; cursor positioning is handled automatically
by the widget.

Replacement sketch:

```rust
// UiState:
pub prompt: TextArea<'static>,

// UiState::new():
let mut prompt = TextArea::default();
prompt.set_max_lines(1);
prompt.set_style(Style::default().fg(C_TEXT));
prompt.set_cursor_line_style(Style::default()); // no line highlight in single-line
prompt.set_block(
    Block::default()
        .borders(Borders::NONE)
        .title(Span::styled(" task> ", Style::default().fg(C_BRAND))),
);

// handle_key() replaces all of app.rs:209-287:
if !is_navigation_key(&key) {
    ui.prompt.input(key); // handles all editing including UTF-8-safe word ops
    return Ok(false);
}

// On Enter:
let text = ui.prompt.lines()[0].clone();
ui.prompt = TextArea::default(); // clear

// input.rs render_prompt_bar() replaced by:
f.render_widget(&ui.prompt, area); // widget handles cursor positioning
```

**Integration complexity:** Medium. Requires threading `TextArea` through the event
handler — compatible with current `ui: &mut UiState` parameter. The `submit_input()`
path is unchanged, just reading `ui.prompt.lines()[0]` instead of `ui.prompt_input`.

**Verdict:** Highly recommended. Eliminates ~80 lines of brittle cursor code (including
a real UTF-8 bug), adds undo/redo for free. The `>=0.23,<1` range means it works today
with ratatui 0.30 — no fork needed.

---

### 2. `tui-scrollview`

**What it provides:** A `ScrollView` widget that wraps arbitrary content in a
scrollable viewport with automatic scrollbar rendering. Content is drawn to an
off-screen buffer first, then a window of it is rendered to the actual frame. The
`ScrollViewState` tracks scroll position.

| Field | Value |
|-------|-------|
| Crate | `tui-scrollview` |
| Latest version | 0.6.2 |
| Author | joshka (Josh McKinney) |
| GitHub | github.com/ratatui/tui-widgets (consolidated) |
| Downloads | ~27,000/month, 28 dependent crates |
| Last updated | Active in tui-widgets monorepo |
| License | MIT OR Apache-2.0 |

**Ratatui 0.30 compatibility:**
> ✅ Maintained by joshka, a core ratatui contributor, in the `tui-widgets` monorepo.
> The consolidation into the ratatui org ensures ratatui version tracking stays current.

**Conductor-TUI replacement target:**

- **`app.rs:34`** — `UiState.scroll_offset: u16` — a bare counter shared across
  panels; different panels fighting over one offset when both are visible.
- **`app.rs:199-205`, `app.rs:325-334`** — Up/Down key handling increments/decrements
  the global `scroll_offset` without bounds checking.
- **`panels.rs:359-362`** — `render_task_detail()` uses `Paragraph::new(lines).scroll((scroll_offset, 0))` with no visual scrollbar feedback.
- **`musician.rs`** — Musician output uses the same pattern: `Paragraph.scroll((scroll_offset, 0))` with no indicator showing whether more content exists below.

Problems with the current approach:
- No visual scrollbar — users cannot tell if content extends below the viewport.
- Single global `scroll_offset` means the task detail modal and musician panels share
  one counter, causing conflicts.
- No bounds check — offset can silently exceed content length.

Replacement sketch:

```rust
// UiState — per-panel independent scroll state:
pub musician_scroll: ScrollViewState,
pub task_detail_scroll: ScrollViewState,

// In render_musician_column:
let mut scroll_view = ScrollView::new(Size::new(area.width, total_lines));
scroll_view.render_widget(
    Paragraph::new(lines).wrap(Wrap { trim: false }),
    Rect::new(0, 0, area.width, total_lines),
);
f.render_stateful_widget(scroll_view, area, &mut ui.musician_scroll);
// Scrollbar rendered automatically.

// Keyboard:
KeyCode::Up => ui.musician_scroll.scroll_up(),
KeyCode::Down => ui.musician_scroll.scroll_down(),
```

**Integration complexity:** Medium. `ScrollView` requires content height to be known
upfront to size the virtual buffer — musician output line count is already computed.
The primary work is splitting `scroll_offset` into per-panel states and updating
two render sites.

**Verdict:** Worth adopting for the task detail modal and musician panels to gain
visible scrollbars and per-panel independent scroll. The task detail modal
(`panels.rs:359-363`) is the smallest and cleanest first integration point.

---

### 3. `tui-big-text`

**What it provides:** Renders large pixel text using glyphs from the `font8x8` crate.
Each character becomes an 8×8 grid of terminal cells, producing oversized "banner" text.
Six pixel density levels from `Full` (8×8 cells per char) down to `Sextant` (4×3 cells).
Supports color, alignment, and composition with other widgets.

| Field | Value |
|-------|-------|
| Crate | `tui-big-text` |
| Author | joshka |
| GitHub | github.com/ratatui/tui-widgets (consolidated) |
| Last updated | Active in tui-widgets |
| License | MIT OR Apache-2.0 |

**Ratatui 0.30 compatibility:**
> ✅ Maintained by a core ratatui contributor in the tui-widgets monorepo.

**Conductor-TUI replacement/enhancement target:**

- **`components/header.rs`** — Currently the header uses a single-line `Paragraph`
  for the "Conductor" title text. `BigText` could render a stylized banner when the
  terminal is wide enough (≥80 cols) and phase is `Init`.
- **Splash screen** — During `OrchestraPhase::Init` before any musicians have spawned
  (`state.musicians.is_empty()` in `app.rs:496-498`), a big "CONDUCTOR" banner fills
  the content area:

  ```rust
  // In render_musicians() when musicians.is_empty():
  let big = BigText::builder()
      .pixel_size(PixelSize::HalfHeight) // 4 rows tall per char — fits in content area
      .style(Style::default().fg(C_BRAND))
      .lines(vec![Line::from("CONDUCTOR")])
      .build()
      .unwrap();
  let centered = centered_rect(80, 40, area);
  f.render_widget(big, centered);
  ```

**Integration complexity:** Low. Standalone widget; add to `Cargo.toml` and call
`render_widget`. The splash logic is 10–15 lines in `render_content()`.

**Verdict:** Low-priority but zero-effort win for visual polish on the idle screen.
`PixelSize::HalfHeight` is the right default — each char is 4 rows tall, "CONDUCTOR"
(9 chars) fits comfortably in terminals ≥90 columns wide.

---

### 4. `tui-tree-widget`

**What it provides:** An interactive tree view widget with collapsible/expandable
nodes, keyboard navigation (arrow keys, space to toggle open/close), and `TreeState`
for tracking open/selected nodes. Items are `TreeItem<Identifier>` structs with
arbitrary child nesting. Supports custom styled labels per node.

| Field | Value |
|-------|-------|
| Crate | `tui-tree-widget` |
| Version | **0.24.0** (released February 2026) |
| Author | EdJoPaTo |
| GitHub | github.com/EdJoPaTo/tui-rs-tree-widget |
| Last updated | Very recent — v0.24.0 is ~1 month old |
| License | MIT |

**Ratatui 0.30 compatibility:**
> ✅ HIGH CONFIDENCE. v0.24.0 was released February 2026 — almost certainly tracks
> ratatui 0.30. Verify by checking the repo `Cargo.toml` for the ratatui dep range.

**Conductor-TUI replacement/enhancement target:**

- **`insights::render_task_graph()`** (called from `app.rs:508`) — Currently renders a
  flat ASCII task list with `←dep_idx` text arrows. This is unreadable for 6+ tasks
  with multiple dependencies:

  ```rust
  // CURRENT — flat list, not a real graph:
  // T3: ● Build backend ←T1 ←T2
  // T5: ● Integration tests ←T1 ←T2 ←T3 ←T4
  ```

- **`panels.rs:103-151`** — The task list in `render_plan_review()` could show
  dependencies as nested children.

Replacement sketch:

```rust
// Build tree from tasks grouped by dependency depth:
fn build_task_tree(tasks: &[Task]) -> Vec<TreeItem<'static, usize>> {
    // Root = tasks with no dependencies
    tasks.iter()
        .filter(|t| t.dependencies.is_empty())
        .map(|root| build_subtree(root, tasks))
        .collect()
}

fn build_subtree(task: &Task, all_tasks: &[Task]) -> TreeItem<'static, usize> {
    let viz = theme::task_viz(&task.status);
    let label = Line::from(vec![
        Span::styled(format!("{} T{}: ", viz.dot, task.index + 1), Style::default().fg(viz.color)),
        Span::styled(task.title.clone(), Style::default().fg(C_TEXT)),
    ]);
    let children: Vec<TreeItem<usize>> = all_tasks.iter()
        .filter(|t| t.dependencies.contains(&task.index))
        .map(|t| build_subtree(t, all_tasks))
        .collect();
    TreeItem::new(task.index, label, children).unwrap()
}

// UiState gains:
pub task_tree_state: TreeState<usize>,

// Render:
let tree = Tree::new(&items)
    .block(Block::default().borders(Borders::ALL).title("Task DAG"))
    .highlight_style(Style::default().fg(C_BRAND).add_modifier(Modifier::BOLD));
f.render_stateful_widget(tree, area, &mut ui.task_tree_state);
```

**Limitation:** `tui-tree-widget` renders trees, not general DAGs. Tasks with multiple
parents (dependencies) can only appear once in the tree. For the conductor, tasks are
typically organized in waves (dependency-depth levels), making tree representation
largely correct — but diamond-dependency patterns (task depending on two separate
chains) will appear only in one branch.

**Integration complexity:** Medium. Tree construction from dependency indices requires
a topological depth-assignment + parent-selection algorithm (~50–80 lines). The widget
integration itself is straightforward once tree items are built.

**Verdict:** Good fit for task dependency visualization. The current flat `render_task_graph`
is a known UX gap; a navigable tree is a significant improvement for plans with 6+ tasks.

---

### 5. `tui-logger`

**What it provides:** An in-TUI log viewer that integrates with the standard `log`
crate facade (and optionally `tracing` via `tracing-log`). Log records are captured
into a ring buffer and rendered as a scrollable `TuiLoggerWidget`. Supports per-target
log level filtering, search, and color-coded severity. `TuiLoggerSmartWidget` adds a
split view: filter selector on the left, scrollable log on the right.

| Field | Value |
|-------|-------|
| Crate | `tui-logger` |
| Version | 0.17.2 (latest as of research date; 0.18.x may exist — check crates.io) |
| Stars | ~293 |
| Author | gin66 |
| GitHub | github.com/gin66/tui-logger |
| Last updated | Active (2025–2026) |
| License | MIT |

**Ratatui 0.30 compatibility:**
> ⚠️ **VERIFY BEFORE USE.** The `Cargo.toml` currently pins `ratatui = "0.29"`. This is a
> semver-breaking boundary — conductor-tui's `ratatui = "0.30"` will conflict at compile
> time. Check GitHub issues/PRs for a pending 0.30 update. The maintainer historically
> updates within 1–2 weeks of ratatui releases. If needed, `tui-logger-forked` exists
> on crates.io as a fallback.

**Conductor-TUI enhancement target:**

The conductor's internal `tracing` events (orchestra phase transitions, musician spawn,
token budget checks, bridge errors) are currently invisible in the TUI — they either
go to stderr (hidden in alternate screen mode) or are lost entirely. A dedicated log
panel would surface:

- `tracing::info!("Phase transition: {:?} → {:?}", old, new)`
- `tracing::warn!("Musician {} exceeded token budget", idx)`
- `tracing::error!("Bridge parse error: {}", e)`

```rust
// main.rs setup:
tui_logger::init_logger(log::LevelFilter::Info).unwrap();
tui_logger::set_default_level(log::LevelFilter::Info);
// With tracing bridge:
use tracing_log::LogTracer;
LogTracer::init().unwrap();

// New UiState field:
pub logger_state: TuiWidgetState,

// In a new "Log" tab or insights panel section:
let log_widget = TuiLoggerSmartWidget::default()
    .style_error(Style::default().fg(C_ERROR))
    .style_warn(Style::default().fg(Color::Yellow))
    .style_info(Style::default().fg(C_TEXT))
    .output_timestamp(Some("%H:%M:%S".to_string()))
    .output_level(Some(TuiLoggerLevelOutput::Abbreviated))
    .state(&mut ui.logger_state);
f.render_widget(log_widget, log_area);
```

**Integration complexity:** Medium-high. Requires registering `tui-logger` as the
global log subscriber, which may interact with the existing `tracing-subscriber` setup.
The `log`↔`tracing` bridge (`tracing-log`) crate handles forwarding.

**Verdict:** Valuable for debugging orchestra state — not recommended for the primary
musician output panels (which need ANSI passthrough, not structured log levels). Hold
until ratatui 0.30 compat is confirmed.

---

### 6. `ratatui-image`

**What it provides:** Inline image rendering for ratatui, supporting Sixels, Kitty
Graphics Protocol, iTerm2 protocol, and unicode half-block fallback. Auto-detects
terminal capabilities. Handles font-size querying, pixel-to-cell mapping, and
protocol-specific rendering.

| Field | Value |
|-------|-------|
| Crate | `ratatui-image` |
| Author | benjajaja + ratatui org |
| GitHub | github.com/ratatui/ratatui-image |
| Last updated | Active (2025, under ratatui org) |
| License | MIT |

**Ratatui 0.30 compatibility:**
> ✅ Co-maintained by the ratatui org; expected to track current ratatui versions.

**Conductor-TUI potential use:**

- **`app.rs:357`** — `extract_image_paths()` already handles image path extraction from
  user input; `UserAction::SubmitTask`/`RefinePlan` accept `images: Option<Vec<PathBuf>>`.
  A future enhancement could show image thumbnails in the task detail modal.
- **Task result display** — If musicians produce image outputs, render thumbnails inline
  in output panels.

**Integration complexity:** High. Terminal support varies significantly: Kitty protocol
in Kitty/WezTerm, Sixels in mlterm/xterm, half-blocks everywhere. Requires async setup
to query terminal capabilities before first render. Adds an image-processing dependency
chain.

**Verdict:** Low immediate priority — conductor is a code-orchestration tool, not an
image viewer. The half-block fallback ensures graceful degradation, but the async setup
complexity is not justified for the current feature set.

---

### 7. `tui-popup`

**What it provides:** A `Popup` widget that centers arbitrary content over the existing
UI with automatic `Clear` rendering, configurable border styling, and optional
`PopupState` for drag-to-reposition behavior. Composes with any widget via `KnownSize`.

| Field | Value |
|-------|-------|
| Crate | `tui-popup` |
| Version | 0.7.0 |
| Author | joshka (now in tui-widgets) |
| GitHub | github.com/ratatui/tui-widgets |
| Last updated | Active in tui-widgets |
| License | MIT OR Apache-2.0 |

**Ratatui 0.30 compatibility:**
> ✅ Maintained by joshka in the tui-widgets monorepo; tracks current ratatui.

**Conductor-TUI replacement target:**

The `centered_rect()` + `clear_area()` helpers in `app.rs:532-555` (19-line math) are
a hand-rolled reimplementation of exactly what `tui-popup` provides. They are called
from three overlay sites:

- **`panels.rs:175-176`** — `render_session_browser`: `centered_rect(70, 70, area)` + `clear_area(f, popup)`.
- **`panels.rs:240-247`** — `render_task_detail`: `centered_rect(70, 80, area)` + `clear_area(f, popup)`.
- **`input.rs:53-55`** — `render_keyboard_help`: `centered_rect(60, 60, area)` + `f.render_widget(Clear, popup)`.

All three reduce to a standard popup pattern:

```rust
// After replacement — render_task_detail sketch:
let popup = Popup::new(task_detail_content_widget) // content implements Widget + KnownSize
    .title(Span::styled(" Task Detail ", Style::default().fg(C_BRAND)))
    .border_style(Style::default().fg(C_BRAND));
f.render_stateful_widget(popup, area, &mut state);
// No manual centered_rect(), no manual Clear.
```

**Integration complexity:** Low. The `KnownSizeWrapper` handles widgets that don't
implement `KnownSize` natively. For existing content (rendered into an inner `Rect`),
wrapping in a newtype that returns a fixed size is 5–10 lines per overlay.

**Verdict:** Strongly recommended. Eliminates the duplicated `centered_rect` helper,
standardizes all three overlay call-sites, and adds draggable popup support for free.

---

### 8. `tui-input`

**What it provides:** A lightweight, single-line text input state struct for ratatui.
Tracks cursor position and input string. Headless — rendering is left to the caller via
`.value()` and `.cursor()`. Designed for inline form inputs. Supports crossterm, termion,
and termwiz backends via feature flags.

| Field | Value |
|-------|-------|
| Crate | `tui-input` |
| Author | sayanarijit |
| GitHub | github.com/sayanarijit/tui-input |
| Last updated | Active (2025, updated ~10 days before research date) |
| License | MIT |

**Ratatui 0.30 compatibility:**
> ✅ Crossterm-based event handling doesn't depend on the ratatui version.
> Actively updated — verify by checking crates.io for current version's ratatui dep.

**Conductor-TUI replacement target:**

Same as `tui-textarea` (`app.rs:209-287`) but lighter:

```rust
// UiState:
pub prompt: tui_input::Input,

// handle_key() replaces app.rs:209-287:
use tui_input::backend::crossterm::EventHandler;
ui.prompt.handle_event(&Event::Key(key));

// input.rs render_prompt_bar() — stays mostly the same, just reads state:
let prompt = Paragraph::new(Line::from(vec![
    Span::styled(format!(" {prefix}> "), Style::default().fg(C_BRAND)),
    Span::styled(ui.prompt.value(), Style::default().fg(C_TEXT)),
]));
f.render_widget(prompt, area);
// Manual cursor position (same as current but reading prompt.cursor()):
let cursor_x = area.x + (prefix.len() + 3) as u16 + ui.prompt.cursor() as u16;
f.set_cursor_position((cursor_x, area.y));
```

**vs tui-textarea:**
- `tui-input` — headless, simpler, preserves full rendering control. No undo/redo,
  no multi-line. Best if the exact current prompt bar appearance must be preserved.
- `tui-textarea` — opinionated rendering, adds undo/redo and multi-line. Requires
  replacing the render function too.

**Integration complexity:** Very low. Near-drop-in for `UiState.prompt_input` +
`prompt_cursor`. The `handle_event()` API covers all the key cases in `app.rs:209-287`
correctly, including UTF-8-safe cursor handling.

**Verdict:** Good lighter alternative to `tui-textarea` for the conductor prompt
specifically — single-line, undo/redo not needed, and the headless API keeps the
existing prompt bar rendering.

---

### 9. `tui-prompts`

**What it provides:** Higher-level dialog prompts built on top of ratatui: `TextPrompt`,
`PasswordPrompt`, `SelectPrompt`, and composable multi-step dialog flows. Part of the
joshka `tui-widgets` ecosystem.

| Field | Value |
|-------|-------|
| Crate | `tui-prompts` |
| Version | 0.4.x (check tui-widgets for latest) |
| Author | joshka (in tui-widgets) |
| GitHub | github.com/ratatui/tui-widgets |
| Last updated | Active |
| License | MIT OR Apache-2.0 |

**Ratatui 0.30 compatibility:**
> ✅ Maintained in tui-widgets by a core ratatui contributor.

**Conductor-TUI potential use:**

- **Plan refinement input** — During `PlanReview` phase, a `TextPrompt` could provide
  a styled inline dialog.
- **Initial task submission** — The `Init` phase prompt bar.

**Integration complexity:** Low-Medium.

**Verdict:** Partially overlaps with `tui-input` and `tui-textarea`. The conductor
prompt bar has a custom phase-aware prefix (`task>`, `refine>`, `guidance>`) and a
specific bottom-bar placement that `tui-prompts` doesn't accommodate cleanly.
`tui-input` or `tui-textarea` are better fits. `tui-prompts` would be appropriate
if the conductor added explicit modal confirmation dialogs (e.g., "Approve plan? [y/N]").

---

### 10. `tui-term`

**What it provides:** A pseudoterminal (PTY) widget for ratatui that renders the output
of a real subprocess inside a ratatui frame. Uses `vt100` for terminal emulation.
Supports spawning child processes inside the widget.

| Field | Value |
|-------|-------|
| Crate | `tui-term` |
| Author | a-kenji |
| GitHub | github.com/a-kenji/tui-term |
| Last updated | Active but described as "active development / work in progress" |
| License | MIT OR Apache-2.0 |

**Ratatui 0.30 compatibility:**
> ⚠️ Experimental status. Verify ratatui version alignment manually before adopting.

**Conductor-TUI potential use:**

Musician panels display Claude CLI output via `MusicianState.output_lines` populated
by the bridge. Claude CLI produces ANSI-rich terminal output. `tui-term` could render
this with full terminal fidelity — colors, cursor positioning, progress bars — instead
of the current plain-string approach.

**Integration complexity:** Very High. The conductor bridge (`conductor-bridge/src/session.rs`)
spawns Claude CLI as a child process and reads NDJSON from its stdout via line-by-line
parsing — it does **not** use a PTY. Integrating `tui-term` would require rewriting the
bridge to use a PTY (e.g., via `portable-pty`), which breaks the structured NDJSON
parsing that the rest of the system depends on.

**Verdict:** Interesting concept, not practical without a significant bridge architectural
change. Use `ansi-to-tui` instead — it handles ANSI in the structured output lines
without requiring PTY infrastructure.

---

### 11. `ansi-to-tui`

**What it provides:** Converts bytes or strings containing ANSI SGR escape sequences
into `ratatui::text::Text` with equivalent `Style` attributes. Supports bold, italic,
underline, strikethrough, 256-color indexed, and 24-bit truecolor. Unknown/malformed
sequences are silently ignored. Optional zero-copy API (borrows from input). Optional
`simd` feature for UTF-8 decode acceleration.

| Field | Value |
|-------|-------|
| Crate | `ansi-to-tui` |
| Version | **8.0.1** |
| Author | ratatui org (official ecosystem) |
| GitHub | github.com/ratatui/ansi-to-tui |
| Last updated | Active (official ratatui org repo) |
| License | MIT |

**Ratatui 0.30 compatibility:**
> ✅ Part of the official ratatui organization; tracks ratatui releases.

**Conductor-TUI replacement target:**

The musician output panels in `musician.rs` display Claude Code CLI output. Claude
Code produces ANSI-colored terminal output: colored file paths, bold section headers,
green/red diff lines, progress indicators. The current rendering in `musician.rs:113-131`
iterates `MusicianState.output_lines` (a `Vec<String>`) and wraps each in a `Span` with
a heuristic `theme::output_line_style()` — which applies styles based on text prefix
detection (`line.starts_with("[USER]")` etc.). ANSI escape sequences in the raw output
are either displayed literally or silently mangled.

```rust
// CURRENT musician.rs — heuristic styling, discards ANSI:
let lines: Vec<Line> = output_lines.iter()
    .map(|l| Line::styled(l.as_str(), theme::output_line_style(l, recency)))
    .collect();

// WITH ansi-to-tui:
use ansi_to_tui::IntoText;

let lines: Vec<Line> = output_lines.iter()
    .flat_map(|l| {
        l.as_bytes().into_text()
            .unwrap_or_else(|_| l.as_str().into())
            .lines
    })
    .collect();
```

**Caveat on bridge format:** The conductor bridge parses NDJSON from Claude CLI. If
the `content` field in the NDJSON messages is already plain text (markdown, not ANSI),
`ansi-to-tui` won't help for that field. However, if the bridge passes through any raw
terminal output or if internal conductor `tracing` output is routed to a pane,
`ansi-to-tui` is directly useful. **Verify in `conductor-bridge`** whether output lines
contain ANSI before integrating.

**Integration complexity:** Very Low. `use ansi_to_tui::IntoText;` + `.into_text()?`
on any `&str` or `String`. The result is `ratatui::text::Text` that renders directly.

**Verdict:** Highest immediate impact for correctness — if ANSI is present in musician
output. Verify first; if ANSI is confirmed, this is a 5-line change with significant
visual improvement.

---

### 12. `tui-widget-list`

**What it provides:** A `ListView` stateful widget where each item implements `Widget`.
Unlike ratatui's built-in `List` (which takes pre-rendered `ListItem` text structs),
`tui-widget-list` renders arbitrary `Widget` implementations as list items. Features:
vertical/horizontal scroll axis, scroll padding, infinite scrolling (wrap-around),
mouse hit-test support, per-item `PreRender` hook.

| Field | Value |
|-------|-------|
| Crate | `tui-widget-list` |
| Author | preiter93 |
| GitHub | github.com/preiter93/tui-widget-list |
| Last updated | Active (2025) |
| License | MIT |

**Ratatui 0.30 compatibility:**
> ✅ Actively maintained; featured in official ratatui third-party showcase. Verify
> the latest release's ratatui dep range before adding.

**Conductor-TUI replacement target:**

- **`panels.rs:103-151`** — The task list in `render_plan_review()` uses `List::new(items)`
  with 40+ lines of manual `Line`/`Span` construction per item plus manual `▸` selection
  indicator. A `ListView` with a custom `TaskItem` widget handles selection state natively
  and allows richer per-item rendering (status badge color, variable height for items with
  many dependencies).

- **`panels.rs:192-227`** — `render_session_browser()` hand-rolls a `Table` widget with
  manual per-row style selection by index comparison. `ListView` replaces this with
  proper stateful selection.

```rust
// Custom list item:
struct TaskItem { task: Task, is_selected: bool }

impl Widget for TaskItem {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let viz = theme::task_viz(&self.task.status);
        let style = if self.is_selected { Style::default().fg(C_BRAND) } else { Style::default().fg(C_TEXT) };
        // Render status dot, title, file count — full layout control
    }
}

// UiState gains:
pub task_list_state: ListViewState,

// In render_plan_review:
let items: Vec<TaskItem> = tasks.iter().enumerate()
    .map(|(i, t)| TaskItem { task: t.clone(), is_selected: i == ui.plan_selected })
    .collect();
let list = ListView::builder()
    .scroll_padding(2)
    .build(items);
f.render_stateful_widget(list, chunks[2], &mut ui.task_list_state);
```

**Integration complexity:** Low-Medium. Requires implementing `Widget` for custom item
types (~30–50 lines per type). The selection state management is simpler than the current
manual index comparison.

**Verdict:** Good fit for the session browser and plan review task list. Reduces per-row
selection boilerplate and enables richer item rendering.

---

### 13. `tui-markdown`

**What it provides:** Renders Markdown text as styled ratatui `Text`. Headers become
bold, code spans get monospace-styled blocks, bullet lists get indented items, and
emphasis is styled. Part of the `tui-widgets` monorepo (experimental status).

| Field | Value |
|-------|-------|
| Crate | `tui-markdown` |
| Author | joshka (in tui-widgets) |
| GitHub | github.com/ratatui/tui-widgets |
| Last updated | Active (experimental) |
| License | MIT OR Apache-2.0 |

**Ratatui 0.30 compatibility:**
> ✅ Same tui-widgets monorepo.

**Conductor-TUI enhancement target:**

`panels.rs:268-316` — `render_task_detail()` renders `task.description`, `task.why`,
and `task.acceptance_criteria` as plain text line-by-line. The conductor agent
(Claude) likely generates these fields as Markdown. Currently all formatting (headers,
bullets, code blocks) is displayed as raw characters.

```rust
// In render_task_detail, replace:
for line in task.description.lines() {
    lines.push(Line::from(Span::styled(line, Style::default().fg(C_TEXT))));
}

// With:
let desc_md = tui_markdown::from_str(&task.description);
f.render_widget(Paragraph::new(desc_md), desc_area);
```

**Integration complexity:** Low. Drop-in if task fields contain markdown.
**Caveat:** Verify that task descriptions from `conductor_agent.rs` actually contain
Markdown before integrating — the benefit is zero on plain text.

**Verdict:** Low priority until confirmed that task descriptions contain Markdown.
Easy to add when the time comes.

---

## Additional Crates Found

### `tachyonfx`

**What it provides:** A shader-like effects and animation library for ratatui. 40+
built-in effects including fade in/out, color shifts, sweep animations, dissolve,
glitch, and geometric distortions. Effects compose and chain via `parallel()` and
`sequential()`. Supports a Rust DSL string syntax for defining effects at runtime.

| Field | Value |
|-------|-------|
| Crate | `tachyonfx` |
| Version | **0.11.1** |
| Stars | **~1,161** (March 2026) — one of the most starred ratatui community crates |
| Author | junkdog (in ratatui org) |
| GitHub | github.com/ratatui/tachyonfx |
| License | MIT |

**Ratatui 0.30 compatibility:** ✅ In the official ratatui org, tracks releases.

**Conductor-TUI use:** Phase transitions (`Init` → `PlanReview` → `Executing`) are
currently instantaneous. A `slide_in` when the musician grid appears, or a brief
`dissolve` when a panel is dismissed, would add visual continuity. Also pairs well
with `tui-big-text` for an animated splash screen.

**Integration complexity:** Medium. Effects require a per-frame timer and a render loop
that calls `effect.process(duration, buf, area)` each frame. The conductor's tokio
select loop with 50ms poll is compatible. Effects are applied to `Buffer` after normal
rendering — no restructuring needed.

**Verdict:** Low-priority for functionality but high-impact for polish. Subtle effects
(50ms fade) are appropriate for a productivity tool. Hold until core UX improvements
are done.

---

### `rat-theme`

**What it provides:** Pre-built color schemes and theme tokens for ratatui applications.
Semantic named slots (primary, secondary, text, error, warning) with multiple palette
variants (dark, light, Solarized, etc.).

**Ratatui 0.30 compatibility:** Check crates.io — part of the `rat-salsa` ecosystem.

**Conductor-TUI fit:** The conductor has a purpose-built theme in `theme.rs` with 9
semantic color constants (`C_BRAND`, `C_TEXT`, `C_DIM`, etc.). `rat-theme` would add
a dependency for marginal benefit over the existing `const Color` approach.

**Verdict:** Not recommended. The existing `theme.rs` is lightweight and adequate. If
multiple theme variants (dark/light) are ever needed, extend `theme.rs` with a `Theme`
struct rather than importing a third-party framework.

---

### `color-to-tui`

Parses HTML hex colors and named colors into `ratatui::style::Color`. Useful if the
conductor ever supports user-configurable theme colors from a config file.

**Verdict:** Low priority. No current need.

---

## Compatibility Matrix Summary

| Crate | Latest Version | ratatui 0.30 | Action |
|-------|----------------|--------------|--------|
| `ansi-to-tui` | 8.0.1 | ✅ Yes (ratatui org) | **Verify ANSI presence, then adopt** |
| `tui-textarea` | 0.7.0 | ✅ Yes (`>=0.23,<1`) | **Adopt** |
| `tui-popup` | 0.7.0 | ✅ Yes (tui-widgets) | **Adopt** |
| `tui-input` | ~0.10 | ✅ Yes (crossterm-based) | **Evaluate vs tui-textarea** |
| `tui-widget-list` | ~0.12+ | ✅ Likely | **Evaluate** |
| `tui-tree-widget` | 0.24.0 | ✅ Likely (Feb 2026) | **Evaluate** |
| `tachyonfx` | 0.11.1 | ✅ Yes (ratatui org) | **Optional/later** |
| `tui-big-text` | ~0.6+ | ✅ Yes (tui-widgets) | **Optional** |
| `tui-scrollview` | 0.6.2 | ✅ Yes (tui-widgets) | **Evaluate** |
| `tui-prompts` | ~0.4+ | ✅ Yes (tui-widgets) | **Low priority** |
| `tui-markdown` | experimental | ✅ Yes (tui-widgets) | **Verify markdown in task fields first** |
| `tui-logger` | 0.17.2 | ⚠️ Pinned 0.29 | **Wait for 0.30 update** |
| `tui-term` | ~0.2 | ⚠️ Experimental | **Not recommended — arch mismatch** |
| `ratatui-image` | ~2.x | ✅ Yes (ratatui org) | **Defer** |

---

## Code Impact Reference

For each crate, the specific conductor-tui code it replaces (with file + line numbers):

| Crate | File | Lines | Code Being Replaced |
|-------|------|-------|---------------------|
| `tui-textarea` / `tui-input` | `app.rs` | 209–287 | Hand-rolled `String`/cursor prompt input (~80 lines, UTF-8 bug) |
| `tui-textarea` / `tui-input` | `components/input.rs` | 21–49 | `render_prompt_bar()` manual cursor positioning |
| `tui-popup` | `app.rs` | 532–555 | `centered_rect()` + `clear_area()` helpers (19 lines) |
| `tui-popup` | `components/panels.rs` | 175–176 | `centered_rect(70,70)+clear_area` in session browser |
| `tui-popup` | `components/panels.rs` | 240–247 | `centered_rect(70,80)+clear_area` in task detail |
| `tui-popup` | `components/input.rs` | 53–55 | `centered_rect(60,60)+Clear` in keyboard help |
| `ansi-to-tui` | `components/musician.rs` | 113–131 | Raw string → styled `Text` conversion (output_lines loop) |
| `tui-scrollview` | `app.rs` | 34, 199–205, 325–334 | `UiState.scroll_offset` + manual scroll key logic |
| `tui-scrollview` | `components/panels.rs` | 359–362 | `Paragraph::scroll((scroll_offset, 0))` in task detail |
| `tui-tree-widget` | `components/insights.rs` | (task graph fn) | `render_task_graph()` flat ASCII approximation |
| `tui-widget-list` | `components/panels.rs` | 103–151 | Plan review task `List` with manual `▸` indicator |
| `tui-widget-list` | `components/panels.rs` | 192–227 | Session browser `Table` with manual row selection |
| `tui-big-text` | `components/header.rs` | (header title) | Plain `Paragraph` title / empty state screen |
| `tui-markdown` | `components/panels.rs` | 268–316 | Task detail plain-text field rendering |

---

## Recommendations: Adoption Order

**Phase 1 — Correctness (minimal API surface changes):**
1. `ansi-to-tui` — Verify ANSI presence in musician output first. If present: add to
   `Cargo.toml`, wrap output lines with `.into_text()?`. 5-line change, significant
   visual improvement.

**Phase 2 — Reduce boilerplate:**
2. `tui-input` (or `tui-textarea`) — Replace `UiState.{prompt_input, prompt_cursor}`
   + `app.rs:209-287` with a proper input widget. Fixes a real UTF-8 cursor bug. Choose
   `tui-input` to keep the existing prompt bar rendering; choose `tui-textarea` for
   undo/redo.
3. `tui-popup` — Replace the three `centered_rect + Clear` callsites. Can delete both
   helper functions from `app.rs`. Pure refactor — no behavior change.

**Phase 3 — Enhanced UX:**
4. `tui-widget-list` — Upgrade session browser and plan review task list. Add scroll
   padding and proper selection state.
5. `tui-tree-widget` — Replace `render_task_graph()` with an interactive tree. Improves
   readability for plans with 6+ tasks.
6. `tui-scrollview` — Per-panel scroll state with visible scrollbars. Start with the
   task detail modal (`panels.rs:359-362`).

**Phase 4 — Polish:**
7. `tui-big-text` — Header branding / idle state screen.
8. `tachyonfx` — Phase transition animations (after core UX is stable).
9. `tui-logger` — Internal conductor log pane (after ratatui 0.30 compat confirmed).

---

*Sources: crates.io, lib.rs, docs.rs, GitHub repositories, ratatui/awesome-ratatui,
ratatui.rs third-party widget showcase*
