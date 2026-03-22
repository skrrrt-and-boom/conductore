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

// ── Color Palette (true-color RGB) ────────────────────────────────────────────

/// Near-black background — warmer than pure black
pub const BG: Color = Color::Rgb(10, 10, 10);
/// Slightly lighter surface for elevated areas
pub const SURFACE: Color = Color::Rgb(20, 20, 20);
/// Very subtle border/separator — recedes visually
pub const BORDER: Color = Color::Rgb(30, 30, 30);
/// Bright white primary text
pub const TEXT_PRIMARY: Color = Color::Rgb(232, 232, 232);
/// Medium gray secondary text
pub const TEXT_SECONDARY: Color = Color::Rgb(136, 136, 136);
/// Dim gray muted text
pub const TEXT_MUTED: Color = Color::Rgb(85, 85, 85);
/// Cool blue-cyan accent — used sparingly (headings, focus, brand)
pub const ACCENT: Color = Color::Rgb(74, 158, 255);
/// Soft green success state
pub const SUCCESS: Color = Color::Rgb(52, 211, 153);
/// Soft red error state
pub const ERROR: Color = Color::Rgb(248, 113, 113);
/// Warm amber warning state
pub const WARNING: Color = Color::Rgb(251, 191, 36);

// ── Legacy Aliases (keep callers outside this file compiling) ─────────────────

/// Borders, dividers — recede
pub const C_FRAME: Color = BORDER;
/// Primary content
pub const C_TEXT: Color = TEXT_PRIMARY;
/// Secondary info, hints
pub const C_DIM: Color = TEXT_MUTED;
/// Logo, prompt prefix — SPARINGLY
pub const C_BRAND: Color = ACCENT;
/// Running, success, completed
pub const C_ACTIVE: Color = SUCCESS;
/// Ready, warning, planning
pub const C_READY: Color = WARNING;
/// Failed, paused, rate limited
pub const C_ERROR: Color = ERROR;
/// Data viz (sparklines, waveforms)
pub const C_INFO: Color = ACCENT;
/// Musical theme touches
pub const C_ACCENT: Color = ACCENT;

// ── Style Helpers ─────────────────────────────────────────────────────────────

/// Primary text style
pub fn s_text() -> Style {
    Style::default().fg(TEXT_PRIMARY)
}

/// Muted/dim text style
pub fn s_dim() -> Style {
    Style::default().fg(TEXT_MUTED)
}

/// Accent color style — use sparingly
pub fn s_accent() -> Style {
    Style::default().fg(ACCENT)
}

/// Success/green state style
pub fn s_success() -> Style {
    Style::default().fg(SUCCESS)
}

/// Error/red state style
pub fn s_error() -> Style {
    Style::default().fg(ERROR)
}

/// Warning/amber state style
pub fn s_warning() -> Style {
    Style::default().fg(WARNING)
}

/// Heading style: accent color + bold
pub fn s_heading() -> Style {
    Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
}

