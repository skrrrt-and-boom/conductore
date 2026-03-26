//! Prompt bar and keyboard help overlay.
//!
//! Port of PromptBar.tsx + KeyboardHelpOverlay.tsx.

use ratatui::{
    layout::{Constraint, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Clear, Paragraph, Row, Table},
    Frame,
};
use unicode_width::UnicodeWidthStr;

use conductor_types::OrchestraPhase;

use crate::{
    app::{centered_rect, SLASH_COMMANDS},
    theme::{C_BRAND, C_DIM, C_READY, C_TEXT, SURFACE},
    widgets::render_borderless_panel,
};

/// Render the prompt input bar at the bottom of the screen.
pub fn render_prompt_bar(
    f: &mut Frame,
    area: Rect,
    input_text: &str,
    cursor_pos: usize,
    phase: &OrchestraPhase,
) {
    // Phase-aware dim label; `›` is always the accent prompt arrow.
    let phase_label = match phase {
        OrchestraPhase::Init => "task",
        OrchestraPhase::PlanReview => "refine",
        OrchestraPhase::PhaseExecuting | OrchestraPhase::Executing => "guidance",
        _ => "",
    };

    // Build input spans with command highlighting
    let input_spans = if input_text.starts_with('/') {
        let trimmed = input_text.trim();
        let cmd_part = trimmed.split_whitespace().next().unwrap_or(trimmed);
        let is_valid = SLASH_COMMANDS.contains(&cmd_part)
            || SLASH_COMMANDS.iter().any(|c| c.starts_with(cmd_part) && cmd_part.len() > 1);

        let cmd_color = if is_valid { C_READY } else { C_DIM };
        let cmd_len = cmd_part.len().min(input_text.len());
        let rest = &input_text[cmd_len..];

        vec![
            Span::styled(&input_text[..cmd_len], Style::default().fg(cmd_color)),
            Span::styled(rest, Style::default().fg(C_TEXT)),
        ]
    } else {
        vec![Span::styled(input_text, Style::default().fg(C_TEXT))]
    };

    // Calculate horizontal scroll offset so the cursor stays visible
    let prefix_display = if phase_label.is_empty() {
        " › ".to_string()
    } else {
        format!(" {} › ", phase_label)
    };
    let prefix_w = UnicodeWidthStr::width(prefix_display.as_str()) as u16;
    let visible_width = area.width.saturating_sub(prefix_w);
    let input_before_cursor = &input_text[..cursor_pos.min(input_text.len())];
    let cursor_col = UnicodeWidthStr::width(input_before_cursor) as u16;

    // Scroll the input text so the cursor is always within the visible area
    let h_scroll = if cursor_col >= visible_width {
        (cursor_col - visible_width + 1) as usize
    } else {
        0
    };

    // Slice the input text to the visible window (char-aware)
    let visible_input: String = input_text
        .chars()
        .scan(0usize, |w, c| {
            let cw = unicode_width::UnicodeWidthChar::width(c).unwrap_or(0);
            *w += cw;
            Some((*w, c))
        })
        .skip_while(|&(w, _)| w <= h_scroll)
        .map(|(_, c)| c)
        .collect();

    // Rebuild input spans on the visible portion
    let visible_spans = if input_text.starts_with('/') && h_scroll == 0 {
        // Re-use existing highlighting logic only when not scrolled (command is visible)
        input_spans.iter().map(|s| s.clone()).collect::<Vec<_>>()
    } else {
        vec![Span::styled(&visible_input, Style::default().fg(C_TEXT))]
    };

    // Build prefix: optional dim phase label + accent › arrow
    let prefix_spans: Vec<Span<'static>> = if phase_label.is_empty() {
        vec![
            Span::raw(" "),
            Span::styled("›", Style::default().fg(C_BRAND)),
            Span::raw(" "),
        ]
    } else {
        vec![
            Span::raw(" "),
            Span::styled(phase_label.to_string(), Style::default().fg(C_DIM)),
            Span::raw(" "),
            Span::styled("›", Style::default().fg(C_BRAND)),
            Span::raw(" "),
        ]
    };
    let mut spans: Vec<Span> = prefix_spans.into_iter().collect();
    spans.extend(visible_spans);

    let prompt = Paragraph::new(Line::from(spans)).style(Style::default().bg(SURFACE));
    f.render_widget(prompt, area);

    // Show cursor position accounting for horizontal scroll
    let cursor_x = area.x + prefix_w + cursor_col.saturating_sub(h_scroll as u16);
    if cursor_x < area.x + area.width {
        f.set_cursor_position((cursor_x, area.y));
    }
}

/// Render the keyboard help overlay as a centered popup.
pub fn render_keyboard_help(f: &mut Frame, area: Rect) {
    let popup = centered_rect(60, 70, area);
    f.render_widget(Clear, popup);

    let inner = render_borderless_panel(f, popup, Some("Keyboard Shortcuts"), SURFACE);

    let rows = vec![
        Row::new(["Tab / Shift+Tab", "Switch panel / autocomplete cmd"]),
        Row::new(["←  →", "Previous / next musician"]),
        Row::new(["↑  ↓", "Prompt history / scroll / navigate"]),
        Row::new(["Enter", "Approve plan / submit input"]),
        Row::new(["Esc", "Dismiss overlay / clear input"]),
        Row::new(["?", "Toggle this help"]),
        Row::new(["d", "Task detail (in plan review)"]),
        Row::new(["q / Ctrl+C", "Quit"]),
        Row::new(["", ""]),
        Row::new(["Opt+← / Opt+→", "Jump word left / right"]),
        Row::new(["Ctrl+W", "Delete word backward"]),
        Row::new(["Ctrl+A / Ctrl+E", "Jump to start / end of line"]),
        Row::new(["Ctrl+U", "Clear input line"]),
        Row::new(["", ""]),
        Row::new(["  COMMANDS", ""]).style(Style::default().fg(C_BRAND)),
        Row::new(["/sessions", "Browse past sessions"]),
        Row::new(["/resume <id>", "Resume a past session"]),
        Row::new(["/help", "Toggle this help"]),
        Row::new(["/quit", "Quit conductor"]),
    ];

    let widths = [Constraint::Length(18), Constraint::Min(20)];
    let table = Table::new(rows, widths)
        .column_spacing(2)
        .style(Style::default().fg(C_TEXT));

    f.render_widget(table, inner);
}
