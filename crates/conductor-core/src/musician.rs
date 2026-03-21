//! Musician agent — executes a single task in an isolated git worktree.
//!
//! Streams NDJSON events to the orchestra via channels, detects rate
//! limits, and produces a TaskResult on completion.
//!
//! Supports prompt injection via `inject_prompt()` — messages are
//! sent via stdin and queued until Claude finishes its current turn.

use std::collections::HashSet;

use conductor_bridge::{ClaudeSession, ClaudeSessionOptions};
use conductor_types::{
    Checkpoint, ClaudeEvent, ClaudeEventType, MusicianState, MusicianStatus, OrchestraEvent, Task,
    TaskResult, TokenUsage,
};
use tokio::sync::mpsc;

use crate::conductor_agent::load_project_instructions;
use crate::rate_limiter::is_rate_limit_message;
use crate::token_estimate::{calibrate_token_estimate, estimate_tokens};
use crate::tool_summary::summarize_tool_use;

/// Maximum number of output lines kept per musician.
const OUTPUT_BUFFER_SIZE: usize = 200;
/// Maximum total characters across all output lines (100KB).
const OUTPUT_BUFFER_MAX_CHARS: usize = 100_000;

/// Build the musician system prompt for a task.
fn build_musician_prompt(task: &Task, project_path: &str) -> String {
    let project_instructions = load_project_instructions(project_path, 12_000);
    let conventions_block = if project_instructions.is_empty() {
        String::new()
    } else {
        format!(
            "## Project Conventions\nFollow these project conventions from CLAUDE.md:\n\n{project_instructions}\n\n---\n\n"
        )
    };

    let file_scope = task
        .file_scope
        .iter()
        .map(|f| format!("- {f}"))
        .collect::<Vec<_>>()
        .join("\n");

    let acceptance = task
        .acceptance_criteria
        .iter()
        .enumerate()
        .map(|(i, c)| format!("{}. {c}", i + 1))
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        "You are a Musician agent in the Orchestra orchestration system. You have been assigned a specific task to complete.\n\
         \n\
         ## Your Task\n\
         **{title}**\n\
         \n\
         {description}\n\
         \n\
         {conventions}## File Scope\n\
         You should primarily work within these files:\n\
         {file_scope}\n\
         \n\
         ## Acceptance Criteria\n\
         {acceptance}\n\
         \n\
         ## Instructions\n\
         1. Read and understand the relevant files\n\
         2. Implement the changes described in the task\n\
         3. Do NOT modify files outside your file scope unless absolutely necessary\n\
         4. **Checkpoint Protocol**: After every 5 turns of editing, or after completing a logical unit of work:\n\
         \x20\x20 a. Run `npm run typecheck` (or the project's verification command)\n\
         \x20\x20 b. If errors: fix them immediately before continuing\n\
         \x20\x20 c. Commit your progress: `git add -A && git commit -m \"checkpoint: <brief summary of what was done>\"`\n\
         \x20\x20 This protects your work against crashes and lets the system track your progress.\n\
         5. After all implementation is done, run final verification:\n\
         \x20\x20 - Run `npm run typecheck` in the relevant package directory\n\
         \x20\x20 - If typecheck fails, fix the errors before finishing\n\
         \x20\x20 - Report any unfixable issues in your summary\n\
         6. Write a brief summary of what you changed when done\n\
         7. If you discover something surprising (API changed, type renamed, unexpected pattern), share it clearly in your summary — other musicians may need this information\n\
         \n\
         Complete the task thoroughly. Do not ask for confirmation — execute autonomously.",
        title = task.title,
        description = task.description,
        conventions = conventions_block,
        file_scope = file_scope,
        acceptance = acceptance,
    )
}

/// Musician agent — executes tasks via Claude CLI sessions in isolated worktrees.
pub struct Musician {
    id: String,
    index: usize,
    model: String,
    original_model: String,
    max_turns: u32,
    state: MusicianState,
    session: Option<ClaudeSession>,
}

impl Musician {
    /// Create a new idle musician.
    pub fn new(id: String, index: usize, model: String, max_turns: u32) -> Self {
        let state = MusicianState {
            id: id.clone(),
            index,
            status: MusicianStatus::Idle,
            current_task: None,
            output_lines: Vec::new(),
            tokens_used: 0,
            token_usage: TokenUsage::default(),
            tokens_estimated: false,
            started_at: None,
            elapsed_ms: 0,
            worktree_path: None,
            branch: None,
            checkpoint: None,
            prompt_sent: None,
            cost_usd: 0.0,
        };
        Self {
            id,
            index,
            model: model.clone(),
            original_model: model,
            max_turns,
            state,
            session: None,
        }
    }

