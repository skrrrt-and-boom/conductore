# Ratatui 0.30 Built-in Widgets — Conductor TUI Audit

> **Purpose**: Assess every ratatui 0.30 built-in widget against the current
> conductor TUI implementation and identify concrete upgrade opportunities.
>
> **Codebase scanned** (direct source read, accurate line refs):
> - `crates/conductor-tui/src/theme.rs` — 475 lines
> - `crates/conductor-tui/src/components/musician.rs` — 143 lines
> - `crates/conductor-tui/src/components/insights.rs` — 140 lines
> - `crates/conductor-tui/src/components/panels.rs` — 365 lines
> - `crates/conductor-tui/src/components/header.rs` — 97 lines
> - `crates/conductor-tui/src/components/status.rs` — 62 lines
> - `crates/conductor-tui/src/app.rs` — 556 lines
>
> **ratatui version**: `0.30` (confirmed in `Cargo.toml:28`)

---

## Summary Table

| Widget / Feature | Current State | Conductor Target | Effort |
|-----------------|--------------|-----------------|--------|
| `Gauge` | `pbar()` hand-rolled (`theme.rs:161`) | Replace stats row in musician columns | Drop-in |
| `LineGauge` | `pbar()` same helper | Header bar task-progress segment | Moderate |
| `Sparkline` | `sparkline_data()` helper exists (`theme.rs:149`), **widget never imported** | Musician column token trends | Drop-in |
| `Chart` | Nothing | Token burn rate, cost over time (new Stats view) | Significant |
| `BarChart` | Nothing | Per-musician token comparison in insights panel | Moderate |
| `Tabs` | Implicit switching via phase/flags | Explicit tab bar: Musicians / Tasks / Insights / Log | Moderate |
| `Scrollbar` | `scroll_offset: u16` with no visual indicator (`app.rs:44`) | Musician output panels + task detail modal | Moderate |
| `Canvas` | Flat text list with `←` dep notation (`insights.rs:72`) | Task dependency DAG | Significant |
| `calendar::Monthly` | Nothing | Session history heatmap in session browser | Significant |
| `BorderType` | Plain on every `Block` — no `border_type()` calls | Rounded (focus), Double (modals), Thick (review) | Drop-in |
| `Modifier::*` | `BOLD` in one place only (`panels.rs:193`) | Error lines, section headers, selection, cancelled tasks | Drop-in |
| `Color::Rgb` / `Color::Indexed` | 9 named colors in `theme.rs:14-30` | Richer semantic palette, smooth recency fade | Drop-in |
| `Line::alignment()` | Never used; status gap padded manually (`status.rs:53`) | Centered modal titles, right-aligned stats | Drop-in |

---

## 1. `Gauge` — Horizontal Progress Bar

### What it does
Renders a filled horizontal bar with an optional label centered inside it.
Accepts `ratio(f64)` (0.0–1.0) or `percent(u16)`. Fill and background colors
are independent. Supports Unicode sub-block precision via `use_unicode(true)`.

```rust
use ratatui::widgets::Gauge;

Gauge::default()
    .gauge_style(Style::default().fg(C_ACTIVE).bg(C_FRAME))
    .ratio(0.72)
    .label(Span::styled("72%  89K  3m05s", Style::default().fg(C_TEXT)))
```

### Current conductor equivalent
**`theme.rs:161-165` — `pbar()`**

```rust
pub fn pbar(pct: f64, width: usize) -> String {
    let filled = (pct * width as f64).round() as usize;
    let filled = filled.min(width);
    "█".repeat(filled) + &"░".repeat(width - filled)
}
```

`pbar()` produces a `String` that is then embedded in a `Span`. The `Gauge`
widget does the same thing natively, with no string allocation, and adds label
support. It also respects the render area width automatically.

### Concrete usage in conductor
In `render_musician_column()` (`musician.rs:107-111`), the layout already
carves a 1-row `stats_area`. Replace the plain `Paragraph` stats with a `Gauge`:

```rust
// musician.rs:134-142 — replace:
let tok = theme::format_tokens(musician.tokens_used, musician.tokens_estimated);
let elapsed = theme::elapsed(musician.elapsed_ms);

// Task completion ratio (0.5 = in progress, 1.0 = done, 0.0 = idle)
let ratio = match musician.status {
    MusicianStatus::Running   => 0.5,
    MusicianStatus::Completed => 1.0,
    _                         => 0.0,
};
let gauge = Gauge::default()
    .gauge_style(Style::default()
        .fg(theme::status_display(&musician.status).color)
        .bg(C_FRAME))
    .ratio(ratio)
    .label(format!("{tok}  {elapsed}"));  // existing info preserved as label
f.render_widget(gauge, stats_area);
```

A global task-completion `Gauge` can also go in `header.rs:45-52` alongside
the existing `"{done}/{total} tasks"` span to make phase progress visual.

