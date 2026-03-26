//! Shared visual primitives for the Grok-inspired TUI design.
//!
//! All render functions follow the pure render pattern:
//! `fn render_xxx(f: &mut Frame, area: Rect, ...)` — no side effects.
//! Composable helpers (status dot, key hint) return styled spans for the
//! caller to embed in larger [`Line`] / [`Paragraph`] constructs.

use ratatui::{
    layout::{Alignment, Rect},
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Paragraph},
    Frame,
};

use crate::theme::{ACCENT_DIM, C_ACCENT, C_DIM, C_FRAME, SURFACE, TEXT_LABEL, TEXT_PRIMARY};

// ── Section Header ────────────────────────────────────────────────────────────

/// Renders a minimal single-line section header.
///
/// Title is drawn in accent color on the left. When `badge` is provided (e.g.
/// a count like `"3"`), it is right-aligned in dim. No borders.
pub fn render_section_header(f: &mut Frame, area: Rect, title: &str, badge: Option<&str>) {
    let line = if let Some(badge_text) = badge {
        let title_len = title.chars().count();
        let badge_len = badge_text.chars().count();
        let total = area.width as usize;
        // Pad between title and badge so badge lands at the right edge
        let padding = total.saturating_sub(title_len).saturating_sub(badge_len);
        Line::from(vec![
            Span::styled(title.to_string(), Style::default().fg(C_ACCENT)),
            Span::styled(" ".repeat(padding), Style::default()),
            Span::styled(badge_text.to_string(), Style::default().fg(C_DIM)),
        ])
    } else {
        Line::from(Span::styled(title.to_string(), Style::default().fg(C_ACCENT)))
    };

    f.render_widget(Paragraph::new(line), area);
}

// ── Thin Separator ────────────────────────────────────────────────────────────

/// Renders a full-width horizontal rule using `─` in the border (frame) color.
/// Expects `area` to be exactly 1 row tall.
pub fn render_thin_separator(f: &mut Frame, area: Rect) {
    let line = "─".repeat(area.width as usize);
    f.render_widget(
        Paragraph::new(Span::styled(line, Style::default().fg(C_FRAME))),
        area,
    );
}

// ── Status Dot ────────────────────────────────────────────────────────────────

/// Returns a `●` span colored with `status_color`.
///
/// Use the result inside a [`Line`] alongside other spans:
/// ```ignore
/// Line::from(vec![render_status_dot(C_ACTIVE), Span::raw(" Running")])
/// ```
pub fn render_status_dot(status_color: Color) -> Span<'static> {
    Span::styled("●", Style::default().fg(status_color))
}

// ── Progress Bar ──────────────────────────────────────────────────────────────

/// Renders a minimalist inline progress bar.
///
/// Uses `━` (filled, accent color) and `╌` (empty, border color). `progress`
/// is clamped to `[0.0, 1.0]`. `width` is the character width of the bar.
pub fn render_progress_bar(f: &mut Frame, area: Rect, progress: f64, width: usize) {
    let progress = progress.clamp(0.0, 1.0);
    let filled = ((progress * width as f64).round() as usize).min(width);
    let empty = width - filled;

    let line = Line::from(vec![
        Span::styled("━".repeat(filled), Style::default().fg(C_ACCENT)),
        Span::styled("╌".repeat(empty), Style::default().fg(C_FRAME)),
    ]);
    f.render_widget(Paragraph::new(line), area);
}

// ── Empty State ───────────────────────────────────────────────────────────────

/// Renders centered dim text for empty states ("No insights yet", "Waiting…").
///
/// Text is horizontally centered and placed at the vertical midpoint of `area`.
pub fn render_empty_state(f: &mut Frame, area: Rect, message: &str) {
    if area.height == 0 || area.width == 0 {
        return;
    }
    let y_offset = area.height / 2;
    let row = Rect {
        x: area.x,
        y: area.y + y_offset,
        width: area.width,
        height: 1,
    };
    let p = Paragraph::new(Span::styled(message.to_string(), Style::default().fg(C_DIM)))
        .alignment(Alignment::Center);
    f.render_widget(p, row);
}

// ── Card ──────────────────────────────────────────────────────────────────────

/// Renders a borderless card with a surface background.
///
/// When `focused` is true a 2-column left-edge accent strip is drawn, giving
/// a visual indicator without heavy borders. Returns the inner [`Rect`]
/// available for card content (excludes the accent strip when focused).
pub fn render_card(f: &mut Frame, area: Rect, focused: bool) -> Rect {
    // Surface background fill
    let bg = Block::default().style(Style::default().bg(Color::Black));
    f.render_widget(bg, area);

    if focused && area.width > 2 {
        // 2-column accent strip on the left edge
        let strip = Rect { x: area.x, y: area.y, width: 2, height: area.height };
        let accent_strip = Block::default().style(Style::default().bg(C_ACCENT));
        f.render_widget(accent_strip, strip);

        // Return inner rect excluding the strip
        Rect {
            x: area.x + 2,
            y: area.y,
            width: area.width.saturating_sub(2),
            height: area.height,
        }
    } else {
        area
    }
}

