// ── Theme — color palette, status maps, and formatting helpers ───────────────
//
// Design: Structure recedes (near-invisible borders). Content pops (bright text).
// Accent is rare and meaningful — headings, focus, brand only.
//
// Inspired by Grok/x.ai minimalist dark aesthetic.
// Ported from: src/components/tui-utils.ts

use conductor_types::state::{MusicianStatus, OrchestraPhase, TaskStatus};
use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

// ── Theme Struct ─────────────────────────────────────────────────────────────

/// Centralized color theme for the TUI. All colors are true-color RGB.
pub struct Theme {
    /// Near-black background — warmer than pure black
    pub bg: Color,
    /// Slightly lighter surface for elevated areas
    pub surface: Color,
    /// Very subtle border/separator — recedes visually
    pub border: Color,
    /// Bright white primary text
    pub text_primary: Color,
    /// Medium gray secondary text
    pub text_secondary: Color,
    /// Dim gray muted text
    pub text_muted: Color,
    /// Cool blue-cyan accent — used sparingly (headings, focus, brand)
    pub accent: Color,
    /// Soft green success state
    pub success: Color,
    /// Soft red error state
    pub error: Color,
    /// Warm amber warning state
    pub warning: Color,
    /// Cards/panels that float above surface — one step elevated
    pub surface_elevated: Color,
    /// Dimmed accent for inactive tab indicators — subdued but recognizable
    pub accent_dim: Color,
    /// Slightly brighter border for focused panels — still subtle
    pub border_focus: Color,
    /// Section labels, slightly brighter than muted but not full secondary
    pub text_label: Color,
}

/// The default Grok-inspired dark theme.
pub const THEME: Theme = Theme {
    bg: Color::Rgb(10, 10, 10),
    surface: Color::Rgb(20, 20, 20),
    border: Color::Rgb(30, 30, 30),
    text_primary: Color::Rgb(232, 232, 232),
    text_secondary: Color::Rgb(136, 136, 136),
    text_muted: Color::Rgb(85, 85, 85),
    accent: Color::Rgb(74, 158, 255),
    success: Color::Rgb(52, 211, 153),
    error: Color::Rgb(248, 113, 113),
    warning: Color::Rgb(251, 191, 36),
    surface_elevated: Color::Rgb(25, 25, 25),
    accent_dim: Color::Rgb(45, 100, 180),
    border_focus: Color::Rgb(50, 50, 50),
    text_label: Color::Rgb(100, 100, 100),
};

// ── impl Theme — style methods ──────────────────────────────────────────────

impl Theme {
    /// Primary text style
    pub fn s_text(&self) -> Style {
        Style::default().fg(self.text_primary)
    }

    /// Muted/dim text style
    pub fn s_dim(&self) -> Style {
        Style::default().fg(self.text_muted)
    }

    /// Accent color style — use sparingly
    pub fn s_accent(&self) -> Style {
        Style::default().fg(self.accent)
    }

    /// Success/green state style
    pub fn s_success(&self) -> Style {
        Style::default().fg(self.success)
    }

    /// Error/red state style
    pub fn s_error(&self) -> Style {
        Style::default().fg(self.error)
    }

    /// Warning/amber state style
    pub fn s_warning(&self) -> Style {
        Style::default().fg(self.warning)
    }

    /// Heading style: accent color + bold
    pub fn s_heading(&self) -> Style {
        Style::default().fg(self.accent).add_modifier(Modifier::BOLD)
    }

    /// Separator style: border color for thin lines
    pub fn s_separator(&self) -> Style {
        Style::default().fg(self.border)
    }

    /// Label style: text_label color for section headers
    pub fn s_label(&self) -> Style {
        Style::default().fg(self.text_label)
    }

    /// Surface background style
    pub fn s_surface(&self) -> Style {
        Style::default().bg(self.surface)
    }

    /// Elevated surface background style — for cards/panels floating above surface
    pub fn s_surface_elevated(&self) -> Style {
        Style::default().bg(self.surface_elevated)
    }

    /// Active tab style: accent color + bold
    pub fn s_tab_active(&self) -> Style {
        Style::default().fg(self.accent).add_modifier(Modifier::BOLD)
    }