### Effort: **Drop-in**
No new state needed. `pbar()` callers already have a `f64` ratio. Remove
`pbar()` from `theme.rs`, swap each callsite to `Gauge` or `LineGauge`.

---

## 2. `LineGauge` — Single-Row Thin Progress Bar

### What it does
Like `Gauge` but renders as a single thin line with `filled_style` and
`unfilled_style` for the two halves. Takes a `line_set` parameter:
`symbols::line::NORMAL`, `THICK`, or `DOUBLE`. Ideal for 1-row constraints.

```rust
use ratatui::widgets::LineGauge;
use ratatui::symbols;

LineGauge::default()
    .ratio(0.45)
    .filled_style(Style::default().fg(C_ACTIVE))
    .unfilled_style(Style::default().fg(C_FRAME))
    .line_set(symbols::line::THICK)
```

### Current conductor equivalent
Same `pbar()` helper. `LineGauge` is more appropriate than `Gauge` when the
area is a 1-row bar with other content alongside it.

### Concrete usage in conductor
In `header.rs:43-53`, the task progress block renders `"{done}/{total} tasks"`.
Split the header `Rect` horizontally to give 20 chars to a `LineGauge`:

```rust
// header.rs — after phase spans, carve a 20-char rect for inline progress:
let line_gauge = LineGauge::default()
    .ratio(if total > 0 { done as f64 / total as f64 } else { 0.0 })
    .filled_style(Style::default().fg(C_ACTIVE))
    .unfilled_style(Style::default().fg(C_FRAME))
    .line_set(symbols::line::NORMAL);
f.render_widget(line_gauge, progress_rect);
```

### Effort: **Moderate**
The header is currently a single `Paragraph` — splitting the `Rect` into
sub-areas for gauge + text requires the header to use `Layout` internally.

---

## 3. `Sparkline` — Inline Mini Bar Chart

### What it does
Renders a row of Unicode block characters (▁▂▃▄▅▆▇█) scaled from a `&[u64]`
slice. Width is the render area width. Each value is normalized against the
max in the dataset. Supports `direction()` (LTR or RTL), `max()` override.

```rust
use ratatui::widgets::Sparkline;

let data: &[u64] = &[0, 2, 4, 7, 5, 3, 6, 8, 1];
let sparkline = Sparkline::default()
    .data(data)
    .style(Style::default().fg(C_INFO));
f.render_widget(sparkline, area);
```

### Current conductor equivalent — **halfway done**
**`theme.rs:149-158` — `sparkline_data()`**

```rust
pub fn sparkline_data(values: &[f64], width: usize) -> Vec<u64> {
    (0..width).map(|i| {
        let vi = ((i as f64 / width as f64) * values.len() as f64) as usize;
        let vi = vi.min(values.len().saturating_sub(1));
        let v = values.get(vi).copied().unwrap_or(0.0).clamp(0.0, 1.0);
        (v * 7.0).floor() as u64
    }).collect()
}
```

This helper **converts `&[f64]` → `Vec<u64>` in exactly the 0–7 range that
`Sparkline` consumes**, but the `Sparkline` widget is never imported or used
anywhere in the TUI. The infrastructure is complete; only the widget call is
missing.

### Concrete usage in conductor
In `render_musician_column()` (`musician.rs:107-111`), a 1-row sparkline
above or beside the stats area shows token usage rate over time. If
`token_history: Vec<f64>` is added to `MusicianState`:

```rust
// musician.rs — split stats_area into sparkline + text:
let stats_chunks = Layout::horizontal([
    Constraint::Min(10),    // sparkline fills remaining width
    Constraint::Length(18), // "{tok}  {elapsed}" text
]).split(stats_area);

let spark_data = theme::sparkline_data(&musician.token_history, stats_chunks[0].width as usize);
let sparkline = Sparkline::default()
    .data(&spark_data)
    .style(Style::default().fg(C_INFO));
f.render_widget(sparkline, stats_chunks[0]);
f.render_widget(Paragraph::new(stats_line), stats_chunks[1]);
```

Without `token_history`, a simpler approach: accumulate token deltas in
`UiState` per musician index and use those as the sparkline dataset.

### Effort: **Drop-in** (widget itself) / **Moderate** (adding history data)
The helper is done. The widget import + render call is ~5 lines. The blocking
work is accumulating a token history time-series in `MusicianState` or `UiState`.

---

## 4. `Chart` with `Dataset` — XY Line/Scatter Plot

### What it does
Full-featured XY chart with labeled axes, configurable bounds, multiple
overlaid datasets, and three marker styles: `Marker::Dot`, `Marker::Braille`
(high-resolution), `Marker::Block`. `GraphType::Line` connects points;
`GraphType::Scatter` renders dots only.

