use conductor_types::state::{MusicianState, MusicianStatus};
use ratatui::layout::Rect;

// ─── Breakpoints ─────────────────────────────────────────────────────────────

/// Below this width, hide non-essential panels.
pub const NARROW: u16 = 80;
/// Above this width, allow multi-column layouts.
pub const WIDE: u16 = 160;
/// Above this height, show more output lines.
pub const TALL: u16 = 40;

// ─── Default Sizing ──────────────────────────────────────────────────────────

pub const INSIGHTS_WIDTH: u16 = 36;
pub const INSIGHTS_WIDTH_MIN: u16 = 24;
pub const INSIGHTS_WIDTH_MAX: u16 = 40;
pub const CELL_PADDING: u16 = 1;
pub const CELL_PADDING_MIN: u16 = 0;
pub const MAX_OUTPUT_LINES: usize = 8;
pub const MAX_OUTPUT_LINES_MIN: usize = 3;
pub const MAX_OUTPUT_LINES_MAX: usize = 20;
pub const TRUNCATE_AT: usize = 72;
pub const MIN_COLUMN_WIDTH: u16 = 28;
pub const COLLAPSED_WIDTH: u16 = 4;

// ─── Layout Configuration ────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub struct LayoutConfig {
    /// Whether to show the insights panel (hidden on narrow terminals).
    pub show_insights_panel: bool,
    /// Number of columns for musician panels.
    pub musician_columns: usize,
    /// Maximum output lines to display per musician.
    pub max_output_lines: usize,
    /// Character position at which to truncate text.
    pub truncate_at: usize,
    /// Horizontal padding for cells.
    pub cell_padding: u16,
    /// Horizontal padding for content within areas.
    pub content_padding: u16,
    /// Vertical gap between sections.
    pub section_gap: u16,
    /// Width of the insights panel.
    pub insights_panel_width: u16,
    /// Minimum column width before collapsing idle musicians.
    pub min_column_width: u16,
}

/// Computes layout configuration based on terminal dimensions.
pub fn get_layout_config(width: u16, height: u16) -> LayoutConfig {
    let is_narrow = width < NARROW;
    let is_wide = width > WIDE;
    let is_tall = height > TALL;

    // Insights panel visibility and width
    let show_insights_panel = !is_narrow;
    let insights_panel_width = if is_narrow {
        0
    } else if is_wide {
        INSIGHTS_WIDTH_MAX
    } else {
        let from_percent = (width as f32 * 0.25).floor() as u16;
        INSIGHTS_WIDTH_MIN.max(INSIGHTS_WIDTH.min(from_percent))
    };

    // Musician columns
    let musician_columns = if is_wide { 4 } else if is_narrow { 1 } else { 3 };

    // Output lines based on height
    let max_output_lines = if is_tall {
        MAX_OUTPUT_LINES_MAX
    } else if height < 24 {
        MAX_OUTPUT_LINES_MIN
    } else {
        MAX_OUTPUT_LINES
    };

    // Text truncation based on available width
    let available_width = if show_insights_panel {
        width.saturating_sub(insights_panel_width + 4)
    } else {
        width.saturating_sub(4)
    };
    let truncate_at = 40_usize.max((available_width.saturating_sub(8) as usize).min(120));

    // Cell padding (border/chrome padding)
    let cell_padding = if is_narrow { CELL_PADDING_MIN } else { CELL_PADDING };

    // Content padding (horizontal inset for readable text)
    let content_padding = if is_wide { 2 } else if is_narrow { 0 } else { 1 };

    LayoutConfig {
        show_insights_panel,
        musician_columns,
        max_output_lines,
        truncate_at,
        cell_padding,
        content_padding,
        section_gap: 1,
        insights_panel_width,
        min_column_width: MIN_COLUMN_WIDTH,
    }
}

/// Shrinks a `Rect` by `h_pad` on left/right and `v_pad` on top/bottom.
pub fn padded_rect(area: Rect, h_pad: u16, v_pad: u16) -> Rect {
    Rect {
        x: area.x + h_pad,
        y: area.y + v_pad,
        width: area.width.saturating_sub(h_pad * 2),
        height: area.height.saturating_sub(v_pad * 2),
    }
}

/// Applies `config.content_padding` horizontally to `area`, leaving vertical
/// position and height unchanged.
pub fn inner_content_rect(area: Rect, config: &LayoutConfig) -> Rect {
    padded_rect(area, config.content_padding, 0)
}

