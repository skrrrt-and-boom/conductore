//! Insight panel and task dependency graph.
//!
//! Port of InsightPanel.tsx + TaskGraph.tsx.

use ratatui::{
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph},
    Frame,
};

use conductor_types::{Insight, InsightCategory, Task};

use crate::theme::{self, THEME};

/// Render the insights panel on the right side of the screen.
pub fn render_insight_panel(f: &mut Frame, area: Rect, insights: &[Insight]) {
    let title = format!(" Insights ({}) ", insights.len());
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(THEME.border))
        .title(Span::styled(title, Style::default().fg(THEME.accent)));

    let inner = block.inner(area);
    f.render_widget(block, area);

    if insights.is_empty() {
        let empty = Paragraph::new(Span::styled(
            "No insights yet",
            Style::default().fg(THEME.text_muted),
        ));
        f.render_widget(empty, inner);
        return;
    }

    let max_items = inner.height as usize;
    let items: Vec<ListItem> = insights
        .iter()
        .rev()
        .take(max_items)
        .map(|insight| {
            let icon = category_icon(&insight.category);
            let cat_color = category_color(&insight.category);

            let title_line = Line::from(vec![
                Span::styled(icon, Style::default().fg(cat_color)),
                Span::raw(" "),
                Span::styled(
                    theme::trunc(&insight.title, (inner.width as usize).saturating_sub(6)),
                    Style::default().fg(THEME.text_primary),
                ),
            ]);

            let body_line = Line::from(Span::styled(
                format!(
                    "  {}",
                    theme::trunc(&insight.body, (inner.width as usize).saturating_sub(4))
                ),
                Style::default().fg(THEME.text_muted),
            ));

            ListItem::new(vec![title_line, body_line])
        })
        .collect();

    let list = List::new(items);
    f.render_widget(list, inner);
}

/// Render a vertical task dependency graph.
pub fn render_task_graph(f: &mut Frame, area: Rect, tasks: &[Task]) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(THEME.border))
        .title(Span::styled(" Tasks ", Style::default().fg(THEME.text_primary)));

    let inner = block.inner(area);
    f.render_widget(block, area);

    if tasks.is_empty() {
        return;
    }

    let lines: Vec<Line> = tasks
        .iter()
        .take(inner.height as usize)
        .map(|task| {
            let viz = theme::task_viz(&task.status);
            let mut spans = vec![
                Span::styled(format!(" {} ", viz.dot), Style::default().fg(viz.color)),
                Span::styled(
                    format!("{}. ", task.index + 1),
                    Style::default().fg(THEME.text_muted),
                ),
                Span::styled(
                    theme::trunc(&task.title, (inner.width as usize).saturating_sub(12)),
                    Style::default().fg(THEME.text_primary),
                ),
            ];

            // Show dependency arrows if any
            if !task.dependencies.is_empty() {
                let deps: Vec<String> = task.dependencies.iter().map(|d| format!("{}", d + 1)).collect();
                spans.push(Span::styled(
                    format!(" ← {}", deps.join(",")),
                    Style::default().fg(THEME.text_muted),
                ));
            }

            Line::from(spans)
        })
        .collect();

    let graph = Paragraph::new(lines);
    f.render_widget(graph, inner);
}

fn category_icon(cat: &InsightCategory) -> &'static str {
    match cat {
        InsightCategory::Pattern => "◆",
        InsightCategory::Architecture => "◈",
        InsightCategory::Tool => "◇",
        InsightCategory::Decision => "●",
        InsightCategory::Concept => "○",
        InsightCategory::Why => "★",
    }
}

fn category_color(cat: &InsightCategory) -> ratatui::style::Color {
    match cat {
        InsightCategory::Pattern => THEME.accent,
        InsightCategory::Architecture => THEME.accent,
        InsightCategory::Tool => THEME.text_muted,
        InsightCategory::Decision => THEME.warning,
        InsightCategory::Concept => THEME.accent,
        InsightCategory::Why => THEME.warning,
    }
}
