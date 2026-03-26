//! Conductor agent — uses a persistent Claude session for all planning phases.
//!
//! Ports `src/conductor.ts` from the TypeScript conductor.
//! The conductor creates a persistent `ClaudeSession` that stays alive across
//! exploration, decomposition, detailing, review, and guidance, so Claude
//! retains full context from codebase exploration through to final review.

use std::collections::HashMap;
use std::path::Path;

use conductor_bridge::{ClaudeSession, ClaudeSessionOptions};
use conductor_types::{
    truncate_str, truncate_str_tail, AnalysisDirective, AnalysisResult, ClaudeEvent,
    ClaudeEventType, CodebaseMap, GuidanceActions, GuidanceInput, OrchestraEvent, Phase,
    PhaseReviewAction, PhaseReviewResult, Plan, PlanInsight, Task, TaskStatus,
};
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::tool_summary::summarize_tool_use;
use crate::CoreError;

// ─── Result types ───────────────────────────────────────────────

/// Result of decomposing into phases.
pub struct DecomposeResult {
    pub phases: Vec<Phase>,
    pub summary: String,
    pub learning_notes: Vec<String>,
    pub insights: Option<Vec<PlanInsight>>,
    pub estimated_minutes: u32,
}

/// Result from the legacy flat review.
pub struct ReviewResult {
    pub action: String,
    pub summary: Option<String>,
    pub task_indices: Option<Vec<usize>>,
    pub new_tasks: Option<Vec<serde_json::Value>>,
    pub instructions: Option<String>,
    pub adjusted_instructions: Option<String>,
    pub insights: Option<Vec<PlanInsight>>,
}

/// Input for the legacy flat review.
pub struct ReviewInput {
    pub task_results: String,
    pub shared_memory: String,
    pub diffs: Option<HashMap<usize, String>>,
    pub verification_results: Option<HashMap<usize, String>>,
}

// ─── ConductorAgent ─────────────────────────────────────────────

pub struct ConductorAgent {
    model: String,
    cwd: String,
    session: Option<ClaudeSession>,
    event_rx: Option<mpsc::Receiver<ClaudeEvent>>,
    /// All prompts sent to the conductor, in order.
    pub prompts_sent: Vec<String>,
    /// Optional event channel for streaming output to orchestra.
    event_tx: Option<mpsc::Sender<OrchestraEvent>>,
    /// Channel for receiving user guidance during operations.
    guidance_rx: Option<mpsc::Receiver<String>>,
}

impl ConductorAgent {
    pub fn new(model: String, cwd: String) -> Self {
        Self {
            model,
            cwd,
            session: None,
            event_rx: None,
            prompts_sent: Vec::new(),
            event_tx: None,
            guidance_rx: None,
        }
    }

    /// Set the event channel for streaming output to orchestra.
    pub fn set_event_tx(&mut self, tx: mpsc::Sender<OrchestraEvent>) {
        self.event_tx = Some(tx);
    }

    /// Set the guidance channel for receiving user messages during operations.
    pub fn set_guidance_rx(&mut self, rx: mpsc::Receiver<String>) {
        self.guidance_rx = Some(rx);
    }

    /// Returns whether the session is alive.
    pub fn has_session(&self) -> bool {
        self.session
            .as_ref()
            .is_some_and(|s| !s.is_closed())
    }

    /// Inject a user guidance message into the running conductor session.
    pub async fn inject_message(&mut self, text: &str) -> Result<(), CoreError> {
        if let Some(ref mut session) = self.session {
            if !session.is_closed() {
                session
                    .send_message(text)
                    .await
                    .map_err(|e| CoreError::Channel(format!("conductor inject failed: {e}")))?;
            }
        }
        Ok(())
    }

    // ─── V1: Single-pass planning ────────────────────────────────

    /// Explore the project and create a plan in one pass.
    pub async fn plan(
        &mut self,
        project_path: &str,
        task_description: &str,
        reference_session_context: Option<&str>,
    ) -> Result<Plan, CoreError> {
        let prompt = plan_prompt(project_path, task_description, reference_session_context);

        self.create_session(ClaudeSessionOptions {
            model: self.model.clone(),
            max_turns: Some(30),
            allowed_tools: Some(vec![
                "Read".into(),
                "Glob".into(),
                "Grep".into(),
                "Bash(git status)".into(),
                "Bash(git log)".into(),
            ]),
            append_system_prompt: None,
            cwd: Some(self.cwd.clone()),
        });

        let output = self.send_and_collect(&prompt).await?;
        parse_plan_from_output(&output)
    }

    // ─── V2: Three-pass planning ─────────────────────────────────

    /// Phase 1: Explore the codebase.
    pub async fn explore(
        &mut self,
        project_path: &str,
        task_description: &str,
        reference_session_context: Option<&str>,
    ) -> Result<CodebaseMap, CoreError> {
        let prompt = explore_prompt(project_path, task_description, reference_session_context);

        self.create_session(ClaudeSessionOptions {
            model: self.model.clone(),
            max_turns: Some(20),
            allowed_tools: Some(vec![
                "Read".into(),
                "Glob".into(),
                "Grep".into(),
                "Bash(git status)".into(),
                "Bash(git log)".into(),
            ]),
            append_system_prompt: None,
            cwd: Some(self.cwd.clone()),
        });

        let output = self.send_and_collect(&prompt).await?;
        let mut parsed: CodebaseMap = parse_json_from_output(&output)?;
        // Auto-generate IDs for directives that the LLM didn't provide
        if let Some(ref mut directives) = parsed.analysis_directives {
            for (i, d) in directives.iter_mut().enumerate() {
                if d.id.is_empty() {
                    d.id = format!("analyst-{}", i + 1);
                }
            }
        }
        Ok(parsed)
    }

    /// Phase 2: Decompose into phases.
    pub async fn decompose_phases(
        &mut self,
        task_description: &str,
    ) -> Result<DecomposeResult, CoreError> {
        self.ensure_session()?;
        let prompt = decompose_prompt(task_description);
        let output = self.send_and_collect(&prompt).await?;
        parse_decompose_result(&output)
    }

    /// Phase 2b: Decompose with analysis results.
    pub async fn decompose_with_analysis(
        &mut self,
        task_description: &str,
        analysis_results: &[AnalysisResult],
    ) -> Result<DecomposeResult, CoreError> {
        self.ensure_session()?;
        let prompt = decompose_with_analysis_prompt(task_description, analysis_results);
        let output = self.send_and_collect(&prompt).await?;
        parse_decompose_result(&output)
    }

    /// Retry decomposition in the same session by nudging the LLM to produce valid JSON.
    pub async fn retry_decompose(&mut self) -> Result<DecomposeResult, CoreError> {
        self.ensure_session()?;
        let prompt = "Your previous response could not be parsed as JSON. \
            Please respond ONLY with the JSON plan object as specified in the earlier instructions. \
            Use ```json fences. Do not include any other text outside the JSON block.";
        let output = self.send_and_collect(prompt).await?;
        parse_decompose_result(&output)
    }

    /// Phase 3: Detail a specific phase's tasks.
    pub async fn detail_phase(
        &mut self,
        phase: &Phase,
        completed_phases: &[Phase],
    ) -> Result<Vec<Task>, CoreError> {
        // Re-create session if closed (e.g. after long phase execution)
        if !self.has_session() {
            self.create_session(ClaudeSessionOptions {
                model: self.model.clone(),
                max_turns: Some(10),
                allowed_tools: Some(vec!["Read".into(), "Glob".into(), "Grep".into()]),
                append_system_prompt: None,
                cwd: Some(self.cwd.clone()),
            });
        }
        let prompt = detail_phase_prompt(phase, completed_phases);
        let output = self.send_and_collect(&prompt).await?;
        let parsed: serde_json::Value = parse_json_value_from_output(&output)?;
        let raw_tasks = parsed
            .get("tasks")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        Ok(parse_tasks(&raw_tasks))
    }

    /// Retry phase detailing in the same session by nudging the LLM to produce valid JSON.
    pub async fn retry_detail_phase(&mut self) -> Result<Vec<Task>, CoreError> {
        // Re-create session if closed
        if !self.has_session() {
            self.create_session(ClaudeSessionOptions {
                model: self.model.clone(),
                max_turns: Some(10),
                allowed_tools: Some(vec!["Read".into(), "Glob".into(), "Grep".into()]),
                append_system_prompt: None,
                cwd: Some(self.cwd.clone()),
            });
        }
        let prompt = "Your previous response was cut off or contained invalid JSON. \
            Please respond ONLY with the JSON object containing a \"tasks\" array as specified \
            in the earlier instructions. Use ```json fences. Do not include any other text outside the JSON block.";
        let output = self.send_and_collect(prompt).await?;
        let parsed: serde_json::Value = parse_json_value_from_output(&output)?;
        let raw_tasks = parsed
            .get("tasks")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        Ok(parse_tasks(&raw_tasks))
    }

