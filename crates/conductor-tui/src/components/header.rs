//! Top status bar — phase info on left, tab indicators on right.
//!
//! Layout: `[phase dot + name + stats …] [flexible spacer] [tab bar]`
//!
//! Unicode-width is used so multi-byte symbols (●, ✓, …) measure correctly.

use unicode_width::UnicodeWidthStr;
use ratatui::{
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

use conductor_types::{OrchestraState, TaskStatus};

use crate::app::Tab;
use crate::theme::{self, THEME};

// ── Header rendering ──────────────────────────────────────────────────────────

/// Render the single-row header bar.
///
/// Left side: phase indicator dot · phase name · task progress · musician count
/// · elapsed time.
///
/// Right side: tab indicators for every tab that is visible in the current
/// phase, built with [`theme::tab_indicator`].  The active tab is highlighted
/// in accent; inactive visible tabs are dimmed.
pub fn render_header(f: &mut Frame, area: Rect, state: &OrchestraState, active_tab: &Tab) {
    let phase_d = theme::phase_display(&state.phase);

    // ── Left spans ────────────────────────────────────────────────────────────

    let done = state
        .tasks
        .iter()
        .filter(|t| t.status == TaskStatus::Completed)
        .count();
    let total = state.tasks.len();
    let musician_count = state.musicians.len();

    let mut left: Vec<Span> = vec![
        Span::styled(format!(" {} ", phase_d.sym), Style::default().fg(phase_d.color)),
        Span::styled(format!("{:?}", state.phase), Style::default().fg(phase_d.color)),
    ];

    if total > 0 {
        left.push(Span::raw("  "));
        left.push(Span::styled(
            format!("{done}/{total} tasks"),
            Style::default().fg(THEME.text_primary),
        ));
    }

    if musician_count > 0 {
        left.push(Span::raw("  "));
        left.push(Span::styled(
            format!("{musician_count} musicians"),
            Style::default().fg(THEME.text_primary),
        ));
    }

    if state.elapsed_ms > 0 {
        left.push(Span::raw("  "));
        left.push(Span::styled(
            theme::elapsed(state.elapsed_ms),
            Style::default().fg(THEME.text_primary),
        ));
    }

    if !state.insights.is_empty() {
        left.push(Span::raw("  "));
        left.push(Span::styled(
            format!("{} insights", state.insights.len()),
            Style::default().fg(THEME.accent),
        ));
    }

    // ── Right spans — tab bar ─────────────────────────────────────────────────

    let mut right: Vec<Span> = Vec::new();

    for tab in Tab::ALL {
        if !tab.is_visible(&state.phase) {
            continue;
        }
        if !right.is_empty() {
            right.push(Span::raw(" "));
        }
        let is_active = tab == active_tab;
        // tab_indicator returns Vec<Span<'_>>; empty when !is_visible (already guarded above).
        right.extend(theme::tab_indicator(tab.label(), tab.key(), is_active, true));
    }
    // Trailing single space keeps the rightmost tab off the edge.
    right.push(Span::raw(" "));

    // ── Flexible spacer ───────────────────────────────────────────────────────

    let left_width: usize = left.iter().map(|s| s.content.as_ref().width()).sum();
    let right_width: usize = right.iter().map(|s| s.content.as_ref().width()).sum();
    let total_cols = area.width as usize;
    let spacer = total_cols.saturating_sub(left_width + right_width);

    // ── Compose final line ────────────────────────────────────────────────────

    let mut spans = left;
    spans.push(Span::raw(" ".repeat(spacer)));
    spans.extend(right);

    f.render_widget(
        Paragraph::new(Line::from(spans)).style(Style::default().fg(THEME.text_muted)),
        area,
    );
}
