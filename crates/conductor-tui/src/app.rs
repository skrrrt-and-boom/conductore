//! TUI application — event loop, state management, and top-level layout composition.

use std::collections::HashSet;
use std::io::{self, Stdout};

use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand,
};
use tokio::sync::mpsc;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    prelude::CrosstermBackend,
    widgets::Clear,
    Frame, Terminal,
};

use conductor_core::{prompt_history, task_store::TaskStore};
use conductor_types::{extract_image_paths, OrchestraPhase, OrchestraState, SessionData, UserAction};

/// Available slash commands for autocomplete and highlighting.
pub const SLASH_COMMANDS: &[&str] = &[
    "/help",
    "/sessions",
    "/list",
    "/resume",
    "/quit",
    "/q",
];

use crate::{
    components::{analyst, conductor, header, input, insights, musician, panels, status},
    layout::{get_layout_config, LayoutConfig},
};

// ─── Tab System ──────────────────────────────────────────────────────────────

/// Which content fills the main area between header and prompt bars.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Tab {
    Orchestra, // 1 — Musician grid + task graph + insights
    Plan,      // 2 — Plan review, task list, refinement chat
    Stats,     // 3 — Token costs, timing, per-musician gauges
    Diff,      // 4 — Aggregated file diffs, per-musician attribution
    Log,       // 5 — Full event log, filterable
}

impl Tab {
    pub const ALL: &[Tab] = &[Tab::Orchestra, Tab::Plan, Tab::Stats, Tab::Diff, Tab::Log];

    pub fn label(&self) -> &'static str {
        match self {
            Tab::Orchestra => "Orchestra",
            Tab::Plan => "Plan",
            Tab::Stats => "Stats",
            Tab::Diff => "Diff",
            Tab::Log => "Log",
        }
    }

    pub fn key(&self) -> char {
        match self {
            Tab::Orchestra => '1',
            Tab::Plan => '2',
            Tab::Stats => '3',
            Tab::Diff => '4',
            Tab::Log => '5',
        }
    }

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

    pub fn is_visible(&self, phase: &OrchestraPhase) -> bool {
        match self {
            Tab::Orchestra => true,
            Tab::Log => true,
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

    /// Returns the tab to auto-switch to when entering the given phase, if any.
    pub fn auto_switch(phase: &OrchestraPhase) -> Option<Tab> {
        match phase {
            OrchestraPhase::PlanReview => Some(Tab::Plan),
            OrchestraPhase::PhaseExecuting | OrchestraPhase::Executing => Some(Tab::Orchestra),
            OrchestraPhase::PhaseMerging | OrchestraPhase::Integrating => Some(Tab::Diff),
            OrchestraPhase::FinalReview | OrchestraPhase::Complete => Some(Tab::Stats),
            OrchestraPhase::Failed => Some(Tab::Log),
            _ => None,
        }
    }
}

// ─── Per-Tab State ───────────────────────────────────────────────────────────

/// Per-tab state container — each tab owns its own scroll and selection state.
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
            diff: DiffTabState::new(),
            log: LogTabState::new(),
        }
    }
}

#[derive(Default)]
pub struct OrchestraTabState {
    pub focused_musician: usize,
    pub musician_scroll: Vec<u16>,
    pub focus_mode: bool,
    pub conductor_scroll: u16,
}

#[derive(Default)]
pub struct PlanTabState {
    pub plan_selected: usize,
    pub plan_scroll: u16,
    pub refinement_scroll: u16,
    pub refinement_focused: bool,
    pub task_detail: Option<usize>,
    pub task_detail_scroll: u16,
}

#[derive(Default)]
pub struct StatsTabState {
    pub scroll: u16,
}

pub struct DiffTabState {
    pub diff_scroll: u16,
    pub expanded_files: HashSet<usize>,
    pub selected_file: usize,
    pub musician_filter: Option<usize>,
}

impl DiffTabState {
    pub fn new() -> Self {
        Self {
            diff_scroll: 0,
            expanded_files: HashSet::new(),
            selected_file: 0,
            musician_filter: None,
        }
    }
}

pub struct LogTabState {
    pub log_scroll: u16,
    pub log_filter: Option<String>,
    pub auto_scroll: bool,
    pub source_filter: Option<LogSourceFilter>,
}