    /// Review a completed phase — decide next action.
    pub async fn review_phase(
        &mut self,
        phase: &Phase,
        all_phases: &[Phase],
        diffs: &HashMap<usize, String>,
        retry_attempt: u32,
    ) -> Result<PhaseReviewResult, CoreError> {
        // Re-create session if closed
        if !self.has_session() {
            self.create_session(ClaudeSessionOptions {
                model: self.model.clone(),
                max_turns: Some(10),
                allowed_tools: Some(vec!["Read".into(), "Glob".into(), "Grep".into()]),
                append_system_prompt: None,
                cwd: Some(self.cwd.clone()),
            });
        }

        let prompt = phase_review_prompt(phase, all_phases, diffs, retry_attempt);
        let output = self.send_and_collect(&prompt).await?;
        let parsed: serde_json::Value = parse_json_value_from_output(&output)?;
        let mut result = validate_phase_review_result(&parsed);

        // Parse revised phases into full Phase objects if provided
        if result.action == PhaseReviewAction::ReviseRemainingPhases {
            if let Some(serde_json::Value::Array(arr)) = parsed.get("revisedPhases") {
                result.revised_phases = Some(
                    arr.iter()
                        .enumerate()
                        .map(|(i, p)| Phase {
                            id: Uuid::new_v4().to_string(),
                            index: i,
                            title: p
                                .get("title")
                                .and_then(|v| v.as_str())
                                .unwrap_or(&format!("Phase {}", i + 1))
                                .to_string(),
                            description: p
                                .get("description")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string(),
                            status: conductor_types::PhaseStatus::Pending,
                            tasks: Vec::new(),
                            review_result: None,
                            })
                        .collect(),
                );
            }
        }

        Ok(result)
    }

    /// Chat-based plan refinement — sends feedback to the same session.
    /// Optionally includes image attachments (drag-and-drop screenshots, mockups, etc.).
    pub async fn refine_plan(
        &mut self,
        feedback: &str,
        images: Option<&[String]>,
    ) -> Result<(Plan, String), CoreError> {
        self.ensure_session()?;
        let message = format!("{REFINE_PROMPT}\n\nUser feedback: {feedback}");
        let full_output = self
            .send_and_collect_with_images(&message, images)
            .await?;

        let json_start = full_output.find("```json");
        let explanation = match json_start {
            Some(pos) if pos > 0 => full_output[..pos].trim().to_string(),
            _ => String::new(),
        };

        let plan = parse_plan_from_output(&full_output)?;
        Ok((plan, explanation))
    }

    /// Send a direct chat message to the conductor session.
    pub async fn chat(
        &mut self,
        message: &str,
    ) -> Result<String, CoreError> {
        self.ensure_session()?;
        self.send_and_collect(message).await
    }

    /// Legacy flat review of completed work.
    pub async fn review(
        &mut self,
        input: &ReviewInput,
    ) -> Result<ReviewResult, CoreError> {
        let prompt = review_prompt(input);

        if !self.has_session() {
            self.create_session(ClaudeSessionOptions {
                model: self.model.clone(),
                max_turns: Some(10),
                allowed_tools: Some(vec!["Read".into(), "Glob".into(), "Grep".into()]),
                append_system_prompt: None,
                cwd: Some(self.cwd.clone()),
            });
        }

        let output = self.send_and_collect(&prompt).await?;
        let parsed: serde_json::Value = parse_json_value_from_output(&output)?;

        Ok(ReviewResult {
            action: parsed
                .get("action")
                .and_then(|v| v.as_str())
                .unwrap_or("complete")
                .to_string(),
            summary: parsed
                .get("summary")
                .and_then(|v| v.as_str())
                .map(String::from),
            task_indices: parsed.get("taskIndices").and_then(|v| {
                v.as_array().map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_u64().map(|n| n as usize))
                        .collect()
                })
            }),
            new_tasks: parsed
                .get("newTasks")
                .and_then(|v| v.as_array())
                .cloned(),
            instructions: parsed
                .get("instructions")
                .and_then(|v| v.as_str())
                .map(String::from),
            adjusted_instructions: parsed
                .get("adjustedInstructions")
                .and_then(|v| v.as_str())
                .map(String::from),
            insights: parsed.get("insights").and_then(|v| {
                serde_json::from_value::<Vec<PlanInsight>>(v.clone()).ok()
            }),
        })
    }

    /// Mid-execution guidance review.
    pub async fn review_guidance(
        &mut self,
        input: &GuidanceInput,
    ) -> Result<GuidanceActions, CoreError> {
        let prompt = guidance_prompt(input);

        if !self.has_session() {
            self.create_session(ClaudeSessionOptions {
                model: self.model.clone(),
                max_turns: Some(3),
                allowed_tools: Some(Vec::new()),
                append_system_prompt: None,
                cwd: Some(self.cwd.clone()),
            });
        }

        let output = self.send_and_collect(&prompt).await?;
        let parsed: GuidanceActions = parse_json_from_output(&output)?;
        Ok(parsed)
    }

    /// Close the persistent session.
    pub async fn close(&mut self) {
        if let Some(ref mut session) = self.session {
            session.close().await;
        }
        self.session = None;
        self.event_rx = None;
    }

    // ─── Internal ────────────────────────────────────────────────


    /// Send output to the orchestra event channel.
    fn emit(event_tx: &Option<mpsc::Sender<OrchestraEvent>>, line: &str) {
        if let Some(tx) = event_tx {
            let _ = tx.try_send(OrchestraEvent::ConductorOutput(line.to_string()));
        }
    }

    fn ensure_session(&self) -> Result<(), CoreError> {
        if !self.has_session() {
            return Err(CoreError::Bridge(
                "Conductor session is closed — it may have crashed or timed out".into(),
            ));
        }
        Ok(())
    }

    fn create_session(&mut self, options: ClaudeSessionOptions) {
        let session = ClaudeSession::new(options);
        self.session = Some(session);
        self.event_rx = None; // will be created in send_and_collect on first call
    }

    /// Send a message to the session and collect all output until the turn completes.
    async fn send_and_collect(
        &mut self,
        message: &str,
    ) -> Result<String, CoreError> {
        self.send_and_collect_with_images(message, None)
            .await
    }

    /// Send a message with optional image attachments and collect output.
    async fn send_and_collect_with_images(
        &mut self,
        message: &str,
        images: Option<&[String]>,
    ) -> Result<String, CoreError> {
        self.prompts_sent.push(message.to_string());

        let session = self
            .session
            .as_mut()
            .ok_or_else(|| CoreError::Bridge("No session".into()))?;

        let is_new = self.event_rx.is_none();

        if is_new {
            // First message: start the session and get the event channel
            let (event_tx, event_rx) = mpsc::channel::<ClaudeEvent>(256);
            session
                .start(message, event_tx)
                .await
                .map_err(|e| CoreError::Bridge(e.to_string()))?;
            self.event_rx = Some(event_rx);
        } else {
            // Follow-up message (with optional images)
            session
                .send_message_with_images(message, images)
                .await
                .map_err(|e| CoreError::Bridge(e.to_string()))?;
        }

        // Collect events until a Result event signals turn completion.
        // We take guidance_rx out so we can borrow it alongside event_rx and session.
        let mut guidance_rx = self.guidance_rx.take();
        let event_rx = self.event_rx.as_mut().unwrap();
        let event_tx = self.event_tx.clone();
        let mut full_output = String::new();
        let mut in_json_block = false;

        let timeout = tokio::time::Duration::from_secs(300); // 5 minutes
        let deadline = tokio::time::Instant::now() + timeout;

        loop {
            // Helper future for guidance — resolves to Some(msg) if guidance_rx is set
            let guidance_fut = async {
                match guidance_rx.as_mut() {
                    Some(rx) => rx.recv().await,
                    None => std::future::pending().await,
                }
            };

            let event = tokio::select! {
                ev = event_rx.recv() => {
                    match ev {
                        Some(e) => e,
                        None => {
                            // Channel closed
                            if !full_output.is_empty() {
                                break;
                            }
                            self.guidance_rx = guidance_rx;
                            return Err(CoreError::Channel("Session closed before turn completed".into()));
                        }
                    }
                }
                Some(msg) = guidance_fut => {
                    // Inject user guidance into the running session.
                    // The message is queued and delivered when Claude finishes its current turn.
                    if let Some(ref mut session) = self.session {
                        if !session.is_closed() {
                            let _ = session.send_message(&msg).await;
                            Self::emit(&event_tx, &format!("[USER GUIDANCE] {msg}"));
                        }
                    }
                    continue;
                }
                _ = tokio::time::sleep_until(deadline) => {
                    if !full_output.is_empty() {
                        tracing::warn!("send_and_collect timed out, returning partial output");
                        break;
                    }
                    self.guidance_rx = guidance_rx;
                    return Err(CoreError::Timeout("send_and_collect timed out after 300s".into()));
                }
            };

            match event.event_type {
                ClaudeEventType::Assistant => {
                    if event.subtype.as_deref() == Some("thinking") {
                        Self::emit(&event_tx, "Thinking...");
                    } else if let Some(ref msg) = event.message {
                        full_output.push_str(msg);
                        for line in msg.lines() {
                            let trimmed = line.trim();
                            if trimmed.is_empty() {
                                continue;
                            }
                            // Suppress JSON block lines from TUI display
                            if trimmed == "```json" || trimmed.starts_with("```json") {
                                in_json_block = true;
                                continue;
                            }
                            if trimmed == "```" {
                                if in_json_block {
                                    in_json_block = false;
                                }
                                continue;
                            }
                            if in_json_block {
                                continue;
                            }
                            Self::emit(&event_tx, trimmed);
                        }
                    }
                }
                ClaudeEventType::ToolUse => {
                    let summary = summarize_tool_use(
                        event.tool_name.as_deref().unwrap_or(""),
                        event.tool_input.as_ref(),
                    );
                    Self::emit(&event_tx, &format!("> {summary}"));
                }
                ClaudeEventType::ToolResult => {}
                ClaudeEventType::Result => {
                    if let Some(ref result_text) = event.result {
                        full_output.push_str(result_text);
                    }
                    break;
                }
                ClaudeEventType::Error => {
                    Self::emit(
                        &event_tx,
                        &format!(
                            "Error: {}",
                            event.message.as_deref().unwrap_or("Unknown error")
                        ),
                    );
                }
                _ => {}
            }
        }

        // Restore guidance channel
        self.guidance_rx = guidance_rx;

        Ok(full_output)
    }
}

