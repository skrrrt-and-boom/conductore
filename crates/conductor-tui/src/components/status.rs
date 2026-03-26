//! Bottom status line — phase indicator + context-dependent hints.

use ratatui::{
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

use conductor_types::{OrchestraPhase, OrchestraState};

use crate::theme::{self, C_DIM, SEPARATOR_DOT, SURFACE};
use crate::widgets::render_key_hint;

/// Render the single-row status bar at the bottom.
pub fn render_status_line(f: &mut Frame, area: Rect, state: &OrchestraState) {
    let phase_d = theme::phase_display(&state.phase);

    // Left: phase symbol + phase name in phase color, then session ID in dim brackets
    let phase_name = format!("{:?}", state.phase);
    let mut left: Vec<Span<'static>> = vec![
        Span::raw(" "),
        Span::styled(phase_d.sym, Style::default().fg(phase_d.color)),
        Span::raw(" "),
        Span::styled(phase_name, Style::default().fg(phase_d.color)),
    ];

    if !state.config.session_id.is_empty() {
        left.push(Span::styled(
            format!(" [{}]", theme::trunc(&state.config.session_id, 8)),
            Style::default().fg(C_DIM),
        ));
    }

    // Right: context-sensitive hints with middle-dot separators
    let hints: &[(&str, &str)] = match state.phase {
        OrchestraPhase::PlanReview => {
            &[("⏎", "approve"), ("↑↓", "tasks"), ("?", "help"), ("q", "quit")]
        }
        OrchestraPhase::PhaseExecuting | OrchestraPhase::Executing => {
            &[("Tab", "switch"), ("?", "help"), ("q", "quit")]
        }
        OrchestraPhase::Paused | OrchestraPhase::Complete | OrchestraPhase::Failed => {
            &[("q", "quit")]
        }
        _ => &[("?", "help"), ("q", "quit")],
    };

    let mut right: Vec<Span<'static>> = Vec::new();
    for (i, (key, action)) in hints.iter().enumerate() {
        if i > 0 {
            right.push(Span::styled(
                format!(" {} ", SEPARATOR_DOT),
                Style::default().fg(C_DIM),
            ));
        }
        right.extend(render_key_hint(key, action));
    }
    right.push(Span::raw(" "));

    // Flexible spacing between left and right
    let left_len: usize = left.iter().map(|s| s.width()).sum();
    let right_len: usize = right.iter().map(|s| s.width()).sum();
    let gap = (area.width as usize).saturating_sub(left_len + right_len);

    left.push(Span::raw(" ".repeat(gap)));
    left.extend(right);

    let line = Line::from(left);
    let bar = Paragraph::new(line).style(Style::default().bg(SURFACE));
    f.render_widget(bar, area);
}