impl LogTabState {
    pub fn new() -> Self {
        Self {
            log_scroll: 0,
            log_filter: None,
            auto_scroll: true,
            source_filter: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogSourceFilter {
    Conductor,
    Musician(usize),
    System,
}

// ─── UI State ────────────────────────────────────────────────────────────────

/// Local UI state that doesn't leave the TUI. Orchestra doesn't see any of this.
pub struct UiState {
    /// Currently active tab.
    pub active_tab: Tab,
    /// Per-tab state (scroll offsets, selections, etc.).
    pub tab_state: TabState,
    /// Last known phase — used to detect phase transitions for auto-switch.
    pub last_phase: OrchestraPhase,
    /// Keyboard help overlay visible.
    pub show_help: bool,
    /// Session browser overlay visible.
    pub show_sessions: bool,
    /// Insights panel visible on the right.
    pub show_insights: bool,
    /// Current text in the prompt bar.
    pub prompt_input: String,
    /// Cursor position within prompt_input.
    pub prompt_cursor: usize,
    /// History of submitted prompts (oldest first).
    pub prompt_history: Vec<String>,
    /// Current position in history when cycling (None = not browsing history).
    pub history_index: Option<usize>,
    /// Stashed input before history browsing started.
    pub history_stash: String,
    /// Cached layout config from last terminal resize.
    pub layout_config: LayoutConfig,
    /// Selected row in session browser.
    pub session_selected: usize,
    /// Cached session list (loaded on toggle).
    pub sessions: Vec<SessionData>,
    /// Tracks whether the last key was Esc (for ESC+char sequences from Option keys).
    pub last_was_esc: bool,
    /// Force a full terminal clear on the next frame (after closing overlays).
    pub needs_clear: bool,
}

impl UiState {
    pub fn new(width: u16, height: u16) -> Self {
        Self {
            active_tab: Tab::Orchestra,
            tab_state: TabState::default(),
            last_phase: OrchestraPhase::Init,
            show_help: false,
            show_sessions: false,
            show_insights: true,
            prompt_input: String::new(),
            prompt_cursor: 0,
            prompt_history: Vec::new(),
            history_index: None,
            history_stash: String::new(),
            layout_config: get_layout_config(width, height),
            session_selected: 0,
            sessions: Vec::new(),
            last_was_esc: false,
            needs_clear: false,
        }
    }

    // Convenience accessors for backward compatibility during migration.

    /// Currently focused musician panel index.
    pub fn focused_panel(&self) -> usize {
        self.tab_state.orchestra.focused_musician
    }

    /// Scroll offset for the current context (orchestra conductor or musician).
    pub fn scroll_offset(&self) -> u16 {
        match self.active_tab {
            Tab::Orchestra => {
                let orch = &self.tab_state.orchestra;
                orch.musician_scroll
                    .get(orch.focused_musician)
                    .copied()
                    .unwrap_or(orch.conductor_scroll)
            }
            Tab::Plan => {
                if self.tab_state.plan.task_detail.is_some() {
                    self.tab_state.plan.task_detail_scroll
                } else {
                    self.tab_state.plan.plan_scroll
                }
            }
            Tab::Stats => self.tab_state.stats.scroll,
            Tab::Diff => self.tab_state.diff.diff_scroll,
            Tab::Log => self.tab_state.log.log_scroll,
        }
    }

    /// Selected plan task index.
    pub fn plan_selected(&self) -> usize {
        self.tab_state.plan.plan_selected
    }

    /// Whether focus mode (single-musician expanded view) is active.
    pub fn focus_mode(&self) -> bool {
        self.tab_state.orchestra.focus_mode
    }

    /// Task detail overlay index (now lives in Plan tab state).
    pub fn show_task_detail(&self) -> Option<usize> {
        self.tab_state.plan.task_detail
    }
}

// ─── TuiApp ──────────────────────────────────────────────────────────────────

/// The TUI application. Owns a watch receiver for orchestra state and a sender
/// for user actions. Runs the crossterm event loop on the current thread.
pub struct TuiApp {
    state_rx: tokio::sync::watch::Receiver<OrchestraState>,
    action_tx: tokio::sync::mpsc::Sender<UserAction>,
}

impl TuiApp {
    pub fn new(
        state_rx: tokio::sync::watch::Receiver<OrchestraState>,
        action_tx: tokio::sync::mpsc::Sender<UserAction>,
    ) -> Self {
        Self { state_rx, action_tx }
    }

    /// Run the TUI event loop. Blocks until the user quits or orchestra completes.
    pub async fn run(&mut self) -> anyhow::Result<()> {
        // Setup terminal
        enable_raw_mode()?;
        io::stdout().execute(EnterAlternateScreen)?;
        let backend = CrosstermBackend::new(io::stdout());
        let mut terminal = Terminal::new(backend)?;
        terminal.clear()?;

        let size = terminal.size()?;
        let mut ui = UiState::new(size.width, size.height);

        // Load persistent prompt history from disk
        if let Ok(history) = prompt_history::load_history().await {
            ui.prompt_history = history;
        }

        let result = self.event_loop(&mut terminal, &mut ui).await;

        // Restore terminal — always runs even on error
        disable_raw_mode()?;
        io::stdout().execute(LeaveAlternateScreen)?;
        terminal.show_cursor()?;

        result
    }

    async fn event_loop(
        &mut self,
        terminal: &mut Terminal<CrosstermBackend<Stdout>>,
        ui: &mut UiState,
    ) -> anyhow::Result<()> {
        // Spawn a dedicated keyboard reader task to avoid input starvation.
        // Without this, state_rx.changed() wins the select! race during active
        // execution (musicians produce output constantly), dropping keyboard events.
        let (term_tx, mut term_rx) = mpsc::channel::<Event>(64);
        let reader_handle = tokio::task::spawn_blocking(move || {
            loop {
                match event::read() {
                    Ok(evt) => {
                        if term_tx.blocking_send(evt).is_err() {
                            break; // receiver dropped, TUI shutting down
                        }
                    }
                    Err(_) => break,
                }
            }
        });

        let result = self.event_loop_inner(terminal, ui, &mut term_rx).await;

        reader_handle.abort();
        result
    }

    async fn event_loop_inner(
        &mut self,
        terminal: &mut Terminal<CrosstermBackend<Stdout>>,
        ui: &mut UiState,
        term_rx: &mut mpsc::Receiver<Event>,
    ) -> anyhow::Result<()> {
        loop {
            // Draw current state
            let state = self.state_rx.borrow().clone();

            // Clamp indices when musician/task counts shrink
            if !state.musicians.is_empty() {
                ui.tab_state.orchestra.focused_musician =
                    ui.tab_state.orchestra.focused_musician.min(state.musicians.len() - 1);
            } else {
                ui.tab_state.orchestra.focused_musician = 0;
            }
            if !state.tasks.is_empty() {
                ui.tab_state.plan.plan_selected =
                    ui.tab_state.plan.plan_selected.min(state.tasks.len() - 1);
            } else {
                ui.tab_state.plan.plan_selected = 0;
            }

            // Auto-switch tab on phase transitions
            if state.phase != ui.last_phase {
                if let Some(tab) = Tab::auto_switch(&state.phase) {
                    if tab.is_visible(&state.phase) {
                        ui.active_tab = tab;
                    }
                }
                ui.last_phase = state.phase.clone();
            }

            if ui.needs_clear {
                terminal.clear()?;
                ui.needs_clear = false;
            }
            terminal.draw(|f| render_all(f, &state, ui))?;

            // Wait for either a state change or terminal event.
            // Both channels are independent — neither can starve the other.
            tokio::select! {
                _ = self.state_rx.changed() => {
                    // New state available — loop will redraw
                    continue;
                }
                Some(evt) = term_rx.recv() => {
                    match evt {
                        Event::Key(key) => {
                            if self.handle_key(key, ui, &state).await? {
                                break; // quit requested
                            }
                        }
                        Event::Resize(w, h) => {
                            ui.layout_config = get_layout_config(w, h);
                            let _ = self.action_tx.send(UserAction::Resize { width: w, height: h }).await;
                        }
                        _ => {}
                    }
                }
            }
        }
        Ok(())
    }

    /// Handle a key event. Returns true if the app should quit.
    async fn handle_key(
        &self,
        key: KeyEvent,
        ui: &mut UiState,
        state: &OrchestraState,
    ) -> anyhow::Result<bool> {
        // Detect ESC+char sequences from macOS Option key.
        // Terminal sends Option+Left as ESC then 'b', Option+Right as ESC then 'f'.
        if ui.last_was_esc {
            ui.last_was_esc = false;
            match key.code {
                KeyCode::Char('b') => {
                    word_left(&mut ui.prompt_cursor, &ui.prompt_input);
                    return Ok(false);
                }
                KeyCode::Char('f') => {
                    word_right(&mut ui.prompt_cursor, &ui.prompt_input);
                    return Ok(false);
                }
                KeyCode::Char('d') => {
                    word_delete_forward(&mut ui.prompt_cursor, &mut ui.prompt_input);
                    return Ok(false);
                }
                _ => {
                    // Not a recognized ESC sequence — apply the original Esc action (clear input)
                    if !ui.prompt_input.is_empty() || ui.history_index.is_some() {
                        ui.prompt_input.clear();
                        ui.prompt_cursor = 0;
                        ui.history_index = None;
                        ui.history_stash.clear();
                    }
                    // Fall through to handle the current key normally
                }
            }
        }

        // Track Esc for ESC+char detection — only when no overlay is open
        if key.code == KeyCode::Esc
            && !ui.show_help
            && !ui.show_sessions
            && ui.tab_state.plan.task_detail.is_none()
        {
            ui.last_was_esc = true;
            return Ok(false);
        }

        // Ctrl+C always quits
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            let _ = self.action_tx.send(UserAction::Quit).await;
            return Ok(true);
        }

        // Overlays consume keys first
        if ui.show_help {
            if matches!(key.code, KeyCode::Esc | KeyCode::Char('?') | KeyCode::Char('q')) {
                ui.show_help = false;
                ui.needs_clear = true;
            }
            return Ok(false);
        }

        if ui.show_sessions {
            match key.code {
                KeyCode::Esc => {
                    ui.show_sessions = false;
                    ui.needs_clear = true;
                }
                KeyCode::Enter => {
                    if let Some(session) = ui.sessions.get(ui.session_selected) {
                        let session_id = session.id.clone();
                        ui.show_sessions = false;
                        ui.needs_clear = true;
                        let _ = self.action_tx.send(UserAction::ResumeSession { session_id }).await;
                    }
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    ui.session_selected = ui.session_selected.saturating_sub(1);
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    if ui.session_selected + 1 < ui.sessions.len() {
                        ui.session_selected += 1;
                    }
                }
                _ => {}
            }
            return Ok(false);
        }

        if ui.tab_state.plan.task_detail.is_some() {
            match key.code {
                KeyCode::Esc | KeyCode::Char('q') => {
                    ui.tab_state.plan.task_detail = None;
                    ui.needs_clear = true;
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    ui.tab_state.plan.task_detail_scroll =
                        ui.tab_state.plan.task_detail_scroll.saturating_sub(1);
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    ui.tab_state.plan.task_detail_scroll += 1;
                }
                _ => {}
            }
            return Ok(false);
        }

        // Alt+key word navigation (works regardless of prompt state)
        if key.modifiers.contains(KeyModifiers::ALT) {
            match key.code {
                KeyCode::Char('b') => {
                    word_left(&mut ui.prompt_cursor, &ui.prompt_input);
                    return Ok(false);
                }
                KeyCode::Char('f') => {
                    word_right(&mut ui.prompt_cursor, &ui.prompt_input);
                    return Ok(false);
                }
                KeyCode::Char('d') => {
                    word_delete_forward(&mut ui.prompt_cursor, &mut ui.prompt_input);
                    return Ok(false);
                }
                _ => {} // Other Alt combos fall through
            }
        }

        // Prompt input mode — when user is typing or browsing history
        // Up/Down only enter prompt mode if already typing or browsing history,
        // so that bare Up/Down scrolls the chat panel instead.
        // Exception: in Init phase, Up/Down always browse history (nothing to scroll).
        let in_prompt_mode = !ui.prompt_input.is_empty()
            || ui.history_index.is_some()
            || (matches!(key.code, KeyCode::Char(_)) && !is_navigation_key(&key))
            || (state.phase == OrchestraPhase::Init
                && matches!(key.code, KeyCode::Up | KeyCode::Down));
        if in_prompt_mode {
            match key.code {
                KeyCode::Enter => {
                    if !ui.prompt_input.is_empty() {
                        let text = std::mem::take(&mut ui.prompt_input);
                        ui.prompt_cursor = 0;
                        ui.history_index = None;
                        ui.history_stash.clear();

                        // Don't save slash commands to history
                        let trimmed = text.trim();
                        if !trimmed.starts_with('/') {
                            ui.prompt_history.push(text.clone());
                            // Persist to disk (fire-and-forget)
                            let text_clone = text.clone();
                            tokio::spawn(async move {
                                let _ = prompt_history::save_prompt(&text_clone).await;
                            });
                        }

                        match trimmed {
                            "/help" | "/?" => {
                                ui.show_help = !ui.show_help;
                            }
                            "/sessions" | "/list" => {
                                if let Ok(sessions) = TaskStore::list_sessions().await {
                                    // Filter to real sessions (have a prompt / got past Init)
                                    ui.sessions = sessions
                                        .into_iter()
                                        .filter(|s| !s.config.task_description.trim().is_empty())
                                        .collect();
                                }
                                ui.show_sessions = true;
                                ui.session_selected = 0;
                            }
                            "/quit" | "/q" => {
                                let _ = self.action_tx.send(UserAction::Quit).await;
                                return Ok(true);
                            }
                            cmd if cmd.starts_with("/resume") => {
                                let arg = cmd.strip_prefix("/resume").unwrap().trim();
                                if arg.is_empty() {
                                    // Show sessions so user can pick one
                                    if let Ok(sessions) = TaskStore::list_sessions().await {
                                        ui.sessions = sessions
                                            .into_iter()
                                            .filter(|s| !s.config.task_description.trim().is_empty())
                                            .collect();
                                    }
                                    ui.show_sessions = true;
                                    ui.session_selected = 0;
                                } else if let Some(resolved) = TaskStore::resolve_id(arg).await {
                                    let _ = self.action_tx.send(UserAction::ResumeSession {
                                        session_id: resolved,
                                    }).await;
                                }
                            }
                            _ => {
                                self.submit_input(&text, state).await;
                            }
                        }
                    }
                }
                KeyCode::Up => {
                    if !ui.prompt_history.is_empty() {
                        let new_idx = match ui.history_index {
                            None => {
                                ui.history_stash = ui.prompt_input.clone();
                                ui.prompt_history.len() - 1
                            }
                            Some(0) => 0,
                            Some(i) => i - 1,
                        };
                        ui.history_index = Some(new_idx);
                        ui.prompt_input = ui.prompt_history[new_idx].clone();
                        ui.prompt_cursor = ui.prompt_input.len();
                    }
                }
                KeyCode::Down => {
                    if let Some(idx) = ui.history_index {
                        if idx + 1 < ui.prompt_history.len() {
                            let new_idx = idx + 1;
                            ui.history_index = Some(new_idx);
                            ui.prompt_input = ui.prompt_history[new_idx].clone();
                            ui.prompt_cursor = ui.prompt_input.len();
                        } else {
                            ui.history_index = None;
                            ui.prompt_input = std::mem::take(&mut ui.history_stash);
                            ui.prompt_cursor = ui.prompt_input.len();
                        }
                    }
                }
                KeyCode::Esc => {
                    // Exit prompt/history mode — clear input and reset history browsing
                    ui.prompt_input.clear();
                    ui.prompt_cursor = 0;
                    ui.history_index = None;
                    ui.history_stash.clear();
                }
                // Note: Esc is handled globally above (for ESC+char sequence detection)
                // Ctrl+W or Ctrl+Backspace: delete word backward
                KeyCode::Char('w') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    if ui.prompt_cursor > 0 {
                        let old_cursor = ui.prompt_cursor;
                        while ui.prompt_cursor > 0
                            && ui.prompt_input.as_bytes()[ui.prompt_cursor - 1] == b' '
                        {
                            ui.prompt_cursor -= 1;
                        }
                        while ui.prompt_cursor > 0
                            && ui.prompt_input.as_bytes()[ui.prompt_cursor - 1] != b' '
                        {
                            ui.prompt_cursor -= 1;
                        }
                        ui.prompt_input.drain(ui.prompt_cursor..old_cursor);
                    }
                }
                KeyCode::Backspace => {
                    if ui.prompt_cursor > 0 {
                        ui.prompt_cursor -= 1;
                        ui.prompt_input.remove(ui.prompt_cursor);
                    }
                }
                // Ctrl+A: jump to start of line
                KeyCode::Char('a') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    ui.prompt_cursor = 0;
                }
                // Ctrl+E: jump to end of line
                KeyCode::Char('e') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    ui.prompt_cursor = ui.prompt_input.len();
                }
                // Ctrl+U: clear line
                KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    ui.prompt_input.clear();
                    ui.prompt_cursor = 0;
                }
                KeyCode::Left => {
                    ui.prompt_cursor = ui.prompt_cursor.saturating_sub(1);
                }
                KeyCode::Right => {
                    if ui.prompt_cursor < ui.prompt_input.len() {
                        ui.prompt_cursor += 1;
                    }
                }
                KeyCode::Tab => {
                    // Autocomplete slash commands
                    if ui.prompt_input.starts_with('/') {
                        let input = ui.prompt_input.clone();
                        let prefix = input.split_whitespace().next().unwrap_or(&input);
                        let matches: Vec<&&str> = SLASH_COMMANDS
                            .iter()
                            .filter(|cmd| cmd.starts_with(prefix) && **cmd != prefix)
                            .collect();
                        if matches.len() == 1 {
                            let completed = matches[0].to_string();
                            // Replace the command prefix, keep any args
                            let rest = input.strip_prefix(prefix).unwrap_or("");
                            ui.prompt_input = if completed == "/resume" {
                                format!("{completed} {}", rest.trim_start())
                            } else {
                                format!("{completed}{rest}")
                            };
                            ui.prompt_cursor = ui.prompt_input.len();
                        }
                    }
                }
                KeyCode::Char(c) => {
                    ui.prompt_input.insert(ui.prompt_cursor, c);
                    ui.prompt_cursor += 1;
                    ui.history_index = None;
                }
                _ => {}
            }
            return Ok(false);
        }

        // Tab switching via number keys 1-5
        if let KeyCode::Char(c @ '1'..='5') = key.code {
            if let Some(tab) = Tab::from_key(c) {
                if tab.is_visible(&state.phase) {
                    ui.active_tab = tab;
                }
            }
            return Ok(false);
        }

        // Navigation keys (when not typing)
        match key.code {
            KeyCode::Char('q') => {
                let _ = self.action_tx.send(UserAction::Quit).await;
                return Ok(true);
            }
            KeyCode::Char('?') => ui.show_help = !ui.show_help,
            KeyCode::Tab => {
                // Tab/BackTab cycle musicians within Orchestra tab
                let _ = self.action_tx.send(UserAction::FocusNext).await;
                let count = state.musicians.len().max(1);
                let orch = &mut ui.tab_state.orchestra;
                orch.focused_musician = (orch.focused_musician + 1) % count;
            }
            KeyCode::BackTab => {
                let _ = self.action_tx.send(UserAction::FocusPrev).await;
                let count = state.musicians.len().max(1);
                let orch = &mut ui.tab_state.orchestra;
                orch.focused_musician = if orch.focused_musician == 0 {
                    count - 1
                } else {
                    orch.focused_musician - 1
                };
            }
            // Route arrow keys based on active tab
            KeyCode::Left | KeyCode::Right | KeyCode::Up | KeyCode::Down => {
                self.handle_arrow_key(key.code, ui, state).await;
            }
            KeyCode::Enter => {
                if state.phase == OrchestraPhase::PlanReview {
                    let _ = self.action_tx.send(UserAction::ApprovePlan).await;
                }
            }
            KeyCode::Char('d') if state.phase == OrchestraPhase::PlanReview => {
                ui.tab_state.plan.task_detail = Some(ui.tab_state.plan.plan_selected);
                ui.tab_state.plan.task_detail_scroll = 0;
            }
            KeyCode::Esc => {
                if ui.tab_state.orchestra.focus_mode {
                    ui.tab_state.orchestra.focus_mode = false;
                }
            }
            _ => {}
        }

        Ok(false)
    }

    /// Route arrow keys to the appropriate tab handler.
    async fn handle_arrow_key(
        &self,
        code: KeyCode,
        ui: &mut UiState,
        state: &OrchestraState,
    ) {
        match ui.active_tab {
            Tab::Orchestra => {
                let orch = &mut ui.tab_state.orchestra;
                match code {
                    KeyCode::Left => {
                        let count = state.musicians.len().max(1);
                        orch.focused_musician = if orch.focused_musician == 0 {
                            count - 1
                        } else {
                            orch.focused_musician - 1
                        };
                    }
                    KeyCode::Right => {
                        let count = state.musicians.len().max(1);
                        orch.focused_musician = (orch.focused_musician + 1) % count;
                    }
                    KeyCode::Up => {
                        orch.conductor_scroll = orch.conductor_scroll.saturating_sub(1);
                        let _ = self.action_tx.send(UserAction::ScrollUp).await;
                    }
                    KeyCode::Down => {
                        orch.conductor_scroll += 1;
                        let _ = self.action_tx.send(UserAction::ScrollDown).await;
                    }
                    _ => {}
                }
            }
            Tab::Plan => {
                let plan = &mut ui.tab_state.plan;
                match code {
                    KeyCode::Up => {
                        plan.plan_selected = plan.plan_selected.saturating_sub(1);
                    }
                    KeyCode::Down => {
                        if plan.plan_selected + 1 < state.tasks.len() {
                            plan.plan_selected += 1;
                        }
                    }
                    _ => {}
                }
            }
            Tab::Stats => {
                match code {
                    KeyCode::Up => {
                        ui.tab_state.stats.scroll =
                            ui.tab_state.stats.scroll.saturating_sub(1);
                    }
                    KeyCode::Down => {
                        ui.tab_state.stats.scroll += 1;
                    }
                    _ => {}
                }
            }
            Tab::Diff => {
                match code {
                    KeyCode::Up => {
                        ui.tab_state.diff.diff_scroll =
                            ui.tab_state.diff.diff_scroll.saturating_sub(1);
                    }
                    KeyCode::Down => {
                        ui.tab_state.diff.diff_scroll += 1;
                    }
                    _ => {}
                }
            }
            Tab::Log => {
                match code {
                    KeyCode::Up => {
                        ui.tab_state.log.auto_scroll = false;
                        ui.tab_state.log.log_scroll =
                            ui.tab_state.log.log_scroll.saturating_sub(1);
                    }
                    KeyCode::Down => {
                        ui.tab_state.log.log_scroll += 1;
                    }
                    _ => {}
                }
            }
        }
    }

    async fn submit_input(&self, text: &str, state: &OrchestraState) {
        let extracted = extract_image_paths(text);
        let images = if extracted.images.is_empty() {
            None
        } else {
            Some(extracted.images)
        };

        match state.phase {
            OrchestraPhase::Init => {
                let _ = self
                    .action_tx
                    .send(UserAction::SubmitTask {
                        text: extracted.text,
                        images,
                    })
                    .await;
            }
            OrchestraPhase::PlanReview => {
                let _ = self
                    .action_tx
                    .send(UserAction::RefinePlan {
                        text: extracted.text,
                        images,
                    })
                    .await;
            }
            _ => {
                let _ = self
                    .action_tx
                    .send(UserAction::SubmitGuidance {
                        text: extracted.text,
                        images,
                    })
                    .await;
            }
        }
    }
}

