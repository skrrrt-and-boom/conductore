//! Panel overlays — plan review, session browser, task detail modal.
//!
//! Port of PlanReview.tsx, SessionBrowser.tsx, TaskDetailModal.tsx.

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph, Row, Table, Wrap},
    Frame,
};

use conductor_types::{Plan, PlanRefinementMessage, RefinementRole, SessionData, Task};

use crate::{
    app::{centered_rect, clear_area},
    theme::{self, C_BRAND, C_DIM, C_ERROR, C_READY, C_TEXT},
};

// ─── Plan Review ─────────────────────────────────────────────────────────────

/// Render the plan review screen (replaces musician grid during PlanReview phase).
pub fn render_plan_review(
    f: &mut Frame,
    area: Rect,
    plan: Option<&Plan>,
    tasks: &[Task],
    refinement_history: &[PlanRefinementMessage],
    selected: usize,
) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(C_BRAND))
        .title(Span::styled(" Plan Review ", Style::default().fg(C_BRAND)));

    let inner = block.inner(area);
    f.render_widget(block, area);

    if inner.height < 4 {
        return;
    }

    // Vertical layout: summary | refinement chat | task list | controls
    let has_refinement = !refinement_history.is_empty();
    let refinement_height = if has_refinement {
        (refinement_history.len() as u16 * 2 + 2).min(inner.height / 4)
    } else {
        0
    };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),                // summary
            Constraint::Length(refinement_height), // refinement chat
            Constraint::Min(3),                   // task list
            Constraint::Length(1),                 // controls
        ])
        .split(inner);

    // Summary
    if let Some(plan) = plan {
        let summary = Paragraph::new(vec![
            Line::from(Span::styled(
                theme::trunc(&plan.summary, inner.width as usize),
                Style::default().fg(C_TEXT),
            )),
            Line::from(Span::styled(
                format!(
                    "{} tasks · ~{} min · ~{} tokens",
                    tasks.len(),
                    plan.estimated_minutes,
                    theme::format_tokens(plan.estimated_tokens, true)
                ),
                Style::default().fg(C_DIM),
            )),
        ]);
        f.render_widget(summary, chunks[0]);
    }

    // Refinement chat
    if has_refinement {
        let ref_lines: Vec<Line> = refinement_history
            .iter()
            .map(|msg| {
                let (prefix, color) = match msg.role {
                    RefinementRole::User => ("You: ", C_BRAND),
                    RefinementRole::Conductor => ("Conductor: ", C_READY),
                };
                Line::from(vec![
                    Span::styled(prefix, Style::default().fg(color)),
                    Span::styled(
                        theme::trunc(&msg.text, (inner.width as usize).saturating_sub(15)),
                        Style::default().fg(C_TEXT),
                    ),
                ])
            })
            .collect();
        let chat = Paragraph::new(ref_lines).wrap(Wrap { trim: true });
        f.render_widget(chat, chunks[1]);
    }

    // Task list
    let items: Vec<ListItem> = tasks
        .iter()
        .enumerate()
        .map(|(i, task)| {
            let viz = theme::task_viz(&task.status);
            let is_selected = i == selected;
            let indicator = if is_selected { "▸" } else { " " };
            let sel_style = if is_selected {
                Style::default().fg(C_BRAND)
            } else {
                Style::default().fg(C_TEXT)
            };

            let title_line = Line::from(vec![
                Span::styled(indicator, sel_style),
                Span::styled(format!(" {} ", viz.dot), Style::default().fg(viz.color)),
                Span::styled(
                    format!("{}. {}", task.index + 1, theme::trunc(&task.title, (inner.width as usize).saturating_sub(10))),
                    sel_style,
                ),
            ]);

            let detail_line = Line::from(vec![
                Span::styled("    ", Style::default()),
                Span::styled(
                    format!(
                        "Files: {}  Deps: {}",
                        if task.file_scope.is_empty() {
                            "—".to_string()
                        } else {
                            task.file_scope.join(", ")
                        },
                        if task.dependencies.is_empty() {
                            "none".to_string()
                        } else {
                            task.dependencies.iter().map(|d| format!("{}", d + 1)).collect::<Vec<_>>().join(", ")
                        }
                    ),
                    Style::default().fg(C_DIM),
                ),
            ]);

            ListItem::new(vec![title_line, detail_line])
        })
        .collect();

    let list = List::new(items);
    f.render_widget(list, chunks[2]);

    // Controls
    let controls = Paragraph::new(Line::from(vec![
        Span::styled(" [Enter]", Style::default().fg(C_BRAND)),
        Span::styled(" Approve  ", Style::default().fg(C_TEXT)),
        Span::styled("[d]", Style::default().fg(C_BRAND)),
        Span::styled(" Detail  ", Style::default().fg(C_TEXT)),
        Span::styled("type to refine  ", Style::default().fg(C_DIM)),
        Span::styled("q", Style::default().fg(C_BRAND)),
        Span::styled(" quit", Style::default().fg(C_TEXT)),
    ]));
    f.render_widget(controls, chunks[3]);
}

// ─── Session Browser ─────────────────────────────────────────────────────────