```rust
use ratatui::widgets::{Chart, Dataset, Axis};
use ratatui::widgets::GraphType;
use ratatui::symbols;

let dataset = Dataset::default()
    .name("tokens/s")
    .marker(symbols::Marker::Braille)
    .graph_type(GraphType::Line)
    .style(Style::default().fg(C_INFO))
    .data(&[(0.0, 0.0), (10.0, 120.0), (20.0, 350.0), (30.0, 280.0)]);

let chart = Chart::new(vec![dataset])
    .block(Block::default().title(" Token Burn Rate ").borders(Borders::ALL))
    .x_axis(Axis::default()
        .title("Time (s)")
        .bounds([0.0, 60.0])
        .labels(["0", "30s", "60s"]))
    .y_axis(Axis::default()
        .title("Tokens/s")
        .bounds([0.0, 500.0])
        .labels(["0", "250", "500"]));
```

### Current conductor equivalent
Nothing — no time-series visualization exists. `header.rs:71-83` shows
cumulative totals as text only (`"89K  3m05s  $0.24"`).

### Concrete usage in conductor
A new `UiState` field `metrics_history: Vec<(f64, f64)>` (elapsed_secs,
tokens_per_sec) updated each state change. Render in the insights panel
bottom third, or a new `s`-key-toggled "Stats" overlay:

```rust
// New render_metrics_chart() called from render_content():
let chart = Chart::new(vec![
    Dataset::default()
        .name("tokens/s")
        .marker(symbols::Marker::Braille)
        .graph_type(GraphType::Line)
        .style(Style::default().fg(C_INFO))
        .data(&ui.token_rate_history),
    Dataset::default()
        .name("cost/min")
        .marker(symbols::Marker::Dot)
        .graph_type(GraphType::Line)
        .style(Style::default().fg(C_READY))
        .data(&ui.cost_rate_history),
])
.x_axis(Axis::default().bounds([t_start, t_now]))
.y_axis(Axis::default().bounds([0.0, max_rate]));
```

Chart needs ~8 rows minimum to be readable. The insights panel at
`app.rs:478-491` has the height for it.

### Effort: **Significant**
Requires: (1) metric accumulation in `UiState` across state update ticks,
(2) derivative computation (rate = delta_tokens / delta_time), (3) layout
changes to the insights panel or a new overlay, (4) axis bound computation.

---

## 5. `BarChart` — Grouped Bar Comparison

### What it does
Renders vertical or horizontal bars from `BarGroup`/`Bar` structs. Each
`Bar` can have an independent style, value label, and text label. Multiple
`BarGroup`s stack grouped series. Supports `.direction(Direction::Horizontal)`
for horizontal layout.

```rust
use ratatui::widgets::{BarChart, Bar, BarGroup};

let bars: Vec<Bar> = vec![
    Bar::default().value(45_000).label("M1".into()).style(Style::default().fg(C_INFO)),
    Bar::default().value(89_000).label("M2".into()).style(Style::default().fg(C_ACTIVE)),
    Bar::default().value(12_000).label("M3".into()).style(Style::default().fg(C_DIM)),
];

let chart = BarChart::default()
    .block(Block::default().title(" Tokens Used "))
    .bar_width(5)
    .bar_gap(1)
    .value_style(Style::default().fg(C_TEXT))
    .data(BarGroup::default().bars(&bars));
```

**API note for ratatui 0.30**: The old `&[(label, u64)]` tuple API is removed.
Use `BarGroup` + `Bar` structs.

### Current conductor equivalent
Per-musician token totals appear as plain text in `musician.rs:135` stats line
(`"{tok}  {elapsed}"`). No comparative view across musicians exists.

### Concrete usage in conductor
In `render_insight_panel()` (`insights.rs:18-68`), carve the top 6–8 rows
for a `BarChart` showing all musicians' `tokens_used` side by side:

```rust
// In render_insight_panel, split inner vertically:
let panel_chunks = Layout::vertical([
    Constraint::Length(6),  // bar chart
    Constraint::Min(4),     // insight list (existing)
]).split(inner);

let bars: Vec<Bar> = state.musicians.iter().map(|m| {
    Bar::default()
        .value(m.tokens_used)
        .label(format!("M{}", m.index + 1).into())
        .style(Style::default().fg(theme::status_display(&m.status).color))
}).collect();

let chart = BarChart::default()
    .bar_width(3)
    .bar_gap(1)
    .value_style(Style::default().fg(C_DIM))
    .data(BarGroup::default().bars(&bars));
f.render_widget(chart, panel_chunks[0]);
```

`tokens_used` is `u64` on `MusicianState` — matches `Bar::value(u64)` directly.

### Effort: **Moderate**
Needs: (1) layout split in `render_insight_panel`, (2) chart render call.
No new state required.

---

## 6. `Tabs` — View Switcher