/// Returns true if the key is a navigation key that shouldn't start input mode.
fn is_navigation_key(key: &KeyEvent) -> bool {
    matches!(
        key.code,
        KeyCode::Char('q')
            | KeyCode::Char('?')
            | KeyCode::Char('d')
            | KeyCode::Char('j')
            | KeyCode::Char('k')
    ) || key.modifiers.contains(KeyModifiers::CONTROL)
      || key.modifiers.contains(KeyModifiers::ALT)
}

// ─── Word Navigation Helpers ─────────────────────────────────────────────────

fn word_left(cursor: &mut usize, input: &str) {
    let bytes = input.as_bytes();
    while *cursor > 0 && bytes.get(*cursor - 1) == Some(&b' ') {
        *cursor -= 1;
    }
    while *cursor > 0 && bytes.get(*cursor - 1) != Some(&b' ') {
        *cursor -= 1;
    }
}

fn word_right(cursor: &mut usize, input: &str) {
    let len = input.len();
    let bytes = input.as_bytes();
    while *cursor < len && bytes.get(*cursor) != Some(&b' ') {
        *cursor += 1;
    }
    while *cursor < len && bytes.get(*cursor) == Some(&b' ') {
        *cursor += 1;
    }
}

fn word_delete_forward(cursor: &mut usize, input: &mut String) {
    let len = input.len();
    let start = *cursor;
    let bytes = input.as_bytes();
    let mut end = start;
    while end < len && bytes.get(end) != Some(&b' ') {
        end += 1;
    }
    while end < len && bytes.get(end) == Some(&b' ') {
        end += 1;
    }
    input.drain(start..end);
}

