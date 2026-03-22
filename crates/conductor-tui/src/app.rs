//! TUI application — event loop, state management, and top-level layout composition.

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

// ─── UI State ────────────────────────────────────────────────────────────────

/// Local UI state that doesn't leave the TUI. Orchestra doesn't see any of this.
pub struct UiState {
    /// Which musician column has focus (index into musicians vec).
    pub focused_panel: usize,
    /// Vertical scroll offset in the focused musician's output.
    pub scroll_offset: u16,
    /// Keyboard help overlay visible.
    pub show_help: bool,
    /// Session browser overlay visible.
    pub show_sessions: bool,
    /// Insights panel visible on the right.
    pub show_insights: bool,
    /// Task detail modal — Some(task_index) when open.
    pub show_task_detail: Option<usize>,
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
    /// Single-musician expanded view (full-width).
    pub focus_mode: bool,
    /// Cached layout config from last terminal resize.
    pub layout_config: LayoutConfig,
    /// Selected row in plan review task list.
    pub plan_selected: usize,
    /// Selected row in session browser.
    pub session_selected: usize,
    /// Cached session list (loaded on toggle).
    pub sessions: Vec<SessionData>,
    /// Tracks whether the last key was Esc (for ESC+char sequences from Option keys).
    pub last_was_esc: bool,
}

impl UiState {
    pub fn new(width: u16, height: u16) -> Self {
        Self {
            focused_panel: 0,
            scroll_offset: 0,
            show_help: false,
            show_sessions: false,
            show_insights: true,
            show_task_detail: None,
            prompt_input: String::new(),
            prompt_cursor: 0,
            prompt_history: Vec::new(),
            history_index: None,
            history_stash: String::new(),
            focus_mode: false,
            layout_config: get_layout_config(width, height),
            plan_selected: 0,
            session_selected: 0,
            sessions: Vec::new(),
            last_was_esc: false,
        }
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
                ui.focused_panel = ui.focused_panel.min(state.musicians.len() - 1);
            } else {
                ui.focused_panel = 0;
            }
            if !state.tasks.is_empty() {
                ui.plan_selected = ui.plan_selected.min(state.tasks.len() - 1);
            } else {
                ui.plan_selected = 0;
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
            && ui.show_task_detail.is_none()
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
            }
            return Ok(false);
        }

        if ui.show_sessions {
            match key.code {
                KeyCode::Esc => ui.show_sessions = false,
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

        if ui.show_task_detail.is_some() {
            match key.code {
                KeyCode::Esc | KeyCode::Char('q') => ui.show_task_detail = None,
                KeyCode::Up | KeyCode::Char('k') => {
                    ui.scroll_offset = ui.scroll_offset.saturating_sub(1);
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    ui.scroll_offset += 1;
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
        let in_prompt_mode = !ui.prompt_input.is_empty()
            || ui.history_index.is_some()
            || (matches!(key.code, KeyCode::Char(_)) && !is_navigation_key(&key))
            || (key.code == KeyCode::Up && !ui.prompt_history.is_empty());
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
                                    ui.sessions = sessions;
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
                                        ui.sessions = sessions;
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

        // Navigation keys (when not typing)
        match key.code {
            KeyCode::Char('q') => {
                let _ = self.action_tx.send(UserAction::Quit).await;
                return Ok(true);
            }
            KeyCode::Char('?') => ui.show_help = !ui.show_help,
            KeyCode::Tab => {
                let _ = self.action_tx.send(UserAction::FocusNext).await;
                let count = state.musicians.len().max(1);
                ui.focused_panel = (ui.focused_panel + 1) % count;
                ui.scroll_offset = 0;
            }
            KeyCode::BackTab => {
                let _ = self.action_tx.send(UserAction::FocusPrev).await;
                let count = state.musicians.len().max(1);
                ui.focused_panel = if ui.focused_panel == 0 { count - 1 } else { ui.focused_panel - 1 };
                ui.scroll_offset = 0;
            }
            KeyCode::Left => {
                let count = state.musicians.len().max(1);
                ui.focused_panel = if ui.focused_panel == 0 { count - 1 } else { ui.focused_panel - 1 };
                ui.scroll_offset = 0;
            }
            KeyCode::Right => {
                let count = state.musicians.len().max(1);
                ui.focused_panel = (ui.focused_panel + 1) % count;
                ui.scroll_offset = 0;
            }
            KeyCode::Up => {
                if state.phase == OrchestraPhase::PlanReview {
                    ui.plan_selected = ui.plan_selected.saturating_sub(1);
                } else {
                    ui.scroll_offset = ui.scroll_offset.saturating_sub(1);
                    let _ = self.action_tx.send(UserAction::ScrollUp).await;
                }
            }
            KeyCode::Down => {
                if state.phase == OrchestraPhase::PlanReview {
                    if ui.plan_selected + 1 < state.tasks.len() {
                        ui.plan_selected += 1;
                    }
                } else {
                    ui.scroll_offset += 1;
                    let _ = self.action_tx.send(UserAction::ScrollDown).await;
                }
            }
            KeyCode::Enter => {
                if state.phase == OrchestraPhase::PlanReview {
                    let _ = self.action_tx.send(UserAction::ApprovePlan).await;
                }
            }
            KeyCode::Char('d') if state.phase == OrchestraPhase::PlanReview => {
                ui.show_task_detail = Some(ui.plan_selected);
                ui.scroll_offset = 0;
            }
            KeyCode::Esc => {
                if ui.focus_mode {
                    ui.focus_mode = false;
                }
            }
            _ => {}
        }

        Ok(false)
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
    header::render_header(f, main_chunks[0], state);

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

    if let Some(task_idx) = ui.show_task_detail {
        if let Some(task) = state.tasks.get(task_idx) {
            panels::render_task_detail(f, size, task, &state.tasks, ui.scroll_offset);
        }
    }
}

/// Render the main content area based on current phase.
fn render_content(f: &mut Frame, area: Rect, state: &OrchestraState, ui: &UiState) {
    // During PlanReview, show the plan review panel instead of musicians
    if state.phase == OrchestraPhase::PlanReview {
        panels::render_plan_review(
            f,
            area,
            state.plan.as_ref(),
            &state.tasks,
            &state.refinement_history,
            ui.plan_selected,
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
        // Horizontal split: musicians | insights
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
            ui.scroll_offset,
        );
        analyst::render_analyst_grid(f, chunks[1], &state.analysts, &ui.layout_config);
    } else {
        conductor::render_conductor_output(
            f,
            area,
            &state.conductor_output,
            phase_label,
            ui.scroll_offset,
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
            ui.focused_panel,
            &ui.layout_config,
            ui.focus_mode,
            ui.scroll_offset,
        );
    } else {
        musician::render_musician_grid(
            f,
            area,
            &state.musicians,
            ui.focused_panel,
            &ui.layout_config,
            ui.focus_mode,
            ui.scroll_offset,
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