    /// Return a clone of the current state.
    pub fn get_state(&self) -> MusicianState {
        self.state.clone()
    }

    /// Override model for the next task execution (smart model routing).
    pub fn set_model_override(&mut self, model: &str) {
        self.model = model.to_string();
    }

    /// Check if this musician has an active session that accepts prompts.
    pub fn is_interactive(&self) -> bool {
        self.session
            .as_ref()
            .map(|s| !s.is_closed())
            .unwrap_or(false)
    }

    /// Inject a prompt into the running Claude session.
    ///
    /// The message is sent via stdin in stream-json format and will be
    /// processed when Claude finishes its current turn.
    ///
    /// Returns true if the message was sent, false if no session is active.
    pub async fn inject_prompt(&mut self, text: &str) -> bool {
        let session = match self.session.as_mut() {
            Some(s) if !s.is_closed() => s,
            _ => return false,
        };

        if session.send_message(text).await.is_err() {
            return false;
        }

        let status_note = if self.state.status == MusicianStatus::Running {
            "(queued — musician thinking) "
        } else {
            ""
        };
        let label = format!("[USER] {status_note}{text}");
        self.push_output(&label);
        true
    }

    /// Set musician to waiting state (blocked on dependency).
    pub fn set_waiting(&mut self, task: &Task) {
        let dep_list = task
            .dependencies
            .iter()
            .map(|d| format!("Task {}", d + 1))
            .collect::<Vec<_>>()
            .join(", ");
        self.state.status = MusicianStatus::Waiting;
        self.state.current_task = Some(task.clone());
        self.state.output_lines = vec![format!("Waiting for dependencies: {dep_list}")];
    }

    /// Pause the musician (rate limited).
    pub fn pause(&mut self) {
        self.state.status = MusicianStatus::Paused;
    }

    /// Reset to idle, drop session, clear output.
    pub fn reset(&mut self) {
        self.session = None;
        self.model = self.original_model.clone();
        self.state.status = MusicianStatus::Idle;
        self.state.current_task = None;
        self.state.output_lines.clear();
    }

    /// Push a line to the output buffer externally (e.g., queued guidance display).
    pub fn push_external_output(&mut self, text: &str) {
        self.push_output(text);
    }

