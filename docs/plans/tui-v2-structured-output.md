# TUI V2: Structured Tool Call Rendering

> **Status**: Design
> **Author**: Musician Agent
> **Date**: 2026-03-22

## Problem

Musician output panels currently render all content as flat `Vec<String>` lines with prefix-based color heuristics. Structured tool calls (file reads, edits, bash commands) appear as undifferentiated text, making it hard to scan a musician's activity at a glance. This is the biggest visual quality gap in the TUI.

---

## 1. Current Data Flow (with code references)

### Step 1: NDJSON from Claude CLI

Claude Code emits newline-delimited JSON. An assistant turn with a tool call produces a JSON object like:

```json
{
  "type": "assistant",
  "message": {
    "content": [
      { "type": "text", "text": "Let me read that file." },
      { "type": "tool_use", "name": "Read", "input": { "file_path": "/src/main.rs" } }
    ]
  }
}
```

### Step 2: NDJSON → ClaudeEvent (`conductor-bridge/src/parse.rs`)

`parse_claude_event_inner()` (parse.rs:56–170) iterates over content blocks and produces **typed** `ClaudeEvent` structs:

- `"text"` blocks → `ClaudeEvent { event_type: Assistant, message: Some("Let me read..."), ... }` (parse.rs:79–84)
- `"thinking"` blocks → `ClaudeEvent { event_type: Assistant, subtype: Some("thinking"), message: Some("[Thinking...]") }` (parse.rs:86–89)
- `"tool_use"` blocks → `ClaudeEvent { event_type: ToolUse, tool_name: Some("Read"), tool_input: Some({...}), ... }` (parse.rs:91–97)
- `"user"` with `tool_use_result` → `ClaudeEvent { event_type: ToolResult, tool_result_content: Some("file contents") }` (parse.rs:104–118)
- `"rate_limit_event"` → `ClaudeEvent { event_type: Error, subtype: Some("rate_limit"), resets_at: ... }` (parse.rs:121–155)
- `"result"` → `ClaudeEvent { event_type: Result, result: Some("..."), duration_ms, num_turns, is_error }` (parse.rs:157–166)

**At this stage, full structure is preserved.** Tool name, input parameters, and result content are all separate typed fields on `ClaudeEvent` (defined in `conductor-types/src/state.rs:393–418`).

### Step 3: ClaudeEvent → output_lines (THE LOSSY STEP) (`conductor-core/src/musician.rs`)

The `execute()` method (musician.rs:234–535) processes events in a `loop` and flattens them:

| Event Type | Code Location | What Gets Pushed |
|---|---|---|
| `Assistant` | musician.rs:363–374 | Raw `message` string → `push_output(message)` |
| `ToolUse` | musician.rs:376–417 | `summarize_tool_use()` → `push_output(&format!("> {summary}"))` |
| `ToolResult` | musician.rs:420 | **Completely discarded** — `{}` empty match arm |
| `Error` | musician.rs:447–482 | `push_output(&format!("ERROR: {msg}"))` |
| `Result` | musician.rs:422–445 | Sets `received_result`/`result_was_error` flags, checks for rate limit |
| User injection | musician.rs:342–357 | `push_output(&format!("[USER] {msg}"))` via `guidance_rx` select branch |

**The summarization step** (`tool_summary.rs:18–53`) maps tool names to short descriptions:
- `Read` → `"Reading {path}"`, `Write` → `"Writing {path}"`, `Edit` → `"Editing {path}"`
- `Glob` → `"Searching files: {pattern}"`, `Grep` → `"Searching code: {pattern}"`
- `Bash` → `"Running: {command}"` (truncated to 60 chars)
- All others → just the tool name

**The buffer** (`push_output()`, musician.rs:540–556) splits on newlines, caps at 200 lines / 100KB total chars, evicting oldest lines first.

**What's lost:**
- Tool input parameters (file paths, commands, patterns) — only a summary string survives
- Tool results (file contents, command output, exit codes) — entirely dropped
- Block boundaries — a text block followed by a tool use merges into the same flat list
- Semantic type information — recovered only via fragile string prefix matching at render time
- Edit details (old_text/new_text) — available in `tool_input` JSON but never extracted

**Note:** `AnalystState` (state.rs:250–258) also uses `output_lines: Vec<String>` with the same flat pattern. The structured output design applies to analysts too, but this plan focuses on musicians first.

### Step 4: Rendering (`conductor-tui/src/components/musician.rs`)

