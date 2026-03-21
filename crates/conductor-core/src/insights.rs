//! Pattern-matching insight generator — no extra LLM calls.
//!
//! Watches musician events and matches them against a built-in knowledge
//! base of common patterns. This keeps token costs at zero while still
//! providing educational context in the TUI.

use std::collections::HashSet;

use conductor_types::{Insight, InsightCategory, Task, TaskResult};

/// The type of event being processed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InsightEvent {
    ToolUse,
    TaskComplete,
    RateLimit,
    WorktreeMerge,
    TaskAssigned,
}

/// Context passed to rule matchers.
#[derive(Debug, Clone)]
pub struct InsightContext {
    pub event: InsightEvent,
    pub tool_name: Option<String>,
    pub tool_input: Option<serde_json::Value>,
    pub task: Option<Task>,
    pub result: Option<TaskResult>,
    pub decision: Option<String>,
    pub reasoning: Option<String>,
}

/// A single pattern-matching rule that can produce an insight.
struct InsightRule {
    category: InsightCategory,
    title: &'static str,
    body: RuleBody,
    matches: fn(&InsightContext) -> bool,
}

enum RuleBody {
    Static(&'static str),
    Dynamic(fn(&InsightContext) -> String),
}

/// Extracts a string field from a JSON Value object.
fn json_str(input: &serde_json::Value, field: &str) -> Option<String> {
    input.get(field).and_then(|v| v.as_str()).map(String::from)
}

/// Extracts just the filename from a path string.
fn filename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

fn build_rules() -> Vec<InsightRule> {
    vec![
        // ─── Tool usage patterns ────────────────────────────────
        InsightRule {
            category: InsightCategory::Tool,
            title: "Grep before editing",
            body: RuleBody::Dynamic(|ctx| {
                let pattern = ctx
                    .tool_input
                    .as_ref()
                    .and_then(|v| json_str(v, "pattern"))
                    .map(|p| format!("\"{}\"", p))
                    .unwrap_or_else(|| "references".into());
                let task_ctx = ctx
                    .task
                    .as_ref()
                    .map(|t| format!(" for task \"{}\"", t.title))
                    .unwrap_or_default();
                format!(
                    "Searching for {}{} — finding all references before modifying code prevents breaking callers in other files.",
                    pattern, task_ctx
                )
            }),
            matches: |ctx| ctx.event == InsightEvent::ToolUse && ctx.tool_name.as_deref() == Some("Grep"),
        },
        InsightRule {
            category: InsightCategory::Tool,
            title: "Glob for file discovery",
            body: RuleBody::Dynamic(|ctx| {
                let pattern = ctx
                    .tool_input
                    .as_ref()
                    .and_then(|v| json_str(v, "pattern"))
                    .map(|p| format!("\"{}\"", p))
                    .unwrap_or_else(|| "files".into());
                format!(
                    "Scanning {} to understand project structure. More efficient than reading directories manually.",
                    pattern
                )
            }),
            matches: |ctx| ctx.event == InsightEvent::ToolUse && ctx.tool_name.as_deref() == Some("Glob"),
        },
        InsightRule {
            category: InsightCategory::Pattern,
            title: "Reading tests first",
            body: RuleBody::Dynamic(|ctx| {
                let file = ctx
                    .tool_input
                    .as_ref()
                    .and_then(|v| json_str(v, "file_path"))
                    .map(|p| filename(&p).to_string())
                    .unwrap_or_else(|| "test file".into());
                format!(
                    "Reading {} before implementation helps understand expected behavior — tests are executable documentation.",
                    file
                )
            }),
            matches: |ctx| {
                if ctx.event != InsightEvent::ToolUse || ctx.tool_name.as_deref() != Some("Read") {
                    return false;
                }
                ctx.tool_input
                    .as_ref()
                    .and_then(|v| json_str(v, "file_path"))
                    .map(|p| is_test_file(&p))
                    .unwrap_or(false)
            },
        },
        InsightRule {
            category: InsightCategory::Pattern,
            title: "Self-review via git diff",
            body: RuleBody::Static(
                "Reviewing own changes before finishing — a quality gate that catches unintended modifications.",
            ),
            matches: |ctx| {
                if ctx.event != InsightEvent::ToolUse || ctx.tool_name.as_deref() != Some("Bash") {
                    return false;
                }
                ctx.tool_input
                    .as_ref()
                    .and_then(|v| json_str(v, "command"))
                    .map(|cmd| cmd.contains("git diff"))
                    .unwrap_or(false)
            },
        },
        InsightRule {
            category: InsightCategory::Architecture,
            title: "Service extraction",
            body: RuleBody::Dynamic(|ctx| {
                let file = ctx
                    .tool_input
                    .as_ref()
                    .and_then(|v| json_str(v, "file_path"))
                    .map(|p| filename(&p).to_string())
                    .unwrap_or_else(|| "service".into());
                format!(
                    "Creating {} follows the Single Responsibility Principle — isolating logic makes it testable and reusable via dependency injection.",
                    file
                )
            }),
            matches: |ctx| {
                if ctx.event != InsightEvent::ToolUse || ctx.tool_name.as_deref() != Some("Write") {
                    return false;
                }
                ctx.tool_input
                    .as_ref()
                    .and_then(|v| json_str(v, "file_path"))
                    .map(|p| is_service_file(&p))
                    .unwrap_or(false)
            },
        },
        InsightRule {
            category: InsightCategory::Architecture,
            title: "Guard pattern",
            body: RuleBody::Static(
                "NestJS guards run before route handlers in the request pipeline. One decorator protects any endpoint — DRY and consistent authorization.",
            ),
            matches: |ctx| {
                if ctx.event != InsightEvent::ToolUse {
                    return false;
                }
                let is_write_or_edit = matches!(ctx.tool_name.as_deref(), Some("Write") | Some("Edit"));
                if !is_write_or_edit {
                    return false;
                }
                ctx.tool_input
                    .as_ref()
                    .and_then(|v| json_str(v, "file_path"))
                    .map(|p| is_guard_file(&p))
                    .unwrap_or(false)
            },
        },
        InsightRule {
            category: InsightCategory::Pattern,
            title: "Test-driven approach",
            body: RuleBody::Static(
                "Writing tests alongside implementation catches regressions early. Each service change gets a corresponding test update.",
            ),
            matches: |ctx| {
                if ctx.event != InsightEvent::ToolUse {
                    return false;
                }
                let is_write_or_edit = matches!(ctx.tool_name.as_deref(), Some("Write") | Some("Edit"));
                if !is_write_or_edit {
                    return false;
                }
                ctx.tool_input
                    .as_ref()
                    .and_then(|v| json_str(v, "file_path"))
                    .map(|p| is_test_file(&p))
                    .unwrap_or(false)
            },
        },
        // ─── File reading with task context ──────────────────────
        InsightRule {
            category: InsightCategory::Tool,
            title: "Reading source file",
            body: RuleBody::Dynamic(|ctx| {
                let file = ctx
                    .tool_input
                    .as_ref()
                    .and_then(|v| json_str(v, "file_path"))
                    .map(|p| filename(&p).to_string())
                    .unwrap_or_else(|| "file".into());
                let task_ctx = ctx
                    .task
                    .as_ref()
                    .map(|t| format!(" to understand context for \"{}\"", t.title))
                    .unwrap_or_default();
                format!(
                    "Reading {}{}. Understanding existing code before modifying prevents breaking changes.",
                    file, task_ctx
                )
            }),
            matches: |ctx| {
                if ctx.event != InsightEvent::ToolUse || ctx.tool_name.as_deref() != Some("Read") {
                    return false;
                }
                ctx.tool_input
                    .as_ref()
                    .and_then(|v| json_str(v, "file_path"))
                    .map(|p| !is_test_file(&p))
                    .unwrap_or(false)
            },
        },
        // ─── File modification with task context ─────────────────
        InsightRule {
            category: InsightCategory::Tool,
            title: "Modifying file",
            body: RuleBody::Dynamic(|ctx| {
                let file = ctx
                    .tool_input
                    .as_ref()
                    .and_then(|v| json_str(v, "file_path"))
                    .map(|p| filename(&p).to_string())
                    .unwrap_or_else(|| "file".into());
                let task_ctx = ctx
                    .task
                    .as_ref()
                    .map(|t| format!(" for \"{}\"", t.title))
                    .unwrap_or_default();
                let scope = ctx
                    .task
                    .as_ref()
                    .filter(|t| !t.file_scope.is_empty())
                    .map(|t| format!(". Files in scope: {}", t.file_scope.join(", ")))
                    .unwrap_or_default();
                format!("Editing {}{}{}", file, task_ctx, scope)
            }),
            matches: |ctx| {
                if ctx.event != InsightEvent::ToolUse || ctx.tool_name.as_deref() != Some("Edit") {
                    return false;
                }
                ctx.tool_input
                    .as_ref()
                    .and_then(|v| v.get("file_path"))
                    .is_some()
            },
        },
        // ─── Task completion patterns ───────────────────────────
        InsightRule {
            category: InsightCategory::Decision,
            title: "Task completed successfully",
            body: RuleBody::Dynamic(|ctx| {
                let task_title = ctx
                    .task
                    .as_ref()
                    .map(|t| t.title.as_str())
                    .unwrap_or("Task");
                let files = ctx
                    .result
                    .as_ref()
                    .filter(|r| !r.files_modified.is_empty())
                    .map(|r| r.files_modified.join(", "))
                    .unwrap_or_else(|| "none".into());
                format!(
                    "\"{}\" finished. Modified files: {}. File-scoping prevents merge conflicts — like team code ownership at the file level.",
                    task_title, files
                )
            }),
            matches: |ctx| {
                ctx.event == InsightEvent::TaskComplete
                    && ctx.result.as_ref().map(|r| r.success).unwrap_or(false)
            },
        },
        InsightRule {
            category: InsightCategory::Decision,
            title: "Task failed — retry possible",
            body: RuleBody::Dynamic(|ctx| {
                let task_title = ctx
                    .task
                    .as_ref()
                    .map(|t| t.title.as_str())
                    .unwrap_or("Task");
                let error = ctx
                    .result
                    .as_ref()
                    .and_then(|r| r.error.as_deref())
                    .unwrap_or("unknown error");
                format!(
                    "\"{}\" hit an issue: {}. Conductor will review and decide: retry with adjusted instructions, reassign, or mark as blocked.",
                    task_title, error
                )
            }),
            matches: |ctx| {
                ctx.event == InsightEvent::TaskComplete
                    && ctx.result.as_ref().map(|r| !r.success).unwrap_or(false)
            },
        },
        // ─── Infrastructure patterns ────────────────────────────
        InsightRule {
            category: InsightCategory::Concept,
            title: "Rate limit — rolling window",
            body: RuleBody::Static(
                "5-hour rolling window: tokens expire from the oldest edge. Capacity returns gradually, not all at once. Probe checks every 30 seconds.",
            ),
            matches: |ctx| ctx.event == InsightEvent::RateLimit,
        },
        InsightRule {
            category: InsightCategory::Architecture,
            title: "Worktree merge",
            body: RuleBody::Static(
                "Git worktrees let musicians branch in parallel. Merging is clean because file scopes minimize overlap — like feature branches on steroids.",
            ),
            matches: |ctx| ctx.event == InsightEvent::WorktreeMerge,
        },
    ]
}

/// Returns true if the path looks like a test file (e.g. `.spec.ts`, `.test.js`).
fn is_test_file(path: &str) -> bool {
    let extensions = [
        ".spec.ts",
        ".test.ts",
        ".spec.js",
        ".test.js",
        ".spec.tsx",
        ".test.tsx",
        ".spec.jsx",
        ".test.jsx",
    ];
    extensions.iter().any(|ext| path.ends_with(ext))
}

/// Returns true if the path looks like a service file.
fn is_service_file(path: &str) -> bool {
    path.ends_with(".service.ts") || path.ends_with(".service.js")
}

/// Returns true if the path looks like a guard file.
fn is_guard_file(path: &str) -> bool {
    path.ends_with(".guard.ts") || path.ends_with(".guard.js")
}

fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339()
}

