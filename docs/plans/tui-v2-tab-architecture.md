# TUI V2 -- Tab Architecture & State Design

> **Status:** Design document -- ready for implementation
> **Date:** 2026-03-22
> **Depends on:** `crates/conductor-tui/src/app.rs`, `crates/conductor-types/src/state.rs`
> **Feeds into:** All TUI V2 rendering and input handling work

---

## 1. Tab Enum & Visibility Rules

### 1.1 Tab Enum

```rust
// crates/conductor-tui/src/app.rs (new)

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Tab {
    Orchestra, // 1 -- Musician grid + task graph + insights
    Plan,      // 2 -- Plan review, task list, refinement chat, DAG
    Stats,     // 3 -- Token costs, timing, per-musician gauges
    Diff,      // 4 -- Aggregated file diffs, per-musician attribution
    Log,       // 5 -- Full event log, filterable
}

impl Tab {
    /// All tabs in display order.
    pub const ALL: &[Tab] = &[
        Tab::Orchestra,
        Tab::Plan,
        Tab::Stats,
        Tab::Diff,
        Tab::Log,
    ];

    /// Human-readable label for the tab bar.
    pub fn label(&self) -> &'static str {
        match self {
            Tab::Orchestra => "Orchestra",
            Tab::Plan      => "Plan",
            Tab::Stats     => "Stats",
            Tab::Diff      => "Diff",
            Tab::Log       => "Log",
        }
    }

    /// Icon for narrow terminals (shown instead of label when width < NARROW).
    pub fn icon(&self) -> &'static str {
        match self {
            Tab::Orchestra => "\u{266b}",  // musical note
            Tab::Plan      => "\u{1f4cb}", // clipboard
            Tab::Stats     => "\u{1f4ca}", // chart
            Tab::Diff      => "\u{1f4c4}", // page
            Tab::Log       => "\u{1f4dd}", // memo
        }
    }

    /// Shortcut key shown in tab bar (1-5).
    pub fn key(&self) -> char {
        match self {
            Tab::Orchestra => '1',
            Tab::Plan      => '2',
            Tab::Stats     => '3',
            Tab::Diff      => '4',
            Tab::Log       => '5',
        }
    }

    /// Resolve a digit key to a Tab.
    pub fn from_key(c: char) -> Option<Tab> {
        match c {
            '1' => Some(Tab::Orchestra),
            '2' => Some(Tab::Plan),
            '3' => Some(Tab::Stats),
            '4' => Some(Tab::Diff),
            '5' => Some(Tab::Log),
            _ => None,
        }
    }
}

impl Default for Tab {
    fn default() -> Self {
        Tab::Orchestra
    }
}
```

### 1.2 Tab Visibility per OrchestraPhase

Not all tabs make sense in every phase. The tab bar only renders visible tabs; pressing a hidden tab's key is a no-op.

| Phase | Orchestra | Plan | Stats | Diff | Log |
|-------|-----------|------|-------|------|-----|
| `Init` | **visible** | -- | -- | -- | **visible** |
| `Planning` / `Exploring` / `Analyzing` / `Decomposing` | **visible** | -- | -- | -- | **visible** |
| `PlanReview` | **visible** | **visible** (auto-focus) | -- | -- | **visible** |
| `PhaseDetailing` | **visible** | **visible** | **visible** | -- | **visible** |
| `PhaseExecuting` / `Executing` | **visible** | **visible** | **visible** | -- | **visible** |
| `PhaseMerging` / `Integrating` | **visible** | **visible** | **visible** | **visible** (auto-focus) | **visible** |
| `PhaseReviewing` / `Reviewing` / `FinalReview` | **visible** | **visible** | **visible** | **visible** | **visible** |
| `Complete` | **visible** | **visible** | **visible** | **visible** | **visible** |
| `Failed` | **visible** | **visible** | **visible** | **visible** | **visible** |
| `Paused` / `Probing` | inherit from `previous_phase` | | | | |

**Note on `Paused`/`Probing`:** These are transient states that can occur during any phase. Tab visibility should not change when entering these states. The `previous_phase` field in `UiState` preserves the visibility context. The `is_visible` implementation treats them permissively (all tabs visible except Diff) since we can't know the preceding phase from the phase enum alone -- the event loop's phase-transition logic handles the actual visibility by not updating `previous_phase` for these states.

```rust
impl Tab {
    /// Returns true if this tab should be visible in the given phase.
    pub fn is_visible(&self, phase: &OrchestraPhase) -> bool {
        match self {
            Tab::Orchestra => true,  // Always visible
            Tab::Log       => true,  // Always visible
            Tab::Plan => matches!(
                phase,
                OrchestraPhase::PlanReview
                    | OrchestraPhase::PhaseDetailing
                    | OrchestraPhase::PhaseExecuting
                    | OrchestraPhase::Executing
                    | OrchestraPhase::PhaseMerging
                    | OrchestraPhase::Integrating
                    | OrchestraPhase::PhaseReviewing
                    | OrchestraPhase::Reviewing
                    | OrchestraPhase::FinalReview
                    | OrchestraPhase::Complete
                    | OrchestraPhase::Failed
                    | OrchestraPhase::Paused
                    | OrchestraPhase::Probing
            ),
            Tab::Stats => matches!(
                phase,
                OrchestraPhase::PhaseDetailing
                    | OrchestraPhase::PhaseExecuting
                    | OrchestraPhase::Executing
                    | OrchestraPhase::PhaseMerging
                    | OrchestraPhase::Integrating
                    | OrchestraPhase::PhaseReviewing
                    | OrchestraPhase::Reviewing
                    | OrchestraPhase::FinalReview
                    | OrchestraPhase::Complete
                    | OrchestraPhase::Failed
                    | OrchestraPhase::Paused
                    | OrchestraPhase::Probing
            ),
            Tab::Diff => matches!(
                phase,
                OrchestraPhase::PhaseMerging
                    | OrchestraPhase::Integrating
                    | OrchestraPhase::PhaseReviewing
                    | OrchestraPhase::Reviewing
                    | OrchestraPhase::FinalReview
                    | OrchestraPhase::Complete
                    | OrchestraPhase::Failed
            ),
        }
    }
}
```