`render_musician_column()` (tui/components/musician.rs:79–163) reads `output_lines: Vec<String>` and applies `output_line_style()` (theme.rs:187–202) which uses prefix heuristics:

```
"[USER]..." → Cyan
">"         → DarkGray
"ERROR..."  → Red
(else)      → White/Gray/DarkGray based on recency
```

All lines get the same single-span `Paragraph` treatment — no icons, no collapsibility, no structural differentiation.

### Side Channel: OrchestraEvent (events.rs:4–41)

Events are also sent to the orchestra via `mpsc` channel:
- `MusicianOutput { musician_id, line: String }` — carries the same flat string (musician.rs:368–372)
- `MusicianToolUse { musician_id, tool_name, tool_input }` — carries structured data, but only used for tracking, not display

The `MusicianToolUse` event already preserves full structure. A future enhancement could propagate `OutputBlock` through the event channel too, but it's not required for TUI rendering since the TUI reads `MusicianState` via the `watch` channel, not individual events.

---

## 2. Proposed Structured Output Model

### 2.1 New Type: `OutputBlock`

Replace the flat `output_lines: Vec<String>` with `output_blocks: Vec<OutputBlock>`:

```rust
use serde::{Deserialize, Serialize};

/// A single semantic block of musician output.
///
/// Each block represents a discrete unit of work that can be
/// rendered with type-specific styling in the TUI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputBlock {
    /// When this block was created (RFC 3339).
    pub timestamp: String,
    /// The semantic content of this block.
    pub content: BlockContent,
    /// Whether this block is collapsed in the TUI.
    /// Only meaningful for collapsible block types.
    #[serde(default)]
    pub collapsed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BlockContent {
    /// Free-form assistant text output.
    Text(String),

    /// File read operation.
    FileRead {
        path: String,
        /// Number of lines read (from tool input, if available).
        line_count: Option<u32>,
    },

    /// File write (new file creation).
    FileWrite {
        path: String,
    },

    /// File edit operation (modification of existing file).
    FileEdit {
        path: String,
        /// Raw old_text/new_text from Edit tool input, if available.
        /// Can be rendered as inline diff.
        old_text: Option<String>,
        new_text: Option<String>,
    },

    /// Shell command execution.
    BashCommand {
        command: String,
        /// Output captured from the subsequent ToolResult event.
        output: Option<String>,
        /// Exit code if available.
        exit_code: Option<i32>,
    },

    /// Search operation (Glob or Grep).
    Search {
        tool: String,       // "Glob" or "Grep"
        pattern: String,
        /// Result summary (file count, match count).
        result_summary: Option<String>,
    },

    /// Generic tool call (for tools without specific rendering).
    ToolCall {
        name: String,
        summary: String,
    },

    /// Error message.
    Error(String),

    /// User-injected message (guidance or prompt).
    UserMessage(String),

    /// Thinking indicator (extended thinking block).
    Thinking,
}
```

### 2.2 Updated `MusicianState`

```rust
pub struct MusicianState {
    pub id: String,
    pub index: usize,
    pub status: MusicianStatus,
    pub current_task: Option<Task>,
    // NEW: structured output blocks
    pub output_blocks: Vec<OutputBlock>,
    // KEPT: flat lines for backward compatibility during migration
    pub output_lines: Vec<String>,
    pub started_at: Option<String>,
    pub elapsed_ms: u64,
    pub worktree_path: Option<String>,
    pub branch: Option<String>,
    pub checkpoint: Option<Checkpoint>,
    pub prompt_sent: Option<String>,
}
```

### 2.3 Why This Shape

- **`OutputBlock` wraps `BlockContent`** — the timestamp and collapsed state are orthogonal to the content type, so they live in the wrapper rather than being duplicated in every variant.
- **`FileEdit` captures old/new text** — the Edit tool sends `old_text` and `new_text` in its input, giving us inline diff capability for free.
- **`BashCommand.output` is `Option`** — tool results arrive as a separate `ToolResult` event, so the output is filled in retroactively (see "Correlating ToolUse with ToolResult" below).
- **`Search` is separate from `ToolCall`** — Glob/Grep are the most frequent tools and deserve their own compact rendering.
- **Fully serializable** — all fields are `Serialize + Deserialize`, so session persistence works via serde_json.

---

## 3. Rendering Design Per Block Type

Each `BlockContent` variant maps to a distinct ratatui rendering strategy. All renderings use the existing theme palette from `theme.rs`.

### 3.1 `Text` — Assistant prose

```
Styled paragraph with recency fade (current behavior preserved).
Multi-line text wraps within the panel width.
```

