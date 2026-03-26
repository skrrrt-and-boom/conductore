//! Insight panel and task dependency graph.
//!
//! Port of InsightPanel.tsx + TaskGraph.tsx.

use ratatui::{
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{List, ListItem, Paragraph},
    Frame,
};

use conductor_types::{Insight, InsightCategory, Task};

use crate::theme::{self, C_BRAND, C_DIM, C_INFO, C_TEXT, SURFACE, SURFACE_ELEVATED};
use crate::widgets::{render_borderless_panel, render_empty_state};

/// Render the insights panel on the right side of the screen.
pub fn render_insight_panel(f: &mut Frame, area: Rect, insights: &[Insight]) {
    let title = format!(" Insights ({}) ", insights.len());
    let inner = render_borderless_panel(f, area, Some(&title), SURFACE_ELEVATED);

    if insights.is_empty() {
        render_empty_state(f, inner, "No insights yet");
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
                    Style::default().fg(C_TEXT),
                ),
            ]);

            let body_line = Line::from(Span::styled(
                format!(
                    "  {}",
                    theme::trunc(&insight.body, (inner.width as usize).saturating_sub(4))
                ),
                Style::default().fg(C_DIM),
            ));

            ListItem::new(vec![title_line, body_line])
        })
        .collect();

    let list = List::new(items);
    f.render_widget(list, inner);
}

/// Render a vertical task dependency graph.
pub fn render_task_graph(f: &mut Frame, area: Rect, tasks: &[Task]) {
    let inner = render_borderless_panel(f, area, Some(" Tasks"), SURFACE);

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
                    Style::default().fg(C_DIM),
                ),
                Span::styled(
                    theme::trunc(&task.title, (inner.width as usize).saturating_sub(12)),
                    Style::default().fg(C_TEXT),
                ),
            ];

            // Show dependency arrows if any
            if !task.dependencies.is_empty() {
                let deps: Vec<String> = task.dependencies.iter().map(|d| format!("{}", d + 1)).collect();
                spans.push(Span::styled(
                    format!(" ← {}", deps.join(",")),
                    Style::default().fg(C_DIM),
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
        InsightCategory::Pattern => C_INFO,
        InsightCategory::Architecture => C_BRAND,
        InsightCategory::Tool => C_DIM,
        InsightCategory::Decision => theme::C_READY,
        InsightCategory::Concept => theme::C_ACCENT,
        InsightCategory::Why => theme::C_READY,
    }
}