    /// Execute a task in the given worktree.
    ///
    /// Creates a `ClaudeSession`, streams events through `event_tx`,
    /// and returns a `TaskResult` when the session completes.
    ///
    /// # Arguments
    /// * `task` — the task to execute
    /// * `worktree_path` — working directory for the Claude session
    /// * `branch` — git branch name
    /// * `project_path` — original project root (for loading CLAUDE.md)
    /// * `event_tx` — channel to send events to the orchestra
    /// * `read_only` — if true, restrict tools and skip shared memory
    /// * `shared_memory_content` — optional shared memory to append to system prompt
    pub async fn execute(
        &mut self,
        task: Task,
        worktree_path: &str,
        branch: &str,
        project_path: &str,
        event_tx: mpsc::Sender<OrchestraEvent>,
        read_only: bool,
        shared_memory_content: Option<String>,
    ) -> TaskResult {
        // Reset state for this execution
        let now = chrono::Utc::now().to_rfc3339();
        self.state = MusicianState {
            id: self.id.clone(),
            index: self.index,
            status: MusicianStatus::Running,
            current_task: Some(task.clone()),
            output_lines: Vec::new(),
            tokens_used: 0,
            token_usage: TokenUsage::default(),
            tokens_estimated: true,
            started_at: Some(now),
            elapsed_ms: 0,
            worktree_path: Some(worktree_path.to_string()),
            branch: Some(branch.to_string()),
            checkpoint: None,
            prompt_sent: None,
            cost_usd: 0.0,
        };
        self.send_status_change(&event_tx).await;

        let start = std::time::Instant::now();
        let mut last_message = String::new();
        let mut total_tokens: u64 = 0;
        let mut turn_count: u32 = 0;
        let mut files_modified: HashSet<String> = HashSet::new();
        let mut received_result = false;
        let mut result_was_error = false;

        // Build prompt
        let (prompt, system_append) = if read_only {
            (task.description.clone(), None)
        } else {
            let prompt = build_musician_prompt(&task, project_path);
            let system_append = shared_memory_content
                .filter(|s| !s.is_empty())
                .map(|s| format!("\n\n# Shared Memory (from other musicians)\n{s}"));
            (prompt, system_append)
        };

        // Store the full prompt for inspection
        self.state.prompt_sent = Some(match &system_append {
            Some(sys) => format!("{prompt}\n\n───── APPENDED SYSTEM PROMPT ─────\n{sys}"),
            None => prompt.clone(),
        });

        // Create session
        let allowed_tools = if read_only {
            Some(vec![
                "Read".into(),
                "Glob".into(),
                "Grep".into(),
                "Bash(git log)".into(),
                "Bash(git status)".into(),
            ])
        } else {
            None
        };

        let mut session = ClaudeSession::new(ClaudeSessionOptions {
            model: self.model.clone(),
            max_turns: Some(self.max_turns),
            allowed_tools,
            append_system_prompt: system_append,
            cwd: Some(worktree_path.to_string()),
        });

        // Create internal channel for session events
        let (claude_tx, mut claude_rx) = mpsc::channel::<ClaudeEvent>(256);

        // Start the session
        if let Err(e) = session.start(&prompt, claude_tx).await {
            tracing::error!(error = %e, musician = %self.id, "failed to start Claude session");
            self.state.status = MusicianStatus::Failed;
            self.send_status_change(&event_tx).await;
            return TaskResult {
                success: false,
                files_modified: Vec::new(),
                summary: "Failed to start Claude session".into(),
                error: Some(e.to_string()),
                tokens_used: 0,
                duration_ms: start.elapsed().as_millis() as u64,
                diff: None,
                verification_output: None,
                verification_passed: None,
            };
        }

        self.session = Some(session);

        // Event processing loop
        while let Some(event) = claude_rx.recv().await {
            self.state.elapsed_ms = start.elapsed().as_millis() as u64;

            match event.event_type {
                ClaudeEventType::Assistant => {
                    if let Some(ref message) = event.message {
                        last_message = message.clone();
                        self.push_output(message);
                        let _ = event_tx
                            .send(OrchestraEvent::MusicianOutput {
                                musician_id: self.id.clone(),
                                line: message.clone(),
                            })
                            .await;
                        // Estimate output tokens
                        self.state.token_usage.output += estimate_tokens(message);
                        self.state.tokens_used =
                            self.state.token_usage.input + self.state.token_usage.output;
                    }
                }

                ClaudeEventType::ToolUse => {
                    if let Some(ref tool_name) = event.tool_name {
                        turn_count += 1;
                        let summary =
                            summarize_tool_use(tool_name, event.tool_input.as_ref());
                        self.push_output(&format!("> {summary}"));
                        let _ = event_tx
                            .send(OrchestraEvent::MusicianToolUse {
                                musician_id: self.id.clone(),
                                tool_name: tool_name.clone(),
                                tool_input: event.tool_input.clone(),
                            })
                            .await;

                        // Estimate output tokens from serialized tool input
                        if let Some(ref input) = event.tool_input {
                            if let Ok(serialized) = serde_json::to_string(input) {
                                self.state.token_usage.output += estimate_tokens(&serialized);
                            }
                        }

                        // Track file modifications (Write/Edit tools)
                        if tool_name == "Write" || tool_name == "Edit" {
                            if let Some(ref input) = event.tool_input {
                                if let Some(fp) = input.get("file_path").and_then(|v| v.as_str()) {
                                    files_modified.insert(fp.to_string());
                                }
                            }
                        }

                        // Checkpoint detection: git commit in Bash commands
                        if !read_only
                            && tool_name == "Bash"
                            && event
                                .tool_input
                                .as_ref()
                                .and_then(|v| v.get("command"))
                                .and_then(|v| v.as_str())
                                .map(|cmd| cmd.contains("git commit"))
                                .unwrap_or(false)
                        {
                            self.state.checkpoint = Some(Checkpoint {
                                turn_number: turn_count,
                                files_modified: files_modified.iter().cloned().collect(),
                                timestamp: chrono::Utc::now().to_rfc3339(),
                                token_used: self.state.tokens_used,
                                commit_sha: None,
                            });
                        }
                    }
                }

                ClaudeEventType::ToolResult => {
                    // Estimate input tokens from tool result content
                    if let Some(ref content) = event.tool_result_content {
                        self.state.token_usage.input += estimate_tokens(content);
                        self.state.tokens_used =
                            self.state.token_usage.input + self.state.token_usage.output;
                    }
                }

                ClaudeEventType::Result => {
                    received_result = true;

                    // Track cost
                    if let Some(cost) = event.cost_usd {
                        self.state.cost_usd += cost;
                    }

                    // Calibrate token estimates with actuals
                    if let Some(ref usage) = event.usage {
                        let estimated_total =
                            self.state.token_usage.input + self.state.token_usage.output;
                        let actual_total = usage.input + usage.output;
                        if estimated_total > 0 && actual_total > 0 {
                            calibrate_token_estimate(estimated_total * 4, actual_total);
                        }
                        // Replace estimates with actuals
                        self.state.token_usage = usage.clone();
                        total_tokens = usage.input + usage.output;
                        self.state.tokens_used = total_tokens;
                        self.state.tokens_estimated = false;
                    }

                    if event.is_error == Some(true) {
                        result_was_error = true;
                        // Check for rate limit in result error
                        if let Some(ref result_text) = event.result {
                            if is_rate_limit_message(result_text) {
                                let _ = event_tx
                                    .send(OrchestraEvent::MusicianRateLimit {
                                        musician_id: self.id.clone(),
                                        event: event.clone(),
                                    })
                                    .await;
                            }
                        }
                    }

                    // Close session after receiving result — process stays alive
                    // with stream-json input, must explicitly close
                    if let Some(ref mut s) = self.session {
                        s.close().await;
                    }
                }

                ClaudeEventType::Error => {
                    if event.subtype.as_deref() == Some("rate_limit") {
                        self.state.status = MusicianStatus::Paused;
                        self.send_status_change(&event_tx).await;
                        let _ = event_tx
                            .send(OrchestraEvent::MusicianRateLimit {
                                musician_id: self.id.clone(),
                                event: event.clone(),
                            })
                            .await;

                        // Return partial result — orchestra will handle resume
                        let result = TaskResult {
                            success: false,
                            files_modified: files_modified.into_iter().collect(),
                            summary: "Rate limited — paused for resume".into(),
                            error: Some("rate_limited".into()),
                            tokens_used: total_tokens,
                            duration_ms: start.elapsed().as_millis() as u64,
                            diff: None,
                            verification_output: None,
                            verification_passed: None,
                        };
                        let _ = event_tx
                            .send(OrchestraEvent::MusicianComplete {
                                musician_id: self.id.clone(),
                                result: result.clone(),
                            })
                            .await;
                        return result;
                    }

                    // Non-rate-limit error
                    if let Some(ref msg) = event.message {
                        self.push_output(&format!("ERROR: {msg}"));
                    }
                }

                _ => {}
            }

            // Broadcast status change after every event
            self.send_status_change(&event_tx).await;
        }

        // Session closed — build final result
        let success = received_result && !result_was_error;
        let summary = if last_message.is_empty() {
            if success {
                "Task completed".to_string()
            } else {
                "Session closed unexpectedly".to_string()
            }
        } else {
            last_message
        };

        let result = TaskResult {
            success,
            files_modified: files_modified.into_iter().collect(),
            summary,
            error: if success {
                None
            } else {
                Some("Session closed without successful result".into())
            },
            tokens_used: total_tokens,
            duration_ms: start.elapsed().as_millis() as u64,
            diff: None,
            verification_output: None,
            verification_passed: None,
        };

        self.state.status = if success {
            MusicianStatus::Completed
        } else {
            MusicianStatus::Failed
        };
        self.session = None;

        // Send completion event
        let _ = event_tx
            .send(OrchestraEvent::MusicianComplete {
                musician_id: self.id.clone(),
                result: result.clone(),
            })
            .await;
        self.send_status_change(&event_tx).await;

        result
    }