**Ratatui widgets:** `Paragraph` with `Style::default().fg(recency_color)`
**Height:** Dynamic (word-wrapped line count)
**Collapsible:** No

### 3.2 `FileRead` — File read indicator

```
 📄 src/main.rs                          42 lines
```

**Ratatui widgets:** Single `Line` with spans:
- `Span::styled("📄 ", Style::default().fg(C_DIM))`
- `Span::styled(path, Style::default().fg(C_TEXT))`
- Right-aligned `Span::styled("42 lines", Style::default().fg(C_DIM))`

**Height:** 1 line
**Collapsible:** No (already compact)

### 3.3 `FileWrite` — New file creation

```
 ✚ src/components/NewWidget.tsx
```

**Ratatui widgets:** Single `Line`:
- `Span::styled("✚ ", Style::default().fg(C_ACTIVE))` (green)
- `Span::styled(path, Style::default().fg(C_TEXT))`

**Height:** 1 line
**Collapsible:** No

### 3.4 `FileEdit` — File modification with optional diff

Collapsed (default when panel is narrow):
```
 ✎ src/utils/api.ts
```

Expanded (focus mode or wide panel):
```
 ✎ src/utils/api.ts
   - const timeout = 5000;
   + const timeout = 10000;
```

**Ratatui widgets:**
- Header: `Line` with `✎` icon + path
- Diff lines: `Paragraph` with per-line coloring:
  - `-` lines: `Style::default().fg(C_ERROR)` (red)
  - `+` lines: `Style::default().fg(C_ACTIVE)` (green)

**Height:** 1 (collapsed) or 1 + diff_lines (expanded)
**Collapsible:** Yes — default collapsed, expand in focus mode

### 3.5 `BashCommand` — Shell execution

Collapsed:
```
 $ npm run typecheck                         ✓
```

Expanded:
```
 $ npm run typecheck                         ✓
   > No errors found.
   > Checked 142 files.
```

With error:
```
 $ npm run typecheck                         ✗
   > error TS2345: Argument of type...
```

**Ratatui widgets:**
- Command line: `Line` with spans:
  - `Span::styled("$ ", Style::default().fg(C_BRAND))` (cyan)
  - `Span::styled(command, Style::default().fg(C_TEXT).add_modifier(Modifier::BOLD))`
  - Right-aligned exit indicator: `✓` (green) or `✗` (red)
- Output block: `Paragraph` with `Style::default().fg(C_DIM)`

**Height:** 1 (collapsed) or 1 + output_lines (expanded)
**Collapsible:** Yes — default collapsed, auto-expand on error

### 3.6 `Search` — Glob/Grep results

```
 🔍 Grep: "handleSubmit"                  4 files
```

**Ratatui widgets:** Single `Line` with icon + pattern + result count
**Height:** 1 line
**Collapsible:** No

### 3.7 `ToolCall` — Generic tool use

```
 ⚙ Agent: Exploring codebase for auth patterns
```

**Ratatui widgets:** Single `Line` with generic gear icon + name + summary
**Height:** 1 line
**Collapsible:** No

### 3.8 `Error` — Error block

```
 ⚠ Rate limited (token) — resumes in 45s
```

**Ratatui widgets:** `Line` or `Paragraph` with `Style::default().fg(C_ERROR)` and optional `Modifier::BOLD`
**Height:** Dynamic
**Collapsible:** No

### 3.9 `UserMessage` — User guidance

```
 ▸ Focus on the API client first, skip the tests for now
```

**Ratatui widgets:** `Line` with `Style::default().fg(C_BRAND)` (cyan, matching current `[USER]` behavior)
**Height:** Dynamic (wraps)
**Collapsible:** No

### 3.10 `Thinking` — Extended thinking indicator

```
 ··· thinking
```

**Ratatui widgets:** Single `Line` with `Style::default().fg(C_DIM)`, optionally animated dots
**Height:** 1 line
**Collapsible:** No — auto-replaced when the next text/tool block arrives

---

## 4. Implementation Details

### 4.1 Correlating ToolUse with ToolResult

The current code discards `ToolResult` events entirely (musician.rs:420). To populate `BashCommand.output` or other result fields, we need to correlate:

```rust
// In musician.rs execute() loop:
// Track the last tool block that expects a result
let mut pending_tool_idx: Option<usize> = None;

// On ToolUse:
let block = OutputBlock { content: BlockContent::BashCommand { ... }, ... };
self.state.output_blocks.push(block);
pending_tool_idx = Some(self.state.output_blocks.len() - 1);

// On ToolResult:
if let Some(idx) = pending_tool_idx.take() {
    if let Some(block) = self.state.output_blocks.get_mut(idx) {
        match &mut block.content {
            BlockContent::BashCommand { output, .. } => {
                *output = event.tool_result_content.clone();
            }
            _ => {}
        }
    }
}
```

This is safe because Claude's stream-json protocol guarantees tool results arrive in order after their corresponding tool use.

### 4.2 Block Buffer Limits

The current system caps at 200 lines / 100KB. For blocks:

```rust
const OUTPUT_BLOCK_LIMIT: usize = 100;        // max blocks
const OUTPUT_BLOCK_MAX_CHARS: usize = 100_000; // total serialized size

fn push_block(&mut self, block: OutputBlock) {
    self.state.output_blocks.push(block);
    // Trim oldest blocks
    while self.state.output_blocks.len() > OUTPUT_BLOCK_LIMIT {
        self.state.output_blocks.remove(0);
    }
    // Also maintain the legacy output_lines (during migration)
    // ...
}
```

### 4.3 Block Height Calculation for Scrolling

The TUI needs to know how many terminal rows each block occupies:

```rust
impl OutputBlock {
    /// Calculate the rendered height of this block in terminal rows.
    fn rendered_height(&self, width: u16) -> u16 {
        match &self.content {
            BlockContent::Text(s) => {
                // Word-wrap calculation
                let lines = textwrap::wrap(s, width as usize);
                lines.len() as u16
            }
            BlockContent::FileRead { .. }
            | BlockContent::FileWrite { .. }
            | BlockContent::Search { .. }
            | BlockContent::ToolCall { .. }
            | BlockContent::Thinking => 1,
            BlockContent::FileEdit { old_text, new_text, .. } => {
                if self.collapsed { 1 } else {
                    1 + old_text.as_ref().map(|t| t.lines().count()).unwrap_or(0) as u16
                      + new_text.as_ref().map(|t| t.lines().count()).unwrap_or(0) as u16
                }
            }
            BlockContent::BashCommand { output, .. } => {
                if self.collapsed { 1 } else {
                    1 + output.as_ref().map(|o| o.lines().count()).unwrap_or(0) as u16
                }
            }
            BlockContent::Error(s) => {
                let lines = textwrap::wrap(s, width as usize);
                lines.len() as u16
            }
            BlockContent::UserMessage(s) => {
                let lines = textwrap::wrap(s, width as usize);
                lines.len() as u16
            }
        }
    }
}
```

---

## 5. Migration Path

### Recommended: Incremental, dual-write approach

The migration can be done **without breaking any existing functionality** by running both systems in parallel during the transition.

#### Phase 1: Add types (conductor-types only)

1. Add `OutputBlock`, `BlockContent` to `conductor-types/src/state.rs`
2. Add `output_blocks: Vec<OutputBlock>` to `MusicianState` (alongside existing `output_lines`)
3. `cargo check --workspace` — zero breakage since `output_lines` is untouched

#### Phase 2: Dual-write in musician (conductor-core)

1. Modify `musician.rs` to populate **both** `output_lines` (existing) and `output_blocks` (new)
2. Update the event processing in `execute()`:
   - `Assistant` events → push `BlockContent::Text` + keep `push_output()`
   - `ToolUse` events → push typed block (FileRead/FileEdit/BashCommand/Search/ToolCall) + keep `push_output("> ...")`
   - `ToolResult` events → correlate with pending block + still no-op for `output_lines`
   - `Error` events → push `BlockContent::Error` + keep `push_output("ERROR: ...")`
3. Update `tool_summary.rs` to also return the block type (or create a new `tool_to_block()` function)

#### Phase 3: Render structured blocks (conductor-tui)

1. Update `render_musician_column()` to read `output_blocks` instead of `output_lines`
2. Implement per-block rendering functions
3. Add scroll logic based on block heights
4. Keep `output_line_style()` as fallback for any unstructured lines

#### Phase 4: Remove legacy (cleanup)

1. Remove `output_lines` from `MusicianState`
2. Remove `push_output()` calls from `musician.rs`
3. Remove prefix-based `output_line_style()` from theme.rs
4. Update `SessionData` serialization if needed

**Why not parse-at-render-time?** Parsing `"> Reading /src/main.rs"` back into `BlockContent::FileRead { path: "/src/main.rs" }` is fragile and loses information that was available at the source. The dual-write approach is cleaner — we produce structured data where we have structure (in `musician.rs`) and keep the legacy path alive until the new renderer is ready.

