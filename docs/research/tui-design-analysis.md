# Conductor TUI Design Analysis & Mockups

> **Date:** 2026-03-22
> **Scope:** Deep analysis of best open-source TUIs for coding agents (Ratatui & beyond), followed by concrete mockups for Conductor's next-generation TUI.

---

## Table of Contents

1. [Landscape Analysis — Best Open-Source Coding Agent TUIs](#1-landscape-analysis)
2. [Pattern Extraction — What Makes Them Great](#2-pattern-extraction)
3. [Conductor's Current State & Gaps](#3-conductors-current-state--gaps)
4. [Design Vision — Conductor TUI V2](#4-design-vision)
5. [ASCII Mockups](#5-ascii-mockups)
6. [Implementation Priorities](#6-implementation-priorities)
7. [Crate Shopping List](#7-crate-shopping-list)
8. [Sources](#8-sources)

---

## 1. Landscape Analysis

### 1.1 OpenCode (Go / BubbleTea → OpenTUI)

**Stars:** 20k+ · **Language:** Go → TypeScript (OpenTUI) · **Framework:** BubbleTea → custom

OpenCode is the gold standard for single-agent coding TUIs. Key design wins:

- **Streaming markdown rendering** — Assistant messages render as styled markdown in real-time, not raw text. Code blocks get syntax highlighting, lists get proper indentation.
- **Diff viewer** — Side-by-side or stacked diff rendering with `diff_style: "auto"` that adapts to terminal width. This is critical for code review.
- **@ file references** — Type `@` and get fuzzy file search inline in the input. File content is automatically added to context.
- **Themeable via `tui.json`** — Colors, borders, scroll behavior, diff style, all configurable without recompilation. Supports adaptive themes that detect terminal light/dark mode via OSC queries.
- **Scroll acceleration** — macOS-style momentum scrolling for natural feel. Ramps up speed with rapid scrolling, stays precise for slow movements.
- **Custom commands** — `/commands` stored as markdown files, quickly send predefined prompts.
- **Non-interactive mode** — Pass prompt as CLI arg for scripting/CI.

**Relevance to Conductor:** OpenCode is single-agent, so its layout is a single conversation thread. Conductor needs multi-agent columns — but OpenCode's rendering quality (markdown, diffs, themes) is the bar we should hit within each musician panel.

### 1.2 Crustly (Rust / Ratatui)

**Stars:** ~500 · **Language:** Rust · **Framework:** Ratatui

Closest architectural cousin to Conductor — pure Rust, Ratatui-based, with agent capabilities.

- **Plan Mode** — Database-backed task planning with 10 task types and 6 statuses. Very similar to Conductor's decomposition flow. Tasks are visualized in a navigable list.
- **Markdown + syntax highlighting** — Uses `tui-markdown` or similar for rendering assistant output with proper styling.
- **Multi-provider support** — Anthropic, OpenAI, local LLMs via Ollama. Provider abstraction is clean.
- **307 tests, 100% pass rate** — Shows mature codebase despite 0.1.0-alpha.

**Relevance to Conductor:** Crustly proves Ratatui can deliver polished AI assistant UX in Rust. Its plan mode is basically our PlanReview phase — steal their task type visualization patterns.

### 1.3 OLI (Rust / Ratatui)

**Stars:** ~200 · **Language:** Rust · **Framework:** Ratatui

Open-source Claude Code alternative. Early stage but architecturally sound.

- **LLM-agnostic** — Local + cloud. Uses structured JSON outputs for reliability.
- **Agent capabilities** — File search, edit, command execution with tool visualization.
- **Minimal but functional TUI** — Focuses on the conversation loop with clear tool call rendering.

**Relevance to Conductor:** OLI's approach to rendering tool calls (file edits, commands) inline is useful for our musician output panels. Each musician's output currently renders as raw text — we should show structured tool calls.

### 1.4 Agent-Deck (Go / BubbleTea)

**Stars:** ~2k · **Language:** Go · **Framework:** BubbleTea

Multi-agent session manager — closest to Conductor's multi-agent paradigm.

- **Conductor sessions** — Persistent agent sessions that monitor and orchestrate other sessions. Auto-responds when confident, escalates when can't.
- **Smart status detection** — Knows when an agent is thinking vs. waiting vs. errored.
- **Session forking** — Fork context into a new agent session. Like our worktree isolation but at the session level.
- **Remote control** — Telegram/Slack integration for monitoring.
- **Global search** — Search across all agent conversations.

**Relevance to Conductor:** Agent-Deck validates the multi-agent TUI paradigm. Their status detection and session grouping patterns are directly applicable. Their "conductor session" concept is literally what our Conductor Agent does.

### 1.5 OpenClaude (Ratatui wrapper)

**Stars:** ~300 · **Language:** Rust · **Framework:** Ratatui

Full-featured TUI wrapper around Claude Code CLI.

- **Streaming markdown** — Real-time markdown rendering as Claude streams.
- **Tool call visualization** — Shows tool calls (file reads, edits, bash) as distinct visual blocks.
- **Modal dialogs** — Confirmation dialogs, file picker, settings.
- **Multiple themes** — Dark, light, catppuccin, etc.

**Relevance to Conductor:** Direct proof that wrapping Claude CLI output in a beautiful Ratatui interface is achievable. Their tool call visualization is exactly what our musician panels need.

### 1.6 Rimuru (Rust / Ratatui)

**Stars:** ~800 · **Language:** Rust · **Framework:** Ratatui

Multi-agent cost tracking dashboard with 10 tabs and 15 themes.

- **10-tab layout** — Dashboard, agents, costs, logs, settings, etc. Most tabs we've seen in a Ratatui app.
- **Cost tracking** — Real-time token cost monitoring across agents. Shows spend per agent, per model.
- **15 themes** — Extensive theming system.
- **Multi-agent monitoring** — Unified view of multiple AI coding agents.

**Relevance to Conductor:** Rimuru's tab system and cost tracking are directly applicable. We should track token costs per musician and show aggregate spend. Their 10-tab pattern proves Ratatui can handle complex multi-view apps.

### 1.7 tmuxcc / agtx (Multi-Agent Orchestrators)

**Stars:** ~1k each · **Language:** Various

Terminal multiplexer integrations for agent orchestration.

- **Centralized monitoring** — Single TUI watching multiple tmux panes.
- **Status detection** — Identifies agent states from terminal output patterns.
- **Agent-agnostic** — Works with Claude Code, OpenCode, Codex, Gemini CLI.

**Relevance to Conductor:** We're doing what they do (multi-agent orchestration) but natively, not through tmux. Our architecture is fundamentally better (direct process control via channels), but their UX patterns for multi-agent monitoring are proven.

---

## 2. Pattern Extraction — What Makes Them Great

### 2.1 Universal Patterns (found in 3+ projects)

| Pattern | Found In | Why It Works |
|---------|----------|-------------|
| **Streaming markdown rendering** | OpenCode, OpenClaude, Crustly | Raw text is unreadable for code-heavy AI output. Markdown gives structure. |
| **Tool call blocks** | OpenCode, OLI, OpenClaude | Users need to see what the agent is doing, not just what it's saying. |
| **Theme system** | OpenCode, Rimuru, Crustly | Terminal aesthetics vary wildly. Users need to match their environment. |
| **Vim-style navigation** | OpenCode, Crustly, Agent-Deck | Power users expect `j/k/h/l`, `gg/G`, `/search`. |
| **Status detection/indicators** | Agent-Deck, tmuxcc, Rimuru | Clear visual state per agent eliminates guesswork. |
| **Session persistence** | OpenCode, Crustly, Agent-Deck | Resume interrupted work is table stakes. |

### 2.2 Differentiating Patterns (found in 1-2 projects, high impact)

| Pattern | Found In | Why Conductor Should Adopt |
|---------|----------|--------------------------|
| **Diff viewer** | OpenCode | Conductor reviews merged code — needs inline diff. |
| **@ file references** | OpenCode | Plan refinement benefits from file-aware input. |
| **Cost tracking** | Rimuru | Multi-agent token burn is expensive. Visibility = control. |
| **Scroll acceleration** | OpenCode | Musician output panels can be very long. |
| **Tab system** | Rimuru | Conductor has 5+ distinct views crammed into phase-switching. |
| **Session forking** | Agent-Deck | Conductor's worktree model already supports this — expose it in TUI. |
| **Remote monitoring** | Agent-Deck | Long-running multi-agent tasks benefit from mobile monitoring. |

### 2.3 Anti-Patterns to Avoid

| Anti-Pattern | Seen In | Why |
|--------------|---------|-----|
| **tmux dependency** | tmuxcc, agtx | Fragile, adds setup complexity, can't control layout precisely. |
| **No responsive layout** | Early OLI | Must gracefully degrade from 200-col to 80-col terminals. |
| **Plain text dumps** | Many early projects | Raw LLM output without formatting is unusable. |
| **Mouse-only interactions** | Some web-based UIs | Terminal users expect keyboard-first. |

---

## 3. Conductor's Current State & Gaps

### 3.1 What We Have (conductor-tui as of 2026-03-22)

```
Components: 8 modules (app, theme, layout, header, status, input, musician, panels, insights, conductor, analyst)
Layout: Responsive 3-tier breakpoints (NARROW=80, WIDE=160, TALL=40)
Theme: 9 named colors, semantic mapping (phase→symbol+color, status→dot+color+label)
Input: Hand-rolled prompt bar with word navigation (has UTF-8 bug)
Overlays: Help, session browser, task detail modal
Panels: Musician grid (with column collapse), conductor output, analyst grid, insights, task graph, plan review
Architecture: Pure render functions, channel-based state (watch + mpsc)
```

### 3.2 Gap Analysis

| Area | Current State | Gap | Priority |
|------|--------------|-----|----------|
| **Output rendering** | Raw text with recency fade | No markdown, no syntax highlight, no tool call blocks | ★★★ |
| **Input** | Hand-rolled, UTF-8 bug | Missing undo, selection, history, @-references | ★★★ |
| **Diff viewer** | None | Phase review needs inline diff rendering | ★★★ |
| **Scrolling** | Manual `scroll_offset: u16`, no scrollbar | No visual indicator, no acceleration, no mouse scroll | ★★ |
| **Theming** | Hardcoded 9 colors | No user-configurable themes, no light mode | ★★ |
| **Cost tracking** | None | No visibility into token spend per musician or total | ★★ |
| **Tabs/views** | Phase-switching replaces content | No way to compare phases, review history, or view logs | ★★ |
| **Progress indicators** | `pbar()` helper (unused!) + text counts | No `Gauge`, no `Sparkline`, despite helpers existing | ★ |
| **Border styling** | Plain borders everywhere | No `Rounded`/`Double`/`Thick` for hierarchy | ★ |
| **Animations** | None | No transition polish, loading indicators are text-only | ★ |
| **Mouse support** | None | Panel clicking, scroll wheel, resize drag | ★ |

---

## 4. Design Vision — Conductor TUI V2

### 4.1 Design Principles

1. **Structure recedes, content pops** — Already in `theme.rs`, double down. Borders are DarkGray, content is White, color is semantic.
2. **Information at a glance, detail on demand** — Dashboard summary → click/press to drill down.
3. **Keyboard-first, mouse-friendly** — Every action has a key binding. Mouse adds convenience, not capability.
4. **Progressive disclosure** — Start simple (auto-layout), reveal complexity (tab views, split panes) as the user explores.
5. **The orchestra metaphor is the brand** — Musical notation for phases (♩♪♫♬), instrument names, conductor's podium.

### 4.2 Layout Architecture

```
┌─ Tab Bar ─────────────────────────────────────────────────────────┐
│ [♫ Orchestra]  [📋 Plan]  [📊 Stats]  [📄 Diff]  [📝 Log]       │
├───────────────────────────────────────────────────────────────────┤
│                                                                   │
│                    TAB CONTENT AREA                                │
│                                                                   │
├─ Status Line ─────────────────────────────────────────────────────┤
│ ● Executing  2/8 tasks  4 musicians  5m23s  $0.42     ?: help    │
├─ Prompt Bar ──────────────────────────────────────────────────────┤
│ guidance> type to send guidance to active musicians...            │
└───────────────────────────────────────────────────────────────────┘
```

**Tabs are the primary navigation** — Replace the current phase-dependent content switching with explicit tabs. The current design implicitly shows different content for each phase, which makes it impossible to go back and review the plan while musicians are executing.

### 4.3 Tab Definitions

| Tab | Key | Content | When Visible |
|-----|-----|---------|-------------|
| **Orchestra** | `1` | Musician grid + task graph + insights (current main view) | Always |
| **Plan** | `2` | Plan review, task list, refinement chat, dependency DAG | After decomposition |
| **Stats** | `3` | Token costs, timing, per-musician gauges, sparklines | After execution starts |
| **Diff** | `4` | Aggregated diff of all changes, per-musician diffs | After merging |
| **Log** | `5` | Full conductor + musician event log, filterable | Always |

---

## 5. ASCII Mockups

### 5.1 Orchestra Tab — Execution Phase (Main View)

```
╭─ ♫ Orchestra ──────── 📋 Plan  📊 Stats  📄 Diff  📝 Log ──────╮
│                                                                   │
│  Tasks ─────────────────────────────────────────────────────────  │
│  ● 1. Add retry logic        ✓ 2. Update tests     ◦ 3. Docs    │
│  ● 4. Error handling         × 5. Refactor API  ← 1,2            │
│  ▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓░░░░░░░░░░░░░░░░░░░░░░░░░░░░  2/5 tasks  40% │
│                                                                   │
│  ┌─ M1 ● sonnet ─ Add retry logic ─┐┌─ M2 ● sonnet ─ Error ha… ┐│
│  │ Reading src/api/client.ts...     ││ Reading src/errors/types…  ││
│  │ > Found 3 retry candidates       ││ > Analyzing error hierarc… ││
│  │ Editing src/api/client.ts        ││ Creating src/errors/retry… ││
│  │ ┌──────────────────────────┐     ││                            ││
│  │ │ + import { retry } from… │     ││ Waiting for task 1...      ││
│  │ │ + const MAX_RETRIES = 3; │     ││                            ││
│  │ │   async function fetch…  │     ││                            ││
│  │ └──────────────────────────┘     ││                            ││
│  │ Running npm test...              ││                            ││
│  │ > ✓ 12 passed  ✗ 1 failed       ││                            ││
│  │ ▸ 89K tokens  3m05s        ▂▃▅▇ ││ ▸ 12K tokens  0m45s       ││
│  └──────────────────────────────────┘└────────────────────────────┘│
│  ┌─ M3 ✓ sonnet ─ Update tests ────┐┌─ M4 ○ idle ───────────────┐│
│  │ ✓ Completed in 2m12s            ││ Waiting for assignment...   ││
│  │ Modified: 3 files, +89 -12      ││                             ││
│  │ ▸ 45K tokens  2m12s             ││                             ││
│  └──────────────────────────────────┘└─────────────────────────────┘│
│                                                                   │
│  Insights ──────────────────                                      │
│  ◆ Pattern: All retry logic uses exponential backoff              │
│  ◈ Architecture: Error types extend base ApiError class           │
│  ● Decision: Using native fetch retry, not axios-retry            │
│                                                                   │
├───────────────────────────────────────────────────────────────────┤
│ ● Executing  [sess_a1b2]  2/5 tasks  4 musicians  5m23s  $0.42  │
├───────────────────────────────────────────────────────────────────┤
│ guidance> _                                                       │
╰───────────────────────────────────────────────────────────────────╯
```

**Key improvements over current:**
- Task progress bar with `Gauge` widget replaces plain text count
- Musician panels show **structured tool calls** (file edit blocks with mini-diff, test results with pass/fail)
- **Sparkline** in bottom-right of each musician shows token burn rate
- Completed musicians show summary (files modified, lines changed)
- Insights panel is inline below musicians (not right sidebar) when terminal is narrow

### 5.2 Orchestra Tab — Focused Musician View

```
╭─ ♫ Orchestra ──────── 📋 Plan  📊 Stats  📄 Diff  📝 Log ──────╮
│                                                                   │
│  ◀ M1 ● sonnet ─ Add retry logic to API client ──────── 3m05s ▶ │
│                                                                   │
│  ─── Reading ────────────────────────────────────────────────     │
│  📄 src/api/client.ts (245 lines)                                 │
│  📄 src/api/types.ts (89 lines)                                   │
│  📄 src/config/retry.ts (34 lines)                                │
│                                                                   │
│  ─── Editing ────────────────────────────────────────────────     │
│  📝 src/api/client.ts                                             │
│  ┌────────────────────────────────────────────────────────────┐   │
│  │  42 │-  const response = await fetch(url, options);        │   │
│  │  42 │+  const response = await fetchWithRetry(url, {       │   │
│  │  43 │+    ...options,                                      │   │
│  │  44 │+    maxRetries: config.retry.maxAttempts,            │   │
│  │  45 │+    backoff: 'exponential',                          │   │
│  │  46 │+  });                                                │   │
│  └────────────────────────────────────────────────────────────┘   │
│                                                                   │
│  ─── Bash ───────────────────────────────────────────────────     │
│  $ npm test -- --grep "retry"                                     │
│  ┌────────────────────────────────────────────────────────────┐   │
│  │  PASS  src/api/__tests__/client.test.ts                    │   │
│  │    ✓ retries on 500 (45ms)                                 │   │
│  │    ✓ respects maxRetries (23ms)                            │   │
│  │    ✓ exponential backoff timing (102ms)                    │   │
│  │    ✗ handles network timeout (expected retry, got throw)   │   │
│  │  Tests: 3 passed, 1 failed                                │   │
│  └────────────────────────────────────────────────────────────┘   │
│                                                                   │
│  ─── Token Usage ────────────────────────────────────────────     │
│  Input: 67,234  Output: 21,890  Total: 89,124  Cost: $0.28      │
│  ▁▂▃▅▇█▇▅▃▂▁▁▂▃▅▇█▇▅▃▂▁▁▂▃▅▇                                   │
│                                                                   │
├───────────────────────────────────────────────────────────────────┤
│ ● Executing  [sess_a1b2]  2/5 tasks  4 musicians  5m23s  $0.42  │
├───────────────────────────────────────────────────────────────────┤
│ guidance> _                                                       │
╰───────────────────────────────────────────────────────────────────╯
```

**Key improvements:**
- **Structured tool call rendering** — File reads show file icon + name + line count. Edits show inline diff. Bash shows command + output in a distinct block.
- **Token sparkline** — Shows the burn rate over time, making it easy to spot when the musician is "stuck" (flat line) or making progress (peaks).
- **Navigation arrows** (◀ ▶) to switch between musicians while focused.

### 5.3 Plan Tab — Plan Review Phase

```
╭─ ♫ Orchestra ──────── 📋 Plan ─ 📊 Stats  📄 Diff  📝 Log ─────╮
│                                                                   │
│  Plan: Refactor auth module for compliance                        │
│  8 tasks · ~25 min · 3 phases                                     │
│  ▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓░░░░░░░░░░░  Phase 1 of 3    │
│                                                                   │
│  ┌─ Phase 1: Core Auth ─────────────┐ ┌─ Refinement Chat ───────┐│
│  │                                   │ │                         ││
│  │ ▸ ◦ 1. Extract token storage     │ │ You: Can task 3 and 4   ││
│  │       Files: auth/storage.ts      │ │ run in parallel?        ││
│  │       Deps: none                  │ │                         ││
│  │   ◦ 2. New session validator      │ │ Conductor: Yes! Tasks 3 ││
│  │       Files: auth/validate.ts     │ │ and 4 touch different   ││
│  │       Deps: none                  │ │ files. Updated plan to  ││
│  │   ◦ 3. Migrate existing tokens   │ │ parallelize them in     ││
│  │       Files: auth/migration.ts    │ │ Phase 2.                ││
│  │       Deps: 1                     │ │                         ││
│  │   ◦ 4. Update middleware          │ │                         ││
│  │       Files: middleware/auth.ts   │ │                         ││
│  │       Deps: 1, 2                  │ │                         ││
│  │                                   │ │                         ││
│  ├─ Phase 2: Tests (parallel) ──────┤ │                         ││
│  │   ◦ 5. Unit tests for storage    │ │                         ││
│  │   ◦ 6. Integration tests         │ │                         ││
│  │                                   │ │                         ││
│  ├─ Phase 3: Cleanup ──────────────┤  │                         ││
│  │   ◦ 7. Remove old auth code      │ │                         ││
│  │   ◦ 8. Update documentation      │ │                         ││
│  └───────────────────────────────────┘ └─────────────────────────┘│
│                                                                   │
│  ┌─ Task Detail (press d) ───────────────────────────────────────┐│
│  │ 1. Extract token storage                                      ││
│  │ WHY: Legal requires session tokens stored in encrypted DB,    ││
│  │      not cookies. This task isolates the storage layer.       ││
│  │ FILES: src/auth/storage.ts, src/auth/types.ts                 ││
│  │ ACCEPTANCE: Token CRUD via new StorageService class           ││
│  │ ESTIMATED: ~5 min                                             ││
│  └───────────────────────────────────────────────────────────────┘│
│                                                                   │
├───────────────────────────────────────────────────────────────────┤
│ ◈ PlanReview  [sess_a1b2]  8 tasks  3 phases  ~25 min            │
├───────────────────────────────────────────────────────────────────┤
│ refine> _                                              [Enter] ✓  │
╰───────────────────────────────────────────────────────────────────╯
```

**Key improvements:**
- **Phase grouping** — Tasks are grouped by execution phase, not a flat list. Shows parallelism visually.
- **Side-by-side refinement chat** — Chat history alongside the plan, not in a separate overlay.
- **Inline task detail** — Press `d` to expand detail below the task list, not a modal.
- **Visual DAG** — Dependencies shown inline with phase grouping making the DAG structure obvious.

### 5.4 Stats Tab — Token & Cost Dashboard

```
╭─ ♫ Orchestra ──────── 📋 Plan  📊 Stats ─ 📄 Diff  📝 Log ─────╮
│                                                                   │
│  Session Stats ──────────────────────────────────────────────     │
│  Total tokens: 234,567    Cost: $0.72    Duration: 8m34s          │
│  Input: 178,234  Output: 56,333  Cache hits: 45%                  │
│                                                                   │
│  Token Burn Rate ────────────────────────────────────────────     │
│  ▁▂▃▅▇█▇▅▃▂▁▁▂▃▅▇█▇▅▃▂▁▁▂▃▅▇█▇▅▃▂▁▁▂▃▅▇█▇▅▃▂▁▁▂▃▅▇█▇▅▃▂▁   │
│  ├── Exploring ──┤├── Planning ──┤├────── Executing ──────────┤   │
│                                                                   │
│  Per-Musician Breakdown ─────────────────────────────────────     │
│                                                                   │
│  M1 sonnet  ▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓░░░░  89K tok  $0.28  3m05s  │
│  M2 sonnet  ▓▓▓▓▓▓░░░░░░░░░░░░░░░░░░░  34K tok  $0.11  1m22s  │
│  M3 sonnet  ▓▓▓▓▓▓▓▓▓▓▓▓▓░░░░░░░░░░░  45K tok  $0.14  2m12s  │
│  M4 sonnet  ▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓░░░░░░  66K tok  $0.19  2m55s  │
│                                                                   │
│  Conductor   ▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓ 123K tok  $1.84  ──     │
│                                                                   │
│  Cost by Phase ──────────────────────────────────────────────     │
│  Exploring:   $0.45  ████████████░░░░░░░░  25%                    │
│  Planning:    $0.92  ██████████████████░░  51%                    │
│  Executing:   $0.42  ████████░░░░░░░░░░░  24%   (ongoing)        │
│                                                                   │
│  Model Usage ────────────────────────────────────────────────     │
│  opus    123K tokens  $1.84  ██████████████████████████████ 68%   │
│  sonnet  111K tokens  $0.72  ██████████████████░░░░░░░░░░░ 32%   │
│                                                                   │
├───────────────────────────────────────────────────────────────────┤
│ ● Executing  [sess_a1b2]  2/5 tasks  4 musicians  5m23s  $0.72  │
├───────────────────────────────────────────────────────────────────┤
│ >                                                                 │
╰───────────────────────────────────────────────────────────────────╯
```

**Key improvements:**
- **Cost visibility** — Know exactly how much this orchestration run is costing.
- **Sparkline timeline** — See token burn rate over the entire session, with phase markers.
- **Per-musician gauges** — Compare musician efficiency at a glance using `Gauge` + `BarChart`.
- **Model breakdown** — Opus (conductor) vs Sonnet (musicians) cost split.

### 5.5 Diff Tab — Post-Merge Review

```
╭─ ♫ Orchestra ──────── 📋 Plan  📊 Stats  📄 Diff ─ 📝 Log ─────╮
│                                                                   │
│  Changes: 5 files · +127 -34 ─────────────────────────────────   │
│                                                                   │
│  ┌ src/api/client.ts (+45 -12) ─── M1 ──────────────────────┐   │
│  │  40 │   async function fetchData(url: string) {           │   │
│  │  41 │-    const response = await fetch(url, options);     │   │
│  │  42 │+    const response = await fetchWithRetry(url, {    │   │
│  │  43 │+      ...options,                                   │   │
│  │  44 │+      maxRetries: config.retry.maxAttempts,         │   │
│  │  45 │+      backoff: 'exponential',                       │   │
│  │  46 │+    });                                             │   │
│  │     │                                                     │   │
│  │  78 │+  async function fetchWithRetry(                    │   │
│  │  79 │+    url: string,                                    │   │
│  │  80 │+    options: RetryOptions,                          │   │
│  │  81 │+  ): Promise<Response> {                            │   │
│  │  82 │+    for (let attempt = 0; attempt < options.max…    │   │
│  │  83 │+      try {                                         │   │
│  │  84 │+        return await fetch(url, options);           │   │
│  │  85 │+      } catch (err) {                               │   │
│  │  86 │+        if (attempt === options.maxRetries - 1) …   │   │
│  │  87 │+        await sleep(backoffMs(attempt, options.…    │   │
│  │  88 │+      }                                             │   │
│  │  89 │+    }                                               │   │
│  │  90 │+  }                                                 │   │
│  └───────────────────────────────────────────────────────────┘   │
│                                                                   │
│  ┌ src/api/__tests__/client.test.ts (+52 -0) ─── M3 ─────────┐  │
│  │  (collapsed — press Enter to expand)                       │   │
│  └───────────────────────────────────────────────────────────┘   │
│  ┌ src/errors/retry.ts (+30 -0) ─── M2 ──────────────────────┐  │
│  │  (collapsed — press Enter to expand)                       │   │
│  └───────────────────────────────────────────────────────────┘   │
│                                                                   │
├───────────────────────────────────────────────────────────────────┤
│ ✓ Complete  [sess_a1b2]  5/5 tasks  4 musicians  8m34s  $0.72   │
├───────────────────────────────────────────────────────────────────┤
│ > review complete. q to quit                                      │
╰───────────────────────────────────────────────────────────────────╯
```

**Key improvements:**
- **Collapsible file diffs** — Focus on the important changes, expand on demand.
- **Musician attribution** — Each diff hunk is tagged with which musician made the change.
- **Syntax-aware coloring** — Green for additions, red for deletions (standard diff colors).
- **Line numbers** — Both old and new line numbers for easy cross-referencing.

### 5.6 Narrow Terminal (80-col) — Responsive Degradation

```
╭─ ♫  📋  📊  📄  📝 ──────────────────────╮
│                                            │
│  ● 1. Retry  ✓ 2. Tests  ◦ 3. Docs       │
│  ▓▓▓▓▓▓▓▓▓▓▓▓░░░░░░░░░░░░  2/5  40%     │
│                                            │
│  ┌─ M1 ● Add retry ───────────────────┐   │
│  │ Editing src/api/client.ts          │   │
│  │ + fetchWithRetry(url, {            │   │
│  │ Running npm test...                │   │
│  │ > ✓ 12 passed  ✗ 1 failed         │   │
│  │ 89K  3m05s                    ▂▃▅▇ │   │
│  └────────────────────────────────────┘   │
│  ┌─ M2 ● Error handling ──────────────┐   │
│  │ Waiting for task 1...              │   │
│  │ 12K  0m45s                         │   │
│  └────────────────────────────────────┘   │
│  ┌─ M3 ✓ ── M4 ○ ────────────────────┐   │
│  │ Done 2m12s   │ Idle                │   │
│  └────────────────────────────────────┘   │
│                                            │
├────────────────────────────────────────────┤
│ ● Executing  2/5  $0.42        ?: help    │
├────────────────────────────────────────────┤
│ guidance> _                                │
╰────────────────────────────────────────────╯
```

**Narrow behavior:**
- Tab icons only (no labels)
- Single-column musician layout
- Insights panel hidden
- Idle/done musicians collapsed into shared row
- Truncated task labels

### 5.7 Init Phase — Task Input with Welcome

```
╭─ ♫ Conductor ────────────────────────────────────────────────────╮
│                                                                   │
│                                                                   │
│                    ♫ ♪ ♩  C O N D U C T O R  ♩ ♪ ♫               │
│                                                                   │
│                    Multi-agent AI Orchestrator                     │
│                                                                   │
│                                                                   │
│  Project: /Users/dom/code/my-project                              │
│  Branch:  feature/auth-refactor                                   │
│  Model:   opus (conductor) · sonnet (musicians)                   │
│  Workers: 4 max                                                   │
│                                                                   │
│                                                                   │
│  Recent Sessions ─────────────────────────────────────────────    │
│  ▸ sess_a1b2  Exploring  "Add retry logic"           2h ago      │
│    sess_c3d4  Complete   "Refactor auth module"      1d ago      │
│    sess_e5f6  Failed     "Migrate database"          3d ago      │
│                                                                   │
│                                                                   │
│  Drag & drop files or paste image paths to include context.       │
│  Type your task below, or resume a session with ↑↓ + Enter.      │
│                                                                   │
├───────────────────────────────────────────────────────────────────┤
│ ○ Init  Ready                                        ?: help     │
├───────────────────────────────────────────────────────────────────┤
│ task> Refactor the auth middleware to use encrypted session tok…  │
╰───────────────────────────────────────────────────────────────────╯
```

**Key improvements:**
- **Welcome screen** with project context (path, branch, model config, worker count)
- **Recent sessions** — Resume interrupted work without `conductor resume` CLI
- **File drop hint** — Users can include context images/files
- **ASCII art branding** using `tui-big-text` crate

---

## 6. Implementation Priorities

### Phase 1: Foundation (Must-Have)

| Item | Effort | Impact | Crates |
|------|--------|--------|--------|
| Replace hand-rolled input with `tui-textarea` | S | Fixes UTF-8 bug, adds undo/selection | `tui-textarea` |
| Add `Scrollbar` to all scrollable panels | S | Visual scroll position | built-in |
| Use `Gauge`/`LineGauge` for task progress | S | Replace `pbar()`, add header gauge | built-in |
| Add `BorderType::Rounded` for panels | XS | Instant visual upgrade | built-in |
| Parse ANSI in musician output | M | Color-correct tool output | `ansi-to-tui` |
| Add tab bar navigation | M | Unlock multi-view architecture | built-in `Tabs` |

### Phase 2: Content Quality

| Item | Effort | Impact | Crates |
|------|--------|--------|--------|
| Structured tool call rendering | L | Transform raw text into blocks | Custom parsing |
| Inline diff rendering | L | Code review in Diff tab | Custom or `similar` crate |
| Token cost tracking in Stats tab | M | Cost visibility | Extend `OrchestraState` |
| Sparkline per musician | S | Token burn rate visualization | built-in `Sparkline` |
| Welcome screen for Init phase | M | Better first experience | `tui-big-text` |

### Phase 3: Polish

| Item | Effort | Impact | Crates |
|------|--------|--------|--------|
| Theme system (JSON-configurable) | L | User customization | Custom |
| Transition animations | M | Visual polish | `tachyonfx` |
| Mouse support (click panels, scroll) | M | Accessibility | `crossterm` mouse events |
| @ file references in input | L | Context-aware prompting | `nucleo` (fuzzy finder) |
| Session history heatmap | M | Usage patterns | built-in `Calendar` |

---

## 7. Crate Shopping List

Based on the [community crates survey](community-crates.md) already done, here's the refined list aligned to the mockups:

### Immediate (Phase 1)

| Crate | Version | Purpose | Replaces |
|-------|---------|---------|----------|
| `tui-textarea` | 0.7.0 | Multi-line input with undo/selection | Hand-rolled `prompt_input` in `app.rs` |
| `ansi-to-tui` | 7.0.0 | Parse ANSI escape codes in musician output | Raw text rendering |
| `tui-popup` | via `tui-widgets` | Modal dialogs | `centered_rect + Clear` pattern |

### Soon (Phase 2)

| Crate | Version | Purpose | Replaces |
|-------|---------|---------|----------|
| `tui-scrollview` | via `tui-widgets` | Scrollable content with scrollbar | Manual `scroll_offset` |
| `tui-big-text` | via `tui-widgets` | ASCII art logo | Nothing (new feature) |
| `tui-tree-widget` | 0.24.0 | Task dependency tree | Flat task list |
| `similar` | 2.x | Diff computation | Nothing (new feature) |

### Later (Phase 3)

| Crate | Version | Purpose | Notes |
|-------|---------|---------|-------|
| `tachyonfx` | latest | Transition animations | ratatui org crate |
| `nucleo` | latest | Fuzzy file finder for @-references | Powers most TUI finders |
| `tui-markdown` | experimental | Markdown rendering | May need custom impl |
| `syntect` | 5.x | Syntax highlighting for diffs | Heavy but battle-tested |

---

## 8. Sources

### Projects Analyzed

- [OpenCode](https://github.com/opencode-ai/opencode) — Go-based AI coding agent with BubbleTea TUI
- [OpenCode TUI Docs](https://opencode.ai/docs/tui/) — TUI customization documentation
- [OpenCode Themes](https://opencode.ai/docs/themes/) — Theme system documentation
- [Crustly](https://github.com/jyjeanne/crustly) — Rust/Ratatui AI coding assistant
- [OLI (oli-tui)](https://crates.io/crates/oli-tui) — Rust/Ratatui Claude Code alternative
- [Agent-Deck](https://github.com/asheshgoplani/agent-deck) — Multi-agent session manager
- [OpenClaude](https://github.com/johmara/openclaude) — Ratatui wrapper for Claude Code
- [Rimuru](https://github.com/rohitg00/rimuru) — Multi-agent cost tracking platform
- [tmuxcc](https://github.com/nyanko3141592/tmuxcc) — TUI dashboard for AI coding agents in tmux
- [agtx](https://github.com/fynnfluegge/agtx) — Multi-session AI coding terminal manager
- [Rust-TUI-Coder](https://github.com/Ammar-Alnagar/Rust-TUI-Coder) — Ratatui AI coding assistant
- [Ralph TUI](https://www.linuxlinks.com/ralph-tui-ai-agent-loop-orchestrator/) — AI agent loop orchestrator

### Frameworks & Libraries

- [Ratatui](https://github.com/ratatui/ratatui) — Rust TUI framework (19.1k stars)
- [awesome-ratatui](https://github.com/ratatui/awesome-ratatui) — Curated list of Ratatui projects
- [tui-textarea](https://github.com/rhysd/tui-textarea) — Multi-line text editor widget
- [tui-scrollview](https://github.com/joshka/tui-scrollview) — Scrollable view widget
- [tui-markdown](https://github.com/joshka/tui-markdown) — Markdown rendering for Ratatui
- [Ratatui Third-Party Widgets](https://ratatui.rs/showcase/third-party-widgets/) — Widget showcase
- [tui-widgets](https://github.com/ratatui/tui-widgets) — Official widget collection
- [awesome-tuis](https://github.com/rothgar/awesome-tuis) — General TUI project list

### Articles & Talks

- [BubbleTea vs Ratatui](https://www.glukhov.org/post/2026/02/tui-frameworks-bubbletea-go-vs-ratatui-rust/) — Framework comparison
- [Reverse-engineering Claude's Generative UI](https://michaellivs.com/blog/reverse-engineering-claude-generative-ui/) — Claude Code rendering internals
- [Agentmaxxing: Multi-Agent Parallel](https://vibecoding.app/blog/agentmaxxing) — Multi-agent orchestration trends
- [Building a Terminal Orchestrator for AI Agents in Rust](https://houston.aitinkerers.org/talks/rsvp_AD3Q9uzasnc) — Rust TUI for agents talk