// ─── Project Instructions ───────────────────────────────────────

/// Read CLAUDE.md from a project directory with intelligent truncation.
pub fn load_project_instructions(project_path: &str, max_chars: usize) -> String {
    let base = Path::new(project_path);
    let candidates = [
        base.join("CLAUDE.md"),
        base.join("claude.md"),
        base.join(".claude").join("instructions.md"),
    ];

    for path in &candidates {
        if let Ok(content) = std::fs::read_to_string(path) {
            if content.len() <= max_chars {
                return content;
            }
            return intelligent_truncate(&content, max_chars);
        }
    }
    String::new()
}

/// Intelligently truncate markdown preserving structure.
fn intelligent_truncate(content: &str, max_chars: usize) -> String {
    let sections: Vec<&str> = split_markdown_sections(content);
    if sections.len() <= 2 {
        let mut result = content[..max_chars.min(content.len())].to_string();
        result.push_str("\n\n[...truncated — full file available in project root]");
        return result;
    }

    let first = sections[0];
    let last = sections[sections.len() - 1];
    let middle = &sections[1..sections.len() - 1];

    let mut result = first.to_string();
    let mut remaining = max_chars
        .saturating_sub(first.len())
        .saturating_sub(last.len())
        .saturating_sub(100);

    for section in middle {
        if section.len() <= remaining {
            result.push_str(section);
            remaining -= section.len();
        } else {
            // Extract heading and first paragraph only
            if let Some(heading_end) = section.find('\n') {
                let heading = &section[..heading_end];
                let rest = &section[heading_end + 1..];
                let first_para = rest.split("\n\n").next().unwrap_or("");
                let truncated_para = truncate_str(first_para, 200);
                let summary =
                    format!("{heading}\n{truncated_para}...\n[...section truncated]\n\n");
                if summary.len() <= remaining {
                    result.push_str(&summary);
                    remaining -= summary.len();
                } else {
                    result.push_str(&format!("{heading}\n[...section truncated]\n\n"));
                }
            }
        }
    }

    result.push_str(last);
    result
}

/// Split markdown content at heading boundaries (lines starting with #).
fn split_markdown_sections(content: &str) -> Vec<&str> {
    let mut sections = Vec::new();
    let mut last = 0;

    for (i, _) in content.match_indices("\n#") {
        // Only split at lines that start with 1-3 #'s followed by a space
        let after = &content[i + 1..];
        let hash_count = after.chars().take_while(|&c| c == '#').count();
        if hash_count >= 1
            && hash_count <= 3
            && after.chars().nth(hash_count) == Some(' ')
        {
            if i > last {
                sections.push(&content[last..i + 1]); // include the newline
            }
            last = i + 1;
        }
    }
    if last < content.len() {
        sections.push(&content[last..]);
    }
    if sections.is_empty() {
        sections.push(content);
    }
    sections
}

// ─── JSON Utilities ─────────────────────────────────────────────

/// Extract a JSON block from LLM output.
/// First tries ```json fences, then falls back to brace scanning.
pub fn extract_json_block(text: &str) -> Option<&str> {
    // Try ```json ... ``` first, then plain ``` ... ```
    let fence_start = text.find("```json").map(|s| (s, 7)).or_else(|| {
        // Find a plain ``` fence whose content starts with { or [
        let mut search_from = 0;
        loop {
            let pos = text[search_from..].find("```")?;
            let abs_pos = search_from + pos;
            let after = &text[abs_pos + 3..];
            let trimmed = after.trim_start();
            if trimmed.starts_with('{') || trimmed.starts_with('[') {
                return Some((abs_pos, 3));
            }
            search_from = abs_pos + 3;
        }
    });
    if let Some((start, skip)) = fence_start {
        let json_start = start + skip;
        // Skip optional whitespace/newline after fence
        let json_start = text[json_start..]
            .find(|c: char| !c.is_whitespace() || c == '{' || c == '[')
            .map(|i| json_start + i)
            .unwrap_or(json_start);
        if let Some(end) = text[json_start..].find("```") {
            let block = text[json_start..json_start + end].trim();
            if !block.is_empty() {
                return Some(block);
            }
        }
    }

    // Fallback: string-aware reverse brace scanner
    let bytes = text.as_bytes();
    let mut depth: i32 = 0;
    let mut end: Option<usize> = None;
    let mut in_string = false;

    for i in (0..bytes.len()).rev() {
        let ch = bytes[i];

        // Handle string boundaries (unescaped quotes)
        if ch == b'"' && (i == 0 || bytes[i - 1] != b'\\') {
            in_string = !in_string;
            continue;
        }

        if in_string {
            continue;
        }

        if ch == b'}' {
            if end.is_none() {
                end = Some(i);
            }
            depth += 1;
        } else if ch == b'{' {
            depth -= 1;
        }

        if depth == 0 {
            if let Some(e) = end {
                return Some(&text[i..=e]);
            }
        }
    }

    None
}

/// Strip common LLM JSON artifacts.
pub fn sanitize_json(raw: &str) -> String {
    let mut cleaned = String::with_capacity(raw.len());

    // Strip line comments (// ...) that are outside of strings
    let mut in_string = false;
    let chars: Vec<char> = raw.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let ch = chars[i];

        if ch == '"' && (i == 0 || chars[i - 1] != '\\') {
            in_string = !in_string;
            cleaned.push(ch);
            i += 1;
            continue;
        }

        if !in_string && ch == '/' && i + 1 < chars.len() && chars[i + 1] == '/' {
            // Skip to end of line
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
            continue;
        }

        // Strip BOM/zero-width chars
        if ch == '\u{FEFF}' || ('\u{200B}'..='\u{200D}').contains(&ch) {
            i += 1;
            continue;
        }

        cleaned.push(ch);
        i += 1;
    }

    // Strip trailing commas before } or ]
    let mut result = String::with_capacity(cleaned.len());
    let cleaned_chars: Vec<char> = cleaned.chars().collect();
    let mut j = 0;
    while j < cleaned_chars.len() {
        if cleaned_chars[j] == ',' {
            // Look ahead past whitespace for } or ]
            let mut k = j + 1;
            while k < cleaned_chars.len() && cleaned_chars[k].is_whitespace() {
                k += 1;
            }
            if k < cleaned_chars.len() && (cleaned_chars[k] == '}' || cleaned_chars[k] == ']') {
                // Skip the trailing comma
                j += 1;
                continue;
            }
        }
        result.push(cleaned_chars[j]);
        j += 1;
    }

    result.trim().to_string()
}

/// Parse typed JSON from LLM output, extracting the JSON block first.
fn parse_json_from_output<T: serde::de::DeserializeOwned>(output: &str) -> Result<T, CoreError> {
    let block = extract_json_block(output).ok_or_else(|| CoreError::JsonParse {
        reason: "No JSON block found in LLM output".into(),
        raw_output: output.to_string(),
    })?;
    let sanitized = sanitize_json(block);
    serde_json::from_str(&sanitized).map_err(|e| CoreError::JsonParse {
        reason: format!("Invalid JSON: {e}"),
        raw_output: output.to_string(),
    })
}