// ── Modal Backdrop ────────────────────────────────────────────────────────────

/// Fills `area` with a surface (dark) background before modal content is drawn.
///
/// Call this immediately before rendering the modal's own widgets to prevent
/// underlying content from bleeding through.
pub fn render_modal_backdrop(f: &mut Frame, area: Rect) {
    let bg = Block::default().style(Style::default().bg(Color::Black));
    f.render_widget(bg, area);
}

// ── Key Hint ──────────────────────────────────────────────────────────────────

/// Returns composable spans for a keyboard shortcut hint.
///
/// The key is wrapped in `[…]` and styled with the accent color; the action
/// description follows in dim. Combine multiple hints into a [`Line`]:
/// ```ignore
/// let mut spans: Vec<Span> = Vec::new();
/// spans.extend(render_key_hint("Enter", "approve"));
/// spans.push(Span::raw("  "));
/// spans.extend(render_key_hint("q", "quit"));
/// f.render_widget(Paragraph::new(Line::from(spans)), area);
/// ```
pub fn render_key_hint(key: &str, action: &str) -> Vec<Span<'static>> {
    vec![
        Span::styled(format!("[{key}]"), Style::default().fg(C_ACCENT)),
        Span::styled(" ".to_string(), Style::default()),
        Span::styled(action.to_string(), Style::default().fg(C_DIM)),
    ]
}

// ── Borderless Panel ──────────────────────────────────────────────────────────

/// Fills `area` with `bg` color and optionally renders a title in accent at the
/// top-left (within the 1-cell padding).  Returns the inner [`Rect`] shrunk by
/// 1 cell on every side — callers draw their content there.
///
/// Replaces `Block`-with-borders panels throughout the V2 UI.
pub fn render_borderless_panel(
    f: &mut Frame,
    area: Rect,
    title: Option<&str>,
    bg: Color,
) -> Rect {
    // Fill background
    f.render_widget(Block::default().style(Style::default().bg(bg)), area);

    // Optional title rendered in the top-left padding cell
    if let Some(title_text) = title {
        if area.height > 0 && area.width > 2 {
            let title_area = Rect {
                x: area.x + 1,
                y: area.y,
                width: area.width.saturating_sub(2),
                height: 1,
            };
            f.render_widget(
                Paragraph::new(Span::styled(
                    title_text.to_string(),
                    Style::default().fg(C_ACCENT),
                )),
                title_area,
            );
        }
    }

    // Inner rect: 1-cell padding on all sides
    Rect {
        x: area.x + 1,
        y: area.y + 1,
        width: area.width.saturating_sub(2),
        height: area.height.saturating_sub(2),
    }
}

// ── Tab Content Area ──────────────────────────────────────────────────────────

/// Standard wrapper for tab content: fills `area` with the `SURFACE` background
/// and returns the inner [`Rect`] with 1-cell padding on all sides.
///
/// Call at the top of every tab render function before drawing content.
pub fn render_tab_content_area(f: &mut Frame, area: Rect) -> Rect {
    f.render_widget(Block::default().style(Style::default().bg(SURFACE)), area);

    Rect {
        x: area.x + 1,
        y: area.y + 1,
        width: area.width.saturating_sub(2),
        height: area.height.saturating_sub(2),
    }
}

// ── Inline Key-Value ──────────────────────────────────────────────────────────

/// Returns a [`Line`] with `key` in [`TEXT_LABEL`] and `value` in
/// [`TEXT_PRIMARY`], separated by a space.  Embed in [`Paragraph`] for
/// compact stats displays.
///
/// ```ignore
/// f.render_widget(Paragraph::new(render_inline_kv("Model", "claude-opus-4")), area);
/// ```
pub fn render_inline_kv(key: &str, value: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(key.to_string(), Style::default().fg(TEXT_LABEL)),
        Span::raw(" "),
        Span::styled(value.to_string(), Style::default().fg(TEXT_PRIMARY)),
    ])
}

// ── Tool Icon ─────────────────────────────────────────────────────────────────

