//! Top status bar — time, phase, task progress, musicians.

use ratatui::{
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

use conductor_types::{OrchestraState, TaskStatus};

use crate::theme::{self, C_BRAND, C_DIM, C_TEXT};

/// Render the single-row header bar.
pub fn render_header(f: &mut Frame, area: Rect, state: &OrchestraState) {
    let phase_d = theme::phase_display(&state.phase);

    // Task progress: completed/total
    let done = state
        .tasks
        .iter()
        .filter(|t| t.status == TaskStatus::Completed)
        .count();
    let total = state.tasks.len();

    // Musician model summary
    let musician_count = state.musicians.len();

    let mut spans = vec![
        // Phase indicator
        Span::styled(
            format!(" {} ", phase_d.sym),
            Style::default().fg(phase_d.color),
        ),
        Span::styled(
            format!("{:?}  ", state.phase),
            Style::default().fg(phase_d.color),
        ),
    ];

    // Task count (if any)
    if total > 0 {
        spans.push(Span::styled(
            format!("{done}/{total} tasks  "),
            Style::default().fg(C_TEXT),
        ));
    }

    // Musician count
    if musician_count > 0 {
        spans.push(Span::styled(
            format!("{musician_count} musicians  "),
            Style::default().fg(C_TEXT),
        ));
    }

    // Elapsed
    if state.elapsed_ms > 0 {
        spans.push(Span::styled(
            format!("{}  ", theme::elapsed(state.elapsed_ms)),
            Style::default().fg(C_TEXT),
        ));
    }

    // Insights badge
    if !state.insights.is_empty() {
        spans.push(Span::styled(
            format!("{} insights", state.insights.len()),
            Style::default().fg(C_BRAND),
        ));
    }

    let line = Line::from(spans);
    let header = Paragraph::new(line).style(Style::default().fg(C_DIM));
    f.render_widget(header, area);
}