/// Parse a serde_json::Value from LLM output.
fn parse_json_value_from_output(output: &str) -> Result<serde_json::Value, CoreError> {
    parse_json_from_output(output)
}

/// Parse raw task objects into typed Task vec.
pub fn parse_tasks(raw_tasks: &[serde_json::Value]) -> Vec<Task> {
    let mut tasks: Vec<Task> = raw_tasks
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let title = t
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or(&format!("Task {}", i + 1))
                .to_string();
            let description = t
                .get("description")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let why = t
                .get("why")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let file_scope: Vec<String> = t
                .get("fileScope")
                .and_then(|v| v.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .filter(|s| !s.is_empty())
                        .collect()
                })
                .unwrap_or_default();
            let acceptance_criteria: Vec<String> = t
                .get("acceptanceCriteria")
                .and_then(|v| v.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .filter(|s| !s.is_empty())
                        .collect()
                })
                .unwrap_or_default();
            let dependencies: Vec<usize> = t
                .get("dependencies")
                .and_then(|v| v.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_u64().map(|n| n as usize))
                        .collect()
                })
                .unwrap_or_default();
            let estimated_turns = t
                .get("estimatedTurns")
                .and_then(|v| v.as_u64())
                .unwrap_or(15) as u32;
            let model = t
                .get("model")
                .and_then(|v| v.as_str())
                .map(String::from);

            // Detect truncated tasks
            let is_truncated =
                !description.is_empty() && file_scope.is_empty() && why.is_empty() && acceptance_criteria.is_empty();

            let final_description = if is_truncated {
                format!("{description}\n\n[NOTE: This task may have been truncated during planning. Infer file scope and acceptance criteria from the description.]")
            } else {
                description
            };

            let final_why = if why.is_empty() && is_truncated {
                format!("Part of task: {title}")
            } else {
                why
            };

            let final_criteria = if acceptance_criteria.is_empty() && is_truncated {
                vec![
                    format!("Complete the task described in \"{title}\""),
                    "cargo check passes".to_string(),
                ]
            } else {
                acceptance_criteria
            };

            Task {
                id: Uuid::new_v4().to_string(),
                index: i,
                title,
                description: final_description,
                why: final_why,
                file_scope,
                dependencies,
                acceptance_criteria: final_criteria,
                estimated_turns,
                model,
                status: TaskStatus::Queued,
                assigned_musician: None,
                result: None,
            }
        })
        .collect();

    // Mark tasks with no dependencies as Ready
    for task in &mut tasks {
        if task.dependencies.is_empty() {
            task.status = TaskStatus::Ready;
        }
    }

    tasks
}

/// Parse a Plan from LLM output.
fn parse_plan_from_output(output: &str) -> Result<Plan, CoreError> {
    let parsed: serde_json::Value = parse_json_value_from_output(output)?;

    let raw_tasks = parsed
        .get("tasks")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let tasks = parse_tasks(&raw_tasks);

    let insights: Option<Vec<PlanInsight>> = parsed
        .get("insights")
        .and_then(|v| serde_json::from_value(v.clone()).ok());

    Ok(Plan {
        summary: parsed
            .get("summary")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        tasks,
        dependency_graph: parsed
            .get("dependencyGraph")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        musician_assignment: parsed
            .get("musicianAssignment")
            .or_else(|| parsed.get("workerAssignment"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        learning_notes: parsed
            .get("learningNotes")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default(),
        estimated_minutes: parsed
            .get("estimatedMinutes")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32,
        insights,
    })
}

/// Parse a DecomposeResult from LLM output.
fn parse_decompose_result(output: &str) -> Result<DecomposeResult, CoreError> {
    let parsed: serde_json::Value = parse_json_value_from_output(output)?;

    let phases: Vec<Phase> = parsed
        .get("phases")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .enumerate()
                .map(|(i, p)| {
                    let raw_tasks = p
                        .get("tasks")
                        .and_then(|v| v.as_array())
                        .cloned()
                        .unwrap_or_default();
                    Phase {
                        id: Uuid::new_v4().to_string(),
                        index: i,
                        title: p
                            .get("title")
                            .and_then(|v| v.as_str())
                            .unwrap_or(&format!("Phase {}", i + 1))
                            .to_string(),
                        description: p
                            .get("description")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string(),
                        status: conductor_types::PhaseStatus::Pending,
                        tasks: parse_tasks(&raw_tasks),
                        review_result: None,
                    }
                })
                .collect()
        })
        .unwrap_or_default();

    let insights: Option<Vec<PlanInsight>> = parsed
        .get("insights")
        .and_then(|v| serde_json::from_value(v.clone()).ok());

    Ok(DecomposeResult {
        phases,
        summary: parsed
            .get("summary")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        learning_notes: parsed
            .get("learningNotes")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default(),
        insights,
        estimated_minutes: parsed
            .get("estimatedMinutes")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32,
    })
}

/// Validate a PhaseReviewResult at runtime.
pub fn validate_phase_review_result(raw: &serde_json::Value) -> PhaseReviewResult {
    let action_str = raw
        .get("action")
        .and_then(|v| v.as_str())
        .unwrap_or("continue");

    let action = match action_str {
        "continue" => PhaseReviewAction::Continue,
        "retry_tasks" => PhaseReviewAction::RetryTasks,
        "revise_remaining_phases" => PhaseReviewAction::ReviseRemainingPhases,
        "abort" => PhaseReviewAction::Abort,
        _ => {
            return PhaseReviewResult {
                action: PhaseReviewAction::Continue,
                task_indices: None,
                revised_phases: None,
                summary: raw
                    .get("summary")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Invalid action — defaulting to continue")
                    .to_string(),
            };
        }
    };

    let summary = raw
        .get("summary")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    if action == PhaseReviewAction::RetryTasks {
        let indices: Option<Vec<usize>> = raw.get("taskIndices").and_then(|v| {
            v.as_array().map(|a| {
                a.iter()
                    .filter_map(|v| v.as_u64().map(|n| n as usize))
                    .collect()
            })
        });
        if indices.as_ref().is_none_or(|v| v.is_empty()) {
            return PhaseReviewResult {
                action: PhaseReviewAction::Continue,
                task_indices: None,
                revised_phases: None,
                summary: format!("{summary} (retry_tasks lacked taskIndices — continuing instead)"),
            };
        }
        return PhaseReviewResult {
            action,
            task_indices: indices,
            revised_phases: None,
            summary,
        };
    }

    if action == PhaseReviewAction::ReviseRemainingPhases {
        let has_revised = raw
            .get("revisedPhases")
            .and_then(|v| v.as_array())
            .is_some_and(|a| !a.is_empty());
        if !has_revised {
            return PhaseReviewResult {
                action: PhaseReviewAction::Continue,
                task_indices: None,
                revised_phases: None,
                summary: format!(
                    "{summary} (revise_remaining_phases lacked revisedPhases — continuing instead)"
                ),
            };
        }
    }

    PhaseReviewResult {
        action,
        task_indices: None,
        revised_phases: None,
        summary,
    }
}

// ─── Prompt Templates ───────────────────────────────────────────

