//! TUI application — event loop, state management, and top-level layout composition.

use std::io::{self, Stdout};

use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand,
};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    prelude::CrosstermBackend,
    widgets::Clear,
    Frame, Terminal,
};

use conductor_types::{OrchestraPhase, OrchestraState, SessionData, UserAction};

use crate::{
    components::{header, input, insights, musician, panels, status},
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
            focus_mode: false,
            layout_config: get_layout_config(width, height),
            plan_selected: 0,
            session_selected: 0,
            sessions: Vec::new(),
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
        loop {
            // Draw current state
            let state = self.state_rx.borrow().clone();
            terminal.draw(|f| render_all(f, &state, ui))?;

            // Check for terminal completion
            if matches!(
                state.phase,
                OrchestraPhase::Complete | OrchestraPhase::Failed
            ) {
                // Keep rendering but don't auto-quit — user can review
            }

            // Wait for either a state change or terminal event
            tokio::select! {
                _ = self.state_rx.changed() => {
                    // New state available — loop will redraw
                    continue;
                }
                ready = poll_crossterm_event() => {
                    if !ready {
                        continue;
                    }
                    if let Ok(Event::Key(key)) = event::read() {
                        if self.handle_key(key, ui, &state).await? {
                            break; // quit requested
                        }
                    } else if let Ok(Event::Resize(w, h)) = event::read() {
                        ui.layout_config = get_layout_config(w, h);
                        let _ = self.action_tx.send(UserAction::Resize { width: w, height: h }).await;
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

        // Prompt input mode — when user is typing
        if !ui.prompt_input.is_empty() || matches!(key.code, KeyCode::Char(_)) && !is_navigation_key(&key) {
            match key.code {
                KeyCode::Enter => {
                    if !ui.prompt_input.is_empty() {
                        let text = std::mem::take(&mut ui.prompt_input);
                        ui.prompt_cursor = 0;
                        self.submit_input(&text, state).await;
                    }
                }
                KeyCode::Esc => {
                    ui.prompt_input.clear();
                    ui.prompt_cursor = 0;
                }
                KeyCode::Backspace => {
                    if ui.prompt_cursor > 0 {
                        ui.prompt_cursor -= 1;
                        ui.prompt_input.remove(ui.prompt_cursor);
                    }
                }
                KeyCode::Left => {
                    ui.prompt_cursor = ui.prompt_cursor.saturating_sub(1);
                }
                KeyCode::Right => {
                    if ui.prompt_cursor < ui.prompt_input.len() {
                        ui.prompt_cursor += 1;
                    }
                }
                KeyCode::Char(c) => {
                    ui.prompt_input.insert(ui.prompt_cursor, c);
                    ui.prompt_cursor += 1;
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
        match state.phase {
            OrchestraPhase::PlanReview => {
                let _ = self.action_tx.send(UserAction::RefinePlan(text.to_string())).await;
            }
            _ => {
                let _ = self.action_tx.send(UserAction::SubmitGuidance(text.to_string())).await;
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
}

/// Polls crossterm for an event with a 50ms timeout. Returns true if an event is ready.
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

/// Render the main content area (musicians + insights).
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

    if ui.layout_config.show_insights_panel && ui.show_insights {
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