/// Render the session browser as a centered overlay.
pub fn render_session_browser(
    f: &mut Frame,
    area: Rect,
    sessions: &[SessionData],
    selected: usize,
) {
    let popup = centered_rect(70, 70, area);
    clear_area(f, popup);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(C_BRAND))
        .title(Span::styled(" Sessions ", Style::default().fg(C_BRAND)));

    let inner = block.inner(popup);
    f.render_widget(block, popup);

    if sessions.is_empty() {
        let empty = Paragraph::new(Span::styled("No sessions found", Style::default().fg(C_DIM)));
        f.render_widget(empty, inner);
        return;
    }

    let header = Row::new(["ID", "Phase", "Tasks", "Tokens"])
        .style(Style::default().fg(C_DIM).add_modifier(Modifier::BOLD));

    let rows: Vec<Row> = sessions
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let style = if i == selected {
                Style::default().fg(C_BRAND)
            } else {
                Style::default().fg(C_TEXT)
            };

            let total_tokens = s.tokens.input + s.tokens.output;
            Row::new([
                theme::trunc(&s.id, 8),
                format!("{:?}", s.phase),
                format!("{}", s.tasks.len()),
                theme::format_tokens(total_tokens, s.tokens_estimated),
            ])
            .style(style)
        })
        .collect();

    let widths = [
        Constraint::Length(10),
        Constraint::Length(14),
        Constraint::Length(6),
        Constraint::Length(10),
    ];

    let table = Table::new(rows, widths)
        .header(header)
        .column_spacing(2);

    f.render_widget(table, inner);
}

// ─── Task Detail Modal ───────────────────────────────────────────────────────

/// Render a task detail modal as a centered overlay.
pub fn render_task_detail(
    f: &mut Frame,
    area: Rect,
    task: &Task,
    all_tasks: &[Task],
    scroll_offset: u16,
) {
    let popup = centered_rect(70, 80, area);
    clear_area(f, popup);

    let title = format!(" Task {}: {} ", task.index + 1, theme::trunc(&task.title, 40));
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(C_BRAND))
        .title(Span::styled(title, Style::default().fg(C_BRAND)));

    let inner = block.inner(popup);
    f.render_widget(block, popup);

    let mut lines: Vec<Line> = Vec::new();

    // Status + assigned musician
    let viz = theme::task_viz(&task.status);
    lines.push(Line::from(vec![
        Span::styled(format!("{} {:?}", viz.dot, task.status), Style::default().fg(viz.color)),
        Span::styled(
            format!(
                "   Musician: {}",
                task.assigned_musician.as_deref().unwrap_or("—")
            ),
            Style::default().fg(C_DIM),
        ),
    ]));
    lines.push(Line::raw(""));

    // Description
    lines.push(Line::from(Span::styled("DESCRIPTION", Style::default().fg(C_DIM))));
    for line in task.description.lines() {
        lines.push(Line::from(Span::styled(line, Style::default().fg(C_TEXT))));
    }
    lines.push(Line::raw(""));

    // Why
    if !task.why.is_empty() {
        lines.push(Line::from(Span::styled("WHY", Style::default().fg(C_READY))));
        for line in task.why.lines() {
            lines.push(Line::from(Span::styled(line, Style::default().fg(C_TEXT))));
        }
        lines.push(Line::raw(""));
    }

    // Files
    if !task.file_scope.is_empty() {
        lines.push(Line::from(Span::styled("FILES", Style::default().fg(C_DIM))));
        for file in &task.file_scope {
            lines.push(Line::from(Span::styled(format!("  • {file}"), Style::default().fg(C_TEXT))));
        }
        lines.push(Line::raw(""));
    }

    // Dependencies
    if !task.dependencies.is_empty() {
        lines.push(Line::from(Span::styled("DEPENDENCIES", Style::default().fg(C_DIM))));
        for &dep_idx in &task.dependencies {
            let dep_name = all_tasks
                .get(dep_idx)
                .map(|t| t.title.as_str())
                .unwrap_or("?");
            lines.push(Line::from(Span::styled(
                format!("  • Task {}: {dep_name}", dep_idx + 1),
                Style::default().fg(C_TEXT),
            )));
        }
        lines.push(Line::raw(""));
    }

    // Acceptance criteria
    if !task.acceptance_criteria.is_empty() {
        lines.push(Line::from(Span::styled("ACCEPTANCE CRITERIA", Style::default().fg(C_DIM))));
        for ac in &task.acceptance_criteria {
            lines.push(Line::from(Span::styled(format!("  • {ac}"), Style::default().fg(C_TEXT))));
        }
        lines.push(Line::raw(""));
    }

    // Result (if completed)
    if let Some(result) = &task.result {
        lines.push(Line::from(Span::styled("RESULT", Style::default().fg(C_DIM))));
        let status_label = if result.success { "Success" } else { "Failed" };
        let status_color = if result.success { theme::C_ACTIVE } else { C_ERROR };
        lines.push(Line::from(Span::styled(status_label, Style::default().fg(status_color))));

        if !result.summary.is_empty() {
            for line in result.summary.lines() {
                lines.push(Line::from(Span::styled(
                    format!("  {line}"),
                    Style::default().fg(C_TEXT),
                )));
            }
        }

        if let Some(err) = &result.error {
            lines.push(Line::from(Span::styled(
                format!("  Error: {err}"),
                Style::default().fg(C_ERROR),
            )));
        }

        if !result.files_modified.is_empty() {
            lines.push(Line::from(Span::styled("FILES MODIFIED", Style::default().fg(C_DIM))));
            for file in &result.files_modified {
                lines.push(Line::from(Span::styled(
                    format!("  • {file}"),
                    Style::default().fg(C_TEXT),
                )));
            }
        }
    }

    // Footer
    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled(
        "[Esc] Close  [↑↓] Scroll",
        Style::default().fg(C_DIM),
    )));

    let content = Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .scroll((scroll_offset, 0));

    f.render_widget(content, inner);
}