/// Compute per-column widths. Active musicians get full columns,
/// idle/done musicians get collapsed strips when space is tight.
pub fn compute_column_widths(
    musicians: &[MusicianState],
    total_width: u16,
    min_col_width: u16,
) -> Vec<u16> {
    let count = musicians.len();
    if count == 0 {
        return vec![];
    }

    // Extreme edge case: absolute minimum 3 chars per musician.
    if (total_width as usize) < count * 3 {
        let w = (total_width as usize / count).max(1) as u16;
        return vec![w; count];
    }

    let even_width = total_width / count as u16;

    // If even distribution gives enough width, or few musicians, use it
    if even_width >= min_col_width || count <= 3 {
        let mut widths = vec![even_width; count];
        // Give remainder to last column
        widths[count - 1] += total_width - even_width * count as u16;
        return widths;
    }

    // Too many musicians for even split — collapse inactive ones
    let mut active_indices: Vec<usize> = vec![];
    let mut inactive_indices: Vec<usize> = vec![];

    for (i, m) in musicians.iter().enumerate() {
        match m.status {
            MusicianStatus::Running | MusicianStatus::Waiting => active_indices.push(i),
            _ => inactive_indices.push(i),
        }
    }

    // If no active musicians, give all equal space
    if active_indices.is_empty() {
        let mut widths = vec![even_width; count];
        widths[count - 1] += total_width - even_width * count as u16;
        return widths;
    }

    let collapsed_total = inactive_indices.len() as u16 * COLLAPSED_WIDTH;
    let active_width = total_width
        .saturating_sub(collapsed_total)
        / active_indices.len() as u16;

    let mut widths = vec![0u16; count];
    for &i in &active_indices {
        widths[i] = min_col_width.max(active_width);
    }
    for &i in &inactive_indices {
        widths[i] = COLLAPSED_WIDTH;
    }

    // Adjust for remainder / overflow
    let used: u16 = widths.iter().sum();
    if used < total_width {
        let last_active = *active_indices.last().unwrap();
        widths[last_active] += total_width - used;
    } else if used > total_width {
        // Proportionally scale down all widths so total never exceeds total_width
        let scale = total_width as f32 / used as f32;
        let mut scaled_total: u16 = 0;
        for w in widths.iter_mut() {
            *w = ((*w as f32 * scale).floor() as u16).max(1);
            scaled_total += *w;
        }
        // Distribute remaining pixels (from floor rounding) to last active column
        let remainder = total_width.saturating_sub(scaled_total);
        if remainder > 0 {
            let last_active = *active_indices.last().unwrap();
            widths[last_active] += remainder;
        }
    }

    widths
}

#[cfg(test)]
mod tests {
    use super::*;
    use conductor_types::state::MusicianState;

    fn make_musician(status: MusicianStatus) -> MusicianState {
        MusicianState {
            id: String::new(),
            index: 0,
            status,
            current_task: None,
            output_lines: vec![],
            started_at: None,
            elapsed_ms: 0,
            worktree_path: None,
            branch: None,
            checkpoint: None,
            prompt_sent: None,
        }
    }

    // ─── get_layout_config tests ─────────────────────────────────────────────

    #[test]
    fn narrow_terminal_hides_insights_panel() {
        let cfg = get_layout_config(60, 20);
        assert!(!cfg.show_insights_panel);
        assert_eq!(cfg.insights_panel_width, 0);
        assert_eq!(cfg.cell_padding, CELL_PADDING_MIN);
        assert_eq!(cfg.musician_columns, 1);
    }

    #[test]
    fn normal_terminal_shows_insights_panel() {
        let cfg = get_layout_config(100, 30);
        assert!(cfg.show_insights_panel);
        assert_eq!(cfg.musician_columns, 3);
        assert_eq!(cfg.max_output_lines, MAX_OUTPUT_LINES);
    }

    #[test]
    fn wide_terminal_sets_4_musician_columns() {
        let cfg = get_layout_config(200, 50);
        assert!(cfg.show_insights_panel);
        assert_eq!(cfg.musician_columns, 4);
        assert_eq!(cfg.insights_panel_width, INSIGHTS_WIDTH_MAX);
        assert_eq!(cfg.max_output_lines, MAX_OUTPUT_LINES_MAX);
    }

    #[test]
    fn short_terminal_uses_min_output_lines() {
        let cfg = get_layout_config(100, 20);
        assert_eq!(cfg.max_output_lines, MAX_OUTPUT_LINES_MIN);
    }

    #[test]
    fn tall_terminal_uses_max_output_lines() {
        let cfg = get_layout_config(100, 50);
        assert_eq!(cfg.max_output_lines, MAX_OUTPUT_LINES_MAX);
    }

    #[test]
    fn insights_width_clamped_for_medium_terminal() {
        // width=100, 25% = 25 which is between MIN(24) and DEFAULT(36) → 25
        let cfg = get_layout_config(100, 30);
        assert_eq!(cfg.insights_panel_width, 25);
    }

    #[test]
    fn insights_width_uses_default_when_wide_enough() {
        // width=148, 25% = 37 > DEFAULT(36) → clamped to 36
        let cfg = get_layout_config(148, 30);
        assert_eq!(cfg.insights_panel_width, INSIGHTS_WIDTH);
    }

    #[test]
    fn content_padding_scales_with_terminal_width() {
        let narrow = get_layout_config(60, 30);
        let normal = get_layout_config(120, 30);
        let wide = get_layout_config(200, 30);
        assert_eq!(narrow.content_padding, 0);
        assert_eq!(normal.content_padding, 1);
        assert_eq!(wide.content_padding, 2);
    }