/// Polls crossterm for an event with a 50ms timeout. Returns true if an event is ready.
/// NOTE: Kept for potential use in tests; the main event loop uses a dedicated reader task instead.
#[allow(dead_code)]
async fn poll_crossterm_event() -> bool {
    tokio::task::spawn_blocking(|| {
        event::poll(std::time::Duration::from_millis(50)).unwrap_or(false)
    })
    .await
    .unwrap_or(false)
}

// ─── Top-Level Rendering ─────────────────────────────────────────────────────

/// Render the entire TUI screen. Called every frame.
pub fn render_all(f: &mut Frame, state: &OrchestraState, ui: &UiState) {
    let size = f.area();

    // Main vertical layout: header(1) | content(flex) | status(1) | prompt(1)
    let main_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // header
            Constraint::Min(3),   // content
            Constraint::Length(1), // status line
            Constraint::Length(1), // prompt bar
        ])
        .split(size);

    // Header
    header::render_header(f, main_chunks[0], state, &ui.active_tab);

    // Content area — musicians + optional insights panel
    render_content(f, main_chunks[1], state, ui);

    // Status bar
    status::render_status_line(f, main_chunks[2], state);

    // Prompt bar
    input::render_prompt_bar(f, main_chunks[3], &ui.prompt_input, ui.prompt_cursor, &state.phase);

    // Overlays (rendered last, on top of everything)
    if ui.show_help {
        input::render_keyboard_help(f, size);
    }

    if ui.show_sessions {
        panels::render_session_browser(f, size, &ui.sessions, ui.session_selected);
    }

    if let Some(task_idx) = ui.tab_state.plan.task_detail {
        if let Some(task) = state.tasks.get(task_idx) {
            panels::render_task_detail(f, size, task, &state.tasks, ui.tab_state.plan.task_detail_scroll);
        }
    }
}