    // ─── Private helpers ──────────────────────────────────────

    /// Push lines to the output buffer, respecting size limits.
    fn push_output(&mut self, text: &str) {
        let lines: Vec<&str> = text.split('\n').collect();
        self.state.output_lines.extend(lines.iter().map(|l| l.to_string()));

        // Trim by line count
        if self.state.output_lines.len() > OUTPUT_BUFFER_SIZE {
            let excess = self.state.output_lines.len() - OUTPUT_BUFFER_SIZE;
            self.state.output_lines.drain(..excess);
        }

        // Trim by total character count
        let mut total_chars: usize = self.state.output_lines.iter().map(|l| l.len()).sum();
        while total_chars > OUTPUT_BUFFER_MAX_CHARS && self.state.output_lines.len() > 1 {
            total_chars -= self.state.output_lines[0].len();
            self.state.output_lines.remove(0);
        }
    }

    /// Send a status change event to the orchestra.
    async fn send_status_change(&self, event_tx: &mpsc::Sender<OrchestraEvent>) {
        let _ = event_tx
            .send(OrchestraEvent::MusicianStatusChange {
                musician_id: self.id.clone(),
                state: self.state.clone(),
            })
            .await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_task() -> Task {
        Task {
            id: "t1".into(),
            index: 0,
            title: "Test task".into(),
            description: "Do a thing".into(),
            why: "Testing".into(),
            file_scope: vec!["src/main.rs".into()],
            dependencies: vec![],
            acceptance_criteria: vec!["It works".into()],
            estimated_turns: 5,
            model: None,
            status: conductor_types::TaskStatus::Queued,
            assigned_musician: None,
            result: None,
        }
    }

    #[test]
    fn new_musician_is_idle() {
        let m = Musician::new("m1".into(), 0, "sonnet".into(), 50);
        let state = m.get_state();
        assert_eq!(state.id, "m1");
        assert_eq!(state.index, 0);
        assert_eq!(state.status, MusicianStatus::Idle);
        assert!(state.current_task.is_none());
        assert!(state.output_lines.is_empty());
    }

    #[test]
    fn set_model_override() {
        let mut m = Musician::new("m1".into(), 0, "sonnet".into(), 50);
        assert_eq!(m.model, "sonnet");
        m.set_model_override("opus");
        assert_eq!(m.model, "opus");
        assert_eq!(m.original_model, "sonnet");
    }

    #[test]
    fn reset_restores_original_model() {
        let mut m = Musician::new("m1".into(), 0, "sonnet".into(), 50);
        m.set_model_override("opus");
        m.reset();
        assert_eq!(m.model, "sonnet");
        assert_eq!(m.get_state().status, MusicianStatus::Idle);
        assert!(m.get_state().current_task.is_none());
    }

    #[test]
    fn pause_sets_status() {
        let mut m = Musician::new("m1".into(), 0, "sonnet".into(), 50);
        m.pause();
        assert_eq!(m.get_state().status, MusicianStatus::Paused);
    }

    #[test]
    fn set_waiting_with_deps() {
        let mut m = Musician::new("m1".into(), 0, "sonnet".into(), 50);
        let mut task = make_task();
        task.dependencies = vec![1, 3];
        m.set_waiting(&task);
        let state = m.get_state();
        assert_eq!(state.status, MusicianStatus::Waiting);
        assert!(state.output_lines[0].contains("Task 2"));
        assert!(state.output_lines[0].contains("Task 4"));
    }

    #[test]
    fn is_interactive_no_session() {
        let m = Musician::new("m1".into(), 0, "sonnet".into(), 50);
        assert!(!m.is_interactive());
    }

    #[test]
    fn push_output_respects_line_limit() {
        let mut m = Musician::new("m1".into(), 0, "sonnet".into(), 50);
        for i in 0..250 {
            m.push_output(&format!("line {i}"));
        }
        assert_eq!(m.state.output_lines.len(), OUTPUT_BUFFER_SIZE);
        // Should have kept the last 200 lines
        assert!(m.state.output_lines.last().unwrap().contains("249"));
        assert!(m.state.output_lines.first().unwrap().contains("50"));
    }

    #[test]
    fn push_output_respects_char_limit() {
        let mut m = Musician::new("m1".into(), 0, "sonnet".into(), 50);
        // Each line is ~10KB, push 15 lines = 150KB > 100KB limit
        let big_line = "x".repeat(10_000);
        for _ in 0..15 {
            m.push_output(&big_line);
        }
        let total: usize = m.state.output_lines.iter().map(|l| l.len()).sum();
        assert!(total <= OUTPUT_BUFFER_MAX_CHARS);
    }

    #[test]
    fn push_output_splits_multiline() {
        let mut m = Musician::new("m1".into(), 0, "sonnet".into(), 50);
        m.push_output("line1\nline2\nline3");
        assert_eq!(m.state.output_lines.len(), 3);
        assert_eq!(m.state.output_lines[0], "line1");
        assert_eq!(m.state.output_lines[2], "line3");
    }

    #[test]
    fn push_external_output_adds_line() {
        let mut m = Musician::new("m1".into(), 0, "sonnet".into(), 50);
        m.push_external_output("external message");
        assert_eq!(m.state.output_lines.len(), 1);
        assert_eq!(m.state.output_lines[0], "external message");
    }

    #[test]
    fn build_prompt_includes_task_details() {
        let task = make_task();
        let prompt = build_musician_prompt(&task, "/nonexistent/path");
        assert!(prompt.contains("**Test task**"));
        assert!(prompt.contains("Do a thing"));
        assert!(prompt.contains("src/main.rs"));
        assert!(prompt.contains("1. It works"));
        assert!(prompt.contains("Musician agent"));
    }

    #[tokio::test]
    async fn inject_prompt_no_session() {
        let mut m = Musician::new("m1".into(), 0, "sonnet".into(), 50);
        let sent = m.inject_prompt("hello").await;
        assert!(!sent);
    }
}
