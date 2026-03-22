//! Bottom status line — phase indicator + context-dependent hints.

use ratatui::{
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};
use unicode_width::UnicodeWidthStr;

use conductor_types::{OrchestraPhase, OrchestraState};

use crate::theme::{self, C_DIM};

/// Render the single-row status bar at the bottom.
pub fn render_status_line(f: &mut Frame, area: Rect, state: &OrchestraState) {
    let phase_d = theme::phase_display(&state.phase);

    // Left side: phase indicator + session ID
    let mut left = vec![
        Span::styled(
            format!(" {} ", phase_d.sym),
            Style::default().fg(phase_d.color),
        ),
        Span::styled(
            format!("{:?}", state.phase),
            Style::default().fg(phase_d.color),
        ),
    ];

    if !state.config.session_id.is_empty() {
        left.push(Span::styled(
            format!(" [{}]", theme::trunc(&state.config.session_id, 8)),
            Style::default().fg(C_DIM),
        ));
    }

    // Right side: phase-specific hints
    let hint = match state.phase {
        OrchestraPhase::PlanReview => "[Enter] approve  [↑↓] tasks  type to refine  q quit",
        OrchestraPhase::PhaseExecuting | OrchestraPhase::Executing => {
            "Tab: switch  type: guidance  ?: help  q: quit"
        }
        OrchestraPhase::Paused => "paused — q quit",
        OrchestraPhase::Complete => "done — q quit",
        OrchestraPhase::Failed => "failed — q quit",
        _ => "?: help  q: quit",
    };

    // Build the line with spacing between left and right
    let left_len: usize = left.iter().map(|s| s.width()).sum();
    let right_len = UnicodeWidthStr::width(hint);
    let gap = (area.width as usize).saturating_sub(left_len + right_len + 1);

    left.push(Span::raw(" ".repeat(gap)));
    left.push(Span::styled(hint, Style::default().fg(C_DIM)));

    let line = Line::from(left);
    let bar = Paragraph::new(line);
    f.render_widget(bar, area);
}