---

## 6. ASCII Mockup

A musician panel showing a typical read → edit → test sequence:

```
┌─ M1 ● sonnet  Structured Output Rendering ────────────────────┐
│                                                                │
│  Let me read the state types to understand the current model.  │
│                                                                │
│  📄 crates/conductor-types/src/state.rs              284 lines │
│                                                                │
│  I'll add the OutputBlock enum and update MusicianState.       │
│                                                                │
│  ✎ crates/conductor-types/src/state.rs                         │
│    - pub output_lines: Vec<String>,                            │
│    + pub output_lines: Vec<String>,                            │
│    + pub output_blocks: Vec<OutputBlock>,                      │
│                                                                │
│  ✚ crates/conductor-types/src/output_block.rs                  │
│                                                                │
│  Now let me update the musician to dual-write blocks.          │
│                                                                │
│  📄 crates/conductor-core/src/musician.rs             697 lines │
│                                                                │
│  ✎ crates/conductor-core/src/musician.rs                       │
│                                                                │
│  $ cargo check --workspace                                  ✓  │
│                                                                │
│  $ cargo test --workspace                                   ✓  │
│    > running 42 tests ... ok                                   │
│                                                                │
│  ⚠ warning: unused variable `old_lines` in musician.rs:234     │
│                                                                │
│  ✎ crates/conductor-core/src/musician.rs                       │
│                                                                │
│  $ cargo check --workspace                                  ✓  │
│                                                                │
│ 3m42s                                                          │
└────────────────────────────────────────────────────────────────┘
```

### Focus mode (expanded panel with diff details):

```
┌─ M1 ● sonnet  Structured Output Rendering ───────────────────────────────────┐
│                                                                              │
│  I'll add the OutputBlock enum and update MusicianState.                     │
│                                                                              │
│  ✎ crates/conductor-types/src/state.rs                                       │
│    - pub struct MusicianState {                                              │
│    -     pub output_lines: Vec<String>,                                      │
│    - }                                                                       │
│    + pub struct MusicianState {                                              │
│    +     pub output_lines: Vec<String>,                                      │
│    +     pub output_blocks: Vec<OutputBlock>,                                │
│    + }                                                                       │
│                                                                              │
│  ✚ crates/conductor-types/src/output_block.rs                                │
│                                                                              │
│  $ cargo test -p conductor-types                                          ✓  │
│    > running 12 tests                                                        │
│    > test tests::output_block_round_trip ... ok                              │
│    > test tests::musician_state_round_trip ... ok                            │
│    > ...                                                                     │
│    > test result: ok. 12 passed; 0 failed                                    │
│                                                                              │
│  🔍 Grep: "output_lines"                                          8 files   │
│                                                                              │
│  ▸ Focus on the API client first, skip the tests for now                     │
│                                                                              │
│ 3m42s                                                                        │
└──────────────────────────────────────────────────────────────────────────────┘
```

---

## 7. Crate Change Summary

| Crate | Changes Required | Phase |
|-------|-----------------|-------|
| `conductor-types` | Add `OutputBlock`, `BlockContent` (10 variants incl. `Thinking`); add `output_blocks: Vec<OutputBlock>` to `MusicianState` | Phase 1 |
| `conductor-core` | Dual-write blocks in `musician.rs`; new `tool_to_block()` fn in `tool_summary.rs`; correlate `ToolResult` with pending block | Phase 2 |
| `conductor-bridge` | **No changes needed** — `parse.rs` already produces typed `ClaudeEvent` with tool_name, tool_input, tool_result_content | — |
| `conductor-tui` | New block renderer in `components/musician.rs`; per-block height calc; updated scroll logic; theme additions for diff coloring | Phase 3 |
| `conductor-cli` | No changes needed | — |

---

## 8. Open Questions

1. **Should `FileEdit` capture the full `old_text`/`new_text`?** The Edit tool input can be large. We could truncate to first/last N lines of each, or only store a line count. Recommendation: truncate to 10 lines each, sufficient for visual diff.

2. **Should blocks be expandable via keyboard?** Current TUI has scroll but no per-element interaction. Adding Enter-to-expand would require tracking a "cursor" position within the block list. Recommendation: defer to a later iteration; use auto-expand heuristics (expand in focus mode, collapse in grid view).

3. **Should `ToolResult` content be captured for Read operations?** File contents can be very large. Recommendation: no — for `FileRead`, the path and line count are sufficient. Only capture output for `BashCommand` (truncated to 20 lines).