### What it does
Stateless horizontal tab bar. `select(usize)` marks the active tab.
Tab titles accept `Vec<Line>` so each can contain styled spans.
`divider()` sets the separator character between tabs.
`highlight_style()` styles the active tab.

```rust
use ratatui::widgets::Tabs;

let tabs = Tabs::new(vec!["Musicians", "Tasks", "Insights", "Log"])
    .select(ui.active_tab)
    .style(Style::default().fg(C_DIM))
    .highlight_style(Style::default().fg(C_BRAND).add_modifier(Modifier::BOLD))
    .divider(symbols::DOT);
```

### Current conductor equivalent
No tab concept exists. `app.rs:420-460` (`render_all`) switches views
implicitly: `PlanReview` phase replaces the musician grid, the insights panel
is toggled by an `i` key (not wired in visible keybinds), and there is no
way to see a "Tasks only" or "Conductor Log" view.

### Concrete usage in conductor
Extend `UiState` (`app.rs:27-54`) with `active_tab: usize` and add a tab bar
row to the main layout:

```rust
// app.rs:424-432 — extend main layout to 5 rows:
let main_chunks = Layout::vertical([
    Constraint::Length(1),  // header (existing)
    Constraint::Length(1),  // tab bar (NEW)
    Constraint::Min(3),     // content (existing)
    Constraint::Length(1),  // status (existing)
    Constraint::Length(1),  // prompt (existing)
]).split(size);

let tabs = Tabs::new(["Musicians", "Tasks", "Insights", "Log"])
    .select(ui.active_tab)
    .style(Style::default().fg(C_DIM))
    .highlight_style(Style::default().fg(C_BRAND));
f.render_widget(tabs, main_chunks[1]);
```

`render_content()` at `app.rs:463` would dispatch on `active_tab` instead of
the current mix of phase checks and boolean flags. Key bindings `1–4` or
`F1–F4` switch tabs; `Tab`/`BackTab` already cycle musician focus so a new
modifier is needed to distinguish.

### Effort: **Moderate**
Requires: (1) `active_tab` field in `UiState`, (2) 1-row layout change,
(3) `render_content()` restructure to switch on tab index, (4) key handler
update.

---

## 7. `Scrollbar` — Side Scroll Position Indicator

### What it does
Stateful widget — uses `render_stateful_widget`. `ScrollbarState` tracks
`content_length` (total scrollable lines), `position` (current scroll offset),
and `viewport_content_length` (visible lines). Renders a thumb on a track with
optional begin/end arrows. Supports vertical and horizontal orientation.

```rust
use ratatui::widgets::{Scrollbar, ScrollbarOrientation, ScrollbarState};

// Each frame, construct from existing scroll state:
let mut sb_state = ScrollbarState::new(total_lines)
    .position(scroll_offset as usize);

let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
    .begin_symbol(Some("↑"))
    .end_symbol(Some("↓"))
    .thumb_symbol("█");

f.render_stateful_widget(scrollbar, area, &mut sb_state);
```

### Current conductor equivalent — **state exists, no visual**
**`app.rs:44` — `scroll_offset: u16`**

Two components use `scroll_offset` with zero visual feedback:
- `musician.rs:131` — `Paragraph::new(lines).scroll((scroll_offset, 0))`
- `panels.rs:359-362` — `Paragraph::new(lines).scroll((scroll_offset, 0))`

Users scrolling long musician output have no way to know their position in
the stream or how much content is above/below.

### Concrete usage in conductor

In `render_musician_column()` (`musician.rs:108-132`), overlay a scrollbar
on the right edge of `output_area` only when content overflows:

```rust
// musician.rs — after rendering the output Paragraph:
let total_lines = musician.output_lines.len();
if total_lines > output_area.height as usize {
    let mut sb_state = ScrollbarState::new(total_lines)
        .position(scroll_offset as usize);
    f.render_stateful_widget(
        Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(None)
            .end_symbol(None)
            .thumb_symbol("▐"),
        output_area,
        &mut sb_state,
    );
}
```

Same pattern in `render_task_detail()` at `panels.rs:359`.

**Key design note**: `render_stateful_widget` requires `&mut ScrollbarState`,
which means the state must come from `UiState`, not be constructed inline —
or it must be accepted as a `&mut` parameter in the render function. Constructing
inline per-frame (as shown above) works but throws away the state; keeping it in
`UiState` lets ratatui animate the thumb smoothly.

### Effort: **Moderate**
`scroll_offset` already exists. Add `ScrollbarState` to `UiState`, thread it
through `render_musician_column` and `render_task_detail` as `&mut` parameters,
call `render_stateful_widget`.

---

## 8. `Canvas` — Free-Form Drawing Surface

