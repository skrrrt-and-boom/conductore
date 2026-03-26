//! Musician grid and individual column rendering.
//!
//! Port of MusicianColumn.tsx + SplitDashboard.tsx.

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

use conductor_types::MusicianState;

use crate::{
    layout::{compute_column_widths, LayoutConfig},
    theme,
    widgets::{render_card, render_empty_state, render_inline_kv, render_section_header, render_status_dot},
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

    // Collapsed musicians that couldn't fit: show a status dot strip on the right edge
    let rendered_count = columns.len();
    for m in musicians.iter().skip(rendered_count) {
        let status_d = theme::status_display(&m.status);
        let _dot = render_status_dot(status_d.color);
        // No visible space to render — dots are informational only in the log
    }
}

/// Render a single musician column using borderless card with accent strip focus.
fn render_musician_column(
    f: &mut Frame,
    area: Rect,
    musician: &MusicianState,
    is_focused: bool,
    layout: &LayoutConfig,
    scroll_offset: u16,
) {
    let status_d = theme::status_display(&musician.status);

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

    let header_title = if task_title.is_empty() {
        format!("M{} {} {}", musician.index + 1, status_d.dot, model)
    } else {
        format!(
            "M{} {} {}  {}",
            musician.index + 1,
            status_d.dot,
            model,
            task_title,
        )
    };

    // Render the borderless card — returns inner Rect (excludes accent strip when focused)
    let inner = render_card(f, area, is_focused);

    if inner.height < 2 || inner.width < 4 {
        return;
    }

    // Section header: musician name/status line (1 row)
    let header_area = Rect::new(inner.x, inner.y, inner.width, 1);
    render_section_header(
        f,
        header_area,
        &theme::trunc(&header_title, inner.width as usize),
        None,
    );

    if inner.height < 3 {
        return;
    }

    // Reserve 1 row at bottom for stats, 1 row at top for header
    let content_start = inner.y + 1;
    let content_height = inner.height.saturating_sub(2); // header + stats
    let output_area = Rect::new(inner.x, content_start, inner.width, content_height);
    let stats_area = Rect::new(inner.x, inner.y + inner.height - 1, inner.width, 1);

    // Output lines
    let total_lines = musician.output_lines.len();

    if total_lines == 0 {
        render_empty_state(f, output_area, "Waiting…");
    } else {
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
            total_lines.saturating_sub(content_height as usize) as u16
        } else {
            scroll_offset
        };
        let output = Paragraph::new(lines).scroll((effective_scroll, 0));
        f.render_widget(output, output_area);
    }

    // Stats line: elapsed time via render_inline_kv
    let elapsed = theme::elapsed(musician.elapsed_ms);
    let stats_line = render_inline_kv("elapsed", &elapsed);
    f.render_widget(Paragraph::new(stats_line), stats_area);
}