/// Pattern-matching insight generator.
///
/// Watches musician events and matches them against a built-in knowledge
/// base of common patterns. No LLM calls, no I/O — pure rule matching.
pub struct InsightGenerator {
    insights: Vec<Insight>,
    seen_keys: HashSet<String>,
    rules: Vec<InsightRule>,
}

impl InsightGenerator {
    pub fn new() -> Self {
        Self {
            insights: Vec::new(),
            seen_keys: HashSet::new(),
            rules: build_rules(),
        }
    }

    /// Process a tool_use event from a musician, optionally with task context.
    pub fn on_tool_use(
        &mut self,
        worker_id: &str,
        tool_name: &str,
        tool_input: Option<serde_json::Value>,
        task: Option<&Task>,
    ) {
        self.evaluate(
            &InsightContext {
                event: InsightEvent::ToolUse,
                tool_name: Some(tool_name.to_string()),
                tool_input,
                task: task.cloned(),
                result: None,
                decision: None,
                reasoning: None,
            },
            worker_id,
        );
    }

    /// Process a task completion.
    pub fn on_task_complete(&mut self, worker_id: &str, task: &Task, result: &TaskResult) {
        self.evaluate(
            &InsightContext {
                event: InsightEvent::TaskComplete,
                tool_name: None,
                tool_input: None,
                task: Some(task.clone()),
                result: Some(result.clone()),
                decision: None,
                reasoning: None,
            },
            worker_id,
        );
    }

