// ── Theme — color palette, status maps, and formatting helpers ───────────────
//
// Design: Structure recedes (dim gray). Content pops (white).
// Color is semantic (green/yellow/red) or brand (cyan, sparingly).
//
// Ported from: src/components/tui-utils.ts

use conductor_types::state::{MusicianStatus, OrchestraPhase, TaskStatus};
use ratatui::style::{Color, Style};

// ── Color Palette ─────────────────────────────────────────────────────────────

/// Borders, dividers — recede
pub const C_FRAME: Color = Color::DarkGray;
/// Primary content
pub const C_TEXT: Color = Color::White;
/// Secondary info, hints
pub const C_DIM: Color = Color::DarkGray;
/// Logo, prompt prefix — SPARINGLY
pub const C_BRAND: Color = Color::Cyan;
/// Running, success, completed
pub const C_ACTIVE: Color = Color::Green;
/// Ready, warning, planning
pub const C_READY: Color = Color::Yellow;
/// Failed, paused, rate limited
pub const C_ERROR: Color = Color::Red;
/// Data viz (sparklines, waveforms)
pub const C_INFO: Color = Color::Blue;
/// Musical theme touches
pub const C_ACCENT: Color = Color::Magenta;

// ── Phase Display ─────────────────────────────────────────────────────────────

pub struct PhaseDisplay {
    pub sym: &'static str,
    pub color: Color,
}

