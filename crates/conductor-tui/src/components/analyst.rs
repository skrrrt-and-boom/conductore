//! Analyst grid rendering — shown during the Analyzing phase.

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

use conductor_types::AnalystState;

use crate::{
    layout::LayoutConfig,
    theme::{self, THEME},
};

/// Render the analyst grid — one column per analyst.
pub fn render_analyst_grid(
    f: &mut Frame,
    area: Rect,
    analysts: &[AnalystState],
    layout: &LayoutConfig,
) {
    if analysts.is_empty() {
        return;
    }

    let col_count = analysts.len().min(area.width as usize / layout.min_column_width as usize).max(1);
    let col_width = area.width / col_count as u16;
    let constraints: Vec<Constraint> = (0..col_count)
        .map(|_| Constraint::Length(col_width))
        .collect();

    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(constraints)
        .split(area);

    for (i, analyst) in analysts.iter().enumerate().take(col_count) {
        if let Some(&col_area) = columns.get(i) {
            render_analyst_column(f, col_area, analyst, layout);
        }
    }
}

fn render_analyst_column(
    f: &mut Frame,
    area: Rect,
    analyst: &AnalystState,
    layout: &LayoutConfig,
) {
    let status_d = theme::status_display(&analyst.status);
    let area_label = analyst
        .directive
        .as_ref()
        .map(|d| d.area.as_str())
        .unwrap_or("idle");

    let title = format!(
        "A{} {} {}",
        analyst.index + 1,
        status_d.dot,
        area_label,
    );

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(THEME.accent))
        .title(Span::styled(
            theme::trunc(&title, area.width.saturating_sub(2) as usize),
            Style::default().fg(THEME.text_primary),
        ));

    let inner = block.inner(area);
    f.render_widget(block, area);

    if inner.height < 2 || inner.width < 4 {
        return;
    }

    // Reserve 1 row for stats
    let output_height = inner.height.saturating_sub(1);
    let output_area = Rect::new(inner.x, inner.y, inner.width, output_height);
    let stats_area = Rect::new(inner.x, inner.y + output_height, inner.width, 1);

    let total_lines = analyst.output_lines.len();
    let lines: Vec<Line> = analyst
        .output_lines
        .iter()
        .enumerate()
        .map(|(i, line)| {
            let recency = if total_lines <= 1 {
                1.0
            } else {
                i as f64 / (total_lines - 1) as f64
            };
            Line::from(Span::styled(
                theme::trunc(line, layout.truncate_at),
                theme::output_line_style(line, recency),
            ))
        })
        .collect();

    // Auto-scroll to bottom
    let scroll = total_lines.saturating_sub(output_height as usize) as u16;
    let output = Paragraph::new(lines).scroll((scroll, 0));
    f.render_widget(output, output_area);

    let elapsed = theme::elapsed(analyst.elapsed_ms);
    let stats = Line::from(Span::styled(elapsed, Style::default().fg(THEME.text_muted)));
    f.render_widget(Paragraph::new(stats), stats_area);
}
