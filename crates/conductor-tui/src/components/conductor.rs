//! Conductor output panel — shown during planning phases (Exploring, Analyzing, Decomposing).

use ratatui::{
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

use crate::theme::{self, THEME};

/// Render conductor output as a scrollable panel in the main content area.
pub fn render_conductor_output(
    f: &mut Frame,
    area: Rect,
    conductor_output: &[String],
    phase_label: &str,
    scroll_offset: u16,
) {
    let title = format!(" {} ", phase_label);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(THEME.accent))
        .title(Span::styled(title, Style::default().fg(THEME.accent)));

    let inner = block.inner(area);
    f.render_widget(block, area);

    if inner.height == 0 || inner.width < 4 {
        return;
    }

    if conductor_output.is_empty() {
        let empty = Paragraph::new(Span::styled(
            "Waiting for conductor...",
            Style::default().fg(THEME.text_muted),
        ));
        f.render_widget(empty, inner);
        return;
    }

    let total_lines = conductor_output.len();
    let lines: Vec<Line> = conductor_output
        .iter()
        .enumerate()
        .map(|(i, line)| {
            let recency = if total_lines <= 1 {
                1.0
            } else {
                i as f64 / (total_lines - 1) as f64
            };
            Line::from(Span::styled(
                theme::trunc(line, inner.width as usize),
                theme::output_line_style(line, recency),
            ))
        })
        .collect();

    // Auto-scroll to bottom when scroll_offset is 0 (default)
    let effective_scroll = if scroll_offset == 0 {
        total_lines.saturating_sub(inner.height as usize) as u16
    } else {
        scroll_offset
    };

    let content = Paragraph::new(lines).scroll((effective_scroll, 0));
    f.render_widget(content, inner);
}