/// Render the main content area based on active tab.
fn render_content(f: &mut Frame, area: Rect, state: &OrchestraState, ui: &UiState) {
    match ui.active_tab {
        Tab::Orchestra => render_orchestra_tab(f, area, state, ui),
        Tab::Plan => render_plan_tab(f, area, state, ui),
        Tab::Stats => render_placeholder_tab(f, area, "Stats"),
        Tab::Diff => render_placeholder_tab(f, area, "Diff"),
        Tab::Log => render_placeholder_tab(f, area, "Log"),
    }
}

/// Render the Orchestra tab — the original content rendering.
fn render_orchestra_tab(f: &mut Frame, area: Rect, state: &OrchestraState, ui: &UiState) {
    // During PlanReview, show the plan review panel in Orchestra tab too
    if state.phase == OrchestraPhase::PlanReview {
        panels::render_plan_review(
            f,
            area,
            state.plan.as_ref(),
            &state.tasks,
            &state.refinement_history,
            ui.tab_state.plan.plan_selected,
        );
        return;
    }

    // Planning phases: show conductor output (no musicians exist yet)
    let is_planning = matches!(
        state.phase,
        OrchestraPhase::Init
            | OrchestraPhase::Planning
            | OrchestraPhase::Exploring
            | OrchestraPhase::Analyzing
            | OrchestraPhase::Decomposing
    ) && state.musicians.is_empty();

    if is_planning {
        render_planning_content(f, area, state, ui);
    } else if ui.layout_config.show_insights_panel && ui.show_insights {
        let content_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Min(20),
                Constraint::Length(ui.layout_config.insights_panel_width),
            ])
            .split(area);

        render_musicians(f, content_chunks[0], state, ui);
        insights::render_insight_panel(f, content_chunks[1], &state.insights);
    } else {
        render_musicians(f, area, state, ui);
    }
}