    /// Inactive tab style: muted text
    pub fn s_tab_inactive(&self) -> Style {
        Style::default().fg(self.text_muted)
    }

    /// Returns the display Style for a single output line based on its content
    /// and how recent it is in the visible output window.
    ///
    /// Special prefixes take priority over recency:
    ///   `[USER]` → accent (user input)
    ///   `>`      → text_muted (tool/command output)
    ///   `ERROR`  → error (errors)
    ///
    /// For regular lines, recency fades from text_primary (newest) to text_muted (oldest).
    /// `recency`: 0.0 (oldest visible) → 1.0 (most recent)
    pub fn output_line_style(&self, line: &str, recency: f64) -> Style {
        let color = if line.starts_with("[USER]") {
            self.accent
        } else if line.starts_with('>') {
            self.text_muted
        } else if line.starts_with("ERROR") {
            self.error
        } else if recency > 0.7 {
            self.text_primary
        } else if recency > 0.3 {
            self.text_secondary
        } else {
            self.text_muted
        };
        Style::default().fg(color)
    }

    /// Returns the border color for a panel based on its focus state.
    /// Focused → accent (blue-cyan), unfocused → border (near-invisible).
    pub fn focus_border_color(&self, is_focused: bool) -> Color {
        if is_focused { self.accent } else { self.border }
    }

    /// Returns styled spans for a tab indicator in `[1]Orchestra` format.
    ///
    /// - Active: accent color + bold
    /// - Visible but inactive: muted text
    /// - Not visible: empty vec (skip entirely)
    pub fn tab_indicator(&self, label: &str, key: char, is_active: bool, is_visible: bool) -> Vec<Span<'_>> {
        if !is_visible {
            return vec![];
        }
        let text = format!("[{}]{}", key, label);
        if is_active {
            vec![Span::styled(text, self.s_tab_active())]
        } else {
            vec![Span::styled(text, self.s_tab_inactive())]
        }
    }
}

// ── Semantic Color Constants (aliases to THEME fields) ──────────────────────

/// Near-black background — warmer than pure black
pub const BG: Color = THEME.bg;
/// Slightly lighter surface for elevated areas
pub const SURFACE: Color = THEME.surface;
/// Very subtle border/separator — recedes visually
pub const BORDER: Color = THEME.border;
/// Bright white primary text
pub const TEXT_PRIMARY: Color = THEME.text_primary;
/// Medium gray secondary text
pub const TEXT_SECONDARY: Color = THEME.text_secondary;
/// Dim gray muted text
pub const TEXT_MUTED: Color = THEME.text_muted;
/// Cool blue-cyan accent — used sparingly (headings, focus, brand)
pub const ACCENT: Color = THEME.accent;
/// Soft green success state
pub const SUCCESS: Color = THEME.success;
/// Soft red error state
pub const ERROR: Color = THEME.error;
/// Warm amber warning state
pub const WARNING: Color = THEME.warning;
/// Cards/panels that float above SURFACE — one step elevated
pub const SURFACE_ELEVATED: Color = THEME.surface_elevated;
/// Dimmed accent for inactive tab indicators — subdued but recognizable
pub const ACCENT_DIM: Color = THEME.accent_dim;
/// Slightly brighter border for focused panels — still subtle
pub const BORDER_FOCUS: Color = THEME.border_focus;
/// Section labels, slightly brighter than MUTED but not full secondary
pub const TEXT_LABEL: Color = THEME.text_label;


// ── Legacy Aliases (keep callers outside this file compiling) ─────────────────

/// Borders, dividers — recede
pub const C_FRAME: Color = THEME.border;
/// Primary content
pub const C_TEXT: Color = THEME.text_primary;
/// Secondary info, hints
pub const C_DIM: Color = THEME.text_muted;
/// Logo, prompt prefix — SPARINGLY
pub const C_BRAND: Color = THEME.accent;
/// Running, success, completed
pub const C_ACTIVE: Color = THEME.success;
/// Ready, warning, planning
pub const C_READY: Color = THEME.warning;
/// Failed, paused, rate limited
pub const C_ERROR: Color = THEME.error;
/// Data viz (sparklines, waveforms)
pub const C_INFO: Color = THEME.accent;
/// Musical theme touches
pub const C_ACCENT: Color = THEME.accent;

