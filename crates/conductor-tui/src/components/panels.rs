//! Panel overlays — plan review, session browser, task detail modal.
//!
//! Port of PlanReview.tsx, SessionBrowser.tsx, TaskDetailModal.tsx.

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{List, ListItem, Paragraph, Row, Table, Wrap},
    Frame,
};

use conductor_types::{Plan, PlanRefinementMessage, RefinementRole, SessionData, Task};

use crate::{
    app::centered_rect,
    theme::{self, C_BRAND, C_DIM, C_ERROR, C_READY, C_TEXT, SURFACE},
    widgets::{render_borderless_panel, render_inline_kv, render_key_hint, render_modal_backdrop, render_section_header},
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
    let inner = render_borderless_panel(f, area, Some(" Plan Review"), SURFACE);

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
        let clean_summary = theme::strip_control_chars(&plan.summary);
        let summary = Paragraph::new(vec![
            Line::from(Span::styled(
                theme::trunc(&clean_summary, inner.width as usize),
                Style::default().fg(C_TEXT),
            )),
            Line::from(Span::styled(
                format!(
                    "{} tasks · ~{} min",
                    tasks.len(),
                    plan.estimated_minutes,
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
                    format!("{}. {}", task.index + 1, theme::trunc(&theme::strip_control_chars(&task.title), (inner.width as usize).saturating_sub(10))),
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

    // Controls bar using render_key_hint spans
    let mut spans: Vec<Span> = Vec::new();
    spans.push(Span::raw(" "));
    spans.extend(render_key_hint("Enter", "Approve"));
    spans.push(Span::raw("  "));
    spans.extend(render_key_hint("d", "Detail"));
    spans.push(Span::raw("  "));
    spans.extend(render_key_hint("type", "refine"));
    spans.push(Span::raw("  "));
    spans.extend(render_key_hint("q", "quit"));
    f.render_widget(Paragraph::new(Line::from(spans)), chunks[3]);
}

// ─── Session Browser ─────────────────────────────────────────────────────────

/// Render the session browser as a centered overlay.
pub fn render_session_browser(
    f: &mut Frame,
    area: Rect,
    sessions: &[SessionData],
    selected: usize,
) {
    let popup = centered_rect(80, 70, area);
    render_modal_backdrop(f, popup);

    let inner = render_borderless_panel(f, popup, Some(" Sessions"), SURFACE);

    if sessions.is_empty() {
        let empty = Paragraph::new(Span::styled("No sessions found", Style::default().fg(C_DIM)));
        f.render_widget(empty, inner);
        return;
    }

    let header = Row::new(["ID", "Prompt", "Phase", "Tasks"])
        .style(Style::default().fg(C_DIM).add_modifier(Modifier::BOLD));

    // Max width for prompt column (fill remaining space)
    let prompt_width = inner.width.saturating_sub(10 + 16 + 6 + 6) as usize; // ID + Phase + Tasks + spacing

    let rows: Vec<Row> = sessions
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let style = if i == selected {
                Style::default().fg(C_BRAND)
            } else {
                Style::default().fg(C_TEXT)
            };

            // Show first line of task description as prompt
            let prompt = s.config.task_description
                .lines()
                .next()
                .unwrap_or("")
                .trim();

            Row::new([
                theme::trunc(&s.id, 8),
                theme::trunc(prompt, prompt_width),
                format!("{:?}", s.phase),
                format!("{}", s.tasks.len()),
            ])
            .style(style)
        })
        .collect();

    let widths = [
        Constraint::Length(10),
        Constraint::Min(20),
        Constraint::Length(16),
        Constraint::Length(6),
    ];

    let table = Table::new(rows, widths)
        .header(header)
        .column_spacing(2);

    f.render_widget(table, inner);
}

// ─── Task Detail Modal ───────────────────────────────────────────────────────

/// Row items for the task detail modal content.
enum RowItem {
    SectionHeader(String),
    Content(Line<'static>),
    Blank,
}

/// Render a task detail modal as a centered overlay.
pub fn render_task_detail(
    f: &mut Frame,
    area: Rect,
    task: &Task,
    all_tasks: &[Task],
    scroll_offset: u16,
) {
    let popup = centered_rect(70, 80, area);
    render_modal_backdrop(f, popup);

    let title = format!(" Task {}: {} ", task.index + 1, theme::trunc(&task.title, 40));
    let inner = render_borderless_panel(f, popup, Some(&title), SURFACE);

    // Build a list of row items to render
    let mut rows: Vec<RowItem> = Vec::new();

    // Status line
    let viz = theme::task_viz(&task.status);
    rows.push(RowItem::Content(Line::from(vec![
        Span::styled(format!("{} {:?}", viz.dot, task.status), Style::default().fg(viz.color)),
    ])));

    // Musician via render_inline_kv
    rows.push(RowItem::Content(
        render_inline_kv("Musician:", task.assigned_musician.as_deref().unwrap_or("—"))
    ));
    rows.push(RowItem::Blank);

    // Description
    rows.push(RowItem::SectionHeader("DESCRIPTION".to_string()));
    let clean_desc = theme::strip_control_chars(&task.description);
    for line in clean_desc.lines() {
        rows.push(RowItem::Content(Line::from(Span::styled(line.to_string(), Style::default().fg(C_TEXT)))));
    }
    rows.push(RowItem::Blank);

    // Why
    if !task.why.is_empty() {
        rows.push(RowItem::SectionHeader("WHY".to_string()));
        for line in task.why.lines() {
            rows.push(RowItem::Content(Line::from(Span::styled(line.to_string(), Style::default().fg(C_TEXT)))));
        }
        rows.push(RowItem::Blank);
    }

    // Files
    if !task.file_scope.is_empty() {
        rows.push(RowItem::SectionHeader("FILES".to_string()));
        for file in &task.file_scope {
            rows.push(RowItem::Content(Line::from(Span::styled(format!("  • {file}"), Style::default().fg(C_TEXT)))));
        }
        rows.push(RowItem::Blank);
    }

    // Dependencies
    if !task.dependencies.is_empty() {
        rows.push(RowItem::SectionHeader("DEPENDENCIES".to_string()));
        for &dep_idx in &task.dependencies {
            let dep_name = all_tasks
                .get(dep_idx)
                .map(|t| t.title.as_str())
                .unwrap_or("?");
            rows.push(RowItem::Content(Line::from(Span::styled(
                format!("  • Task {}: {dep_name}", dep_idx + 1),
                Style::default().fg(C_TEXT),
            ))));
        }
        rows.push(RowItem::Blank);
    }

    // Acceptance criteria
    if !task.acceptance_criteria.is_empty() {
        rows.push(RowItem::SectionHeader("ACCEPTANCE CRITERIA".to_string()));
        for ac in &task.acceptance_criteria {
            rows.push(RowItem::Content(Line::from(Span::styled(format!("  • {ac}"), Style::default().fg(C_TEXT)))));
        }
        rows.push(RowItem::Blank);
    }

    // Result (if completed)
    if let Some(result) = &task.result {
        rows.push(RowItem::SectionHeader("RESULT".to_string()));
        let status_label = if result.success { "Success".to_string() } else { "Failed".to_string() };
        let status_color = if result.success { theme::C_ACTIVE } else { C_ERROR };
        rows.push(RowItem::Content(Line::from(Span::styled(status_label, Style::default().fg(status_color)))));

        if !result.summary.is_empty() {
            for line in result.summary.lines() {
                rows.push(RowItem::Content(Line::from(Span::styled(
                    format!("  {line}"),
                    Style::default().fg(C_TEXT),
                ))));
            }
        }

        if let Some(err) = &result.error {
            rows.push(RowItem::Content(Line::from(Span::styled(
                format!("  Error: {err}"),
                Style::default().fg(C_ERROR),
            ))));
        }

        if !result.files_modified.is_empty() {
            rows.push(RowItem::SectionHeader("FILES MODIFIED".to_string()));
            for file in &result.files_modified {
                rows.push(RowItem::Content(Line::from(Span::styled(
                    format!("  • {file}"),
                    Style::default().fg(C_TEXT),
                ))));
            }
        }
    }

    // Footer hint
    rows.push(RowItem::Blank);
    rows.push(RowItem::Content({
        let mut spans: Vec<Span> = Vec::new();
        spans.extend(render_key_hint("Esc", "Close"));
        spans.push(Span::raw("  "));
        spans.extend(render_key_hint("↑↓", "Scroll"));
        Line::from(spans)
    }));

    // Render visible rows respecting scroll_offset
    let skip = scroll_offset as usize;
    let max_rows = inner.height as usize;

    for (i, row) in rows.iter().skip(skip).take(max_rows).enumerate() {
        let row_rect = Rect {
            x: inner.x,
            y: inner.y + i as u16,
            width: inner.width,
            height: 1,
        };
        match row {
            RowItem::SectionHeader(title) => {
                render_section_header(f, row_rect, title, None);
            }
            RowItem::Content(line) => {
                f.render_widget(Paragraph::new(line.clone()), row_rect);
            }
            RowItem::Blank => {
                // empty row — nothing to render
            }
        }
    }
}