/// Render the Plan tab content.
fn render_plan_tab(f: &mut Frame, area: Rect, state: &OrchestraState, ui: &UiState) {
    // Reuse plan review renderer when plan data is available
    if state.plan.is_some() || !state.tasks.is_empty() {
        panels::render_plan_review(
            f,
            area,
            state.plan.as_ref(),
            &state.tasks,
            &state.refinement_history,
            ui.tab_state.plan.plan_selected,
        );
    } else {
        render_placeholder_tab(f, area, "Plan");
    }
}

/// Render a placeholder empty state for tabs not yet implemented.
fn render_placeholder_tab(f: &mut Frame, area: Rect, label: &str) {
    use ratatui::widgets::{Block, Borders, Paragraph};
    use ratatui::style::{Color, Style};

    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!(" {label} "))
        .border_style(Style::default().fg(Color::DarkGray));
    let text = Paragraph::new(format!("  {label} tab — coming soon"))
        .style(Style::default().fg(Color::DarkGray))
        .block(block);
    f.render_widget(text, area);
}

/// Render content during planning phases (conductor output + optional analysts).
fn render_planning_content(f: &mut Frame, area: Rect, state: &OrchestraState, ui: &UiState) {
    let phase_label = format!("{:?}", state.phase);

    if ui.layout_config.show_insights_panel && ui.show_insights {
        let content_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Min(20),
                Constraint::Length(ui.layout_config.insights_panel_width),
            ])
            .split(area);

        render_planning_main(f, content_chunks[0], state, ui, &phase_label);
        insights::render_insight_panel(f, content_chunks[1], &state.insights);
    } else {
        render_planning_main(f, area, state, ui, &phase_label);
    }
}