// ── Free Function Wrappers (delegate to THEME) ──────────────────────────────

/// Primary text style
pub fn s_text() -> Style {
    THEME.s_text()
}

/// Muted/dim text style
pub fn s_dim() -> Style {
    THEME.s_dim()
}

/// Accent color style — use sparingly
pub fn s_accent() -> Style {
    THEME.s_accent()
}

/// Success/green state style
pub fn s_success() -> Style {
    THEME.s_success()
}

/// Error/red state style
pub fn s_error() -> Style {
    THEME.s_error()
}

/// Warning/amber state style
pub fn s_warning() -> Style {
    THEME.s_warning()
}

/// Heading style: accent color + bold
pub fn s_heading() -> Style {
    THEME.s_heading()
}

/// Separator style: border color for thin lines
pub fn s_separator() -> Style {
    THEME.s_separator()
}

/// Label style: TEXT_LABEL color for section headers
pub fn s_label() -> Style {
    THEME.s_label()
}

/// Surface background style
pub fn s_surface() -> Style {
    THEME.s_surface()
}

/// Elevated surface background style — for cards/panels floating above SURFACE
pub fn s_surface_elevated() -> Style {
    THEME.s_surface_elevated()
}

/// Active tab style: accent color + bold
pub fn s_tab_active() -> Style {
    THEME.s_tab_active()
}

/// Inactive tab style: muted text
pub fn s_tab_inactive() -> Style {
    THEME.s_tab_inactive()
}

/// Returns the display Style for a single output line based on its content
/// and how recent it is in the visible output window.
pub fn output_line_style(line: &str, recency: f64) -> Style {
    THEME.output_line_style(line, recency)
}

/// Returns the border color for a panel based on its focus state.
pub fn focus_border_color(is_focused: bool) -> Color {
    THEME.focus_border_color(is_focused)
}

/// Returns styled spans for a tab indicator in `[1]Orchestra` format.
pub fn tab_indicator(label: &str, key: char, is_active: bool, is_visible: bool) -> Vec<Span<'_>> {
    THEME.tab_indicator(label, key, is_active, is_visible)
}

// ── Separator Helpers ─────────────────────────────────────────────────────────

/// Thin horizontal line character
pub const SEPARATOR_CHAR: &str = "─";

/// Subtle dot separator
pub const SEPARATOR_DOT: &str = "·";

/// Returns a string of SEPARATOR_CHAR repeated `width` times.
pub fn separator_line(width: usize) -> String {
    SEPARATOR_CHAR.repeat(width)
}

/// Renders a full-width thin horizontal line in BORDER color.
pub fn render_separator(f: &mut Frame, area: Rect) {
    let line = separator_line(area.width as usize);
    let paragraph = Paragraph::new(Line::from(Span::styled(line, s_separator())));
    f.render_widget(paragraph, area);
}

// ── Phase Display ─────────────────────────────────────────────────────────────

pub struct PhaseDisplay {
    pub sym: &'static str,
    pub color: Color,
}

/// Map an OrchestraPhase to its display symbol and color.
pub fn phase_display(phase: &OrchestraPhase) -> PhaseDisplay {
    match phase {
        OrchestraPhase::Init => PhaseDisplay { sym: "○", color: THEME.text_muted },
        OrchestraPhase::Planning => PhaseDisplay { sym: "◉", color: THEME.warning },
        OrchestraPhase::Exploring => PhaseDisplay { sym: "◉", color: THEME.warning },
        OrchestraPhase::Analyzing => PhaseDisplay { sym: "◉", color: THEME.accent },
        OrchestraPhase::Decomposing => PhaseDisplay { sym: "◉", color: THEME.warning },
        OrchestraPhase::PlanReview => PhaseDisplay { sym: "◈", color: THEME.accent },
        OrchestraPhase::PhaseDetailing => PhaseDisplay { sym: "◉", color: THEME.accent },
        OrchestraPhase::PhaseExecuting => PhaseDisplay { sym: "●", color: THEME.success },
        OrchestraPhase::PhaseMerging => PhaseDisplay { sym: "◉", color: THEME.accent },
        OrchestraPhase::PhaseReviewing => PhaseDisplay { sym: "◉", color: THEME.warning },
        OrchestraPhase::Executing => PhaseDisplay { sym: "●", color: THEME.success },
        OrchestraPhase::Reviewing => PhaseDisplay { sym: "◉", color: THEME.warning },
        OrchestraPhase::FinalReview => PhaseDisplay { sym: "◉", color: THEME.warning },
        OrchestraPhase::Integrating => PhaseDisplay { sym: "◉", color: THEME.accent },
        OrchestraPhase::Paused => PhaseDisplay { sym: "◎", color: THEME.error },
        OrchestraPhase::Probing => PhaseDisplay { sym: "◎", color: THEME.accent },
        OrchestraPhase::Complete => PhaseDisplay { sym: "✓", color: THEME.success },
        OrchestraPhase::Failed => PhaseDisplay { sym: "✗", color: THEME.error },
    }
}