### What it does
A drawing surface using a coordinate system (`x_bounds`, `y_bounds`). A `paint`
closure receives a `Context` and calls `ctx.draw()` with primitive shapes:
`Line`, `Rectangle`, `Circle`, `Points`, `Map`. Text is placed with `ctx.print()`.
`Marker::Braille` gives 2×4 sub-cell resolution for smooth diagonal lines.

```rust
use ratatui::widgets::Canvas;
use ratatui::widgets::canvas::{Line as CanvasLine, Circle};

let canvas = Canvas::default()
    .block(Block::default().title(" Task DAG ").borders(Borders::ALL))
    .x_bounds([0.0, 100.0])
    .y_bounds([0.0, 50.0])  // NOTE: origin is bottom-left; y increases upward
    .marker(symbols::Marker::Braille)
    .paint(|ctx| {
        ctx.draw(&CanvasLine { x1: 10.0, y1: 40.0, x2: 50.0, y2: 20.0, color: C_DIM });
        ctx.draw(&Circle { x: 50.0, y: 20.0, radius: 3.0, color: C_ACTIVE });
        ctx.print(48.0, 20.0, Span::styled("T3", Style::default().fg(C_TEXT)));
    });
```

**Coordinate system gotcha**: `y_bounds = [min, max]` where `max` is the *top*
of the canvas. This is opposite to terminal row ordering — factor in when
mapping task rows to Y coordinates.

### Current conductor equivalent
**`insights.rs:72-117` — `render_task_graph()`**

A flat `Paragraph` with lines like `" ● 1. Title ← 2,3"`. Dependencies are
text suffixes, not visual connections. With 8+ tasks and multi-level dependency
chains, the structure is opaque.

### Concrete usage in conductor
A Canvas-based DAG replaces `render_task_graph()`:

```rust
// New render_task_dag() replacing render_task_graph():
// 1. Topological sort tasks → assign column (depth level)
// 2. Assign row within each column
// 3. Map (col, row) → (canvas_x, canvas_y)

Canvas::default()
    .x_bounds([0.0, area.width as f64])
    .y_bounds([0.0, area.height as f64 * 2.0]) // Braille doubles Y resolution
    .marker(symbols::Marker::Braille)
    .paint(|ctx| {
        for task in tasks {
            let (nx, ny) = dag_pos(task.index, &positions);
            // Draw dependency edges first (behind nodes)
            for &dep_idx in &task.dependencies {
                let (dx, dy) = dag_pos(dep_idx, &positions);
                ctx.draw(&CanvasLine { x1: dx, y1: dy, x2: nx, y2: ny, color: C_DIM });
            }
            // Draw node
            let viz = theme::task_viz(&task.status);
            ctx.print(nx - 1.0, ny, Span::styled(
                format!("{} T{}", viz.dot, task.index + 1),
                Style::default().fg(viz.color),
            ));
        }
    })
```

The layout algorithm (topological sort + coordinate assignment) is ~60–80 lines
of additional logic. Canvas API usage itself is ~30 lines.

### Effort: **Significant**
The Canvas API is straightforward. The work is the DAG layout algorithm:
topological sort of `task.dependencies`, column-depth assignment, Y packing
within columns. This lives in a new `dag_layout.rs` helper.

---

## 9. `calendar::Monthly` — Monthly Calendar View

### What it does
Renders a calendar grid for a given month. Each day can be styled via
`CalendarEventStore` mapping `time::Date` → `Style` for session highlights.

```rust
use ratatui::widgets::calendar::{CalendarEventStore, Monthly};
use time::{Date, Month};

let mut events = CalendarEventStore::default();
events.add(
    Date::from_calendar_date(2025, Month::March, 15).unwrap(),
    Style::default().fg(C_BRAND),
);

let cal = Monthly::new(
    Date::from_calendar_date(2025, Month::March, 1).unwrap(),
    events,
)
.show_month_header(Style::default().fg(C_TEXT))
.show_weekdays_header(Style::default().fg(C_DIM));
```

### Dependency note
`calendar::Monthly` uses the `time` crate for date types. The conductor
workspace uses `chrono` (`Cargo.toml:24`). A new dependency is required:

```toml
# Cargo.toml workspace.dependencies:
time = { version = "0.3", features = ["formatting"] }
```

A conversion shim from `chrono::NaiveDate` → `time::Date` is ~4 lines.

### Current conductor equivalent
**`panels.rs:169-228` — `render_session_browser()`**

A plain `Table` of session ID / phase / task count / tokens. No date-based
navigation exists; users must scroll through an ordered list.

### Concrete usage in conductor
Split `render_session_browser()` horizontally: calendar on the left, detail
table on the right:

```rust
// In render_session_browser():
let chunks = Layout::horizontal([
    Constraint::Length(22),  // calendar (~22 chars wide)
    Constraint::Min(30),     // existing session table
]).split(inner);

let mut events = CalendarEventStore::default();
for session in sessions {
    if let Some(date) = session.created_at_as_time_date() {
        events.add(date, Style::default().fg(C_ACTIVE));
    }
}
let cal = Monthly::new(current_month, events)
    .show_month_header(Style::default().fg(C_TEXT))
    .show_weekdays_header(Style::default().fg(C_DIM));
f.render_widget(cal, chunks[0]);
// existing table renders in chunks[1]
```

### Effort: **Significant**
New `time` crate dependency, date conversion logic in `SessionData`, layout
change to the session browser. Lower priority — the table-based browser
already covers the use case adequately.

---

## 10. `Block` Border Types

### Current state
Every `Block` in the codebase uses `Block::default().borders(Borders::ALL)`
with no `.border_type()` call, defaulting to `BorderType::Plain`.
A grep of all `.rs` files confirms zero `border_type` calls exist.

### Available `BorderType` variants

| Variant | Characters | Visual Effect |
|---------|-----------|---------------|
| `Plain` (default) | `─ │ ┌ ┐ └ ┘` | Thin single-line (current everywhere) |
| `Rounded` | `─ │ ╭ ╮ ╰ ╯` | Rounded corners — softer, modern |
| `Double` | `═ ║ ╔ ╗ ╚ ╝` | Double-line — strong visual weight |
| `Thick` | `━ ┃ ┏ ┓ ┗ ┛` | Bold single-line — maximum emphasis |
| `QuadrantInside` | block chars | Optical "inset panel" effect |
| `QuadrantOutside` | block chars | Optical "raised panel" effect |

### Concrete usage in conductor

**Recommended per-component scheme:**

```rust
// musician.rs:92 — focused column gets Rounded:
let block = Block::default()
    .borders(Borders::ALL)
    .border_type(if is_focused { BorderType::Rounded } else { BorderType::Plain })
    .border_style(Style::default().fg(border_color));

// panels.rs:33 — Plan Review (blocking phase): Thick border signals urgency:
Block::default()
    .borders(Borders::ALL)
    .border_type(BorderType::Thick)
    .border_style(Style::default().fg(C_BRAND))

// panels.rs:178 — Session browser overlay: Double lifts it above background:
Block::default()
    .borders(Borders::ALL)
    .border_type(BorderType::Double)
    .border_style(Style::default().fg(C_BRAND))

// panels.rs:247 — Task detail modal: Double (same as session browser):
Block::default()
    .borders(Borders::ALL)
    .border_type(BorderType::Double)
    .border_style(Style::default().fg(C_BRAND))

// insights.rs:21 — Insights panel: Rounded (soft/informational):
Block::default()
    .borders(Borders::ALL)
    .border_type(BorderType::Rounded)
    .border_style(Style::default().fg(C_FRAME))
```

A `theme::focused_border_block(is_focused)` helper could return a pre-built
`Block` encapsulating border type + color, replacing the current
`theme::focus_border_color()` at `theme.rs:202`.

### Effort: **Drop-in**
One `.border_type(X)` addition per `Block`. Pure visual change, zero logic
or state impact.

---

## 11. `Style` Modifiers

### Current state
`Modifier::BOLD` appears in exactly one place: `panels.rs:193` (session browser
table header). No other modifiers are used anywhere.

### Available modifiers

| Modifier | Effect | Terminal support |
|----------|--------|-----------------|
| `BOLD` | Heavier font weight | Universal |
| `DIM` | Reduced intensity | Universal |
| `ITALIC` | Slanted text | Most modern terminals |
| `UNDERLINED` | Underline | Universal |
| `REVERSED` | Swap fg/bg | Universal |
| `CROSSED_OUT` | Strikethrough | Most modern terminals |
| `SLOW_BLINK` | ~0.5Hz blink | Limited (avoid for UI) |
| `RAPID_BLINK` | >3Hz blink | Very limited (avoid) |
| `HIDDEN` | Invisible text | Universal |

Note: `Modifier::DIM` and `Color::DarkGray` (`C_DIM`) serve similar purposes
via different mechanisms. `Color::DarkGray` is more predictable across terminal
themes and is already used consistently.

### Concrete opportunities in conductor

| Location | Current | Proposed |
|----------|---------|----------|
| `musician.rs:97` — focused column title | `fg(C_BRAND)` | `+ Modifier::BOLD` |
| `panels.rs:268-310` — section labels `"DESCRIPTION"`, `"WHY"`, `"FILES"` | `fg(C_DIM)` | `+ Modifier::UNDERLINED` |
| `panels.rs:111-113` — plan review selected task | `fg(C_BRAND)` | `Modifier::REVERSED` (conventional TUI selection) |
| `panels.rs:199-201` — session browser selected row | `fg(C_BRAND)` | `Modifier::REVERSED` |
| `theme.rs:183` — `output_line_style()` ERROR lines | `fg(Color::Red)` | `+ Modifier::BOLD` |
| `theme.rs:91-101` — `task_viz()` Cancelled/Blocked | `fg(C_DIM)` on dot | Apply `Modifier::CROSSED_OUT` to task *title* text |
| `panels.rs:321-323` — task result `"Success"`/`"Failed"` | `fg(status_color)` | `+ Modifier::BOLD` |