    /// Process a Conductor planning decision.
    pub fn on_conductor_decision(&mut self, decision: &str, reasoning: &str) {
        self.insights.push(Insight {
            timestamp: now_iso(),
            category: InsightCategory::Decision,
            title: decision.to_string(),
            body: reasoning.to_string(),
            source: "conductor".to_string(),
        });
    }

    /// Process a rate limit event.
    pub fn on_rate_limit(&mut self) {
        self.evaluate(
            &InsightContext {
                event: InsightEvent::RateLimit,
                tool_name: None,
                tool_input: None,
                task: None,
                result: None,
                decision: None,
                reasoning: None,
            },
            "system",
        );
    }

    /// Process a worktree merge.
    pub fn on_worktree_merge(&mut self, worker_id: &str) {
        self.evaluate(
            &InsightContext {
                event: InsightEvent::WorktreeMerge,
                tool_name: None,
                tool_input: None,
                task: None,
                result: None,
                decision: None,
                reasoning: None,
            },
            worker_id,
        );
    }

    /// Generate a "why" insight when a task is assigned to a musician.
    pub fn on_task_assigned(&mut self, worker_id: &str, task: &Task) {
        let deps_explain = if !task.dependencies.is_empty() {
            let deps: Vec<String> = task
                .dependencies
                .iter()
                .map(|d| format!("Task {}", d + 1))
                .collect();
            format!("\nDepends on: {}", deps.join(", "))
        } else {
            "\nNo dependencies — can start immediately.".to_string()
        };

        let model_explain = task
            .model
            .as_ref()
            .map(|m| {
                let desc = match m.as_str() {
                    "opus" => "complex task requiring deep reasoning",
                    "haiku" => "simple/trivial task",
                    _ => "standard complexity",
                };
                format!("\nModel: {} ({})", m, desc)
            })
            .unwrap_or_default();

        let file_scope = if !task.file_scope.is_empty() {
            format!("\nFiles: {}", task.file_scope.join(", "))
        } else {
            String::new()
        };

        let criteria = if !task.acceptance_criteria.is_empty() {
            format!("\nSuccess criteria: {}", task.acceptance_criteria.join("; "))
        } else {
            String::new()
        };

        let why_text = if !task.why.is_empty() {
            &task.why
        } else {
            &task.description
        };

        self.insights.push(Insight {
            timestamp: now_iso(),
            category: InsightCategory::Why,
            title: format!("Task {} — \"{}\"", task.index + 1, task.title),
            body: format!(
                "{}{}{}{}{}",
                why_text, deps_explain, model_explain, file_scope, criteria
            ),
            source: worker_id.to_string(),
        });
    }

