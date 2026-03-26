//! Musician grid and individual column rendering.
//!
//! Port of MusicianColumn.tsx + SplitDashboard.tsx.

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

use conductor_types::MusicianState;

use crate::{
    layout::{compute_column_widths, LayoutConfig},
    theme::{self, THEME},
};

/// Render the musician grid — splits area into columns, one per musician.
pub fn render_musician_grid(
    f: &mut Frame,
    area: Rect,
    musicians: &[MusicianState],
    focused_idx: usize,
    layout: &LayoutConfig,
    focus_mode: bool,
    scroll_offset: u16,
) {
    if musicians.is_empty() {
        return;
    }

    // Focus mode: render only the focused musician at full width
    if focus_mode {
        if let Some(m) = musicians.get(focused_idx) {
            render_musician_column(f, area, m, true, layout, scroll_offset);
        }
        return;
    }

    let widths = compute_column_widths(musicians, area.width, layout.min_column_width);
    let constraints: Vec<Constraint> = widths.iter().map(|&w| Constraint::Length(w)).collect();

    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(constraints)
        .split(area);

    for (i, m) in musicians.iter().enumerate() {
        if let Some(&col_area) = columns.get(i) {
            let is_focused = i == focused_idx;
            let offset = if is_focused { scroll_offset } else { 0 };
            render_musician_column(f, col_area, m, is_focused, layout, offset);
        }
    }

    // Show indicator if any musicians couldn't be rendered
    let hidden_count = musicians.len().saturating_sub(columns.len());
    if hidden_count > 0 {
        let badge = format!("+{hidden_count}");
        let badge_w = badge.len() as u16 + 1;
        if area.width >= badge_w {
            let badge_area = Rect::new(
                area.x + area.width - badge_w,
                area.y,
                badge_w,
                1,
            );
            f.render_widget(
                Paragraph::new(Span::styled(badge, Style::default().fg(THEME.text_muted))),
                badge_area,
            );
        }
    }
}

/// Render a single musician column with bordered block, output lines, and stats.
fn render_musician_column(
    f: &mut Frame,
    area: Rect,
    musician: &MusicianState,
    is_focused: bool,
    layout: &LayoutConfig,
    scroll_offset: u16,
) {
    let status_d = theme::status_display(&musician.status);
    let border_color = theme::focus_border_color(is_focused);

    // Title: "M1 ● opus" + task title if any
    let model = musician
        .current_task
        .as_ref()
        .and_then(|t| t.model.as_deref())
        .unwrap_or("idle");

    let task_title = musician
        .current_task
        .as_ref()
        .map(|t| theme::trunc(&t.title, layout.truncate_at.saturating_sub(15)))
        .unwrap_or_default();

    let title = format!(
        "M{} {} {}  {}",
        musician.index + 1,
        status_d.dot,
        model,
        task_title,
    );

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color))
        .title(Span::styled(
            theme::trunc(&title, area.width.saturating_sub(2) as usize),
            Style::default().fg(if is_focused { THEME.accent } else { THEME.text_primary }),
        ));

    let inner = block.inner(area);
    f.render_widget(block, area);

    if inner.height < 2 || inner.width < 4 {
        return;
    }

    // Reserve 1 row at bottom for stats
    let output_height = inner.height.saturating_sub(1);
    let output_area = Rect::new(inner.x, inner.y, inner.width, output_height);
    let stats_area = Rect::new(inner.x, inner.y + output_height, inner.width, 1);

    // Output lines
    let total_lines = musician.output_lines.len();
    let lines: Vec<Line> = musician
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

    // Auto-scroll to bottom when scroll_offset is 0 (default/no manual scroll)
    let effective_scroll = if scroll_offset == 0 {
        total_lines.saturating_sub(output_height as usize) as u16
    } else {
        scroll_offset
    };
    let output = Paragraph::new(lines).scroll((effective_scroll, 0));
    f.render_widget(output, output_area);

    // Stats line: elapsed
    let elapsed = theme::elapsed(musician.elapsed_ms);
    let stats = Line::from(Span::styled(elapsed, Style::default().fg(THEME.text_muted)));
    f.render_widget(Paragraph::new(stats), stats_area);
}