fn plan_prompt(
    project_path: &str,
    task_description: &str,
    reference_session_context: Option<&str>,
) -> String {
    let project_instructions = load_project_instructions(project_path, 16000);
    let conventions_block = if project_instructions.is_empty() {
        String::new()
    } else {
        format!(
            "## Project Conventions & Instructions\n\
             The following is from the project's CLAUDE.md — musicians MUST follow these conventions:\n\n\
             {project_instructions}\n\n---\n\n"
        )
    };
    let reference_block = match reference_session_context {
        Some(ctx) => format!(
            "## Reference Session (previous work to build upon)\n\
             The user wants you to analyse and build upon work from a previous session. \
             Use this context to understand what was already done, what succeeded, what failed, \
             and what can be improved.\n\n{ctx}\n\n---\n\n"
        ),
        None => String::new(),
    };

    format!(
        "You are the Conductor agent in the Orchestra orchestration system. \
         Your role is to analyze a codebase and create a detailed execution plan.\n\n\
         PROJECT: {project_path}\n\
         TASK: {task_description}\n\n\
         {conventions_block}{reference_block}\
         ## Instructions\n\n\
         1. Explore the project structure to understand the codebase\n\
         2. Identify the files and modules relevant to the task\n\
         3. Create a plan that breaks the task into independent subtasks\n\
         4. Assign a model tier to each task based on complexity:\n\
            - \"opus\" for complex architectural tasks, refactors, or tasks requiring deep reasoning\n\
            - \"sonnet\" (default) for standard implementation tasks\n\
            - \"haiku\" for simple/mechanical tasks (renames, adding imports, boilerplate)\n\n\
         ## Output Format\n\n\
         You MUST output your plan as a JSON code block at the END of your response. Format:\n\n\
         ```json\n\
         {{\n\
           \"summary\": \"Brief description of the overall plan\",\n\
           \"tasks\": [\n\
             {{\n\
               \"title\": \"Short task title\",\n\
               \"description\": \"Detailed instructions for the musician\",\n\
               \"why\": \"Explanation of WHY this task is needed and what problem it solves\",\n\
               \"fileScope\": [\"src/path/to/file.ts\"],\n\
               \"dependencies\": [],\n\
               \"acceptanceCriteria\": [\"Criteria 1\", \"Criteria 2\"],\n\
               \"estimatedTurns\": 15,\n\
               \"model\": \"sonnet\"\n\
             }}\n\
           ],\n\
           \"dependencyGraph\": \"1 --> 2 --> 3\\n1 --> 4 --> 3\",\n\
           \"musicianAssignment\": \"M1: Tasks 1, 3\\nM2: Tasks 2\\nM3: Task 4\",\n\
           \"learningNotes\": [\n\
             \"Brief architectural insight about the codebase\"\n\
           ],\n\
           \"insights\": [\n\
             {{\n\
               \"category\": \"architecture|pattern|concept|decision\",\n\
               \"title\": \"Short title (max 50 chars)\",\n\
               \"body\": \"2-3 sentence educational explanation.\"\n\
             }}\n\
           ],\n\
           \"estimatedTokens\": 350000,\n\
           \"estimatedMinutes\": 45\n\
         }}\n\
         ```\n\n\
         ## Educational Insights\n\
         Generate 3-5 educational insights about the codebase and the approach you're taking.\n\
         Focus on things the user would learn by watching an expert work:\n\
         - WHY certain architectural decisions were made (not just what they are)\n\
         - HOW different parts of the system connect and depend on each other\n\
         - WHAT trade-offs were considered and why this approach was chosen\n\
         - PATTERNS that are reusable knowledge beyond this specific task\n\n\
         Make insights specific to THIS codebase — reference actual files, types, and conventions.\n\
         Avoid generic programming advice. Teach the user something they'd only learn by deeply reading the code.\n\n\
         CRITICAL JSON RULES:\n\
         - Output ONLY valid JSON inside the ```json fences — no comments, no trailing commas\n\
         - Use double quotes for all keys and string values (standard JSON, not JavaScript)\n\
         - Ensure the JSON block is COMPLETE — do not truncate\n\
         - The JSON block must be the LAST thing in your response\n\n\
         IMPORTANT:\n\
         - dependencies is an array of task INDICES (0-based) that must complete before this task starts\n\
         - fileScope should minimize overlap between tasks to prevent merge conflicts\n\
         - Each task's \"why\" should explain the reasoning, not just restate the task\n\
         - learningNotes should contain project-specific insights, not generic advice\n\
         - Keep tasks focused — a musician should be able to complete one in 10-30 turns\n\
         - model should reflect task complexity: \"opus\" for hard tasks, \"haiku\" for trivial ones, \"sonnet\" for everything else"
    )
}

fn explore_prompt(
    project_path: &str,
    task_description: &str,
    reference_session_context: Option<&str>,
) -> String {
    let project_instructions = load_project_instructions(project_path, 16000);
    let conventions_block = if project_instructions.is_empty() {
        String::new()
    } else {
        format!(
            "## Project Conventions & Instructions\n{project_instructions}\n\n---\n\n"
        )
    };
    let reference_block = match reference_session_context {
        Some(ctx) => format!("## Reference Session\n{ctx}\n\n---\n\n"),
        None => String::new(),
    };

    format!(
        "You are the Conductor agent in the Orchestra orchestration system (V2). \
         Your role is to deeply explore and understand a codebase before planning.\n\n\
         PROJECT: {project_path}\n\
         TASK: {task_description}\n\n\
         {conventions_block}{reference_block}\
         ## Phase 1: Exploration\n\n\
         Thoroughly explore the project to build a mental map. You have 20 turns with read-only tools.\n\n\
         Focus on:\n\
         1. Project structure — directories, key files, entry points\n\
         2. Architecture — how modules connect, data flow patterns\n\
         3. Relevant code — files and patterns related to the task\n\
         4. Conventions — naming patterns, testing approaches, build system\n\
         5. Dependencies — what depends on what, which changes cascade\n\n\
         After exploring, output your findings as a JSON code block.\n\n\
         **Analysis Decision**: Decide whether the task needs deep analysis before decomposition.\n\
         Set `analysisNeeded: true` if ANY of these apply:\n\
         - The task touches 5+ distinct modules or subsystems\n\
         - Complex data flows that you couldn't fully trace in 20 turns\n\
         - You expect the plan will have 20+ tasks\n\
         - There are critical integration points you're uncertain about\n\n\
         Set `analysisNeeded: false` if:\n\
         - The task is localized to 1-3 modules\n\
         - You have high confidence after exploration\n\
         - The task is primarily additive (new files, not modifying existing flows)\n\n\
         When `analysisNeeded: true`, include `analysisDirectives` — specific questions for \
         Opus-level analysts to investigate in parallel.\n\n\
         ```json\n\
         {{\n\
           \"summary\": \"Brief architectural summary of the project\",\n\
           \"modules\": [\n\
             {{\n\
               \"path\": \"src/module/\",\n\
               \"purpose\": \"What this module does\",\n\
               \"keyFiles\": [\"src/module/main.ts\"],\n\
               \"dependencies\": [\"src/other/\"]\n\
             }}\n\
           ],\n\
           \"patterns\": [\"Pattern 1 observed in the codebase\"],\n\
           \"conventions\": [\"Convention 1 from CLAUDE.md or discovered\"],\n\
           \"analysisNeeded\": false,\n\
           \"analysisDirectives\": []\n\
         }}\n\
         ```\n\n\
         CRITICAL JSON RULES:\n\
         - Output ONLY valid JSON inside the ```json fences — no comments, no trailing commas\n\
         - Use double quotes for all keys and string values\n\
         - If `analysisNeeded` is false, omit `analysisDirectives` or set it to an empty array\n\
         - The JSON block must be the LAST thing in your response"
    )
}

/// Build the analyst prompt for a specific directive.
pub fn analyst_prompt(
    directive: &AnalysisDirective,
    codebase_summary: &str,
    project_path: &str,
) -> String {
    let project_instructions = load_project_instructions(project_path, 8000);
    let conventions_block = if project_instructions.is_empty() {
        String::new()
    } else {
        format!("## Project Conventions\n{project_instructions}\n\n---\n\n")
    };
    let file_hints = directive
        .file_hints
        .iter()
        .map(|f| format!("`{f}`"))
        .collect::<Vec<_>>()
        .join(", ");

    format!(
        "You are an Analyst agent in the Orchestra system. \
         Your job is to deeply investigate a specific area of the codebase and report findings.\n\n\
         PROJECT: {project_path}\n\n\
         {conventions_block}\
         ## Codebase Context\n{codebase_summary}\n\n\
         ## Your Investigation Area: {area}\n\n\
         **Question**: {question}\n\n\
         **Starting Points**: {file_hints}\n\n\
         ## Instructions\n\n\
         1. Read the starting point files thoroughly\n\
         2. Trace data flows, call chains, and dependencies\n\
         3. Look for patterns, edge cases, and potential risks\n\
         4. Identify all files that would need changes related to this area\n\
         5. Note any conventions or constraints that implementers must follow\n\n\
         Be thorough — your findings will be used to create detailed implementation tasks.\n\n\
         ## Output\n\n\
         After investigating, output your findings as a JSON code block:\n\n\
         ```json\n\
         {{\n\
           \"findings\": \"Detailed markdown findings\",\n\
           \"keyFiles\": [\"src/path/to/important.ts\"],\n\
           \"patterns\": [\"Pattern 1 observed in this area\"],\n\
           \"risks\": [\"Risk 1\"]\n\
         }}\n\
         ```\n\n\
         CRITICAL JSON RULES:\n\
         - Output ONLY valid JSON inside the ```json fences — no comments, no trailing commas\n\
         - Use double quotes for all keys and string values\n\
         - The JSON block must be the LAST thing in your response",
        area = directive.area,
        question = directive.question,
    )
}