/// Render the main area during planning: conductor output, with analyst grid below if analyzing.
fn render_planning_main(
    f: &mut Frame,
    area: Rect,
    state: &OrchestraState,
    ui: &UiState,
    phase_label: &str,
) {
    if !state.analysts.is_empty() && state.phase == OrchestraPhase::Analyzing {
        // Split: conductor output on top, analyst grid below
        let conductor_height = (area.height / 3).max(5);
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(conductor_height), Constraint::Min(3)])
            .split(area);

        conductor::render_conductor_output(
            f,
            chunks[0],
            &state.conductor_output,
            phase_label,
            ui.tab_state.orchestra.conductor_scroll,
        );
        analyst::render_analyst_grid(f, chunks[1], &state.analysts, &ui.layout_config);
    } else {
        conductor::render_conductor_output(
            f,
            area,
            &state.conductor_output,
            phase_label,
            ui.tab_state.orchestra.conductor_scroll,
        );
    }
}

/// Render the musician grid or single focused musician.
fn render_musicians(f: &mut Frame, area: Rect, state: &OrchestraState, ui: &UiState) {
    if state.musicians.is_empty() {
        return;
    }

    // Task graph above musicians if there are tasks
    if !state.tasks.is_empty() && area.height > 10 {
        let graph_height = ((state.tasks.len() as u16) + 2).min(area.height / 3).max(3);
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(graph_height), Constraint::Min(3)])
            .split(area);

        insights::render_task_graph(f, chunks[0], &state.tasks);
        musician::render_musician_grid(
            f,
            chunks[1],
            &state.musicians,
            ui.focused_panel(),
            &ui.layout_config,
            ui.focus_mode(),
            ui.scroll_offset(),
        );
    } else {
        musician::render_musician_grid(
            f,
            area,
            &state.musicians,
            ui.focused_panel(),
            &ui.layout_config,
            ui.focus_mode(),
            ui.scroll_offset(),
        );
    }
}

/// Helper: create a centered rect for modal overlays.
pub fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

/// Helper: render a Clear widget to blank an area before drawing an overlay.
pub fn clear_area(f: &mut Frame, area: Rect) {
    f.render_widget(Clear, area);
}
