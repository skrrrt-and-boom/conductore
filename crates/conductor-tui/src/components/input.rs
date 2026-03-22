//! Prompt bar and keyboard help overlay.
//!
//! Port of PromptBar.tsx + KeyboardHelpOverlay.tsx.

use ratatui::{
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Row, Table},
    Frame,
};

use conductor_types::OrchestraPhase;

use crate::{
    app::centered_rect,
    theme::{C_BRAND, C_TEXT},
};

/// Render the prompt input bar at the bottom of the screen.
pub fn render_prompt_bar(
    f: &mut Frame,
    area: Rect,
    input_text: &str,
    cursor_pos: usize,
    phase: &OrchestraPhase,
) {
    let prefix = match phase {
        OrchestraPhase::Init => "task",
        OrchestraPhase::PlanReview => "refine",
        OrchestraPhase::PhaseExecuting | OrchestraPhase::Executing => "guidance",
        _ => ">",
    };

    let prompt = Paragraph::new(Line::from(vec![
        Span::styled(
            format!(" {prefix}> "),
            Style::default().fg(C_BRAND),
        ),
        Span::styled(input_text, Style::default().fg(C_TEXT)),
    ]));

    f.render_widget(prompt, area);

    // Show cursor position
    let cursor_x = area.x + (prefix.len() + 3) as u16 + cursor_pos as u16;
    if cursor_x < area.x + area.width {
        f.set_cursor_position((cursor_x, area.y));
    }
}

/// Render the keyboard help overlay as a centered popup.
pub fn render_keyboard_help(f: &mut Frame, area: Rect) {
    let popup = centered_rect(60, 60, area);
    f.render_widget(Clear, popup);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(C_BRAND))
        .title(Span::styled(
            " Keyboard Shortcuts ",
            Style::default().fg(C_BRAND),
        ));

    let inner = block.inner(popup);
    f.render_widget(block, popup);

    let rows = vec![
        Row::new(["Tab / Shift+Tab", "Switch musician panel"]),
        Row::new(["←  →", "Previous / next musician"]),
        Row::new(["↑  ↓", "Scroll output / navigate tasks"]),
        Row::new(["Enter", "Approve plan / submit input"]),
        Row::new(["Esc", "Dismiss overlay / clear input"]),
        Row::new(["?", "Toggle this help"]),
        Row::new(["d", "Task detail (in plan review)"]),
        Row::new(["q / Ctrl+C", "Quit"]),
        Row::new(["", ""]),
        Row::new(["Type text", "Enter guidance or plan refinement"]),
    ];

    let widths = [Constraint::Length(18), Constraint::Min(20)];
    let table = Table::new(rows, widths)
        .column_spacing(2)
        .style(Style::default().fg(C_TEXT));

    f.render_widget(table, inner);
}

use ratatui::layout::Constraint;