### 1.3 Auto-Switch Rules

When the orchestra phase transitions, the TUI auto-focuses the most relevant tab:

| Phase Transition | Auto-Switch To | Rationale |
|-----------------|----------------|-----------|
| `* -> PlanReview` | `Tab::Plan` | User needs to review/approve the plan |
| `* -> PhaseExecuting` / `* -> Executing` | `Tab::Orchestra` | Musicians are now active |
| `* -> PhaseMerging` / `* -> Integrating` | `Tab::Diff` | User wants to see merged changes |
| `* -> FinalReview` | `Tab::Stats` | Show final cost/timing summary |
| `* -> Complete` | `Tab::Stats` | Show final results |
| `* -> Failed` | `Tab::Log` | User needs to diagnose what went wrong |

**No auto-switch for:** `Paused`, `Probing`, `PhaseDetailing`, `PhaseReviewing`, `Init`, `Planning`, `Exploring`, `Analyzing`, `Decomposing`. These either happen too fast or shouldn't disrupt the user's current view.

```rust
/// Returns the tab to auto-switch to on phase transition, or None to stay.
pub fn auto_switch_tab(new_phase: &OrchestraPhase) -> Option<Tab> {
    match new_phase {
        OrchestraPhase::PlanReview => Some(Tab::Plan),
        OrchestraPhase::PhaseExecuting | OrchestraPhase::Executing => Some(Tab::Orchestra),
        OrchestraPhase::PhaseMerging | OrchestraPhase::Integrating => Some(Tab::Diff),
        OrchestraPhase::FinalReview | OrchestraPhase::Complete => Some(Tab::Stats),
        OrchestraPhase::Failed => Some(Tab::Log),
        _ => None,
    }
}
```

### 1.4 Integration with UiState

The `active_tab` field is added to `UiState`. It replaces the implicit phase-dependent content switching currently in `render_content()`.

```rust
pub struct UiState {
    // NEW: Tab system
    pub active_tab: Tab,
    pub previous_phase: OrchestraPhase, // track phase transitions for auto-switch

    // EXISTING fields (unchanged)
    pub focused_panel: usize,
    pub show_help: bool,
    pub show_sessions: bool,
    pub show_insights: bool,
    // ... etc
}
```

In the event loop, before each render:

```rust
// Detect phase transitions and auto-switch tabs
if state.phase != ui.previous_phase {
    // Don't update previous_phase for transient states (preserves visibility context)
    let is_transient = matches!(
        state.phase,
        OrchestraPhase::Paused | OrchestraPhase::Probing
    );

    if let Some(tab) = auto_switch_tab(&state.phase) {
        if tab.is_visible(&state.phase) {
            ui.active_tab = tab;
        }
    }

    // If active tab became invisible, fall back to Orchestra
    if !ui.active_tab.is_visible(&state.phase) {
        ui.active_tab = Tab::Orchestra;
    }

    if !is_transient {
        ui.previous_phase = state.phase.clone();
    }
}
```

---

## 2. Per-Tab State

### 2.1 The Problem

Currently `UiState.scroll_offset` is shared across all views. When the user scrolls in the musician grid and then switches to viewing a task detail, the scroll position is lost/conflated. Specifically:

- `app.rs:43` -- `scroll_offset: u16` is used by musician grid, conductor output, AND task detail overlay
- `app.rs:325` -- Task detail modal reuses `scroll_offset` for its own scrolling
- `app.rs:569-579` -- Up/Down in planning phase increments the same `scroll_offset` used by execution phase
- Switching between views (e.g., plan review -> execution) leaves stale scroll positions

### 2.2 TabState Struct

Each tab owns its own scroll and selection state:

```rust
use std::collections::HashSet;

/// Per-tab UI state. Lives inside UiState, indexed by Tab.
pub struct TabState {
    pub orchestra: OrchestraTabState,
    pub plan: PlanTabState,
    pub stats: StatsTabState,
    pub diff: DiffTabState,
    pub log: LogTabState,
}

impl Default for TabState {
    fn default() -> Self {
        Self {
            orchestra: OrchestraTabState::default(),
            plan: PlanTabState::default(),
            stats: StatsTabState::default(),
            diff: DiffTabState::default(),
            log: LogTabState::default(),
        }
    }
}

// -- Orchestra Tab --

#[derive(Default)]
pub struct OrchestraTabState {
    /// Which musician panel has focus (index into musicians vec).
    pub focused_musician: usize,
    /// Independent scroll offset per musician panel.
    /// Grows dynamically as musicians are added. Index = musician index.
    pub musician_scroll: Vec<u16>,
    /// Single-musician expanded view.
    pub focus_mode: bool,
    /// Conductor output scroll (used during planning phases when no musicians exist).
    pub conductor_scroll: u16,
}

impl OrchestraTabState {
    /// Ensure musician_scroll has entries for all musicians.
    /// Called in the event loop when musician count changes.
    pub fn sync_musician_count(&mut self, count: usize) {
        if self.musician_scroll.len() < count {
            self.musician_scroll.resize(count, 0);
        }
        // Clamp focused_musician
        if count > 0 {
            self.focused_musician = self.focused_musician.min(count - 1);
        } else {
            self.focused_musician = 0;
        }
    }

    /// Get scroll offset for the currently focused musician.
    pub fn focused_scroll(&self) -> u16 {
        self.musician_scroll
            .get(self.focused_musician)
            .copied()
            .unwrap_or(0)
    }

    /// Mutably get scroll offset for the currently focused musician.
    pub fn focused_scroll_mut(&mut self) -> &mut u16 {
        if self.focused_musician < self.musician_scroll.len() {
            &mut self.musician_scroll[self.focused_musician]
        } else {
            // Should not happen if sync_musician_count is called, but be safe
            self.musician_scroll.resize(self.focused_musician + 1, 0);
            &mut self.musician_scroll[self.focused_musician]
        }
    }
}

// -- Plan Tab --

#[derive(Default)]
pub struct PlanTabState {
    /// Selected task in the task list.
    pub plan_selected: usize,
    /// Scroll offset for the task list itself.
    pub plan_scroll: u16,
    /// Scroll offset for the refinement chat history.
    pub refinement_scroll: u16,
    /// Whether the refinement chat panel is focused (vs task list).
    pub refinement_focused: bool,
    /// Task detail expanded view (Some(task_index) when open).
    pub task_detail: Option<usize>,
    /// Scroll offset within the task detail view.
    pub task_detail_scroll: u16,
}

// -- Stats Tab --

#[derive(Default)]
pub struct StatsTabState {
    // Fixed layout -- gauges, sparklines, cost summary.
    // No scroll needed for MVP. If content grows:
    pub scroll: u16,
}

// -- Diff Tab --

pub struct DiffTabState {
    /// Vertical scroll through the diff.
    pub diff_scroll: u16,
    /// Which files are expanded (showing full hunks). Others show only headers.
    pub expanded_files: HashSet<usize>,
    /// Selected file index (for keyboard navigation).
    pub selected_file: usize,
    /// Filter: show only diffs from this musician (None = all).
    pub musician_filter: Option<usize>,
}

impl Default for DiffTabState {
    fn default() -> Self {
        Self {
            diff_scroll: 0,
            expanded_files: HashSet::new(),
            selected_file: 0,
            musician_filter: None,
        }
    }
}

// -- Log Tab --

pub struct LogTabState {
    /// Vertical scroll through the log.
    pub log_scroll: u16,
    /// Text filter applied to log entries (None = show all).
    pub log_filter: Option<String>,
    /// Auto-scroll to bottom when new entries arrive.
    pub auto_scroll: bool,
    /// Filter by event source (None = all, Some(index) = musician at that index,
    /// usize::MAX = conductor-only). Matches LogEntrySource for filtering.
    pub source_filter: Option<LogSourceFilter>,
}

impl Default for LogTabState {
    fn default() -> Self {
        Self {
            log_scroll: 0,
            log_filter: None,
            auto_scroll: true, // Start with auto-scroll ON
            source_filter: None,
        }
    }
}

/// Filter for log source. Kept in TUI layer (not in conductor-types)
/// because it's purely a UI concern.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogSourceFilter {
    Conductor,
    Musician(usize), // musician index -- matches LogEntrySource::Musician.index
    System,
}

impl LogSourceFilter {
    /// Check if a LogEntrySource matches this filter.
    pub fn matches(&self, source: &LogEntrySource) -> bool {
        match (self, source) {
            (LogSourceFilter::Conductor, LogEntrySource::Conductor) => true,
            (LogSourceFilter::Musician(idx), LogEntrySource::Musician { index, .. }) => idx == index,
            (LogSourceFilter::System, LogEntrySource::System) => true,
            _ => false,
        }
    }
}
```

### 2.3 Migration from Current UiState

| Current Field | New Location | Notes |
|--------------|-------------|-------|
| `scroll_offset` | **REMOVED** | Replaced by per-tab scroll fields |
| `focused_panel` | `tab_state.orchestra.focused_musician` | Same semantics |
| `focus_mode` | `tab_state.orchestra.focus_mode` | Same semantics |
| `plan_selected` | `tab_state.plan.plan_selected` | Same semantics |
| `show_task_detail` | `tab_state.plan.task_detail` | Moves to Plan tab (no longer an overlay) |
| `show_insights` | Stays in `UiState` | Cross-tab (insights panel can show on any tab) |
| `show_help` | Stays in `UiState` | Overlay, not tab-specific |
| `show_sessions` | Stays in `UiState` | Overlay, not tab-specific |
| `prompt_input` / `prompt_cursor` / history | Stays in `UiState` | Shared prompt bar across all tabs |
| `layout_config` | Stays in `UiState` | Terminal-level, not tab-specific |

### 2.4 Updated UiState