/// Map an OrchestraPhase to its display symbol and color.
pub fn phase_display(phase: &OrchestraPhase) -> PhaseDisplay {
    match phase {
        OrchestraPhase::Init => PhaseDisplay { sym: "○", color: C_DIM },
        OrchestraPhase::Planning => PhaseDisplay { sym: "◉", color: C_READY },
        OrchestraPhase::Exploring => PhaseDisplay { sym: "◉", color: C_READY },
        OrchestraPhase::Analyzing => PhaseDisplay { sym: "◉", color: C_INFO },
        OrchestraPhase::Decomposing => PhaseDisplay { sym: "◉", color: C_READY },
        OrchestraPhase::PlanReview => PhaseDisplay { sym: "◈", color: C_BRAND },
        OrchestraPhase::PhaseDetailing => PhaseDisplay { sym: "◉", color: C_INFO },
        OrchestraPhase::PhaseExecuting => PhaseDisplay { sym: "●", color: C_ACTIVE },
        OrchestraPhase::PhaseMerging => PhaseDisplay { sym: "◉", color: C_INFO },
        OrchestraPhase::PhaseReviewing => PhaseDisplay { sym: "◉", color: C_READY },
        OrchestraPhase::Executing => PhaseDisplay { sym: "●", color: C_ACTIVE },
        OrchestraPhase::Reviewing => PhaseDisplay { sym: "◉", color: C_READY },
        OrchestraPhase::FinalReview => PhaseDisplay { sym: "◉", color: C_READY },
        OrchestraPhase::Integrating => PhaseDisplay { sym: "◉", color: C_INFO },
        OrchestraPhase::Paused => PhaseDisplay { sym: "◎", color: C_ERROR },
        OrchestraPhase::Probing => PhaseDisplay { sym: "◎", color: C_ACCENT },
        OrchestraPhase::Complete => PhaseDisplay { sym: "✓", color: C_ACTIVE },
        OrchestraPhase::Failed => PhaseDisplay { sym: "✗", color: C_ERROR },
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
        MusicianStatus::Idle => StatusDisplay { color: C_DIM, label: "IDLE", dot: "○" },
        MusicianStatus::Running => StatusDisplay { color: C_ACTIVE, label: "ACTIVE", dot: "●" },
        MusicianStatus::Waiting => StatusDisplay { color: C_READY, label: "WAIT", dot: "◎" },
        MusicianStatus::Paused => StatusDisplay { color: C_ERROR, label: "PAUSE", dot: "◎" },
        MusicianStatus::Completed => StatusDisplay { color: C_ACTIVE, label: "DONE", dot: "✓" },
        MusicianStatus::Failed => StatusDisplay { color: C_ERROR, label: "FAIL", dot: "✗" },
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
        TaskStatus::InProgress => TaskViz { dot: "●", color: C_ACTIVE },
        TaskStatus::Completed => TaskViz { dot: "✓", color: C_ACTIVE },
        TaskStatus::Ready => TaskViz { dot: "◦", color: C_READY },
        TaskStatus::Queued => TaskViz { dot: "·", color: C_DIM },
        TaskStatus::Blocked => TaskViz { dot: "×", color: C_DIM },
        TaskStatus::Failed => TaskViz { dot: "✗", color: C_ERROR },
        TaskStatus::Review => TaskViz { dot: "◉", color: C_INFO },
        TaskStatus::Cancelled => TaskViz { dot: "·", color: C_DIM },
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
///   `[USER]` → Cyan (user input)
///   `>`      → DarkGray (tool/command output)
///   `ERROR`  → Red (errors)
///
/// For regular lines, recency fades from White (newest) to DarkGray (oldest).
/// `recency`: 0.0 (oldest visible) → 1.0 (most recent)
pub fn output_line_style(line: &str, recency: f64) -> Style {
    let color = if line.starts_with("[USER]") {
        Color::Cyan
    } else if line.starts_with('>') {
        Color::DarkGray
    } else if line.starts_with("ERROR") {
        Color::Red
    } else if recency > 0.7 {
        Color::White
    } else if recency > 0.3 {
        Color::Gray
    } else {
        Color::DarkGray
    };
    Style::default().fg(color)
}

// ── Focus Helpers ─────────────────────────────────────────────────────────────

pub const FOCUS_INDICATOR: &str = "▸";

/// Returns the border color for a panel based on its focus state.
/// Focused → Cyan (brand), unfocused → DarkGray.
pub fn focus_border_color(is_focused: bool) -> Color {
    if is_focused { C_BRAND } else { C_FRAME }
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
        assert_eq!(d.color, C_DIM);
    }

    #[test]
    fn phase_display_executing() {
        let d = phase_display(&OrchestraPhase::Executing);
        assert_eq!(d.sym, "●");
        assert_eq!(d.color, C_ACTIVE);
    }

    #[test]
    fn phase_display_complete() {
        let d = phase_display(&OrchestraPhase::Complete);
        assert_eq!(d.sym, "✓");
        assert_eq!(d.color, C_ACTIVE);
    }

    #[test]
    fn phase_display_failed() {
        let d = phase_display(&OrchestraPhase::Failed);
        assert_eq!(d.sym, "✗");
        assert_eq!(d.color, C_ERROR);
    }

    #[test]
    fn phase_display_plan_review() {
        let d = phase_display(&OrchestraPhase::PlanReview);
        assert_eq!(d.sym, "◈");
        assert_eq!(d.color, C_BRAND);
    }

    // status_display()

    #[test]
    fn status_display_idle() {
        let d = status_display(&MusicianStatus::Idle);
        assert_eq!(d.label, "IDLE");
        assert_eq!(d.dot, "○");
        assert_eq!(d.color, C_DIM);
    }

    #[test]
    fn status_display_running() {
        let d = status_display(&MusicianStatus::Running);
        assert_eq!(d.label, "ACTIVE");
        assert_eq!(d.dot, "●");
        assert_eq!(d.color, C_ACTIVE);
    }

    #[test]
    fn status_display_waiting() {
        let d = status_display(&MusicianStatus::Waiting);
        assert_eq!(d.label, "WAIT");
        assert_eq!(d.dot, "◎");
        assert_eq!(d.color, C_READY);
    }

    #[test]
    fn status_display_paused() {
        let d = status_display(&MusicianStatus::Paused);
        assert_eq!(d.label, "PAUSE");
        assert_eq!(d.dot, "◎");
        assert_eq!(d.color, C_ERROR);
    }

    #[test]
    fn status_display_completed() {
        let d = status_display(&MusicianStatus::Completed);
        assert_eq!(d.label, "DONE");
        assert_eq!(d.dot, "✓");
        assert_eq!(d.color, C_ACTIVE);
    }

    #[test]
    fn status_display_failed() {
        let d = status_display(&MusicianStatus::Failed);
        assert_eq!(d.label, "FAIL");
        assert_eq!(d.dot, "✗");
        assert_eq!(d.color, C_ERROR);
    }

    // output_line_style()

    #[test]
    fn output_line_style_user() {
        let s = output_line_style("[USER] hello", 0.0);
        assert_eq!(s.fg, Some(Color::Cyan));
    }

    #[test]
    fn output_line_style_tool() {
        let s = output_line_style("> some output", 1.0);
        assert_eq!(s.fg, Some(Color::DarkGray));
    }

    #[test]
    fn output_line_style_error() {
        let s = output_line_style("ERROR: something failed", 1.0);
        assert_eq!(s.fg, Some(Color::Red));
    }

    #[test]
    fn output_line_style_recency_high() {
        let s = output_line_style("normal line", 0.8);
        assert_eq!(s.fg, Some(Color::White));
    }

    #[test]
    fn output_line_style_recency_mid() {
        let s = output_line_style("normal line", 0.5);
        assert_eq!(s.fg, Some(Color::Gray));
    }

    #[test]
    fn output_line_style_recency_low() {
        let s = output_line_style("normal line", 0.1);
        assert_eq!(s.fg, Some(Color::DarkGray));
    }

    #[test]
    fn output_line_style_recency_boundary_high() {
        // exactly 0.7 is NOT > 0.7, so falls to mid
        let s = output_line_style("normal line", 0.7);
        assert_eq!(s.fg, Some(Color::Gray));
    }

    #[test]
    fn output_line_style_recency_boundary_mid() {
        // exactly 0.3 is NOT > 0.3, so falls to dim
        let s = output_line_style("normal line", 0.3);
        assert_eq!(s.fg, Some(Color::DarkGray));
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
        assert_eq!(focus_border_color(true), C_BRAND);
    }

    #[test]
    fn focus_border_color_unfocused() {
        assert_eq!(focus_border_color(false), C_FRAME);
    }
}