```rust
// theme.rs:179-193 — ERROR lines should be bold:
} else if line.starts_with("ERROR") {
    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)

// panels.rs:111 — selection via REVERSED is more idiomatic:
let sel_style = if is_selected {
    Style::default().add_modifier(Modifier::REVERSED)
} else {
    Style::default().fg(C_TEXT)
};

// insights.rs task title for Cancelled tasks:
if matches!(task.status, TaskStatus::Cancelled | TaskStatus::Blocked) {
    title_style = title_style.add_modifier(Modifier::CROSSED_OUT);
}
```

### Effort: **Drop-in**
Each is a `.add_modifier()` call on an existing `Style`. No new state.

---

## 12. `Color::Rgb` and `Color::Indexed` — Richer Palette

### Current state (`theme.rs:14-30`)
All 9 semantic colors use named terminal colors:

```rust
pub const C_FRAME:  Color = Color::DarkGray;
pub const C_TEXT:   Color = Color::White;
pub const C_DIM:    Color = Color::DarkGray;   // same as C_FRAME!
pub const C_BRAND:  Color = Color::Cyan;
pub const C_ACTIVE: Color = Color::Green;
pub const C_READY:  Color = Color::Yellow;
pub const C_ERROR:  Color = Color::Red;
pub const C_INFO:   Color = Color::Blue;
pub const C_ACCENT: Color = Color::Magenta;
```

**Notable**: `C_FRAME` and `C_DIM` are identical (`Color::DarkGray`). They
serve different semantic purposes (structure vs. secondary text) but cannot
be visually distinguished in the current palette.

### `Color::Rgb(r, g, b)` — 24-bit true color
Requires terminal true color support (iTerm2, Alacritty, Kitty, WezTerm,
Windows Terminal, most Linux terminals from ~2016+).

**Proposed palette expansion for `theme.rs`:**

```rust
// Additional semantic constants using Rgb:

/// Separator/frame color — precise dark gray, distinct from C_DIM
pub const C_FRAME:   Color = Color::Rgb(50, 50, 55);
/// Secondary text — slightly lighter than frame
pub const C_DIM:     Color = Color::Rgb(90, 90, 100);
/// Brand cyan — tuned for dark backgrounds
pub const C_BRAND:   Color = Color::Rgb(0, 210, 255);
/// Success/completed (softer than terminal Green)
pub const C_ACTIVE:  Color = Color::Rgb(80, 200, 120);
/// Running/in-progress (deep amber, distinct from completed)
pub const C_READY:   Color = Color::Rgb(255, 175, 40);
/// Error — slightly softer red
pub const C_ERROR:   Color = Color::Rgb(220, 70, 70);
/// Data visualization (teal, distinct from C_BRAND cyan)
pub const C_INFO:    Color = Color::Rgb(40, 160, 200);
/// Cost display — gold, not green (money ≠ success)
pub const C_COST:    Color = Color::Rgb(220, 180, 40);
/// Conductor agent messages (soft purple, distinct from user cyan)
pub const C_CONDUCTOR: Color = Color::Rgb(160, 120, 255);
```

### `Color::Indexed(u8)` — xterm-256 palette
Supported by virtually all modern terminals. Indices 232–255 are a grayscale
ramp from near-black (232) to near-white (255).

**Use in `output_line_style()` (`theme.rs:179-193`)** for smoother recency
fade instead of the current 3-step (White/Gray/DarkGray):

```rust
pub fn output_line_style(line: &str, recency: f64) -> Style {
    let color = if line.starts_with("[USER]") {
        Color::Cyan
    } else if line.starts_with('>') {
        Color::Indexed(240) // ~#585858, tool output
    } else if line.starts_with("ERROR") {
        Color::Red
    } else {
        // Smooth 18-step ramp: index 238 (~#444) → 255 (~#eee)
        let idx = 238 + (recency * 17.0) as u8;
        Color::Indexed(idx.min(255))
    };
    Style::default().fg(color)
}
```

### Effort: **Drop-in** (constant swap)
Replace constants in `theme.rs`. For `Color::Rgb` safety, add a compile-time
or runtime truecolor detection guard (or make it a config flag).

---

## 13. `Line::alignment()` and `Span` Advanced Features

### `Line::alignment()`
`Line` in ratatui 0.30 supports `.alignment(Alignment::Center | Right | Left)`.
This is per-line; `Paragraph::alignment()` sets the default for all lines.

**Current state**: Never used. `status.rs:51-55` manually computes a gap string
to right-align hint text:

```rust
// status.rs:51-55 — current manual right-alignment:
let left_len: usize = left.iter().map(|s| s.width()).sum();
let right_len = hint.len();
let gap = (area.width as usize).saturating_sub(left_len + right_len + 1);
left.push(Span::raw(" ".repeat(gap)));
left.push(Span::styled(hint, Style::default().fg(C_DIM)));
```

With `Line::alignment()`, a two-`Paragraph` approach (left-aligned + right-aligned
over the same `Rect`) eliminates the gap computation:

```rust
// status.rs — cleaner:
let left_para = Paragraph::new(Line::from(left));
let right_para = Paragraph::new(hint).alignment(Alignment::Right);
f.render_widget(left_para, area);
f.render_widget(right_para, area);
```

**Modal title centering** (`panels.rs:34,182,244`): All modal titles are
left-aligned block titles. `Alignment::Center` on the title `Line`:

```rust
// panels.rs:34 — centered Plan Review title:
Block::default()
    .title(
        Line::from(Span::styled(" Plan Review ", Style::default().fg(C_BRAND)))
            .alignment(Alignment::Center)
    )
```

### Hyperlinks in ratatui 0.30
Ratatui 0.30 does **not** have built-in OSC 8 hyperlink support in the widget
API. Clickable terminal hyperlinks require injecting raw escape sequences
manually outside the widget pipeline — not recommended for the conductor TUI
without a dedicated abstraction.

### Effort: **Drop-in**
Alignment: 1-line change per affected widget. Removes manual gap calculation
in `status.rs`.

---

## Priority Roadmap

### Tier 1 — Immediate wins (drop-in, high impact, zero risk)
1. **`BorderType::Rounded`** on focused musician columns (`musician.rs:92`) —
   visual focus differentiation beyond color alone
2. **`BorderType::Double`** on modal overlays (`panels.rs:178, 247`) —
   clear visual elevation
3. **`Modifier::REVERSED`** for selected list items (`panels.rs:111, 199`) —
   conventional TUI selection pattern
4. **`Modifier::BOLD`** on focused column titles and ERROR lines
5. **`Modifier::CROSSED_OUT`** on Cancelled/Blocked task titles (`insights.rs`)
6. **`Line::alignment(Alignment::Center)`** on modal titles; remove gap hack
   in `status.rs`

### Tier 2 — Near-term (moderate effort, meaningful UX lift)
7. **`Scrollbar`** on musician output + task detail — add `ScrollbarState` to
   `UiState`, thread through render functions as `&mut`
8. **`Sparkline`** widget — the data helper `sparkline_data()` is done; add
   history accumulation to `MusicianState` or `UiState`
9. **`Gauge`** replacing `pbar()` — drop `theme::pbar()`, use `Gauge` with
   the existing ratio values
10. **`BarChart`** in insights panel — per-musician token comparison,
    no new state required
11. **`Color::Rgb`** palette — separate `C_FRAME` from `C_DIM` (currently
    identical), add `C_COST` and `C_CONDUCTOR` semantic colors
12. **`Tabs`** for explicit view switching — add `active_tab` to `UiState`,
    restructure `render_content()`

### Tier 3 — Long-term (significant effort, high payoff)
13. **`Canvas` DAG** — replace flat task list with proper node-link graph;
    requires topological sort + coordinate layout algorithm (~150 lines)
14. **`Chart`** time-series — requires metric accumulation infrastructure
    in `UiState`, derivative (rate) computation
15. **`calendar::Monthly`** — new `time` crate dependency; low priority unless
    session history navigation becomes a core feature

---

## Appendix: ratatui 0.30 API Notes

- **`BarChart`**: Old `&[(label, u64)]` tuple API removed in ~0.27. Use
  `BarGroup::default().bars(&[Bar::default().value(n).label(...)...])`.

- **`Sparkline` direction**: Gained `.direction(RenderDirection::RightToLeft)`
  in ~0.27 — useful for "most recent on right" time-series display.

- **`Calendar`**: Uses `time::Date`, not `chrono::DateTime`. The conductor
  workspace currently depends on `chrono` only; `time` would be a new
  workspace dependency.

- **`Canvas` Y axis**: `y_bounds = [min, max]` where `max` is the *top* of
  the canvas (coordinate origin bottom-left). Terminal row 0 is at the top,
  which is the *opposite* convention — flip Y when mapping terminal rows to
  canvas coordinates.

- **`Block::title_alignment()`**: Removed; use `Line::alignment()` on the
  `Line` passed to `Block::title()` instead.

- **`Scrollbar`**: Stateful widget — requires `render_stateful_widget` with
  `&mut ScrollbarState`. Cannot use `render_widget`.

- **`Chart` axis labels**: `.labels()` accepts `Vec<Span>` since ~0.28,
  enabling colored/styled axis tick labels.