```rust
pub struct UiState {
    // -- Tab System --
    pub active_tab: Tab,
    pub previous_phase: OrchestraPhase,
    pub tab_state: TabState,

    // -- Overlays (cross-tab) --
    pub show_help: bool,
    pub show_sessions: bool,
    pub show_insights: bool,

    // -- Prompt Bar (shared) --
    pub prompt_input: String,
    pub prompt_cursor: usize,
    pub prompt_history: Vec<String>,
    pub history_index: Option<usize>,
    pub history_stash: String,

    // -- Session Browser --
    pub session_selected: usize,
    pub sessions: Vec<SessionData>,

    // -- Terminal State --
    pub layout_config: LayoutConfig,
    pub last_was_esc: bool,
    pub needs_clear: bool,
}

impl UiState {
    pub fn new(width: u16, height: u16) -> Self {
        Self {
            active_tab: Tab::Orchestra,
            previous_phase: OrchestraPhase::Init,
            tab_state: TabState::default(),
            show_help: false,
            show_sessions: false,
            show_insights: true,
            prompt_input: String::new(),
            prompt_cursor: 0,
            prompt_history: Vec::new(),
            history_index: None,
            history_stash: String::new(),
            session_selected: 0,
            sessions: Vec::new(),
            layout_config: get_layout_config(width, height),
            last_was_esc: false,
            needs_clear: false,
        }
    }
}
```

---

## 3. New Data Requirements for OrchestraState

### 3.1 Stats Tab Data

The Stats tab needs token/cost/timing data that doesn't currently exist in `OrchestraState`.

#### New fields for `OrchestraState` (in `conductor-types/src/state.rs`):

```rust
pub struct OrchestraState {
    // ... existing fields ...

    // NEW: Aggregate statistics
    pub stats: Option<OrchestraStats>,
}
```

#### New types (in `conductor-types/src/state.rs`):

```rust
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OrchestraStats {
    /// Per-musician token and cost breakdown.
    pub musician_stats: Vec<MusicianStats>,
    /// Conductor agent's own token usage (planning, reviewing, guidance).
    /// The conductor typically uses opus and accounts for the largest share of cost.
    pub conductor_stats: ConductorStats,
    /// Per-phase cost breakdown.
    pub phase_costs: Vec<PhaseCost>,
    /// Total tokens (input + output) across ALL agents (conductor + musicians).
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    /// Total estimated cost in USD (conductor + musicians).
    pub total_cost_usd: f64,
    /// Model usage breakdown (model_id -> token count).
    pub model_usage: Vec<ModelUsage>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ConductorStats {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cost_usd: f64,
    /// Number of LLM calls made by the conductor agent.
    pub calls: u32,
    /// Per-call token counts for sparkline rendering.
    pub token_history: Vec<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MusicianStats {
    pub musician_id: String,
    pub musician_index: usize,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cost_usd: f64,
    pub turns: u32,
    pub duration_ms: u64,
    /// Per-turn token counts for sparkline rendering.
    pub token_history: Vec<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhaseCost {
    pub phase_name: String,
    pub phase_index: usize,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cost_usd: f64,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelUsage {
    pub model_id: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cost_usd: f64,
}
```

#### Where computed:

| Field | Computed in | Source Data |
|-------|------------|------------|
| `MusicianStats.input_tokens` / `output_tokens` | `conductor-core/src/orchestra.rs` | Parsed from `ClaudeEvent` `duration_api_ms`, `num_turns` fields; future: parse usage from Claude NDJSON stream |
| `MusicianStats.cost_usd` | `conductor-core/src/orchestra.rs` | Calculated from token counts x model pricing table |
| `ConductorStats.*` | `conductor-core/src/conductor_agent.rs` | Accumulated from each conductor LLM call (planning, decomposing, reviewing, guidance) |
| `PhaseCost` | `conductor-core/src/orchestra.rs` | Aggregated from `MusicianStats` + `ConductorStats` grouped by `current_phase_index` |
| `ModelUsage` | `conductor-core/src/orchestra.rs` | Aggregated by model: conductor uses `config.conductor_model` (typically opus), musicians use `config.musician_model` (typically sonnet) |
| `token_history` | `conductor-core/src/musician.rs` | Append on each `ClaudeEvent::Result` |

**Note on conductor cost dominance:** The conductor agent typically uses an expensive model (opus) for planning, reviewing, and guidance. In production runs, conductor cost often exceeds total musician cost (the Stats tab mockup shows Conductor at $1.84 vs all musicians combined at $0.72). Tracking conductor stats separately is essential for cost optimization insights.

### 3.2 Diff Tab Data

#### New fields for `OrchestraState`:

```rust
pub struct OrchestraState {
    // ... existing fields ...

    // NEW: Aggregated diffs
    pub aggregated_diffs: Vec<FileDiff>,
}
```