/// Separator style: border color for thin lines
pub fn s_separator() -> Style {
    Style::default().fg(BORDER)
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
        OrchestraPhase::Init => PhaseDisplay { sym: "○", color: TEXT_MUTED },
        OrchestraPhase::Planning => PhaseDisplay { sym: "◉", color: WARNING },
        OrchestraPhase::Exploring => PhaseDisplay { sym: "◉", color: WARNING },
        OrchestraPhase::Analyzing => PhaseDisplay { sym: "◉", color: ACCENT },
        OrchestraPhase::Decomposing => PhaseDisplay { sym: "◉", color: WARNING },
        OrchestraPhase::PlanReview => PhaseDisplay { sym: "◈", color: ACCENT },
        OrchestraPhase::PhaseDetailing => PhaseDisplay { sym: "◉", color: ACCENT },
        OrchestraPhase::PhaseExecuting => PhaseDisplay { sym: "●", color: SUCCESS },
        OrchestraPhase::PhaseMerging => PhaseDisplay { sym: "◉", color: ACCENT },
        OrchestraPhase::PhaseReviewing => PhaseDisplay { sym: "◉", color: WARNING },
        OrchestraPhase::Executing => PhaseDisplay { sym: "●", color: SUCCESS },
        OrchestraPhase::Reviewing => PhaseDisplay { sym: "◉", color: WARNING },
        OrchestraPhase::FinalReview => PhaseDisplay { sym: "◉", color: WARNING },
        OrchestraPhase::Integrating => PhaseDisplay { sym: "◉", color: ACCENT },
        OrchestraPhase::Paused => PhaseDisplay { sym: "◎", color: ERROR },
        OrchestraPhase::Probing => PhaseDisplay { sym: "◎", color: ACCENT },
        OrchestraPhase::Complete => PhaseDisplay { sym: "✓", color: SUCCESS },
        OrchestraPhase::Failed => PhaseDisplay { sym: "✗", color: ERROR },
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
        MusicianStatus::Idle => StatusDisplay { color: TEXT_MUTED, label: "IDLE", dot: "○" },
        MusicianStatus::Running => StatusDisplay { color: SUCCESS, label: "ACTIVE", dot: "●" },
        MusicianStatus::Waiting => StatusDisplay { color: WARNING, label: "WAIT", dot: "◎" },
        MusicianStatus::Paused => StatusDisplay { color: ERROR, label: "PAUSE", dot: "◎" },
        MusicianStatus::Completed => StatusDisplay { color: SUCCESS, label: "DONE", dot: "✓" },
        MusicianStatus::Failed => StatusDisplay { color: ERROR, label: "FAIL", dot: "✗" },
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
        TaskStatus::InProgress => TaskViz { dot: "●", color: SUCCESS },
        TaskStatus::Completed => TaskViz { dot: "✓", color: SUCCESS },
        TaskStatus::Ready => TaskViz { dot: "◦", color: WARNING },
        TaskStatus::Queued => TaskViz { dot: "·", color: TEXT_MUTED },
        TaskStatus::Blocked => TaskViz { dot: "×", color: TEXT_MUTED },
        TaskStatus::Failed => TaskViz { dot: "✗", color: ERROR },
        TaskStatus::Review => TaskViz { dot: "◉", color: ACCENT },
        TaskStatus::Cancelled => TaskViz { dot: "·", color: TEXT_MUTED },
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

// ── Output Line Color ─────────────────────────────────────────────────────────

/// Returns the display Style for a single output line based on its content
/// and how recent it is in the visible output window.
///
/// Special prefixes take priority over recency:
///   `[USER]` → ACCENT (user input)
///   `>`      → TEXT_MUTED (tool/command output)
///   `ERROR`  → ERROR (errors)
///
/// For regular lines, recency fades from TEXT_PRIMARY (newest) to TEXT_MUTED (oldest).
/// `recency`: 0.0 (oldest visible) → 1.0 (most recent)
pub fn output_line_style(line: &str, recency: f64) -> Style {
    let color = if line.starts_with("[USER]") {
        ACCENT
    } else if line.starts_with('>') {
        TEXT_MUTED
    } else if line.starts_with("ERROR") {
        ERROR
    } else if recency > 0.7 {
        TEXT_PRIMARY
    } else if recency > 0.3 {
        TEXT_SECONDARY
    } else {
        TEXT_MUTED
    };
    Style::default().fg(color)
}

// ── Focus Helpers ─────────────────────────────────────────────────────────────

pub const FOCUS_INDICATOR: &str = "▸";

/// Returns the border color for a panel based on its focus state.
/// Focused → ACCENT (blue-cyan), unfocused → BORDER (near-invisible).
pub fn focus_border_color(is_focused: bool) -> Color {
    if is_focused { ACCENT } else { BORDER }
}

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

    // output_line_style()

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

    // focus_border_color()

    #[test]
    fn focus_border_color_focused() {
        assert_eq!(focus_border_color(true), ACCENT);
    }

    #[test]
    fn focus_border_color_unfocused() {
        assert_eq!(focus_border_color(false), BORDER);
    }

    // style helpers

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
}