fn decompose_prompt(task_description: &str) -> String {
    format!(
        "## Phase 2: Decomposition\n\n\
         Based on your exploration, break the task into sequential PHASES. \
         Each phase contains tasks that can run in parallel.\n\n\
         TASK: {task_description}\n\n\
         Key principles:\n\
         - Phases execute SEQUENTIALLY — Phase 2 starts only after Phase 1 completes and is reviewed\n\
         - Tasks WITHIN a phase execute in PARALLEL (with dependency ordering)\n\
         - Earlier phases should lay foundations; later phases build on them\n\
         - If a later phase depends on decisions made in an earlier phase, keep them separate\n\
         - Only detail the FIRST phase's tasks fully — later phases stay as skeletons\n\n\
         Output as a JSON code block:\n\n\
         ```json\n\
         {{\n\
           \"summary\": \"Overall approach summary\",\n\
           \"phases\": [\n\
             {{\n\
               \"title\": \"Phase 1 title\",\n\
               \"description\": \"What this phase accomplishes\",\n\
               \"tasks\": [\n\
                 {{\n\
                   \"title\": \"Task title\",\n\
                   \"description\": \"Detailed instructions for the musician\",\n\
                   \"why\": \"Why this task is needed\",\n\
                   \"fileScope\": [\"src/path/to/file.ts\"],\n\
                   \"dependencies\": [],\n\
                   \"acceptanceCriteria\": [\"Criteria 1\"],\n\
                   \"estimatedTurns\": 15,\n\
                   \"model\": \"sonnet\"\n\
                 }}\n\
               ]\n\
             }},\n\
             {{\n\
               \"title\": \"Phase 2 title (skeleton)\",\n\
               \"description\": \"What this phase will accomplish — details TBD after Phase 1\",\n\
               \"tasks\": []\n\
             }}\n\
           ],\n\
           \"learningNotes\": [\"Insight about the codebase\"],\n\
           \"insights\": [\n\
             {{\n\
               \"category\": \"architecture|pattern|concept|decision\",\n\
               \"title\": \"Short title\",\n\
               \"body\": \"Educational explanation\"\n\
             }}\n\
           ],\n\
           \"estimatedTokens\": 350000,\n\
           \"estimatedMinutes\": 45\n\
         }}\n\
         ```\n\n\
         CRITICAL: Only the FIRST phase should have fully detailed tasks. \
         Later phases should have title + description but empty tasks array — \
         they will be detailed when their turn comes.\n\n\
         CRITICAL JSON RULES:\n\
         - Output ONLY valid JSON inside the ```json fences\n\
         - Use double quotes, no trailing commas\n\
         - The JSON block must be the LAST thing in your response"
    )
}