#### New types:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileDiff {
    pub file_path: String,
    pub hunks: Vec<DiffHunk>,
    /// Total lines added across all hunks.
    pub lines_added: usize,
    /// Total lines removed across all hunks.
    pub lines_removed: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffHunk {
    pub header: String,           // @@ -10,5 +10,7 @@
    pub lines: Vec<DiffLine>,
    /// Which musician produced this hunk.
    pub musician_id: Option<String>,
    pub musician_index: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffLine {
    pub kind: DiffLineKind,
    pub content: String,
    pub old_line: Option<usize>,
    pub new_line: Option<usize>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum DiffLineKind {
    Context,
    Added,
    Removed,
}
```

#### Where computed:

| Field | Computed in | Source Data |
|-------|------------|------------|
| `FileDiff` list | `conductor-core/src/orchestra.rs` (during merge phase) | `git diff` output from worktree merges |
| `DiffHunk.musician_id` | `conductor-core/src/orchestra.rs` | Track which musician's worktree contributed each merge |
| Aggregation | Merge-time in `orchestra.rs` | Run `git diff --no-color` after each worktree merge, parse output |

**Note:** Per-musician diffs are also partially available from `TaskResult.diff` (already exists in `state.rs:144`). The Diff tab aggregates these per-task diffs into a unified per-file view, deduplicating overlapping changes when multiple musicians touch the same file.

### 3.3 Log Tab Data

#### New fields for `OrchestraState`:

```rust
pub struct OrchestraState {
    // ... existing fields ...

    // NEW: Structured event log
    pub event_log: Vec<LogEntry>,
}
```

#### New types:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    pub timestamp: String,
    pub source: LogEntrySource,
    pub level: LogLevel,
    pub message: String,
    /// Optional structured data (tool name, file path, etc.)
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum LogEntrySource {
    Conductor,
    Musician { id: String, index: usize },
    System,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum LogLevel {
    Debug,
    Info,
    Warn,
    Error,
}
```

#### Where computed:

| Field | Computed in | Source Data |
|-------|------------|------------|
| `LogEntry` | `conductor-core/src/orchestra.rs` | Constructed from `OrchestraEvent` variants as they arrive |
| Conductor entries | `handle_event(ConductorOutput)` | Wrap existing `conductor_output` pushes |
| Musician entries | `handle_event(MusicianOutput)` | Wrap existing musician output buffering |
| System entries | Various places in `orchestra.rs` | Phase transitions, errors, rate limits |

**Note on `conductor_output`:** The existing `conductor_output: Vec<String>` field is kept for backward compatibility and for the Orchestra tab's conductor output panel. `event_log` is a superset that includes structured metadata, log levels, and source attribution.

### 3.4 Summary of OrchestraState Changes

```rust
pub struct OrchestraState {
    // EXISTING (unchanged):
    pub phase: OrchestraPhase,
    pub config: OrchestraConfig,
    pub tasks: Vec<Task>,
    pub plan: Option<Plan>,
    pub phases: Vec<Phase>,
    pub current_phase_index: Option<usize>,
    pub musicians: Vec<MusicianState>,
    pub analysts: Vec<AnalystState>,
    pub analysis_results: Vec<AnalysisResult>,
    pub rate_limit: RateLimitState,
    pub started_at: String,
    pub elapsed_ms: u64,
    pub conductor_output: Vec<String>,
    pub conductor_prompts: Vec<String>,
    pub guidance_queue_size: usize,
    pub plan_validation: Option<PlanValidation>,
    pub refinement_history: Vec<PlanRefinementMessage>,
    pub insights: Vec<Insight>,

    // NEW:
    pub stats: Option<OrchestraStats>,       // Stats tab data
    pub aggregated_diffs: Vec<FileDiff>,      // Diff tab data
    pub event_log: Vec<LogEntry>,            // Log tab data
}

// In OrchestraState::new(), add:
//   stats: None,
//   aggregated_diffs: Vec::new(),
//   event_log: Vec::new(),
```

---

## 4. render_all() Refactor Plan

### 4.1 Current Call Graph (BEFORE)

```
render_all(f, state, ui)
|-- header::render_header(f, header_area, state)
|-- render_content(f, content_area, state, ui)
|   |-- [if PlanReview] panels::render_plan_review(f, area, plan, tasks, history, selected)
|   |-- [if planning && no musicians] render_planning_content(f, area, state, ui)
|   |   |-- render_planning_main(f, area, state, ui, label)
|   |   |   |-- conductor::render_conductor_output(f, area, output, label, scroll)
|   |   |   +-- analyst::render_analyst_grid(f, area, analysts, layout)
|   |   +-- insights::render_insight_panel(f, area, insights)
|   +-- [else] render_musicians(f, area, state, ui)
|       |-- insights::render_task_graph(f, area, tasks)
|       |-- musician::render_musician_grid(f, area, musicians, focused, layout, focus, scroll)
|       +-- insights::render_insight_panel(f, area, insights)
|-- status::render_status_line(f, status_area, state)
|-- input::render_prompt_bar(f, prompt_area, input, cursor, phase)
+-- [overlays]
    |-- input::render_keyboard_help(f, size)
    |-- panels::render_session_browser(f, size, sessions, selected)
    +-- panels::render_task_detail(f, size, task, tasks, scroll)
```

**Problems:**
- `render_content()` is phase-driven, not tab-driven -- user can't view plan while executing
- `scroll_offset` is passed as a single value into different views (app.rs:43)
- No tab bar rendered anywhere
- Task detail overlay uses the shared `scroll_offset` (app.rs:325, 744)
- `render_planning_content` and `render_musicians` both independently handle insights panel layout

### 4.2 New Call Graph (AFTER)

```
render_all(f, state, ui)
|-- header::render_header(f, header_area, state)
|-- tab_bar::render_tab_bar(f, tab_bar_area, ui.active_tab, &state.phase)   [NEW]
|-- render_tab_content(f, content_area, state, ui)                           [NEW dispatcher]
|   |-- [horizontal split: tab content | insights panel (if visible)]
|   |
|   |-- [Tab::Orchestra] render_orchestra_tab(f, area, state, &ui.tab_state.orchestra, &ui.layout_config)
|   |   |-- [if planning] render_planning_content(f, area, state, &orchestra_state)
|   |   |   |-- conductor::render_conductor_output(f, area, output, label, conductor_scroll)
|   |   |   +-- analyst::render_analyst_grid(f, area, analysts, layout)
|   |   +-- [if executing] render_musicians(f, area, state, &orchestra_state, &layout_config)
|   |       |-- insights::render_task_graph(f, area, tasks)
|   |       +-- musician::render_musician_grid(f, area, musicians, focused, layout, focus, per_musician_scroll)
|   |
|   |-- [Tab::Plan] render_plan_tab(f, area, state, &ui.tab_state.plan)     [NEW]
|   |   |-- panels::render_plan_review(f, area, plan, tasks, history, plan_state)
|   |   +-- [if task_detail.is_some()] panels::render_task_detail(f, area, task, tasks, task_detail_scroll)
|   |
|   |-- [Tab::Stats] render_stats_tab(f, area, state, &ui.tab_state.stats)  [NEW]
|   |   |-- stats::render_cost_summary(f, area, stats)
|   |   |-- stats::render_conductor_gauge(f, area, conductor_stats)
|   |   |-- stats::render_musician_gauges(f, area, musician_stats)
|   |   |-- stats::render_phase_breakdown(f, area, phase_costs)
|   |   +-- stats::render_model_usage(f, area, model_usage)
|   |
|   |-- [Tab::Diff] render_diff_tab(f, area, state, &ui.tab_state.diff)     [NEW]
|   |   |-- diff::render_file_list(f, area, diffs, expanded, selected, musician_filter)
|   |   +-- diff::render_hunk_view(f, area, hunks, diff_scroll)
|   |
|   +-- [Tab::Log] render_log_tab(f, area, state, &ui.tab_state.log)        [NEW]
|       +-- log::render_event_log(f, area, entries, log_scroll, log_filter, source_filter, auto_scroll)
|
|-- [if show_insights] insights::render_insight_panel(f, side_area, insights)  [SHARED]
|-- status::render_status_line(f, status_area, state)                          [SHARED]
|-- input::render_prompt_bar(f, prompt_area, input, cursor, phase)             [SHARED]
+-- [overlays]                                                                 [SHARED]
    |-- input::render_keyboard_help(f, size)
    +-- panels::render_session_browser(f, size, sessions, selected)
```

### 4.3 Shared vs Tab-Specific Components

| Component | Scope | Rationale |
|-----------|-------|-----------|
| `header::render_header` | Shared | Always shows session/phase info |
| `tab_bar::render_tab_bar` | Shared (new) | Always visible, shows tab navigation |
| `status::render_status_line` | Shared | Always shows phase/timing/cost summary |
| `input::render_prompt_bar` | Shared | Prompt input works on any tab |
| `insights::render_insight_panel` | Shared | Can toggle on any tab (right sidebar) |
| `input::render_keyboard_help` | Overlay | Modal, rendered on top |
| `panels::render_session_browser` | Overlay | Modal, rendered on top |
| `panels::render_task_detail` | **Plan tab only** | No longer an overlay; embedded in Plan tab |

### 4.4 Layout Changes

Current vertical layout (4 sections):
```
header(1) | content(flex) | status(1) | prompt(1)
```

New vertical layout (5 sections):
```
header(1) | tab_bar(1) | content(flex) | status(1) | prompt(1)
```

```rust
let main_chunks = Layout::default()
    .direction(Direction::Vertical)
    .constraints([
        Constraint::Length(1), // header
        Constraint::Length(1), // tab bar         <-- NEW
        Constraint::Min(3),   // tab content area
        Constraint::Length(1), // status line
        Constraint::Length(1), // prompt bar
    ])
    .split(size);
```

The insights panel split is applied once in `render_tab_content` (not duplicated per tab):

```rust
fn render_tab_content(f: &mut Frame, area: Rect, state: &OrchestraState, ui: &UiState) {
    // Split for insights panel if visible
    let (tab_area, insights_area) = if ui.layout_config.show_insights_panel && ui.show_insights {
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Min(20),
                Constraint::Length(ui.layout_config.insights_panel_width),
            ])
            .split(area);
        (chunks[0], Some(chunks[1]))
    } else {
        (area, None)
    };

    // Dispatch to tab-specific render
    match ui.active_tab {
        Tab::Orchestra => render_orchestra_tab(f, tab_area, state, ui),
        Tab::Plan      => render_plan_tab(f, tab_area, state, ui),
        Tab::Stats     => render_stats_tab(f, tab_area, state, ui),
        Tab::Diff      => render_diff_tab(f, tab_area, state, ui),
        Tab::Log       => render_log_tab(f, tab_area, state, ui),
    }

    // Insights panel (shared across tabs)
    if let Some(area) = insights_area {
        insights::render_insight_panel(f, area, &state.insights);
    }
}
```

### 4.5 Tab Bar Renderer

New file: `crates/conductor-tui/src/components/tab_bar.rs`

```rust
use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    Frame,
};
use conductor_types::OrchestraPhase;
use crate::app::Tab;
use crate::layout::NARROW;

/// Render the tab bar. Active tab is highlighted, invisible tabs are dimmed/hidden.
pub fn render_tab_bar(f: &mut Frame, area: Rect, active_tab: Tab, phase: &OrchestraPhase) {
    let narrow = area.width < NARROW;
    let mut spans = Vec::new();

    for (i, tab) in Tab::ALL.iter().enumerate() {
        if i > 0 {
            spans.push(Span::raw("  "));
        }

        if !tab.is_visible(phase) {
            // Hidden tab -- render dimmed with key hint
            spans.push(Span::styled(
                format!(" {} ", tab.key()),
                Style::default().fg(Color::DarkGray),
            ));
            continue;
        }

        // Use icon-only labels on narrow terminals
        let label = if narrow {
            format!(" {} {} ", tab.key(), tab.icon())
        } else {
            format!(" {} {} ", tab.key(), tab.label())
        };

        if *tab == active_tab {
            spans.push(Span::styled(
                label,
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ));
        } else {
            spans.push(Span::styled(
                label,
                Style::default().fg(Color::Gray),
            ));
        }
    }

    let line = Line::from(spans);
    f.render_widget(line, area);
}
```

### 4.6 New Files

| File | Purpose |
|------|---------|
| `components/tab_bar.rs` | Tab bar rendering (`render_tab_bar`) |
| `components/stats.rs` | Stats tab components (gauges, sparklines, cost tables) |
| `components/diff.rs` | Diff tab components (file list, hunk viewer) |
| `components/log.rs` | Log tab components (filterable event list) |

### 4.7 Existing Function Mapping

| Current Function | New Home | Changes |
|-----------------|----------|---------|
| `render_all` | `app.rs` | Add tab bar row, delegate to `render_tab_content` |
| `render_content` | **REMOVED** | Replaced by `render_tab_content` dispatch |
| `render_planning_content` | `app.rs` (called from `render_orchestra_tab`) | Takes `OrchestraTabState` for `conductor_scroll` |
| `render_planning_main` | `app.rs` (called from `render_planning_content`) | Takes `conductor_scroll` instead of shared `scroll_offset` |
| `render_musicians` | `app.rs` (called from `render_orchestra_tab`) | Takes `OrchestraTabState` for per-musician scroll |
| `panels::render_plan_review` | Called from `render_plan_tab` | Takes `PlanTabState` for `plan_selected`, `plan_scroll` |
| `panels::render_task_detail` | Called from `render_plan_tab` (inline, not overlay) | Takes `task_detail_scroll` instead of shared `scroll_offset` |

---

## 5. Key Binding Design

### 5.1 Global Bindings (Active on All Tabs)

| Key | Action | Notes |
|-----|--------|-------|
| `Ctrl+C` | Quit | Always works, even during overlays |
| `q` | Quit | Only when not typing |
| `?` | Toggle help overlay | |
| `1` | Switch to Orchestra tab | Only if visible in current phase |
| `2` | Switch to Plan tab | Only if visible |
| `3` | Switch to Stats tab | Only if visible |
| `4` | Switch to Diff tab | Only if visible |
| `5` | Switch to Log tab | Only if visible |
| `i` | Toggle insights panel | Sidebar toggle |
| `Enter` | Submit prompt / approve plan | Context-dependent |
| Text input | Type into prompt bar | Starts prompt mode |
| `Esc` | Clear prompt / close overlay / exit focus mode | Priority cascade |
| `Up/Down` (in prompt mode) | History navigation | |

### 5.2 Orchestra Tab Bindings

| Key | Action | Condition |
|-----|--------|-----------|
| `Tab` | Focus next musician | When musicians exist |
| `Shift+Tab` | Focus prev musician | When musicians exist |
| `Left/Right` | Focus prev/next musician | When musicians exist |
| `Up/Down` | Scroll focused musician output | Not in prompt mode |
| `f` | Toggle focus mode (expand musician) | |
| `j/k` | Scroll down/up (vim-style) | Not in prompt mode |

### 5.3 Plan Tab Bindings

| Key | Action | Condition |
|-----|--------|-----------|
| `Up/Down` or `j/k` | Navigate task list | Task list focused |
| `Enter` | Approve plan | PlanReview phase only |
| `d` | Toggle task detail view | |
| `Tab` | Switch focus: task list <-> refinement chat | |
| `Up/Down` (in refinement) | Scroll refinement history | Refinement focused |

### 5.4 Stats Tab Bindings

| Key | Action | Condition |
|-----|--------|-----------|
| `Up/Down` or `j/k` | Scroll (if content overflows) | |

### 5.5 Diff Tab Bindings

| Key | Action | Condition |
|-----|--------|-----------|
| `Up/Down` or `j/k` | Scroll through diff / move file selection | |
| `Enter` or `Space` | Toggle file expansion | On a file header |
| `e` | Expand all files | |
| `c` | Collapse all files | |
| `m` | Cycle musician filter (all -> M1 -> M2 -> ... -> all) | |
| `PageUp/PageDown` | Large scroll jumps | |

### 5.6 Log Tab Bindings

| Key | Action | Condition |
|-----|--------|-----------|
| `Up/Down` or `j/k` | Scroll log | Disables auto-scroll |
| `/` | Start typing a text filter | Opens filter input |
| `Esc` | Clear filter | When filter is active |
| `a` | Toggle auto-scroll | |
| `s` | Cycle source filter (all -> conductor -> M1 -> ...) | |
| `PageUp/PageDown` | Large scroll jumps | |
| `G` | Jump to bottom (re-enable auto-scroll) | |
| `g` | Jump to top | |

### 5.7 Overlay Interaction with Tabs

Overlays (help, session browser) take priority over tab-specific bindings. The key dispatch order:

```
Key Event
  1. Overlays consume first (help: Esc/q/?, sessions: Esc/Enter/j/k)
  2. ESC+char sequence detection (macOS Option key)
  3. Global: Ctrl+C quit
  4. Prompt mode (if typing -- consumes most keys)
  5. Global navigation (1-5 tab switch, ?, q, i)
  6. Tab-specific bindings (dispatched by ui.active_tab)
```

This mirrors the current dispatch order in `handle_key` (app.rs:230-601) but inserts tab switching at step 5 and tab-specific dispatch at step 6.

### 5.8 handle_key Tab Dispatch Sketch

The current monolithic `handle_key` splits into:

```rust
async fn handle_key(&self, key: KeyEvent, ui: &mut UiState, state: &OrchestraState) -> anyhow::Result<bool> {
    // 1. ESC+char sequence detection (unchanged from current)
    // 2. Ctrl+C quit (unchanged)
    // 3. Overlay dispatch (unchanged: help, sessions, task_detail for Plan tab)
    // 4. Alt+key word navigation (unchanged)
    // 5. Prompt mode (unchanged)

    // 6. Tab switching (NEW)
    if let KeyCode::Char(c @ '1'..='5') = key.code {
        if let Some(tab) = Tab::from_key(c) {
            if tab.is_visible(&state.phase) {
                ui.active_tab = tab;
                ui.needs_clear = true;
            }
        }
        return Ok(false);
    }

    // 7. Tab-specific navigation (NEW dispatch)
    match ui.active_tab {
        Tab::Orchestra => self.handle_orchestra_key(key, ui, state).await,
        Tab::Plan      => self.handle_plan_key(key, ui, state).await,
        Tab::Stats     => self.handle_stats_key(key, ui, state).await,
        Tab::Diff      => self.handle_diff_key(key, ui, state).await,
        Tab::Log       => self.handle_log_key(key, ui, state).await,
    }
}
```

Each `handle_*_key` method contains only the bindings for that tab, extracted from the current flat match block.

### 5.9 Updated is_navigation_key

```rust
fn is_navigation_key(key: &KeyEvent, active_tab: Tab) -> bool {
    // Global navigation keys (never start prompt mode)
    let global = matches!(
        key.code,
        KeyCode::Char('q')
            | KeyCode::Char('?')
            | KeyCode::Char('i')
            | KeyCode::Char('1')
            | KeyCode::Char('2')
            | KeyCode::Char('3')
            | KeyCode::Char('4')
            | KeyCode::Char('5')
    );

    // Tab-specific navigation keys
    let tab_specific = match active_tab {
        Tab::Orchestra => matches!(key.code, KeyCode::Char('f') | KeyCode::Char('j') | KeyCode::Char('k')),
        Tab::Plan      => matches!(key.code, KeyCode::Char('d') | KeyCode::Char('j') | KeyCode::Char('k')),
        Tab::Stats     => matches!(key.code, KeyCode::Char('j') | KeyCode::Char('k')),
        Tab::Diff      => matches!(key.code,
            KeyCode::Char('j') | KeyCode::Char('k') | KeyCode::Char('e')
            | KeyCode::Char('c') | KeyCode::Char('m')
        ),
        Tab::Log       => matches!(key.code,
            KeyCode::Char('j') | KeyCode::Char('k') | KeyCode::Char('a')
            | KeyCode::Char('s') | KeyCode::Char('G') | KeyCode::Char('g')
            | KeyCode::Char('/')
        ),
    };

    global || tab_specific
        || key.modifiers.contains(KeyModifiers::CONTROL)
        || key.modifiers.contains(KeyModifiers::ALT)
}
```

---

## 6. Edge Cases & Design Decisions

### 6.1 Terminal Resize During Tab Switch

When the terminal resizes, `layout_config` is recomputed (unchanged from current behavior). Tab-specific state (scroll offsets, selections) is not affected by resize -- the render functions clamp any out-of-bounds values at render time.

### 6.2 Event Log Growth

`event_log` grows unboundedly during long sessions. **Mitigation:** Cap at 10,000 entries in `orchestra.rs`, dropping oldest entries. The Log tab should show a "truncated" indicator when the cap is hit.

### 6.3 Diff Data Availability

`aggregated_diffs` is populated during merge phases. If the user switches to the Diff tab before any merges, show an empty state: "No changes merged yet." The tab is only *visible* after `PhaseMerging` starts (see Section 1.2).

### 6.4 Stats Data Availability

`stats` starts as `None`. The Stats tab renders "Waiting for execution data..." when `None`. Stats are populated as soon as the first musician completes a turn (via `ClaudeEvent::Result`).

### 6.5 Plan Tab After Execution Starts

The Plan tab remains accessible during and after execution. It shows the approved plan as read-only (task statuses update live). The refinement chat is only interactive during `PlanReview` phase; in other phases, it displays the history read-only.

---

## 7. Implementation Order

1. **Add `Tab` enum + `TabState` to `app.rs`** -- Pure additions, no behavior change yet
2. **Add new types to `conductor-types/src/state.rs`** -- `OrchestraStats`, `ConductorStats`, `FileDiff`, `LogEntry` + fields on `OrchestraState`
3. **Refactor `render_all()` -> tab dispatch** -- Insert `render_tab_bar`, replace `render_content` with `render_tab_content`
4. **Migrate scroll/selection state** -- Move fields from flat `UiState` into `TabState` sub-structs
5. **Add tab switching key bindings** -- `1-5` keys, auto-switch on phase transitions
6. **Implement Stats tab renderer** -- New `components/stats.rs`
7. **Implement Diff tab renderer** -- New `components/diff.rs`
8. **Implement Log tab renderer** -- New `components/log.rs`
9. **Wire up stats computation** in `conductor-core/src/orchestra.rs` and `conductor-core/src/conductor_agent.rs`
10. **Wire up diff aggregation** in `conductor-core/src/orchestra.rs`
11. **Wire up event log** in `conductor-core/src/orchestra.rs`

Steps 1-5 can be done as a single PR (tab infrastructure). Steps 6-8 are independent and can be parallelized. Steps 9-11 require backend changes and can be done after the TUI renders are in place (using placeholder/empty data).