// ── Status Display ────────────────────────────────────────────────────────────

pub struct StatusDisplay {
    pub color: Color,
    pub label: &'static str,
    pub dot: &'static str,
}

/// Map a MusicianStatus to its display color, label, and dot.
pub fn status_display(status: &MusicianStatus) -> StatusDisplay {
    match status {
        MusicianStatus::Idle => StatusDisplay { color: THEME.text_muted, label: "IDLE", dot: "○" },
        MusicianStatus::Running => StatusDisplay { color: THEME.success, label: "ACTIVE", dot: "●" },
        MusicianStatus::Waiting => StatusDisplay { color: THEME.warning, label: "WAIT", dot: "◎" },
        MusicianStatus::Paused => StatusDisplay { color: THEME.error, label: "PAUSE", dot: "◎" },
        MusicianStatus::Completed => StatusDisplay { color: THEME.success, label: "DONE", dot: "✓" },
        MusicianStatus::Failed => StatusDisplay { color: THEME.error, label: "FAIL", dot: "✗" },
    }
}

// ── Task Visualization ────────────────────────────────────────────────────────

pub struct TaskViz {
    pub dot: &'static str,
    pub color: Color,
}

/// Map a TaskStatus to its visual dot and color.
pub fn task_viz(status: &TaskStatus) -> TaskViz {
    match status {
        TaskStatus::InProgress => TaskViz { dot: "●", color: THEME.success },
        TaskStatus::Completed => TaskViz { dot: "✓", color: THEME.success },
        TaskStatus::Ready => TaskViz { dot: "◦", color: THEME.warning },
        TaskStatus::Queued => TaskViz { dot: "·", color: THEME.text_muted },
        TaskStatus::Blocked => TaskViz { dot: "×", color: THEME.text_muted },
        TaskStatus::Failed => TaskViz { dot: "✗", color: THEME.error },
        TaskStatus::Review => TaskViz { dot: "◉", color: THEME.accent },
        TaskStatus::Cancelled => TaskViz { dot: "·", color: THEME.text_muted },
    }
}

// ── Formatting ────────────────────────────────────────────────────────────────

/// Format a duration in milliseconds as a human-readable string.
/// Examples: "5s", "2m05s", "1h30m"
pub fn elapsed(ms: u64) -> String {
    let s = ms / 1000;
    let m = s / 60;
    if m >= 60 {
        format!("{}h{}m", m / 60, m % 60)
    } else if m > 0 {
        format!("{}m{:02}s", m, s % 60)
    } else {
        format!("{}s", s)
    }
}

/// Strip ANSI escape sequences and control characters from a string.
/// Preserves newlines and tabs; removes everything else that could corrupt TUI rendering.
pub fn strip_control_chars(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            // Skip ANSI escape sequence: ESC [ ... final_byte
            if chars.peek() == Some(&'[') {
                chars.next();
                for c2 in chars.by_ref() {
                    if c2.is_ascii_alphabetic() || c2 == '~' {
                        break;
                    }
                }
            }
        } else if c.is_control() && c != '\n' && c != '\t' {
            // Skip control characters
        } else {
            result.push(c);
        }
    }
    result
}