    /// Add a custom insight (e.g., from Conductor's learningNotes).
    /// Deduplicates by title.
    pub fn add_insight(&mut self, insight: Insight) {
        if !self.seen_keys.contains(&insight.title) {
            self.seen_keys.insert(insight.title.clone());
            self.insights.push(insight);
        }
    }

    /// Get the latest N insights for TUI display.
    pub fn get_insights(&self, count: usize) -> &[Insight] {
        let len = self.insights.len();
        if count >= len {
            &self.insights
        } else {
            &self.insights[len - count..]
        }
    }

    /// Get all insights.
    pub fn get_all_insights(&self) -> &[Insight] {
        &self.insights
    }

    fn evaluate(&mut self, context: &InsightContext, source: &str) {
        for rule in &self.rules {
            let dedup_key = format!("{}:{:?}:{}", rule.title, rule.category, source);
            if (rule.matches)(context) && !self.seen_keys.contains(&dedup_key) {
                self.seen_keys.insert(dedup_key);
                let body = match &rule.body {
                    RuleBody::Static(s) => (*s).to_string(),
                    RuleBody::Dynamic(f) => f(context),
                };
                self.insights.push(Insight {
                    timestamp: now_iso(),
                    category: rule.category.clone(),
                    title: rule.title.to_string(),
                    body,
                    source: source.to_string(),
                });
                break; // One insight per event
            }
        }
    }
}