/// Returns a styled icon [`Span`] for a Claude tool name.
///
/// | Tool  | Icon |
/// |-------|------|
/// | Read  | `[R]` |
/// | Edit  | `[E]` |
/// | Bash  | `[$]` |
/// | Write | `[W]` |
/// | Grep  | `[?]` |
/// | Glob  | `[G]` |
/// | other | `[·]` |
///
/// Icons are rendered in [`ACCENT_DIM`] to distinguish them from body text
/// without competing with primary accent elements.
pub fn render_tool_icon(tool_name: &str) -> Span<'static> {
    let icon = match tool_name {
        "Read" => "[R]",
        "Edit" => "[E]",
        "Bash" => "[$]",
        "Write" => "[W]",
        "Grep" => "[?]",
        "Glob" => "[G]",
        _ => "[·]",
    };
    Span::styled(icon, Style::default().fg(ACCENT_DIM))
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::{ACCENT, TEXT_MUTED};

    #[test]
    fn render_inline_kv_key_uses_label_color() {
        let line = render_inline_kv("Model", "claude-opus");
        let key_span = &line.spans[0];
        assert_eq!(key_span.content, "Model");
        assert_eq!(key_span.style.fg, Some(TEXT_LABEL));
    }

    #[test]
    fn render_inline_kv_value_uses_primary_color() {
        let line = render_inline_kv("Model", "claude-opus");
        let val_span = &line.spans[2];
        assert_eq!(val_span.content, "claude-opus");
        assert_eq!(val_span.style.fg, Some(TEXT_PRIMARY));
    }

    #[test]
    fn render_inline_kv_has_three_spans() {
        let line = render_inline_kv("k", "v");
        assert_eq!(line.spans.len(), 3);
        assert_eq!(line.spans[1].content, " ");
    }

    #[test]
    fn render_tool_icon_known_tools() {
        assert_eq!(render_tool_icon("Read").content, "[R]");
        assert_eq!(render_tool_icon("Edit").content, "[E]");
        assert_eq!(render_tool_icon("Bash").content, "[$]");
        assert_eq!(render_tool_icon("Write").content, "[W]");
        assert_eq!(render_tool_icon("Grep").content, "[?]");
        assert_eq!(render_tool_icon("Glob").content, "[G]");
    }

    #[test]
    fn render_tool_icon_unknown_tool_returns_dot() {
        assert_eq!(render_tool_icon("WebFetch").content, "[·]");
        assert_eq!(render_tool_icon("").content, "[·]");
    }

    #[test]
    fn render_tool_icon_uses_accent_dim_color() {
        let span = render_tool_icon("Read");
        assert_eq!(span.style.fg, Some(ACCENT_DIM));
    }

    #[test]
    fn render_tool_icon_unknown_not_accent() {
        let span = render_tool_icon("Unknown");
        // ACCENT_DIM, not ACCENT
        assert_ne!(span.style.fg, Some(ACCENT));
        assert_eq!(span.style.fg, Some(ACCENT_DIM));
    }

    #[test]
    fn render_borderless_panel_returns_inner_rect_shrunk_by_one() {
        use ratatui::{backend::TestBackend, Terminal};
        let backend = TestBackend::new(40, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                let area = ratatui::layout::Rect { x: 0, y: 0, width: 20, height: 10 };
                let inner = render_borderless_panel(f, area, None, Color::Black);
                assert_eq!(inner.x, 1);
                assert_eq!(inner.y, 1);
                assert_eq!(inner.width, 18);
                assert_eq!(inner.height, 8);
            })
            .unwrap();
    }

    #[test]
    fn render_tab_content_area_returns_inner_rect_shrunk_by_one() {
        use ratatui::{backend::TestBackend, Terminal};
        let backend = TestBackend::new(40, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                let area = ratatui::layout::Rect { x: 0, y: 0, width: 30, height: 15 };
                let inner = render_tab_content_area(f, area);
                assert_eq!(inner.x, 1);
                assert_eq!(inner.y, 1);
                assert_eq!(inner.width, 28);
                assert_eq!(inner.height, 13);
            })
            .unwrap();
    }

    #[test]
    fn render_section_header_renders_without_panic() {
        use ratatui::{backend::TestBackend, Terminal};
        let backend = TestBackend::new(40, 1);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                let area = ratatui::layout::Rect { x: 0, y: 0, width: 40, height: 1 };
                render_section_header(f, area, "Title", Some("5"));
            })
            .unwrap();
    }

    #[test]
    fn render_status_dot_uses_given_color() {
        let span = render_status_dot(Color::Red);
        assert_eq!(span.content, "●");
        assert_eq!(span.style.fg, Some(Color::Red));
    }

    #[test]
    fn render_key_hint_format() {
        let spans = render_key_hint("q", "quit");
        assert_eq!(spans[0].content, "[q]");
        assert_eq!(spans[0].style.fg, Some(ACCENT));
        assert_eq!(spans[2].content, "quit");
        assert_eq!(spans[2].style.fg, Some(TEXT_MUTED));
    }
}