/// Truncate a string to `max` chars, appending '…' if truncated.
pub fn trunc(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
        out.push('…');
        out
    }
}

/// Map a slice of 0.0–1.0 floats to 0–7 u64 values for Ratatui's Sparkline widget.
pub fn sparkline_data(values: &[f64], width: usize) -> Vec<u64> {
    (0..width)
        .map(|i| {
            let vi = ((i as f64 / width as f64) * values.len() as f64) as usize;
            let vi = vi.min(values.len().saturating_sub(1));
            let v = values.get(vi).copied().unwrap_or(0.0).clamp(0.0, 1.0);
            (v * 7.0).floor() as u64
        })
        .collect()
}

/// Render a progress bar with '█' for filled and '░' for empty.
pub fn pbar(pct: f64, width: usize) -> String {
    let filled = (pct * width as f64).round() as usize;
    let filled = filled.min(width);
    "█".repeat(filled) + &"░".repeat(width - filled)
}

// ── Focus Helpers ─────────────────────────────────────────────────────────────

pub const FOCUS_INDICATOR: &str = "▸";

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use conductor_types::state::{MusicianStatus, OrchestraPhase};

    // elapsed()

    #[test]
    fn elapsed_zero() {
        assert_eq!(elapsed(0), "0s");
    }

    #[test]
    fn elapsed_seconds() {
        assert_eq!(elapsed(5000), "5s");
    }

    #[test]
    fn elapsed_minutes() {
        // 65000ms = 65s = 1m05s
        assert_eq!(elapsed(65000), "1m05s");
    }

    #[test]
    fn elapsed_hours() {
        // 3661000ms = 3661s = 61m01s = 1h1m
        assert_eq!(elapsed(3661000), "1h1m");
    }

    // trunc()

    #[test]
    fn trunc_short() {
        assert_eq!(trunc("hello", 10), "hello");
    }

    #[test]
    fn trunc_exact() {
        assert_eq!(trunc("hello", 5), "hello");
    }

    #[test]
    fn trunc_long() {
        assert_eq!(trunc("hello world", 8), "hello w…");
    }

    #[test]
    fn trunc_max_one() {
        assert_eq!(trunc("ab", 1), "…");
    }

    // phase_display()

    #[test]
    fn phase_display_init() {
        let d = phase_display(&OrchestraPhase::Init);
        assert_eq!(d.sym, "○");
        assert_eq!(d.color, TEXT_MUTED);
    }

    #[test]
    fn phase_display_executing() {
        let d = phase_display(&OrchestraPhase::Executing);
        assert_eq!(d.sym, "●");
        assert_eq!(d.color, SUCCESS);
    }

    #[test]
    fn phase_display_complete() {
        let d = phase_display(&OrchestraPhase::Complete);
        assert_eq!(d.sym, "✓");
        assert_eq!(d.color, SUCCESS);
    }

    #[test]
    fn phase_display_failed() {
        let d = phase_display(&OrchestraPhase::Failed);
        assert_eq!(d.sym, "✗");
        assert_eq!(d.color, ERROR);
    }

    #[test]
    fn phase_display_plan_review() {
        let d = phase_display(&OrchestraPhase::PlanReview);
        assert_eq!(d.sym, "◈");
        assert_eq!(d.color, ACCENT);
    }

    // status_display()

    #[test]
    fn status_display_idle() {
        let d = status_display(&MusicianStatus::Idle);
        assert_eq!(d.label, "IDLE");
        assert_eq!(d.dot, "○");
        assert_eq!(d.color, TEXT_MUTED);
    }

    #[test]
    fn status_display_running() {
        let d = status_display(&MusicianStatus::Running);
        assert_eq!(d.label, "ACTIVE");
        assert_eq!(d.dot, "●");
        assert_eq!(d.color, SUCCESS);
    }

    #[test]
    fn status_display_waiting() {
        let d = status_display(&MusicianStatus::Waiting);
        assert_eq!(d.label, "WAIT");
        assert_eq!(d.dot, "◎");
        assert_eq!(d.color, WARNING);
    }

    #[test]
    fn status_display_paused() {
        let d = status_display(&MusicianStatus::Paused);
        assert_eq!(d.label, "PAUSE");
        assert_eq!(d.dot, "◎");
        assert_eq!(d.color, ERROR);
    }

    #[test]
    fn status_display_completed() {
        let d = status_display(&MusicianStatus::Completed);
        assert_eq!(d.label, "DONE");
        assert_eq!(d.dot, "✓");
        assert_eq!(d.color, SUCCESS);
    }

    #[test]
    fn status_display_failed() {
        let d = status_display(&MusicianStatus::Failed);
        assert_eq!(d.label, "FAIL");
        assert_eq!(d.dot, "✗");
        assert_eq!(d.color, ERROR);
    }

    // output_line_style() — free function

    #[test]
    fn output_line_style_user() {
        let s = output_line_style("[USER] hello", 0.0);
        assert_eq!(s.fg, Some(ACCENT));
    }

    #[test]
    fn output_line_style_tool() {
        let s = output_line_style("> some output", 1.0);
        assert_eq!(s.fg, Some(TEXT_MUTED));
    }

    #[test]
    fn output_line_style_error() {
        let s = output_line_style("ERROR: something failed", 1.0);
        assert_eq!(s.fg, Some(ERROR));
    }

    #[test]
    fn output_line_style_recency_high() {
        let s = output_line_style("normal line", 0.8);
        assert_eq!(s.fg, Some(TEXT_PRIMARY));
    }

    #[test]
    fn output_line_style_recency_mid() {
        let s = output_line_style("normal line", 0.5);
        assert_eq!(s.fg, Some(TEXT_SECONDARY));
    }

    #[test]
    fn output_line_style_recency_low() {
        let s = output_line_style("normal line", 0.1);
        assert_eq!(s.fg, Some(TEXT_MUTED));
    }

    #[test]
    fn output_line_style_recency_boundary_high() {
        // exactly 0.7 is NOT > 0.7, so falls to mid
        let s = output_line_style("normal line", 0.7);
        assert_eq!(s.fg, Some(TEXT_SECONDARY));
    }

    #[test]
    fn output_line_style_recency_boundary_mid() {
        // exactly 0.3 is NOT > 0.3, so falls to dim
        let s = output_line_style("normal line", 0.3);
        assert_eq!(s.fg, Some(TEXT_MUTED));
    }

    // pbar()

    #[test]
    fn pbar_full() {
        assert_eq!(pbar(1.0, 5), "█████");
    }

    #[test]
    fn pbar_empty() {
        assert_eq!(pbar(0.0, 5), "░░░░░");
    }

    #[test]
    fn pbar_half() {
        assert_eq!(pbar(0.5, 4), "██░░");
    }

    // sparkline_data()

    #[test]
    fn sparkline_data_all_zero() {
        let data = sparkline_data(&[0.0, 0.0, 0.0], 3);
        assert_eq!(data, vec![0, 0, 0]);
    }

    #[test]
    fn sparkline_data_all_one() {
        let data = sparkline_data(&[1.0, 1.0, 1.0], 3);
        assert_eq!(data, vec![7, 7, 7]);
    }

    #[test]
    fn sparkline_data_half() {
        let data = sparkline_data(&[0.5], 1);
        // 0.5 * 7.0 = 3.5, floor = 3
        assert_eq!(data, vec![3]);
    }

    // focus_border_color() — free function

    #[test]
    fn focus_border_color_focused() {
        assert_eq!(focus_border_color(true), ACCENT);
    }

    #[test]
    fn focus_border_color_unfocused() {
        assert_eq!(focus_border_color(false), BORDER);
    }

    // style helpers — free functions

    #[test]
    fn s_text_returns_primary() {
        assert_eq!(s_text().fg, Some(TEXT_PRIMARY));
    }

    #[test]
    fn s_dim_returns_muted() {
        assert_eq!(s_dim().fg, Some(TEXT_MUTED));
    }

    #[test]
    fn s_accent_returns_accent() {
        assert_eq!(s_accent().fg, Some(ACCENT));
    }

    #[test]
    fn s_success_returns_success() {
        assert_eq!(s_success().fg, Some(SUCCESS));
    }

    #[test]
    fn s_error_returns_error() {
        assert_eq!(s_error().fg, Some(ERROR));
    }

    #[test]
    fn s_warning_returns_warning() {
        assert_eq!(s_warning().fg, Some(WARNING));
    }

    #[test]
    fn s_heading_has_accent_and_bold() {
        let style = s_heading();
        assert_eq!(style.fg, Some(ACCENT));
        assert!(style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn s_separator_returns_border() {
        assert_eq!(s_separator().fg, Some(BORDER));
    }

    // separator helpers

    #[test]
    fn separator_line_zero() {
        assert_eq!(separator_line(0), "");
    }

    #[test]
    fn separator_line_repeats() {
        // SEPARATOR_CHAR is "─" (3 bytes in UTF-8), repeat 3 times
        assert_eq!(separator_line(3), "───");
    }

    #[test]
    fn separator_char_is_thin_line() {
        assert_eq!(SEPARATOR_CHAR, "─");
    }

    #[test]
    fn separator_dot_is_middle_dot() {
        assert_eq!(SEPARATOR_DOT, "·");
    }

    // new style helpers

    #[test]
    fn s_label_returns_text_label() {
        assert_eq!(s_label().fg, Some(TEXT_LABEL));
    }

    #[test]
    fn s_surface_sets_bg() {
        assert_eq!(s_surface().bg, Some(SURFACE));
    }

    #[test]
    fn s_surface_elevated_sets_bg() {
        assert_eq!(s_surface_elevated().bg, Some(SURFACE_ELEVATED));
    }

    #[test]
    fn s_tab_active_has_accent_and_bold() {
        let style = s_tab_active();
        assert_eq!(style.fg, Some(ACCENT));
        assert!(style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn s_tab_inactive_returns_muted() {
        assert_eq!(s_tab_inactive().fg, Some(TEXT_MUTED));
    }

    // tab_indicator() — free function

    #[test]
    fn tab_indicator_invisible_returns_empty() {
        let spans = tab_indicator("Orchestra", '1', true, false);
        assert!(spans.is_empty());
    }

    #[test]
    fn tab_indicator_active_formats_correctly() {
        let spans = tab_indicator("Orchestra", '1', true, true);
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].content, "[1]Orchestra");
        assert_eq!(spans[0].style.fg, Some(ACCENT));
        assert!(spans[0].style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn tab_indicator_inactive_formats_correctly() {
        let spans = tab_indicator("Log", '2', false, true);
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].content, "[2]Log");
        assert_eq!(spans[0].style.fg, Some(TEXT_MUTED));
    }

    #[test]
    fn tab_indicator_inactive_is_not_bold() {
        let spans = tab_indicator("Log", '2', false, true);
        assert!(!spans[0].style.add_modifier.contains(Modifier::BOLD));
    }

    // new color constants

    #[test]
    fn surface_elevated_is_distinct_from_surface() {
        assert_ne!(SURFACE_ELEVATED, SURFACE);
    }

    #[test]
    fn accent_dim_is_distinct_from_accent() {
        assert_ne!(ACCENT_DIM, ACCENT);
    }

    #[test]
    fn border_focus_is_distinct_from_border() {
        assert_ne!(BORDER_FOCUS, BORDER);
    }

    #[test]
    fn text_label_is_distinct_from_muted_and_secondary() {
        assert_ne!(TEXT_LABEL, TEXT_MUTED);
        assert_ne!(TEXT_LABEL, TEXT_SECONDARY);
    }

    // legacy alias consistency

    #[test]
    fn legacy_c_brand_equals_accent() {
        assert_eq!(C_BRAND, ACCENT);
    }

    #[test]
    fn legacy_c_active_equals_success() {
        assert_eq!(C_ACTIVE, SUCCESS);
    }

    #[test]
    fn legacy_c_error_equals_error() {
        assert_eq!(C_ERROR, ERROR);
    }

    #[test]
    fn legacy_c_frame_equals_border() {
        assert_eq!(C_FRAME, BORDER);
    }

    // ── THEME method tests ──────────────────────────────────────────────────

    #[test]
    fn theme_s_text_returns_primary() {
        assert_eq!(THEME.s_text().fg, Some(TEXT_PRIMARY));
    }

    #[test]
    fn theme_s_dim_returns_muted() {
        assert_eq!(THEME.s_dim().fg, Some(TEXT_MUTED));
    }

    #[test]
    fn theme_s_accent_returns_accent() {
        assert_eq!(THEME.s_accent().fg, Some(ACCENT));
    }

    #[test]
    fn theme_s_success_returns_success() {
        assert_eq!(THEME.s_success().fg, Some(SUCCESS));
    }

    #[test]
    fn theme_s_error_returns_error() {
        assert_eq!(THEME.s_error().fg, Some(ERROR));
    }

    #[test]
    fn theme_s_warning_returns_warning() {
        assert_eq!(THEME.s_warning().fg, Some(WARNING));
    }

    #[test]
    fn theme_s_heading_has_accent_and_bold() {
        let style = THEME.s_heading();
        assert_eq!(style.fg, Some(ACCENT));
        assert!(style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn theme_s_separator_returns_border() {
        assert_eq!(THEME.s_separator().fg, Some(BORDER));
    }

    #[test]
    fn theme_s_label_returns_text_label() {
        assert_eq!(THEME.s_label().fg, Some(TEXT_LABEL));
    }

    #[test]
    fn theme_s_surface_sets_bg() {
        assert_eq!(THEME.s_surface().bg, Some(SURFACE));
    }

    #[test]
    fn theme_s_surface_elevated_sets_bg() {
        assert_eq!(THEME.s_surface_elevated().bg, Some(SURFACE_ELEVATED));
    }

    #[test]
    fn theme_s_tab_active_has_accent_and_bold() {
        let style = THEME.s_tab_active();
        assert_eq!(style.fg, Some(ACCENT));
        assert!(style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn theme_s_tab_inactive_returns_muted() {
        assert_eq!(THEME.s_tab_inactive().fg, Some(TEXT_MUTED));
    }

    #[test]
    fn theme_output_line_style_user() {
        let s = THEME.output_line_style("[USER] hello", 0.0);
        assert_eq!(s.fg, Some(ACCENT));
    }

    #[test]
    fn theme_output_line_style_error() {
        let s = THEME.output_line_style("ERROR: something failed", 1.0);
        assert_eq!(s.fg, Some(ERROR));
    }

    #[test]
    fn theme_output_line_style_recency_high() {
        let s = THEME.output_line_style("normal line", 0.8);
        assert_eq!(s.fg, Some(TEXT_PRIMARY));
    }

    #[test]
    fn theme_focus_border_color_focused() {
        assert_eq!(THEME.focus_border_color(true), ACCENT);
    }

    #[test]
    fn theme_focus_border_color_unfocused() {
        assert_eq!(THEME.focus_border_color(false), BORDER);
    }

    #[test]
    fn theme_tab_indicator_active() {
        let spans = THEME.tab_indicator("Test", '1', true, true);
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].content, "[1]Test");
        assert_eq!(spans[0].style.fg, Some(ACCENT));
    }

    #[test]
    fn theme_tab_indicator_invisible() {
        let spans = THEME.tab_indicator("Test", '1', true, false);
        assert!(spans.is_empty());
    }

    // Verify THEME fields match semantic constants

    #[test]
    fn theme_fields_match_constants() {
        assert_eq!(THEME.bg, BG);
        assert_eq!(THEME.surface, SURFACE);
        assert_eq!(THEME.border, BORDER);
        assert_eq!(THEME.text_primary, TEXT_PRIMARY);
        assert_eq!(THEME.text_secondary, TEXT_SECONDARY);
        assert_eq!(THEME.text_muted, TEXT_MUTED);
        assert_eq!(THEME.accent, ACCENT);
        assert_eq!(THEME.success, SUCCESS);
        assert_eq!(THEME.error, ERROR);
        assert_eq!(THEME.warning, WARNING);
        assert_eq!(THEME.surface_elevated, SURFACE_ELEVATED);
        assert_eq!(THEME.accent_dim, ACCENT_DIM);
        assert_eq!(THEME.border_focus, BORDER_FOCUS);
        assert_eq!(THEME.text_label, TEXT_LABEL);
    }
}