impl Default for InsightGenerator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample_task() -> Task {
        Task {
            id: "t1".into(),
            index: 0,
            title: "Fix auth bug".into(),
            description: "Fix the authentication issue".into(),
            why: "Users can't log in".into(),
            file_scope: vec!["src/auth.ts".into()],
            dependencies: vec![],
            acceptance_criteria: vec!["Login works".into()],
            estimated_turns: 5,
            model: Some("opus".into()),
            status: conductor_types::TaskStatus::Queued,
            assigned_musician: None,
            result: None,
        }
    }

    #[test]
    fn grep_tool_generates_insight() {
        let mut generator = InsightGenerator::new();
        generator.on_tool_use(
            "m1",
            "Grep",
            Some(json!({"pattern": "handleAuth"})),
            None,
        );
        let insights = generator.get_all_insights();
        assert_eq!(insights.len(), 1);
        assert_eq!(insights[0].title, "Grep before editing");
        assert!(insights[0].body.contains("\"handleAuth\""));
        assert_eq!(insights[0].source, "m1");
    }

    #[test]
    fn grep_with_task_context() {
        let mut generator = InsightGenerator::new();
        let task = sample_task();
        generator.on_tool_use(
            "m1",
            "Grep",
            Some(json!({"pattern": "login"})),
            Some(&task),
        );
        let insights = generator.get_all_insights();
        assert_eq!(insights.len(), 1);
        assert!(insights[0].body.contains("Fix auth bug"));
    }

    #[test]
    fn glob_tool_generates_insight() {
        let mut generator = InsightGenerator::new();
        generator.on_tool_use(
            "m1",
            "Glob",
            Some(json!({"pattern": "**/*.ts"})),
            None,
        );
        let insights = generator.get_all_insights();
        assert_eq!(insights.len(), 1);
        assert_eq!(insights[0].title, "Glob for file discovery");
        assert!(insights[0].body.contains("\"**/*.ts\""));
    }

    #[test]
    fn reading_test_file_generates_reading_tests_insight() {
        let mut generator = InsightGenerator::new();
        generator.on_tool_use(
            "m1",
            "Read",
            Some(json!({"file_path": "src/auth.spec.ts"})),
            None,
        );
        let insights = generator.get_all_insights();
        assert_eq!(insights.len(), 1);
        assert_eq!(insights[0].title, "Reading tests first");
        assert!(insights[0].body.contains("auth.spec.ts"));
    }

    #[test]
    fn reading_source_file_generates_reading_insight() {
        let mut generator = InsightGenerator::new();
        generator.on_tool_use(
            "m1",
            "Read",
            Some(json!({"file_path": "src/auth.ts"})),
            None,
        );
        let insights = generator.get_all_insights();
        assert_eq!(insights.len(), 1);
        assert_eq!(insights[0].title, "Reading source file");
    }

    #[test]
    fn git_diff_generates_self_review_insight() {
        let mut generator = InsightGenerator::new();
        generator.on_tool_use(
            "m1",
            "Bash",
            Some(json!({"command": "git diff --stat"})),
            None,
        );
        let insights = generator.get_all_insights();
        assert_eq!(insights.len(), 1);
        assert_eq!(insights[0].title, "Self-review via git diff");
    }

    #[test]
    fn service_file_write_generates_service_extraction_insight() {
        let mut generator = InsightGenerator::new();
        generator.on_tool_use(
            "m1",
            "Write",
            Some(json!({"file_path": "src/upload/r2.service.ts"})),
            None,
        );
        let insights = generator.get_all_insights();
        assert_eq!(insights.len(), 1);
        assert_eq!(insights[0].title, "Service extraction");
        assert!(insights[0].body.contains("r2.service.ts"));
    }

    #[test]
    fn guard_file_edit_generates_guard_insight() {
        let mut generator = InsightGenerator::new();
        generator.on_tool_use(
            "m1",
            "Edit",
            Some(json!({"file_path": "src/auth/jwt.guard.ts"})),
            None,
        );
        let insights = generator.get_all_insights();
        assert_eq!(insights.len(), 1);
        assert_eq!(insights[0].title, "Guard pattern");
    }

    #[test]
    fn writing_test_file_generates_test_driven_insight() {
        let mut generator = InsightGenerator::new();
        generator.on_tool_use(
            "m1",
            "Write",
            Some(json!({"file_path": "src/auth.spec.ts"})),
            None,
        );
        let insights = generator.get_all_insights();
        assert_eq!(insights.len(), 1);
        assert_eq!(insights[0].title, "Test-driven approach");
    }

    #[test]
    fn edit_file_generates_modifying_insight() {
        let mut generator = InsightGenerator::new();
        generator.on_tool_use(
            "m1",
            "Edit",
            Some(json!({"file_path": "src/main.ts"})),
            None,
        );
        let insights = generator.get_all_insights();
        assert_eq!(insights.len(), 1);
        assert_eq!(insights[0].title, "Modifying file");
    }

    #[test]
    fn task_complete_success() {
        let mut generator = InsightGenerator::new();
        let task = sample_task();
        let result = TaskResult {
            success: true,
            files_modified: vec!["src/auth.ts".into()],
            summary: "Fixed it".into(),
            error: None,
            tokens_used: 500,
            duration_ms: 3000,
            diff: None,
            verification_output: None,
            verification_passed: None,
        };
        generator.on_task_complete("m1", &task, &result);
        let insights = generator.get_all_insights();
        assert_eq!(insights.len(), 1);
        assert_eq!(insights[0].title, "Task completed successfully");
        assert!(insights[0].body.contains("src/auth.ts"));
    }

    #[test]
    fn task_complete_failure() {
        let mut generator = InsightGenerator::new();
        let task = sample_task();
        let result = TaskResult {
            success: false,
            files_modified: vec![],
            summary: "Failed".into(),
            error: Some("compilation error".into()),
            tokens_used: 200,
            duration_ms: 1000,
            diff: None,
            verification_output: None,
            verification_passed: None,
        };
        generator.on_task_complete("m1", &task, &result);
        let insights = generator.get_all_insights();
        assert_eq!(insights.len(), 1);
        assert_eq!(insights[0].title, "Task failed — retry possible");
        assert!(insights[0].body.contains("compilation error"));
    }

    #[test]
    fn rate_limit_generates_insight() {
        let mut generator = InsightGenerator::new();
        generator.on_rate_limit();
        let insights = generator.get_all_insights();
        assert_eq!(insights.len(), 1);
        assert_eq!(insights[0].title, "Rate limit — rolling window");
        assert_eq!(insights[0].source, "system");
    }

    #[test]
    fn worktree_merge_generates_insight() {
        let mut generator = InsightGenerator::new();
        generator.on_worktree_merge("m2");
        let insights = generator.get_all_insights();
        assert_eq!(insights.len(), 1);
        assert_eq!(insights[0].title, "Worktree merge");
        assert_eq!(insights[0].source, "m2");
    }

    #[test]
    fn conductor_decision_recorded() {
        let mut generator = InsightGenerator::new();
        generator.on_conductor_decision("Reassign task 2", "Musician 1 is rate-limited");
        let insights = generator.get_all_insights();
        assert_eq!(insights.len(), 1);
        assert_eq!(insights[0].title, "Reassign task 2");
        assert_eq!(insights[0].body, "Musician 1 is rate-limited");
        assert_eq!(insights[0].source, "conductor");
    }

    #[test]
    fn task_assigned_generates_why_insight() {
        let mut generator = InsightGenerator::new();
        let task = sample_task();
        generator.on_task_assigned("m1", &task);
        let insights = generator.get_all_insights();
        assert_eq!(insights.len(), 1);
        assert_eq!(insights[0].category, InsightCategory::Why);
        assert!(insights[0].title.contains("Fix auth bug"));
        assert!(insights[0].body.contains("Users can't log in"));
        assert!(insights[0].body.contains("opus"));
        assert!(insights[0].body.contains("src/auth.ts"));
        assert!(insights[0].body.contains("Login works"));
    }

    #[test]
    fn task_assigned_with_dependencies() {
        let mut generator = InsightGenerator::new();
        let mut task = sample_task();
        task.dependencies = vec![0, 2];
        generator.on_task_assigned("m1", &task);
        let insights = generator.get_all_insights();
        assert!(insights[0].body.contains("Task 1"));
        assert!(insights[0].body.contains("Task 3"));
    }

    #[test]
    fn dedup_same_rule_same_source() {
        let mut generator = InsightGenerator::new();
        generator.on_tool_use("m1", "Grep", Some(json!({"pattern": "foo"})), None);
        generator.on_tool_use("m1", "Grep", Some(json!({"pattern": "bar"})), None);
        // Second Grep from same musician should be deduped
        assert_eq!(generator.get_all_insights().len(), 1);
    }

    #[test]
    fn different_sources_not_deduped() {
        let mut generator = InsightGenerator::new();
        generator.on_tool_use("m1", "Grep", Some(json!({"pattern": "foo"})), None);
        generator.on_tool_use("m2", "Grep", Some(json!({"pattern": "bar"})), None);
        // Different musicians can trigger the same insight type
        assert_eq!(generator.get_all_insights().len(), 2);
    }

    #[test]
    fn add_insight_deduplicates_by_title() {
        let mut generator = InsightGenerator::new();
        let insight = Insight {
            timestamp: "2026-01-01T00:00:00Z".into(),
            category: InsightCategory::Concept,
            title: "Custom insight".into(),
            body: "First".into(),
            source: "test".into(),
        };
        generator.add_insight(insight.clone());
        let insight2 = Insight {
            body: "Second".into(),
            ..insight
        };
        generator.add_insight(insight2);
        assert_eq!(generator.get_all_insights().len(), 1);
        assert_eq!(generator.get_all_insights()[0].body, "First");
    }

    #[test]
    fn get_insights_returns_latest_n() {
        let mut generator = InsightGenerator::new();
        generator.on_conductor_decision("D1", "R1");
        generator.on_conductor_decision("D2", "R2");
        generator.on_conductor_decision("D3", "R3");
        assert_eq!(generator.get_insights(2).len(), 2);
        assert_eq!(generator.get_insights(2)[0].title, "D2");
        assert_eq!(generator.get_insights(2)[1].title, "D3");
    }

    #[test]
    fn get_insights_with_count_larger_than_available() {
        let mut generator = InsightGenerator::new();
        generator.on_conductor_decision("D1", "R1");
        assert_eq!(generator.get_insights(10).len(), 1);
    }

    #[test]
    fn one_insight_per_event() {
        let mut generator = InsightGenerator::new();
        // Edit on a guard file could match both "Guard pattern" and "Modifying file"
        // but should only produce one insight
        generator.on_tool_use(
            "m1",
            "Edit",
            Some(json!({"file_path": "src/auth.guard.ts"})),
            None,
        );
        assert_eq!(generator.get_all_insights().len(), 1);
    }
}