    #[test]
    fn section_gap_is_always_one() {
        assert_eq!(get_layout_config(60, 20).section_gap, 1);
        assert_eq!(get_layout_config(120, 30).section_gap, 1);
        assert_eq!(get_layout_config(200, 50).section_gap, 1);
    }

    // ─── compute_column_widths tests ─────────────────────────────────────────

    #[test]
    fn three_equal_musicians_even_split() {
        let musicians = vec![
            make_musician(MusicianStatus::Running),
            make_musician(MusicianStatus::Running),
            make_musician(MusicianStatus::Running),
        ];
        let widths = compute_column_widths(&musicians, 90, MIN_COLUMN_WIDTH);
        assert_eq!(widths.len(), 3);
        assert_eq!(widths.iter().sum::<u16>(), 90);
        // All columns should be equal (30 each)
        assert!(widths.iter().all(|&w| w == 30));
    }

    #[test]
    fn five_musicians_two_active_collapses_inactive() {
        let musicians = vec![
            make_musician(MusicianStatus::Running),   // active
            make_musician(MusicianStatus::Completed), // inactive
            make_musician(MusicianStatus::Idle),      // inactive
            make_musician(MusicianStatus::Completed), // inactive
            make_musician(MusicianStatus::Running),   // active
        ];
        let widths = compute_column_widths(&musicians, 120, MIN_COLUMN_WIDTH);
        assert_eq!(widths.len(), 5);
        assert_eq!(widths.iter().sum::<u16>(), 120);
        // Inactive musicians should be collapsed
        assert_eq!(widths[1], COLLAPSED_WIDTH);
        assert_eq!(widths[2], COLLAPSED_WIDTH);
        assert_eq!(widths[3], COLLAPSED_WIDTH);
        // Active musicians should have more width
        assert!(widths[0] >= MIN_COLUMN_WIDTH);
        assert!(widths[4] >= MIN_COLUMN_WIDTH);
    }

    #[test]
    fn edge_case_zero_musicians() {
        let widths = compute_column_widths(&[], 80, MIN_COLUMN_WIDTH);
        assert!(widths.is_empty());
    }

    #[test]
    fn single_musician_gets_full_width() {
        let musicians = vec![make_musician(MusicianStatus::Running)];
        let widths = compute_column_widths(&musicians, 100, MIN_COLUMN_WIDTH);
        assert_eq!(widths, vec![100]);
    }

    #[test]
    fn widths_sum_to_total() {
        // Many musicians, tight space — ensure total is always correct
        let musicians: Vec<MusicianState> = (0..8)
            .map(|i| {
                make_musician(if i % 2 == 0 {
                    MusicianStatus::Running
                } else {
                    MusicianStatus::Idle
                })
            })
            .collect();
        let widths = compute_column_widths(&musicians, 100, MIN_COLUMN_WIDTH);
        assert_eq!(widths.iter().sum::<u16>(), 100);
    }

    // ─── padded_rect tests ───────────────────────────────────────────────────

    #[test]
    fn padded_rect_shrinks_by_padding() {
        let area = Rect { x: 0, y: 0, width: 40, height: 20 };
        let result = padded_rect(area, 2, 1);
        assert_eq!(result.x, 2);
        assert_eq!(result.y, 1);
        assert_eq!(result.width, 36);  // 40 - 2*2
        assert_eq!(result.height, 18); // 20 - 2*1
    }

    #[test]
    fn padded_rect_zero_padding_is_identity() {
        let area = Rect { x: 5, y: 3, width: 80, height: 24 };
        assert_eq!(padded_rect(area, 0, 0), area);
    }

    #[test]
    fn padded_rect_saturates_at_zero_not_underflow() {
        let area = Rect { x: 0, y: 0, width: 2, height: 2 };
        let result = padded_rect(area, 5, 5);
        // width/height should not underflow
        assert_eq!(result.width, 0);
        assert_eq!(result.height, 0);
    }

    // ─── inner_content_rect tests ────────────────────────────────────────────

    #[test]
    fn inner_content_rect_applies_content_padding_horizontally() {
        let area = Rect { x: 0, y: 0, width: 80, height: 24 };
        let config = get_layout_config(200, 30); // wide → content_padding = 2
        let result = inner_content_rect(area, &config);
        assert_eq!(result.x, 2);
        assert_eq!(result.y, 0);       // no vertical change
        assert_eq!(result.width, 76);  // 80 - 2*2
        assert_eq!(result.height, 24); // unchanged
    }

    #[test]
    fn inner_content_rect_zero_padding_on_narrow() {
        let area = Rect { x: 0, y: 0, width: 60, height: 20 };
        let config = get_layout_config(60, 20); // narrow → content_padding = 0
        assert_eq!(inner_content_rect(area, &config), area);
    }
}