fn decompose_with_analysis_prompt(
    task_description: &str,
    analysis_results: &[AnalysisResult],
) -> String {
    let analysis_section: String = analysis_results
        .iter()
        .map(|r| {
            format!(
                "### {area}\n{findings}\n\n\
                 **Key files**: {key_files}\n\
                 **Patterns**: {patterns}\n\
                 **Risks**: {risks}",
                area = r.area,
                findings = r.findings,
                key_files = r.key_files.join(", "),
                patterns = r.patterns.join("; "),
                risks = r.risks.join("; "),
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n");

    format!(
        "## Phase 2: Decomposition (with Deep Analysis)\n\n\
         ## Analysis Reports\n\
         The following areas were deeply investigated by specialist analysts. \
         Use these findings to create a more granular, well-informed plan.\n\n\
         {analysis_section}\n\n\
         ---\n\n\
         Based on your exploration AND the deep analysis above, break the task into sequential PHASES. \
         Each phase contains tasks that can run in parallel.\n\n\
         TASK: {task_description}\n\n\
         Key principles:\n\
         - Phases execute SEQUENTIALLY — Phase 2 starts only after Phase 1 completes and is reviewed\n\
         - Tasks WITHIN a phase execute in PARALLEL (with dependency ordering)\n\
         - Earlier phases should lay foundations; later phases build on them\n\
         - If a later phase depends on decisions made in an earlier phase, keep them separate\n\
         - Only detail the FIRST phase's tasks fully — later phases stay as skeletons\n\
         - USE the analysis findings to create more specific, targeted tasks with precise file scopes\n\
         - ADDRESS the identified risks in your task descriptions and acceptance criteria\n\n\
         Output as a JSON code block:\n\n\
         ```json\n\
         {{\n\
           \"summary\": \"Overall approach summary\",\n\
           \"phases\": [\n\
             {{\n\
               \"title\": \"Phase 1 title\",\n\
               \"description\": \"What this phase accomplishes\",\n\
               \"tasks\": [\n\
                 {{\n\
                   \"title\": \"Task title\",\n\
                   \"description\": \"Detailed instructions for the musician\",\n\
                   \"why\": \"Why this task is needed\",\n\
                   \"fileScope\": [\"src/path/to/file.ts\"],\n\
                   \"dependencies\": [],\n\
                   \"acceptanceCriteria\": [\"Criteria 1\"],\n\
                   \"estimatedTurns\": 15,\n\
                   \"model\": \"sonnet\"\n\
                 }}\n\
               ]\n\
             }}\n\
           ],\n\
           \"learningNotes\": [\"Insight about the codebase\"],\n\
           \"insights\": [\n\
             {{\n\
               \"category\": \"architecture|pattern|concept|decision\",\n\
               \"title\": \"Short title\",\n\
               \"body\": \"Educational explanation\"\n\
             }}\n\
           ],\n\
           \"estimatedTokens\": 350000,\n\
           \"estimatedMinutes\": 45\n\
         }}\n\
         ```\n\n\
         CRITICAL: Only the FIRST phase should have fully detailed tasks.\n\n\
         CRITICAL JSON RULES:\n\
         - Output ONLY valid JSON inside the ```json fences\n\
         - Use double quotes, no trailing commas\n\
         - The JSON block must be the LAST thing in your response"
    )
}

fn detail_phase_prompt(phase: &Phase, completed_phases: &[Phase]) -> String {
    let completed_context = if completed_phases.is_empty() {
        String::new()
    } else {
        let sections: String = completed_phases
            .iter()
            .map(|p| {
                let task_list: String = p
                    .tasks
                    .iter()
                    .map(|t| {
                        let summary = t
                            .result
                            .as_ref()
                            .map(|r| {
                                format!(": {}", truncate_str(&r.summary, 150))
                            })
                            .unwrap_or_default();
                        format!("- {} [{:?}]{}", t.title, t.status, summary)
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                format!("### {}\n{}\nTasks:\n{}", p.title, p.description, task_list)
            })
            .collect::<Vec<_>>()
            .join("\n\n");
        format!("## Completed Phases\n{sections}\n\n")
    };

    format!(
        "## Phase Detailing\n\n\
         {completed_context}\
         Now detail the tasks for the next phase:\n\n\
         ### {title}\n{description}\n\n\
         Create specific, executable tasks for this phase. \
         Each task should be completable by a single musician in 10-30 turns.\n\n\
         Output as a JSON code block:\n\n\
         ```json\n\
         {{\n\
           \"tasks\": [\n\
             {{\n\
               \"title\": \"Task title\",\n\
               \"description\": \"Detailed instructions — include context from completed phases if relevant\",\n\
               \"why\": \"Why this task is needed\",\n\
               \"fileScope\": [\"src/path/to/file.ts\"],\n\
               \"dependencies\": [],\n\
               \"acceptanceCriteria\": [\"Criteria 1\"],\n\
               \"estimatedTurns\": 15,\n\
               \"model\": \"sonnet\"\n\
             }}\n\
           ]\n\
         }}\n\
         ```\n\n\
         IMPORTANT:\n\
         - dependencies are INDICES within THIS phase's task array (0-based)\n\
         - fileScope should minimize overlap between tasks\n\
         - Include any context from completed phases that musicians need\n\
         - model: \"opus\" for complex tasks, \"haiku\" for trivial, \"sonnet\" for standard\n\n\
         CRITICAL JSON RULES:\n\
         - Output ONLY valid JSON — no comments, no trailing commas\n\
         - The JSON block must be the LAST thing in your response",
        title = phase.title,
        description = phase.description,
    )
}

fn phase_review_prompt(
    phase: &Phase,
    all_phases: &[Phase],
    diffs: &HashMap<usize, String>,
    retry_attempt: u32,
) -> String {
    let task_results: String = phase
        .tasks
        .iter()
        .map(|t| {
            let r = &t.result;
            let verify_status = match r {
                Some(r) => match r.verification_passed {
                    Some(true) => "PASS",
                    Some(false) => "FAIL",
                    None => "N/A",
                },
                None => "N/A",
            };
            let summary = r
                .as_ref()
                .map(|r| r.summary.as_str())
                .unwrap_or("N/A");
            format!(
                "Task {}: {}\nStatus: {:?}\nVerification: {}\nSummary: {}",
                t.index, t.title, t.status, verify_status, summary,
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n");

    let diff_section: String = {
        let mut entries: Vec<_> = diffs.iter().collect();
        entries.sort_by_key(|(k, _)| *k);
        entries
            .iter()
            .map(|(idx, diff)| {
                let truncated = truncate_str(diff, 3000);
                format!("### Task {idx}\n```diff\n{truncated}\n```")
            })
            .collect::<Vec<_>>()
            .join("\n\n")
    };

    let remaining_phases: String = all_phases
        .iter()
        .filter(|p| p.status == conductor_types::PhaseStatus::Pending)
        .map(|p| format!("- {}: {}", p.title, p.description))
        .collect::<Vec<_>>()
        .join("\n");

    let retry_context = if retry_attempt > 0 {
        format!(
            "\n**NOTE**: This is retry attempt {retry_attempt}/3. \
             If the issues are minor or cosmetic, prefer \"continue\" over another retry.\n"
        )
    } else {
        String::new()
    };

    let remaining_text = if remaining_phases.is_empty() {
        "None — this was the last phase".to_string()
    } else {
        remaining_phases
    };

    format!(
        "## Phase Review: {title}\n{retry_context}\n\
         ### Completed Task Results\n{task_results}\n\n\
         ### Git Diffs\n{diff_section}\n\n\
         ### Remaining Phases\n{remaining_text}\n\n\
         ## Instructions\n\n\
         Review the completed phase. Decide the next action:\n\n\
         1. `\"continue\"` — Phase succeeded, proceed to next phase\n\
         2. `\"retry_tasks\"` — Some tasks failed, retry specific ones within this phase\n\
         3. `\"revise_remaining_phases\"` — Phase succeeded but remaining phases need adjustment\n\
         4. `\"abort\"` — Critical failure, stop execution\n\n\
         Output as a JSON code block:\n\n\
         ```json\n\
         {{\n\
           \"action\": \"continue|retry_tasks|revise_remaining_phases|abort\",\n\
           \"taskIndices\": [0, 2],\n\
           \"summary\": \"Explanation of the decision\",\n\
           \"revisedPhases\": [],\n\
           \"insights\": []\n\
         }}\n\
         ```\n\n\
         For \"revise_remaining_phases\", include a `revisedPhases` array with updated phase skeletons.\n\
         For \"retry_tasks\", include `taskIndices` (indices within this phase).\n\n\
         CRITICAL JSON RULES:\n\
         - Output ONLY valid JSON — no comments, no trailing commas\n\
         - The JSON block must be the LAST thing in your response",
        title = phase.title,
    )
}

fn review_prompt(input: &ReviewInput) -> String {
    let sorted_diffs: Vec<_> = input
        .diffs
        .as_ref()
        .map(|d| {
            let mut entries: Vec<_> = d.iter().collect();
            entries.sort_by_key(|(k, _)| *k);
            entries
        })
        .unwrap_or_default();

    let recent_count = sorted_diffs.len().min(10);
    let older_entries = &sorted_diffs[..sorted_diffs.len().saturating_sub(recent_count)];
    let recent_entries = &sorted_diffs[sorted_diffs.len().saturating_sub(recent_count)..];

    let mut diff_section = String::new();
    if !older_entries.is_empty() {
        diff_section.push_str("## Older Task Diffs (summary only)\n");
        for (idx, _) in older_entries {
            diff_section.push_str(&format!(
                "- Task {}: diff available but omitted for context budget\n",
                *idx + 1
            ));
        }
        diff_section.push('\n');
    }
    if !recent_entries.is_empty() {
        diff_section.push_str("## Git Diffs (recent tasks)\n");
        for (idx, diff) in recent_entries {
            let truncated = truncate_str(diff, 3000);
            diff_section.push_str(&format!(
                "### Task {}\n```diff\n{}\n```\n\n",
                *idx + 1,
                truncated,
            ));
        }
    }

    let verify_section = input
        .verification_results
        .as_ref()
        .filter(|v| !v.is_empty())
        .map(|v| {
            let entries: String = v
                .iter()
                .map(|(idx, result)| {
                    let truncated = truncate_str(result, 2000);
                    format!("### Task {}\n```\n{}\n```", idx + 1, truncated)
                })
                .collect::<Vec<_>>()
                .join("\n\n");
            format!("## Verification Results\n{entries}\n\n")
        })
        .unwrap_or_default();

    let shared_mem = if input.shared_memory.len() > 4000 {
        format!(
            "[...earlier entries truncated]\n\n{}",
            truncate_str_tail(&input.shared_memory, 4000)
        )
    } else {
        input.shared_memory.clone()
    };

    format!(
        "Review the completed work from musicians.\n\n\
         ## Completed Task Results\n{task_results}\n\n\
         {diff_section}{verify_section}\
         ## Shared Memory (musician discoveries)\n{shared_mem}\n\n\
         ## Instructions\n\n\
         Review all completed work — pay attention to the actual diffs and verification results, \
         not just the summaries. Decide the next action:\n\n\
         1. If all tasks succeeded and the work looks correct: output `{{\"action\": \"complete\", \"summary\": \"...\"}}`\n\
         2. If some tasks failed or have typecheck errors: output `{{\"action\": \"retry\", \"taskIndices\": [0, 2], \"adjustedInstructions\": \"...\"}}`\n\
         3. If additional tasks are needed: output `{{\"action\": \"extend\", \"newTasks\": [...], \"reason\": \"...\"}}`\n\
         4. If there are integration issues: output `{{\"action\": \"integrate\", \"instructions\": \"...\"}}`\n\n\
         Include an \"insights\" array in your JSON output.\n\n\
         Output your decision as a COMPLETE, valid JSON block inside ```json fences.\n\
         No trailing commas, no comments — strict JSON only.\n\
         The JSON block must be the LAST thing in your response.",
        task_results = input.task_results,
    )
}

fn guidance_prompt(input: &GuidanceInput) -> String {
    let shared_mem = if input.shared_memory.len() > 2000 {
        format!(
            "[...earlier entries truncated]\n\n{}",
            truncate_str_tail(&input.shared_memory, 2000)
        )
    } else {
        input.shared_memory.clone()
    };

    let user_messages: String = input
        .user_messages
        .iter()
        .map(|m| format!("[{}] {}", m.timestamp, m.message))
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        "Process user guidance during task execution.\n\n\
         ## User Guidance\n{user_messages}\n\n\
         ## Current Task Status\n{task_status}\n\n\
         ## Shared Memory (recent)\n{shared_mem}\n\n\
         ## Instructions\n\n\
         The user has sent guidance during execution. Interpret it in context of current progress.\n\
         You can:\n\
         - **addTasks**: Add new tasks to the queue\n\
         - **cancelTasks**: Cancel pending/queued tasks by index (NEVER cancel in_progress tasks)\n\
         - **modifyTasks**: Modify pending task descriptions by index\n\
         - **guidance**: Provide general guidance text for all musicians to see\n\n\
         Be conservative — only propose changes that clearly align with the user's intent.\n\
         Do NOT modify or cancel in_progress tasks.\n\n\
         Output as a JSON code block:\n\
         ```json\n\
         {{\n\
           \"addTasks\": [],\n\
           \"cancelTasks\": [],\n\
           \"modifyTasks\": [],\n\
           \"guidance\": \"Optional guidance for musicians...\",\n\
           \"insights\": []\n\
         }}\n\
         ```\n\n\
         CRITICAL JSON RULES:\n\
         - Output ONLY valid JSON inside the ```json fences — no comments, no trailing commas\n\
         - Use double quotes for all keys and string values\n\
         - Ensure the JSON block is COMPLETE — do not truncate\n\
         - The JSON block must be the LAST thing in your response",
        task_status = input.task_status,
    )
}

const REFINE_PROMPT: &str = "The user wants to adjust the current plan. Make ONLY the changes they requested — do not redesign the entire plan.\n\n\
You may: add, remove, or modify tasks; change descriptions, file scopes, dependencies, or model assignments; reorder or merge tasks; adjust estimates.\n\n\
If the user attached images (screenshots, mockups, etc.), analyze them and incorporate the visual context into relevant task descriptions.\n\n\
First, briefly explain what you're changing and why (2-3 sentences max).\n\
Then output the COMPLETE updated plan as a ```json block (same format as the original plan).\n\n\
CRITICAL: Output complete, valid JSON. No comments, no trailing commas.\n\
The JSON block must be the LAST thing in your response.";

// ─── Tests ──────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_json_block_fenced() {
        let text = "Here is the plan:\n```json\n{\"summary\": \"test\"}\n```\nDone.";
        let block = extract_json_block(text).unwrap();
        assert_eq!(block, "{\"summary\": \"test\"}");
    }

    #[test]
    fn extract_json_block_no_fence_fallback() {
        let text = "Some text {\"action\": \"continue\"} more text";
        let block = extract_json_block(text).unwrap();
        assert_eq!(block, "{\"action\": \"continue\"}");
    }

    #[test]
    fn extract_json_block_nested() {
        let text = "result: {\"a\": {\"b\": 1}, \"c\": 2}";
        let block = extract_json_block(text).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(block).unwrap();
        assert_eq!(parsed["a"]["b"], 1);
        assert_eq!(parsed["c"], 2);
    }

    #[test]
    fn extract_json_block_with_strings_containing_braces() {
        let text = r#"{"msg": "hello {world}"}"#;
        let block = extract_json_block(text).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(block).unwrap();
        assert_eq!(parsed["msg"], "hello {world}");
    }

    #[test]
    fn extract_json_block_none() {
        assert!(extract_json_block("no json here").is_none());
    }

    #[test]
    fn extract_json_block_plain_fence() {
        let text = "Here is the result:\n```\n{\"summary\": \"test\", \"modules\": []}\n```\nDone.";
        let block = extract_json_block(text).unwrap();
        assert_eq!(block, r#"{"summary": "test", "modules": []}"#);
    }

    #[test]
    fn extract_json_block_plain_fence_skips_non_json() {
        // Plain fence with non-JSON content should be skipped, second fence has JSON
        let text = "```\nsome code\n```\n\n```\n{\"key\": \"value\"}\n```";
        let block = extract_json_block(text).unwrap();
        assert_eq!(block, r#"{"key": "value"}"#);
    }

    #[test]
    fn sanitize_json_strips_comments() {
        let input = "{\n// comment\n\"key\": \"value\"\n}";
        let sanitized = sanitize_json(input);
        let parsed: serde_json::Value = serde_json::from_str(&sanitized).unwrap();
        assert_eq!(parsed["key"], "value");
    }

    #[test]
    fn sanitize_json_strips_trailing_commas() {
        let input = r#"{"a": 1, "b": 2, }"#;
        let sanitized = sanitize_json(input);
        let parsed: serde_json::Value = serde_json::from_str(&sanitized).unwrap();
        assert_eq!(parsed["a"], 1);
    }

    #[test]
    fn sanitize_json_preserves_comments_in_strings() {
        let input = r#"{"url": "https://example.com//path"}"#;
        let sanitized = sanitize_json(input);
        let parsed: serde_json::Value = serde_json::from_str(&sanitized).unwrap();
        assert_eq!(parsed["url"], "https://example.com//path");
    }

    #[test]
    fn parse_tasks_basic() {
        let raw: Vec<serde_json::Value> = serde_json::from_str(
            r#"[
                {
                    "title": "Task 1",
                    "description": "Do something",
                    "why": "Because",
                    "fileScope": ["src/main.rs"],
                    "dependencies": [],
                    "acceptanceCriteria": ["It works"],
                    "estimatedTurns": 10,
                    "model": "sonnet"
                }
            ]"#,
        )
        .unwrap();

        let tasks = parse_tasks(&raw);
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].title, "Task 1");
        assert_eq!(tasks[0].status, TaskStatus::Ready); // no deps = Ready
        assert_eq!(tasks[0].model, Some("sonnet".into()));
    }

    #[test]
    fn parse_tasks_with_dependencies() {
        let raw: Vec<serde_json::Value> = serde_json::from_str(
            r#"[
                {"title": "A", "description": "first", "why": "x", "fileScope": [], "dependencies": [], "acceptanceCriteria": ["ok"], "estimatedTurns": 5},
                {"title": "B", "description": "second", "why": "y", "fileScope": [], "dependencies": [0], "acceptanceCriteria": ["ok"], "estimatedTurns": 5}
            ]"#,
        )
        .unwrap();

        let tasks = parse_tasks(&raw);
        assert_eq!(tasks[0].status, TaskStatus::Ready);
        assert_eq!(tasks[1].status, TaskStatus::Queued); // has dependency
        assert_eq!(tasks[1].dependencies, vec![0]);
    }

    #[test]
    fn parse_tasks_truncated_detection() {
        let raw: Vec<serde_json::Value> = serde_json::from_str(
            r#"[{"title": "Truncated", "description": "Some description"}]"#,
        )
        .unwrap();

        let tasks = parse_tasks(&raw);
        assert!(tasks[0].description.contains("[NOTE: This task may have been truncated"));
        assert!(!tasks[0].why.is_empty());
        assert!(!tasks[0].acceptance_criteria.is_empty());
    }

    #[test]
    fn parse_tasks_defaults() {
        let raw: Vec<serde_json::Value> = serde_json::from_str(r#"[{}]"#).unwrap();
        let tasks = parse_tasks(&raw);
        assert_eq!(tasks[0].estimated_turns, 15); // default
        assert!(tasks[0].model.is_none());
    }

    #[test]
    fn validate_phase_review_continue() {
        let raw: serde_json::Value =
            serde_json::from_str(r#"{"action": "continue", "summary": "All good"}"#).unwrap();
        let result = validate_phase_review_result(&raw);
        assert_eq!(result.action, PhaseReviewAction::Continue);
        assert_eq!(result.summary, "All good");
    }

    #[test]
    fn validate_phase_review_invalid_action() {
        let raw: serde_json::Value =
            serde_json::from_str(r#"{"action": "invalid", "summary": "oops"}"#).unwrap();
        let result = validate_phase_review_result(&raw);
        assert_eq!(result.action, PhaseReviewAction::Continue);
        // Uses raw summary when present
        assert_eq!(result.summary, "oops");
    }

    #[test]
    fn validate_phase_review_retry_no_indices() {
        let raw: serde_json::Value =
            serde_json::from_str(r#"{"action": "retry_tasks", "summary": "failed"}"#).unwrap();
        let result = validate_phase_review_result(&raw);
        assert_eq!(result.action, PhaseReviewAction::Continue); // downgraded
        assert!(result.summary.contains("taskIndices"));
    }

    #[test]
    fn validate_phase_review_retry_with_indices() {
        let raw: serde_json::Value = serde_json::from_str(
            r#"{"action": "retry_tasks", "taskIndices": [0, 2], "summary": "retry these"}"#,
        )
        .unwrap();
        let result = validate_phase_review_result(&raw);
        assert_eq!(result.action, PhaseReviewAction::RetryTasks);
        assert_eq!(result.task_indices, Some(vec![0, 2]));
    }

    #[test]
    fn validate_phase_review_revise_no_phases() {
        let raw: serde_json::Value = serde_json::from_str(
            r#"{"action": "revise_remaining_phases", "summary": "need changes"}"#,
        )
        .unwrap();
        let result = validate_phase_review_result(&raw);
        assert_eq!(result.action, PhaseReviewAction::Continue); // downgraded
        assert!(result.summary.contains("revisedPhases"));
    }

    #[test]
    fn validate_phase_review_abort() {
        let raw: serde_json::Value =
            serde_json::from_str(r#"{"action": "abort", "summary": "critical failure"}"#).unwrap();
        let result = validate_phase_review_result(&raw);
        assert_eq!(result.action, PhaseReviewAction::Abort);
    }

    #[test]
    fn load_project_instructions_missing() {
        let result = load_project_instructions("/nonexistent/path/12345", 16000);
        assert!(result.is_empty());
    }

    #[test]
    fn intelligent_truncate_short() {
        let content = "Short content";
        let result = intelligent_truncate(content, 100);
        assert!(result.contains("Short content"));
    }

    #[test]
    fn split_markdown_sections_basic() {
        let content = "# Heading 1\nContent 1\n## Heading 2\nContent 2\n### Heading 3\nContent 3";
        let sections = split_markdown_sections(content);
        assert!(sections.len() >= 2);
    }

    #[test]
    fn conductor_agent_new() {
        let agent = ConductorAgent::new("opus".into(), "/tmp".into());
        assert!(!agent.has_session());
        assert!(agent.prompts_sent.is_empty());
    }

    #[test]
    fn plan_prompt_includes_project() {
        let prompt = plan_prompt("/my/project", "build feature X", None);
        assert!(prompt.contains("/my/project"));
        assert!(prompt.contains("build feature X"));
        assert!(prompt.contains("Conductor agent"));
    }

    #[test]
    fn explore_prompt_includes_task() {
        let prompt = explore_prompt("/proj", "port module", None);
        assert!(prompt.contains("port module"));
        assert!(prompt.contains("Exploration"));
    }

    #[test]
    fn analyst_prompt_includes_directive() {
        let directive = AnalysisDirective {
            id: "d1".into(),
            area: "auth module".into(),
            question: "How does auth work?".into(),
            file_hints: vec!["src/auth.rs".into()],
            estimated_turns: 10,
        };
        let prompt = analyst_prompt(&directive, "Project summary", "/proj");
        assert!(prompt.contains("auth module"));
        assert!(prompt.contains("How does auth work?"));
        assert!(prompt.contains("`src/auth.rs`"));
    }

    #[test]
    fn decompose_prompt_includes_task() {
        let prompt = decompose_prompt("implement feature");
        assert!(prompt.contains("implement feature"));
        assert!(prompt.contains("Decomposition"));
    }

    #[test]
    fn guidance_prompt_includes_messages() {
        let input = GuidanceInput {
            user_messages: vec![conductor_types::GuidanceMessage {
                message: "focus on tests".into(),
                timestamp: "2026-01-01T00:00:00Z".into(),
            }],
            task_status: "3/5 complete".into(),
            shared_memory: "some memory".into(),
        };
        let prompt = guidance_prompt(&input);
        assert!(prompt.contains("focus on tests"));
        assert!(prompt.contains("3/5 complete"));
    }
}
