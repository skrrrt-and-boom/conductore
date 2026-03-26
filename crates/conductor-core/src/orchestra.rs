//! Main orchestra state machine — coordinates musicians, conductor, and task flow.
//!
//! Ports `src/orchestra.ts` from the TypeScript conductor.
//!
//! Architecture:
//! - Orchestra OWNS OrchestraState (no Arc<Mutex>)
//! - `tokio::sync::watch` for state broadcasting (orchestra → TUI)
//! - `tokio::sync::mpsc` for musician → orchestra events
//! - `tokio::sync::mpsc` for user → orchestra actions

use std::collections::HashMap;
use std::future::Future;
use std::path::Path;
use std::pin::Pin;

use conductor_types::{
    AnalysisResult, AnalystState, CodebaseMap, GuidanceActions, GuidanceInput, GuidanceMessage,
    Insight, InsightCategory, MusicianState, MusicianStatus, OrchestraConfig,
    OrchestraEvent, OrchestraPhase, OrchestraState, Phase, PhaseReviewAction, PhaseStatus, Plan,
    PlanRefinementMessage, PlanValidation, RefinementRole, SessionData, Task, TaskResult,
    TaskStatus, UserAction, WorktreeSnapshot, WorktreeStatus,
};
use tokio::process::Command;
use tokio::sync::{mpsc, watch};
use uuid::Uuid;

use crate::caffeinate::Caffeinate;
use crate::conductor_agent::{ConductorAgent, ReviewInput};
use crate::dag::validate_plan;
use crate::insights::InsightGenerator;
use crate::memory::SharedMemory;
use crate::musician::Musician;
use crate::rate_limiter::RateLimiter;
use crate::task_store::TaskStore;
use crate::worktree_manager::WorktreeManager;
use crate::CoreError;

/// Maximum review retries before forcing completion.
const MAX_REVIEW_RETRIES: u32 = 3;
/// Maximum phase retries before forcing continue.
const MAX_PHASE_RETRIES: u32 = 3;
/// Musician stuck timeout (35 minutes).
const MUSICIAN_TIMEOUT_MS: u64 = 35 * 60 * 1000;
/// Default verification timeout (2 minutes).
const DEFAULT_VERIFICATION_TIMEOUT_MS: u64 = 120_000;

/// Load conductor.yml from the project root for project-specific settings.
fn load_conductor_config(project_path: &str) -> (Option<Vec<String>>, Option<u64>) {
    let base = Path::new(project_path);
    let candidates = [
        base.join("conductor.yml"),
        base.join("conductor.yaml"),
        base.join(".conductor.yml"),
    ];

    for path in &candidates {
        if let Ok(content) = std::fs::read_to_string(path) {
            // Simple YAML parsing for verification field
            let mut verification = Vec::new();
            let mut timeout_ms = None;
            let mut in_verification = false;

            for line in content.lines() {
                let trimmed = line.trim();
                if trimmed.starts_with("verification:") {
                    in_verification = true;
                    continue;
                }
                if trimmed.starts_with("verificationTimeout:") {
                    if let Some(val) = trimmed.strip_prefix("verificationTimeout:") {
                        if let Ok(secs) = val.trim().parse::<u64>() {
                            timeout_ms = Some(secs * 1000);
                        }
                    }
                    in_verification = false;
                    continue;
                }
                if in_verification && trimmed.starts_with("- ") {
                    verification.push(trimmed[2..].to_string());
                } else if !trimmed.starts_with('-') && !trimmed.is_empty() {
                    in_verification = false;
                }
            }

            return (
                if verification.is_empty() {
                    None
                } else {
                    Some(verification)
                },
                timeout_ms,
            );
        }
    }
    (None, None)
}

/// Extract a session ID reference from a task description.
///
/// Looks for patterns like "session 0481dc" or "improve on 0481dc".
fn extract_session_reference(task_description: &str) -> Option<String> {
    let lower = task_description.to_lowercase();

    // Match "session <hex_id>" pattern
    for keyword in &["session "] {
        if let Some(pos) = lower.find(keyword) {
            let after = &task_description[pos + keyword.len()..];
            let id: String = after.chars().take_while(|c| c.is_ascii_hexdigit()).collect();
            if id.len() >= 6 && id.len() <= 8 {
                return Some(id.to_lowercase());
            }
        }
    }

    // Match "improve on <id>", "build on <id>", "iterate on <id>", "continue on <id>"
    for keyword in &["improve on ", "build on ", "iterate on ", "continue on ",
                      "improve on top of ", "build on top of ", "iterate on top of ", "continue on top of ",
                      "improve ", "build ", "iterate ", "continue "] {
        if let Some(pos) = lower.find(keyword) {
            let after = &task_description[pos + keyword.len()..];
            let id: String = after.chars().take_while(|c| c.is_ascii_hexdigit()).collect();
            if id.len() >= 6 && id.len() <= 8 {
                return Some(id.to_lowercase());
            }
        }
    }

    None
}

/// Core orchestration state machine.
pub struct Orchestra {
    // State
    state: OrchestraState,
    phase: OrchestraPhase,
    config: OrchestraConfig,
    plan: Option<Plan>,
    tasks: Vec<Task>,
    phases: Vec<Phase>,
    current_phase_index: Option<usize>,
    musicians: Vec<Option<Musician>>,
    musician_states: Vec<MusicianState>,

    // V2: Analysis
    analyst_states: Vec<AnalystState>,
    analysis_results: Vec<AnalysisResult>,
    codebase_map: Option<CodebaseMap>,

    // Worktrees
    worktree_snapshots: Vec<WorktreeSnapshot>,

    // Services (owned)
    conductor: Option<ConductorAgent>,
    rate_limiter: RateLimiter,
    task_store: TaskStore,
    memory: SharedMemory,
    worktree_manager: WorktreeManager,
    insight_generator: InsightGenerator,
    caffeinate: Caffeinate,

    // Tracking
    review_retry_count: u32,
    conductor_output: Vec<String>,
    started_at: String,
    last_memory_sync_offset: usize,
    verification_timeout_ms: u64,

    // Guidance
    guidance_queue: Vec<GuidanceMessage>,

    // Plan validation & refinement
    plan_validation: Option<PlanValidation>,
    refinement_history: Vec<PlanRefinementMessage>,

    // Rate limit tracking
    phase_before_pause: OrchestraPhase,

    // Active musician tracking
    active_musicians: HashMap<String, tokio::task::JoinHandle<()>>,
    active_start_times: HashMap<String, std::time::Instant>,

    // Guidance channels — senders for injecting prompts into spawned musicians
    guidance_channels: HashMap<String, mpsc::Sender<String>>,

    // Musician return channel (spawned tasks send musicians back after execution)
    musician_return_tx: mpsc::Sender<(usize, Musician)>,
    musician_return_rx: Option<mpsc::Receiver<(usize, Musician)>>,

    // Phase-scoped task list (when executing within a phase)
    current_phase_tasks: Option<Vec<usize>>,

    // Channels
    event_tx: mpsc::Sender<OrchestraEvent>,
    event_rx: Option<mpsc::Receiver<OrchestraEvent>>,
    state_tx: watch::Sender<OrchestraState>,
    action_rx: Option<mpsc::Receiver<UserAction>>,
}

impl Orchestra {
    /// Create a new Orchestra.
    ///
    /// Returns (orchestra, state_rx, action_tx) — the TUI uses state_rx to
    /// receive state updates and action_tx to send user actions.
    pub fn new(
        config: OrchestraConfig,
    ) -> (
        Self,
        watch::Receiver<OrchestraState>,
        mpsc::Sender<UserAction>,
    ) {
        let initial_state = OrchestraState::new(config.clone());
        let (state_tx, state_rx) = watch::channel(initial_state.clone());
        let (event_tx, event_rx) = mpsc::channel::<OrchestraEvent>(256);
        let (musician_return_tx, musician_return_rx) = mpsc::channel::<(usize, Musician)>(64);
        let (action_tx, action_rx) = mpsc::channel::<UserAction>(64);

        let task_store = TaskStore::new(&config.session_id);
        let memory_path = task_store.base_path().join("memory").join("SHARED.md");
        let memory = SharedMemory::new(memory_path);

        let mut conductor = ConductorAgent::new(
            config.conductor_model.clone(),
            config.project_path.clone(),
        );
        conductor.set_event_tx(event_tx.clone());
        let rate_limiter = RateLimiter::new(None);
        let worktree_manager = WorktreeManager::new(&config.project_path);
        let insight_generator = InsightGenerator::new();
        let caffeinate = Caffeinate::new();

        // Load project-specific config
        let (proj_verification, proj_timeout) = load_conductor_config(&config.project_path);
        let mut effective_config = config.clone();
        if effective_config.verification.is_none() {
            effective_config.verification = proj_verification;
        }
        let verification_timeout_ms = proj_timeout.unwrap_or(DEFAULT_VERIFICATION_TIMEOUT_MS);

        let orchestra = Self {
            state: initial_state,
            phase: OrchestraPhase::Init,
            config: effective_config,
            plan: None,
            tasks: Vec::new(),
            phases: Vec::new(),
            current_phase_index: None,
            musicians: Vec::new(),
            musician_states: Vec::new(),
            analyst_states: Vec::new(),
            analysis_results: Vec::new(),
            codebase_map: None,
            worktree_snapshots: Vec::new(),
            conductor: Some(conductor),
            rate_limiter,
            task_store,
            memory,
            worktree_manager,
            insight_generator,
            caffeinate,
            review_retry_count: 0,
            conductor_output: Vec::new(),
            started_at: chrono::Utc::now().to_rfc3339(),
            last_memory_sync_offset: 0,
            verification_timeout_ms,
            guidance_queue: Vec::new(),
            plan_validation: None,
            refinement_history: Vec::new(),
            phase_before_pause: OrchestraPhase::Init,
            active_musicians: HashMap::new(),
            active_start_times: HashMap::new(),
            guidance_channels: HashMap::new(),
            musician_return_tx,
            musician_return_rx: Some(musician_return_rx),
            current_phase_tasks: None,
            event_tx,
            event_rx: Some(event_rx),
            state_tx,
            action_rx: Some(action_rx),
        };

        (orchestra, state_rx, action_tx)
    }

    /// Restore orchestra state from a previously persisted session.
    ///
    /// This replaces the orchestra's phase, tasks, phases, and worktree state
    /// with data loaded from disk, enabling true session resume instead of
    /// re-planning from scratch.
    pub fn restore_session(&mut self, session: SessionData) {
        // Swap task store to use the restored session's ID
        self.task_store = TaskStore::new(&session.id);
        self.config.session_id = session.id.clone();
        self.config.task_description = session.config.task_description;

        // Restore phase state
        self.phase = session.phase;
        self.tasks = session.tasks;
        self.started_at = session.started_at;

        if let Some(phases) = session.phases {
            self.phases = phases;
        }
        self.current_phase_index = session.current_phase_index;

        // Restore worktrees
        if let Some(ref snapshots) = session.worktree_state {
            self.worktree_snapshots = snapshots.clone();
            self.worktree_manager.restore_from_snapshots(snapshots);
        }

        // Reset in-progress tasks to Queued (musician processes are dead)
        for task in &mut self.tasks {
            if task.status == TaskStatus::InProgress {
                task.status = TaskStatus::Queued;
                task.assigned_musician = None;
                task.result = None;
            }
        }
        for phase in &mut self.phases {
            for task in &mut phase.tasks {
                if task.status == TaskStatus::InProgress {
                    task.status = TaskStatus::Queued;
                    task.assigned_musician = None;
                    task.result = None;
                }
            }
        }

        // Rebuild Plan object from restored data
        if !self.tasks.is_empty() {
            self.plan = Some(Plan {
                summary: format!("Resumed session {}", session.id),
                tasks: self.tasks.clone(),
                dependency_graph: String::new(),
                musician_assignment: String::new(),
                learning_notes: Vec::new(),
                estimated_minutes: 0,
                insights: None,
            });
        }

        self.broadcast_state();
    }

    /// Build and broadcast the current OrchestraState.
    fn broadcast_state(&mut self) {
        self.state = OrchestraState {
            phase: self.phase.clone(),
            config: self.config.clone(),
            tasks: self.tasks.clone(),
            plan: self.plan.clone(),
            phases: self.phases.clone(),
            current_phase_index: self.current_phase_index,
            musicians: self.musician_states.clone(),
            analysts: self.analyst_states.clone(),
            analysis_results: self.analysis_results.clone(),
            rate_limit: self.rate_limiter.state().clone(),
            started_at: self.started_at.clone(),
            elapsed_ms: elapsed_since(&self.started_at),
            conductor_output: self.conductor_output.clone(),
            conductor_prompts: self.conductor.as_ref().map(|c| c.prompts_sent.clone()).unwrap_or_default(),
            guidance_queue_size: self.guidance_queue.len(),
            plan_validation: self.plan_validation.clone(),
            refinement_history: self.refinement_history.clone(),
            insights: self.insight_generator.get_all_insights().to_vec(),
        };

        let _ = self.state_tx.send(self.state.clone());
    }

    fn set_phase(&mut self, phase: OrchestraPhase) {
        self.phase = phase;
        self.broadcast_state();
    }

    fn push_conductor_output(&mut self, line: &str) {
        self.conductor_output.push(line.to_string());
        if self.conductor_output.len() > 100 {
            let excess = self.conductor_output.len() - 100;
            self.conductor_output.drain(..excess);
        }
    }

    /// Take the conductor out for use in a spawned task.
    fn take_conductor(&mut self) -> Result<ConductorAgent, CoreError> {
        self.conductor
            .take()
            .ok_or_else(|| CoreError::Channel("conductor already taken".into()))
    }

    /// Return the conductor after a spawned task completes.
    fn return_conductor(&mut self, conductor: ConductorAgent) {
        self.conductor = Some(conductor);
    }

    /// Run a conductor operation concurrently with event processing.
    ///
    /// Takes the conductor out, spawns `op` on a tokio task, and runs a
    /// mini event loop that processes orchestra events (including ConductorOutput),
    /// user actions (guidance), and broadcasts state updates to the TUI in real-time.
    async fn run_conductor_op<F, T>(&mut self, op: F) -> Result<T, CoreError>
    where
        F: FnOnce(ConductorAgent) -> Pin<Box<dyn Future<Output = (ConductorAgent, Result<T, CoreError>)> + Send>> + Send + 'static,
        T: Send + 'static,
    {
        let mut conductor = self.take_conductor()?;
        let (result_tx, mut result_rx) = tokio::sync::oneshot::channel();

        // Create a guidance channel — the conductor's send_and_collect will
        // select on this to inject user messages into the Claude session mid-turn.
        let (conductor_guidance_tx, conductor_guidance_rx) = mpsc::channel::<String>(16);
        conductor.set_guidance_rx(conductor_guidance_rx);

        tokio::spawn(async move {
            let result = op(conductor).await;
            let _ = result_tx.send(result);
        });

        // Take channels to process events and actions while conductor runs
        let mut event_rx = self
            .event_rx
            .take()
            .ok_or_else(|| CoreError::Channel("event_rx already taken".into()))?;
        let mut action_rx = self
            .action_rx
            .take()
            .ok_or_else(|| CoreError::Channel("action_rx already taken".into()))?;

        loop {
            tokio::select! {
                biased;
                result = &mut result_rx => {
                    let (conductor, op_result) = result
                        .map_err(|_| CoreError::Channel("conductor op cancelled".into()))?;
                    self.return_conductor(conductor);
                    // Drain any remaining events before returning
                    while let Ok(event) = event_rx.try_recv() {
                        self.handle_event(event).await;
                    }
                    self.event_rx = Some(event_rx);
                    self.action_rx = Some(action_rx);
                    self.broadcast_state();
                    return op_result;
                }
                Some(event) = event_rx.recv() => {
                    self.handle_event(event).await;
                    self.broadcast_state();
                }
                Some(action) = action_rx.recv() => {
                    // Process guidance: queue it and forward to conductor + musicians
                    if let UserAction::SubmitGuidance { ref text, .. } = action {
                        self.queue_guidance(text).await;
                        let _ = conductor_guidance_tx.try_send(text.clone());
                        for tx in self.guidance_channels.values() {
                            let _ = tx.try_send(text.clone());
                        }
                    } else {
                        self.handle_execution_action(action).await;
                    }
                    self.broadcast_state();
                }
            }
        }
    }

    // ─── Main Entry Point ──────────────────────────────────────

    /// Run the full orchestration loop.
    pub async fn run(&mut self) -> Result<(), CoreError> {
        self.caffeinate.start().await;

        self.task_store.init().await?;
        self.memory.init().await?;

        // Auto-prune old sessions
        let _ = TaskStore::keep_recent(20).await;

        // Interactive mode: wait for user to submit task via TUI
        if self.config.task_description.is_empty() {
            self.broadcast_state();
            return Ok(());
        }

        // Branch based on current phase — allows restored sessions to skip planning
        match self.phase {
            OrchestraPhase::Init => {
                // Fresh session — plan from scratch
                match self.do_plan().await {
                    Ok(()) => {}
                    Err(e) => {
                        self.set_phase(OrchestraPhase::Failed);
                        return Err(e);
                    }
                }
            }
            OrchestraPhase::PlanReview => {
                // Already have a plan, go straight to review
                self.push_conductor_output("Resumed session — plan ready for review.");
                self.broadcast_state();
            }
            OrchestraPhase::PhaseDetailing
            | OrchestraPhase::PhaseExecuting
            | OrchestraPhase::PhaseMerging
            | OrchestraPhase::PhaseReviewing => {
                // Mid-execution — present plan for approval, then continue from saved phase
                self.push_conductor_output(&format!(
                    "Resumed session — continuing from phase {}.",
                    self.current_phase_index.map(|i| i + 1).unwrap_or(1),
                ));
                self.set_phase(OrchestraPhase::PlanReview);
                if self.config.headless {
                    self.start_execution().await?;
                }
            }
            OrchestraPhase::FinalReview | OrchestraPhase::Reviewing => {
                self.push_conductor_output("Resumed session — proceeding to review.");
                self.do_final_review().await?;
            }
            OrchestraPhase::Complete | OrchestraPhase::Failed => {
                self.push_conductor_output("Session already finished.");
                self.broadcast_state();
            }
            // Exploring, Analyzing, Decomposing, Planning, etc. — mid-planning, re-plan
            _ => {
                self.phase = OrchestraPhase::Init;
                match self.do_plan().await {
                    Ok(()) => {}
                    Err(e) => {
                        self.set_phase(OrchestraPhase::Failed);
                        return Err(e);
                    }
                }
            }
        }

        Ok(())
    }

    /// Main event loop — called after planning to wait for user approval and execute.
    ///
    /// This should be called from the binary entry point after `run()` returns.
    /// It processes user actions (approve, refine, quit) and musician events.
    pub async fn event_loop(&mut self) -> Result<(), CoreError> {
        let mut event_rx = self
            .event_rx
            .take()
            .ok_or_else(|| CoreError::Channel("event_rx already taken".into()))?;
        let mut action_rx = self
            .action_rx
            .take()
            .ok_or_else(|| CoreError::Channel("action_rx already taken".into()))?;
        let mut tick = tokio::time::interval(tokio::time::Duration::from_millis(500));

        loop {
            tokio::select! {
                Some(event) = event_rx.recv() => {
                    self.handle_event(event).await;
                    self.broadcast_state();
                }
                Some(action) = action_rx.recv() => {
                    // Temporarily return channels so execution methods can use them.
                    // Safe: select! drops the recv() futures, releasing the borrows.
                    self.event_rx = Some(event_rx);
                    self.action_rx = Some(action_rx);
                    self.handle_action(action).await;
                    self.broadcast_state();
                    event_rx = self.event_rx.take().ok_or_else(|| {
                        CoreError::Channel("event_rx not returned after handle_action".into())
                    })?;
                    action_rx = self.action_rx.take().ok_or_else(|| {
                        CoreError::Channel("action_rx not returned after handle_action".into())
                    })?;
                }
                _ = tick.tick() => {
                    // Update elapsed time, check for stuck musicians, sync memory, process guidance
                    self.tick().await;
                    self.broadcast_state();
                }
            }

            if self.phase == OrchestraPhase::Complete || self.phase == OrchestraPhase::Failed {
                break;
            }
        }

        // Put channels back for potential reuse
        self.event_rx = Some(event_rx);
        self.action_rx = Some(action_rx);
        Ok(())
    }

    /// Handle a musician event.
    async fn handle_event(&mut self, event: OrchestraEvent) {
        match event {
            OrchestraEvent::MusicianStatusChange { musician_id, state } => {
                if let Some(idx) = self.musician_states.iter().position(|m| m.id == musician_id) {
                    self.musician_states[idx] = state;
                }
            }
            OrchestraEvent::MusicianOutput { musician_id, line } => {
                let _ = self
                    .task_store
                    .append_log(&musician_id, &line)
                    .await;
            }
            OrchestraEvent::MusicianToolUse {
                musician_id,
                tool_name,
                tool_input,
            } => {
                // Find the musician's current task for insight generation
                if let Some(ms) = self.musician_states.iter().find(|m| m.id == musician_id) {
                    if let Some(ref task) = ms.current_task {
                        self.insight_generator
                            .on_tool_use(&musician_id, &tool_name, tool_input, Some(task));
                    }
                }
            }
            OrchestraEvent::MusicianRateLimit { musician_id, event } => {
                let new_limit = self.rate_limiter.handle_event(&event);
                if new_limit {
                    tracing::warn!(musician = %musician_id, "rate limit detected");
                    self.phase_before_pause = self.phase.clone();
                    self.set_phase(OrchestraPhase::Paused);
                    for musician in self.musicians.iter_mut().filter_map(|m| m.as_mut()) {
                        musician.pause();
                    }
                }
            }
            OrchestraEvent::MusicianComplete {
                musician_id,
                result,
            } => {
                // NOTE: We intentionally do NOT remove from active_musicians here.
                // The JoinHandle must be awaited before drain_musician_returns can
                // pick up the returned musician. Removing it here would lose the
                // handle, leaving the musician slot as None and preventing second-
                // wave task assignment. The event-handler path cleans up finished
                // handles after the drain loop (see execute_loop).
                self.active_start_times.remove(&musician_id);
                self.guidance_channels.remove(&musician_id);

                // Update musician status immediately from the result — don't wait
                // for the separate MusicianStatusChange event, which would arrive
                // on the next select! iteration and cause a race where
                // sync_task_results() can't transition the task (status still Running)
                // while active_musicians is already empty, triggering the stuck check.
                if let Some(idx) = self.musician_states.iter().position(|m| m.id == musician_id) {
                    self.musician_states[idx].status = if result.success {
                        MusicianStatus::Completed
                    } else {
                        MusicianStatus::Failed
                    };
                }

                // Store result AND update task status directly — don't defer to
                // sync_task_results() which relies on musician_states.current_task
                // being correct. Updating here is authoritative and immediate.
                if let Some(ms) = self.musician_states.iter().find(|m| m.id == musician_id) {
                    if let Some(ref task) = ms.current_task {
                        let task_index = task.index;
                        if let Some(t) = self.tasks.iter_mut().find(|t| t.index == task_index) {
                            t.result = Some(result.clone());
                            if t.status == TaskStatus::InProgress {
                                t.status = if result.success {
                                    TaskStatus::Completed
                                } else {
                                    TaskStatus::Failed
                                };
                            }
                        }
                        // Keep phase tasks in sync
                        let new_status = if result.success { TaskStatus::Completed } else { TaskStatus::Failed };
                        self.sync_task_to_phase(task_index, new_status, Some(result.clone()));
                    }
                }

                tracing::info!(
                    musician = %musician_id,
                    success = result.success,
                    "musician completed"
                );
            }
            OrchestraEvent::ConductorOutput(line) => {
                self.push_conductor_output(&line);
            }
            OrchestraEvent::InsightGenerated(insight) => {
                self.insight_generator.add_insight(insight);
            }
            _ => {}
        }
    }

    /// Handle a user action.
    async fn handle_action(&mut self, action: UserAction) {
        match action {
            UserAction::Quit => {
                self.shutdown().await;
                self.set_phase(OrchestraPhase::Complete);
            }
            UserAction::ApprovePlan => {
                if self.phase == OrchestraPhase::PlanReview {
                    if let Err(e) = self.start_execution().await {
                        tracing::error!(error = %e, "failed to start execution");
                        self.set_phase(OrchestraPhase::Failed);
                        let _ = self.persist_state().await;
                    }
                }
            }
            UserAction::RefinePlan { text, images } => {
                if self.phase == OrchestraPhase::PlanReview {
                    if let Err(e) = self.replan(&text, images.as_deref()).await {
                        tracing::error!(error = %e, "replan failed");
                        self.set_phase(OrchestraPhase::PlanReview);
                    }
                }
            }
            UserAction::SubmitTask { text, images: _ } => {
                if self.phase == OrchestraPhase::Init {
                    self.config.task_description = text;
                    if let Err(e) = self.do_plan().await {
                        tracing::error!(error = %e, "planning failed");
                        self.set_phase(OrchestraPhase::Failed);
                    }
                }
            }
            UserAction::SubmitGuidance { text, images: _ } => {
                self.queue_guidance(&text).await;
                // Send to all running musicians via guidance channels
                for tx in self.guidance_channels.values() {
                    let _ = tx.try_send(text.clone());
                }
            }
            UserAction::ResumeSession { session_id } => {
                if self.phase == OrchestraPhase::Init {
                    match TaskStore::resolve_id(&session_id).await {
                        Some(resolved) => {
                            let store = TaskStore::new(&resolved);
                            match store.load_session().await {
                                Ok(Some(prev)) => {
                                    tracing::info!(session_id = %resolved, phase = ?prev.phase, "resuming session");
                                    let prev_phase = prev.phase.clone();
                                    self.restore_session(prev);

                                    match prev_phase {
                                        OrchestraPhase::PlanReview => {
                                            self.push_conductor_output("Resumed — plan ready for review.");
                                            self.broadcast_state();
                                        }
                                        OrchestraPhase::PhaseDetailing
                                        | OrchestraPhase::PhaseExecuting
                                        | OrchestraPhase::PhaseMerging
                                        | OrchestraPhase::PhaseReviewing => {
                                            self.push_conductor_output(&format!(
                                                "Resumed — continuing from phase {}. Approve to proceed.",
                                                self.current_phase_index.map(|i| i + 1).unwrap_or(1),
                                            ));
                                            self.set_phase(OrchestraPhase::PlanReview);
                                            self.broadcast_state();
                                        }
                                        OrchestraPhase::Complete | OrchestraPhase::Failed => {
                                            self.push_conductor_output("Session already finished.");
                                            self.broadcast_state();
                                        }
                                        _ => {
                                            // Mid-planning or Init — re-plan
                                            if let Err(e) = self.do_plan().await {
                                                tracing::error!(error = %e, "planning failed");
                                                self.set_phase(OrchestraPhase::Failed);
                                            }
                                        }
                                    }
                                }
                                Ok(None) => {
                                    self.push_conductor_output(&format!("Session '{session_id}' not found"));
                                }
                                Err(e) => {
                                    self.push_conductor_output(&format!("Failed to load session: {e}"));
                                }
                            }
                        }
                        None => {
                            self.push_conductor_output(&format!("Session '{session_id}' not found or ambiguous"));
                        }
                    }
                }
            }
            _ => {
                // FocusNext, FocusPrev, Scroll, Resize, etc. — TUI handles these locally
            }
        }
    }

    /// Restore musicians returned from completed spawned tasks.
    fn drain_musician_returns(&mut self) {
        if let Some(ref mut rx) = self.musician_return_rx {
            while let Ok((idx, musician)) = rx.try_recv() {
                self.musicians[idx] = Some(musician);
            }
        }
    }

    /// Periodic tick — update elapsed time, check for stuck musicians,
    /// sync memory, and process queued guidance.
    async fn tick(&mut self) {
        let now = std::time::Instant::now();

        // Update elapsed_ms for all active musicians so timers stay fresh
        for (id, start) in &self.active_start_times {
            if let Some(ms) = self.musician_states.iter_mut().find(|m| m.id == *id) {
                ms.elapsed_ms = now.duration_since(*start).as_millis() as u64;
            }
        }

        let mut stuck_ids = Vec::new();
        for (id, start) in &self.active_start_times {
            if now.duration_since(*start).as_millis() as u64 > MUSICIAN_TIMEOUT_MS {
                stuck_ids.push(id.clone());
            }
        }
        for id in stuck_ids {
            if let Some(slot) = self.musicians.iter_mut().find(|m| {
                m.as_ref().map(|x| x.get_state().id == id).unwrap_or(false)
            }) {
                if let Some(m) = slot.as_mut() {
                    m.reset();
                }
            }
            self.active_start_times.remove(&id);
            // Also remove from active_musicians and abort the handle
            if let Some(handle) = self.active_musicians.remove(&id) {
                handle.abort();
                tracing::warn!(musician = %id, "aborted stuck musician handle");
            }
        }

        // Sync shared memory to running musicians
        self.sync_memory_to_active_musicians().await;

        // Process queued user guidance
        if !self.guidance_queue.is_empty() {
            match self.process_guidance().await {
                Ok(actions) => self.apply_guidance_actions(actions).await,
                Err(e) => {
                    tracing::warn!(error = %e, "guidance processing failed (non-fatal)");
                }
            }
        }
    }

    // ─── Planning ──────────────────────────────────────────────

    async fn do_plan(&mut self) -> Result<(), CoreError> {
        self.conductor_output.clear();

        // Load reference session context
        let mut reference_context: Option<String> = None;
        let ref_id = self
            .config
            .reference_session_id
            .clone()
            .or_else(|| extract_session_reference(&self.config.task_description));
        if let Some(ref id) = ref_id {
            if let Ok(Some(summary)) = TaskStore::load_session_summary(id).await {
                reference_context = Some(summary);
                self.push_conductor_output(&format!("Loaded reference session: {id}"));
            }
        }

        // Pass 1: Explore
        self.set_phase(OrchestraPhase::Exploring);
        self.push_conductor_output("Pass 1/3: Exploring codebase...");
        self.broadcast_state();

        let project_path = self.config.project_path.clone();
        let task_desc = self.config.task_description.clone();
        let ref_ctx = reference_context.clone();

        let explore_result = self.run_conductor_op(move |mut conductor| {
            Box::pin(async move {
                let result = conductor.explore(&project_path, &task_desc, ref_ctx.as_deref()).await;
                (conductor, result)
            })
        }).await;

        match explore_result {
            Ok(map) => {
                self.push_conductor_output("Exploration complete.");
                self.codebase_map = Some(map);
            }
            Err(_) => {
                self.push_conductor_output(
                    "Exploration did not produce structured output (continuing to decomposition)."
                );
            }
        }
        self.broadcast_state();

        // Pass 1.5 (conditional): Deep analysis
        if let Some(ref map) = self.codebase_map {
            if map.analysis_needed == Some(true) {
                if let Some(ref directives) = map.analysis_directives {
                    if !directives.is_empty() {
                        self.do_analysis().await;
                    }
                }
            }
        }

        // Pass 2: Decompose into phases
        self.set_phase(OrchestraPhase::Decomposing);
        let pass_label = if self.analysis_results.is_empty() {
            "Pass 2/3"
        } else {
            "Pass 3/3"
        };
        self.push_conductor_output(&format!("{pass_label}: Decomposing into phases..."));
        self.broadcast_state();

        let task_desc = self.config.task_description.clone();
        let analysis_results = self.analysis_results.clone();

        let decomposition = self.run_conductor_op(move |mut conductor| {
            Box::pin(async move {
                let result = if !analysis_results.is_empty() {
                    conductor.decompose_with_analysis(&task_desc, &analysis_results).await
                } else {
                    conductor.decompose_phases(&task_desc).await
                };
                // On JSON parse failure, retry once with a nudge prompt
                let result = match result {
                    Err(CoreError::JsonParse { .. }) => {
                        tracing::warn!("decomposition JSON parse failed, retrying...");
                        conductor.retry_decompose().await
                    }
                    other => other,
                };
                (conductor, result)
            })
        }).await;

        let decomposition = match decomposition {
            Ok(d) => d,
            Err(CoreError::JsonParse { .. }) => {
                // Claude responded conversationally without a plan even after retry.
                // The response is already visible in conductor output (streamed via emit).
                // Show it cleanly and complete — nothing to execute.
                self.push_conductor_output("");
                self.push_conductor_output("No actionable plan was produced. The task may be too vague or conversational.");
                self.push_conductor_output("Tip: describe a specific coding task, e.g. \"Add retry logic to the API client\"");
                self.set_phase(OrchestraPhase::Complete);
                self.broadcast_state();
                let _ = self.persist_state().await;
                return Ok(());
            }
            Err(e) => {
                self.push_conductor_output(&format!("Decomposition failed: {e}"));
                self.set_phase(OrchestraPhase::Failed);
                self.broadcast_state();
                return Err(e);
            }
        };

        self.phases = decomposition.phases;

        // Guard: empty plan means decomposition produced nothing actionable
        if self.phases.is_empty() {
            self.push_conductor_output("Decomposition produced no phases — nothing to execute.");
            self.push_conductor_output(&format!("Summary: {}", decomposition.summary));
            self.set_phase(OrchestraPhase::Complete);
            self.broadcast_state();
            return Ok(());
        }

        // Build flat task list
        let mut global_index = 0;
        for phase in &mut self.phases {
            for task in &mut phase.tasks {
                task.index = global_index;
                global_index += 1;
                self.tasks.push(task.clone());
            }
        }

        // Build Plan object
        self.plan = Some(Plan {
            summary: decomposition.summary.clone(),
            tasks: self.tasks.clone(),
            dependency_graph: self
                .phases
                .iter()
                .enumerate()
                .map(|(i, p)| format!("Phase {}: {} ({} tasks)", i + 1, p.title, p.tasks.len()))
                .collect::<Vec<_>>()
                .join("\n"),
            musician_assignment: String::new(),
            learning_notes: decomposition.learning_notes.clone(),
            estimated_minutes: decomposition.estimated_minutes,
            insights: decomposition.insights.clone(),
        });

        // Add conductor insights
        if let Some(ref insights) = decomposition.insights {
            for insight in insights {
                self.insight_generator.add_insight(Insight {
                    timestamp: chrono::Utc::now().to_rfc3339(),
                    category: insight.category.clone(),
                    title: insight.title.clone(),
                    body: insight.body.clone(),
                    source: "conductor".into(),
                });
            }
        }
        for note in &decomposition.learning_notes {
            self.insight_generator.add_insight(Insight {
                timestamp: chrono::Utc::now().to_rfc3339(),
                category: InsightCategory::Architecture,
                title: "Project insight".into(),
                body: note.clone(),
                source: "conductor".into(),
            });
        }

        // Pre-flight validation
        if let Some(first_phase) = self.phases.first() {
            if !first_phase.tasks.is_empty() {
                let validation = validate_plan(&first_phase.tasks);
                let messages: Vec<String> = validation
                    .issues
                    .iter()
                    .map(|issue| format!("[{:?}] {}", issue.severity, issue.message))
                    .collect();
                self.plan_validation = Some(validation);
                for msg in &messages {
                    self.push_conductor_output(msg);
                }
            }
        }

        self.task_store.save_tasks(&self.tasks).await?;

        if self.config.dry_run {
            self.set_phase(OrchestraPhase::Complete);
            self.persist_state().await?;
            return Ok(());
        }

        self.set_phase(OrchestraPhase::PlanReview);
        self.persist_state().await?;

        // In headless mode, auto-approve the plan and start execution immediately
        // instead of waiting for a UserAction::ApprovePlan that will never come.
        if self.config.headless {
            tracing::info!("headless mode: auto-approving plan");
            self.start_execution().await?;
        }

        Ok(())
    }

    /// Deep analysis phase — spawn parallel read-only analysts.
    async fn do_analysis(&mut self) {
        let directives = match self.codebase_map {
            Some(ref map) => match map.analysis_directives {
                Some(ref d) => d.clone(),
                None => return,
            },
            None => return,
        };
        if directives.is_empty() {
            return;
        }

        self.set_phase(OrchestraPhase::Analyzing);
        self.push_conductor_output(&format!(
            "Deep analysis: {} analyst(s) investigating...",
            directives.len()
        ));
        self.broadcast_state();

        // Initialize analyst states
        self.analyst_states = directives
            .iter()
            .enumerate()
            .map(|(i, d)| AnalystState {
                id: format!("analyst-{i}"),
                index: i,
                status: MusicianStatus::Running,
                directive: Some(d.clone()),
                output_lines: Vec::new(),
                started_at: Some(chrono::Utc::now().to_rfc3339()),
                elapsed_ms: 0,
            })
            .collect();

        // Create analyst musicians — run read-only in project directory
        let event_tx = self.event_tx.clone();
        let mut handles = Vec::new();

        for (i, directive) in directives.iter().enumerate() {
            let mut analyst = Musician::new(
                format!("analyst-{i}"),
                i,
                self.config.conductor_model.clone(),
                directive.estimated_turns,
            );

            let task = Task {
                id: directive.id.clone(),
                index: i,
                title: format!("Analyze: {}", directive.area),
                description: directive.question.clone(),
                why: directive.question.clone(),
                file_scope: directive.file_hints.clone(),
                dependencies: Vec::new(),
                acceptance_criteria: vec!["Produce JSON findings".into()],
                estimated_turns: directive.estimated_turns,
                model: None,
                status: TaskStatus::InProgress,
                assigned_musician: Some(format!("analyst-{i}")),
                result: None,
            };

            let etx = event_tx.clone();
            let project_path = self.config.project_path.clone();
            let (_gtx, guidance_rx) = mpsc::channel::<String>(1);
            let handle = tokio::spawn(async move {
                analyst
                    .execute(task, &project_path, "main", &project_path, etx, true, None, guidance_rx)
                    .await
            });
            handles.push(handle);
        }

        // Collect results
        for (i, handle) in handles.into_iter().enumerate() {
            match handle.await {
                Ok(result) if result.success => {
                    let area = directives[i].area.clone();
                    self.analysis_results.push(AnalysisResult {
                        directive_id: directives[i].id.clone(),
                        area,
                        findings: result.summary.clone(),
                        key_files: Vec::new(),
                        patterns: Vec::new(),
                        risks: Vec::new(),
                        duration_ms: result.duration_ms,
                    });
                    if i < self.analyst_states.len() {
                        self.analyst_states[i].status = MusicianStatus::Completed;
                    }
                }
                Ok(result) => {
                    self.push_conductor_output(&format!(
                        "Analyst {i} ({}) failed: {}",
                        directives[i].area,
                        result.error.as_deref().unwrap_or("unknown")
                    ));
                    if i < self.analyst_states.len() {
                        self.analyst_states[i].status = MusicianStatus::Failed;
                    }
                }
                Err(e) => {
                    self.push_conductor_output(&format!(
                        "Analyst {i} ({}) panicked: {e}",
                        directives[i].area
                    ));
                    if i < self.analyst_states.len() {
                        self.analyst_states[i].status = MusicianStatus::Failed;
                    }
                }
            }
        }

        self.push_conductor_output(&format!(
            "Analysis complete: {}/{} analysts succeeded.",
            self.analysis_results.len(),
            directives.len()
        ));
        self.broadcast_state();
    }

    // ─── Plan Refinement ───────────────────────────────────────

    async fn replan(
        &mut self,
        feedback: &str,
        images: Option<&[String]>,
    ) -> Result<(), CoreError> {
        if self.phase != OrchestraPhase::PlanReview || self.plan.is_none() {
            return Ok(());
        }

        self.refinement_history.push(PlanRefinementMessage {
            role: RefinementRole::User,
            text: feedback.to_string(),
            images: images.map(|i| i.to_vec()),
            timestamp: chrono::Utc::now().to_rfc3339(),
        });

        self.set_phase(OrchestraPhase::Planning);
        self.conductor_output.clear();

        let feedback_owned = feedback.to_string();
        let images_owned = images.map(|i| i.to_vec());

        let result = self.run_conductor_op(move |mut conductor| {
            Box::pin(async move {
                let result = conductor.refine_plan(&feedback_owned, images_owned.as_deref()).await;
                (conductor, result)
            })
        }).await;

        let (plan, explanation) = match result {
            Ok(v) => v,
            Err(e) => {
                self.set_phase(OrchestraPhase::PlanReview);
                return Err(e);
            }
        };

        self.refinement_history.push(PlanRefinementMessage {
            role: RefinementRole::Conductor,
            text: explanation,
            images: None,
            timestamp: chrono::Utc::now().to_rfc3339(),
        });

        self.plan = Some(plan.clone());
        self.tasks = plan.tasks;
        self.plan_validation = Some(validate_plan(&self.tasks));

        self.task_store.save_tasks(&self.tasks).await?;
        self.persist_state().await?;
        self.set_phase(OrchestraPhase::PlanReview);
        Ok(())
    }

    // ─── Execution ─────────────────────────────────────────────

    fn start_execution(&mut self) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), CoreError>> + '_>> {
        Box::pin(self.start_execution_inner())
    }

    async fn start_execution_inner(&mut self) -> Result<(), CoreError> {
        if self.config.musician_count < 1 {
            return Err(CoreError::Channel(format!(
                "Invalid musicianCount: {}",
                self.config.musician_count
            )));
        }

        self.ensure_musicians(self.config.musician_count);

        if !self.phases.is_empty() {
            self.execute_phase_loop().await
        } else {
            self.set_phase(OrchestraPhase::Executing);
            self.execute_loop().await
        }
    }

    /// Create musician instances up to the specified count.
    fn ensure_musicians(&mut self, count: usize) {
        if self.musicians.len() >= count {
            return;
        }

        let start_index = self.musicians.len();
        for i in start_index..count {
            let musician_id = format!("m{}", i + 1);
            let musician = Musician::new(
                musician_id,
                i,
                self.config.musician_model.clone(),
                self.config.max_turns,
            );
            self.musician_states.push(musician.get_state());
            self.musicians.push(Some(musician));
        }
    }

    /// Core execution loop — assigns tasks to idle musicians and waits for completion.
    ///
    /// Uses `select!` to process musician events, user actions, and periodic ticks
    /// concurrently so that user guidance reaches running musicians in real time.
    async fn execute_loop(&mut self) -> Result<(), CoreError> {
        let mut event_rx = self
            .event_rx
            .take()
            .ok_or_else(|| CoreError::Channel("event_rx already taken".into()))?;
        let mut action_rx = self
            .action_rx
            .take()
            .ok_or_else(|| CoreError::Channel("action_rx already taken".into()))?;
        let mut tick = tokio::time::interval(tokio::time::Duration::from_millis(500));
        let mut needs_assignment = true;

        loop {
            // Phase check
            if self.phase != OrchestraPhase::Executing
                && self.phase != OrchestraPhase::PhaseExecuting
            {
                break;
            }

            // Assign tasks to idle musicians when needed
            if needs_assignment {
                self.update_task_readiness();

                if !self.has_remaining_tasks() && self.active_musicians.is_empty() {
                    break;
                }

                self.try_assign_tasks().await;
                needs_assignment = false;
            }

            // If nothing is running and no tasks remain, we're done
            if self.active_musicians.is_empty() && !self.has_remaining_tasks() {
                break;
            }

            // If nothing is running but tasks are blocked, fail
            if self.active_musicians.is_empty() {
                self.set_phase(OrchestraPhase::Failed);
                self.event_rx = Some(event_rx);
                self.action_rx = Some(action_rx);
                return Err(CoreError::Channel(
                    "All remaining tasks are blocked".into(),
                ));
            }

            // Wait for events, user actions, or tick
            tokio::select! {
                Some(event) = event_rx.recv() => {
                    self.handle_event(event).await;
                    // Drain any additional pending events before top-of-loop checks.
                    // This ensures paired events (e.g. MusicianComplete followed by
                    // MusicianStatusChange) are processed together, preventing the
                    // stuck-state check from firing between them.
                    while let Ok(event) = event_rx.try_recv() {
                        self.handle_event(event).await;
                    }

                    // Await finished musician handles so their return_tx.send()
                    // completes before we drain. Without this, the musician slot
                    // stays None and try_assign_tasks can't assign second-wave tasks.
                    let finished: Vec<String> = self.active_musicians.iter()
                        .filter(|(_, h)| h.is_finished())
                        .map(|(id, _)| id.clone())
                        .collect();
                    for id in &finished {
                        if let Some(handle) = self.active_musicians.remove(id) {
                            if let Err(e) = handle.await {
                                tracing::error!(musician = %id, error = %e, "musician task panicked");
                            }
                        }
                    }

                    self.drain_musician_returns();
                    self.sync_task_results().await;
                    needs_assignment = true;
                    self.broadcast_state();
                }
                Some(action) = action_rx.recv() => {
                    self.handle_execution_action(action).await;
                    self.broadcast_state();
                }
                _ = tick.tick() => {
                    self.tick().await;
                    // Check for completed musicians
                    let mut completed = Vec::new();
                    for (id, handle) in &self.active_musicians {
                        if handle.is_finished() {
                            completed.push(id.clone());
                        }
                    }
                    for id in completed {
                        if let Some(handle) = self.active_musicians.remove(&id) {
                            if let Err(e) = handle.await {
                                tracing::error!(musician = %id, error = %e, "musician task panicked");
                            }
                        }
                        self.guidance_channels.remove(&id);
                        self.active_start_times.remove(&id);
                    }
                    self.drain_musician_returns();
                    self.sync_task_results().await;
                    needs_assignment = true;
                    self.broadcast_state();
                }
            }
        }

        // Wait for all remaining musicians
        let handles: Vec<_> = self.active_musicians.drain().map(|(id, h)| (id, h)).collect();
        for (id, handle) in handles {
            if let Err(e) = handle.await {
                tracing::error!(musician = %id, error = %e, "musician task panicked");
            }
        }
        self.active_start_times.clear();
        self.guidance_channels.clear();

        // Drain final events and returned musicians
        while let Ok(event) = event_rx.try_recv() {
            self.handle_event(event).await;
        }
        self.drain_musician_returns();
        self.sync_task_results().await;

        // Return channels
        self.event_rx = Some(event_rx);
        self.action_rx = Some(action_rx);

        Ok(())
    }

    /// Handle user actions during execution (guidance, quit).
    async fn handle_execution_action(&mut self, action: UserAction) {
        match action {
            UserAction::SubmitGuidance { text, images: _ } => {
                self.queue_guidance(&text).await;
                // Send directly to all running musicians via guidance channels
                for tx in self.guidance_channels.values() {
                    let _ = tx.try_send(text.clone());
                }
            }
            UserAction::Quit => {
                self.shutdown().await;
                self.set_phase(OrchestraPhase::Complete);
            }
            _ => {
                // Ignore ApprovePlan, RefinePlan, SubmitTask during execution
            }
        }
    }

    /// Try to assign ready tasks to idle musicians.
    async fn try_assign_tasks(&mut self) {
        // Find idle musicians and assign tasks
        let idle_indices: Vec<usize> = self.musician_states.iter().enumerate()
            .filter(|(idx, ms)| {
                (ms.status == MusicianStatus::Idle || ms.status == MusicianStatus::Completed)
                    && self.musicians.get(*idx).map(|m| m.is_some()).unwrap_or(false)
            })
            .map(|(idx, _)| idx)
            .collect();
        let mut assignments: Vec<(usize, Task)> = Vec::new();
        for idx in idle_indices {
            if let Some(task) = self.get_next_ready_task() {
                assignments.push((idx, task));
            }
        }

        if assignments.is_empty() {
            return;
        }

        let use_worktrees = self.worktree_manager.is_git_repo().await;

        for (idx, mut task) in assignments {
            task.status = TaskStatus::InProgress;
            task.assigned_musician = Some(self.musicians[idx].as_ref().unwrap().get_state().id.clone());

            // Update task in our list
            if let Some(t) = self.tasks.iter_mut().find(|t| t.index == task.index) {
                t.status = TaskStatus::InProgress;
                t.assigned_musician = task.assigned_musician.clone();
            }

            let slug = task
                .title
                .to_lowercase()
                .chars()
                .map(|c| if c.is_alphanumeric() { c } else { '-' })
                .collect::<String>();
            let slug = &slug[..slug.len().min(30)];

            let (worktree_path, branch) = if use_worktrees {
                let musician_id = self.musicians[idx].as_ref().unwrap().get_state().id.clone();
                match self.worktree_manager.create(&musician_id, slug).await {
                    Ok((path, branch)) => (path.to_string_lossy().to_string(), branch),
                    Err(e) => {
                        tracing::error!(error = %e, "failed to create worktree");
                        (self.config.project_path.clone(), "main".into())
                    }
                }
            } else {
                (self.config.project_path.clone(), "main".into())
            };

            // Smart model routing
            if let Some(ref model) = task.model {
                self.musicians[idx].as_mut().unwrap().set_model_override(model);
            }

            // Track worktree snapshot
            if use_worktrees {
                self.worktree_snapshots.push(WorktreeSnapshot {
                    worker_id: self.musicians[idx].as_ref().unwrap().get_state().id.clone(),
                    task_index: task.index,
                    branch: branch.clone(),
                    path: worktree_path.clone(),
                    last_commit_sha: String::new(),
                    status: WorktreeStatus::Active,
                });
            }

            // Read shared memory for the musician
            let shared_mem = self.memory.read().await.ok().filter(|s| !s.is_empty());

            // Generate assignment insight
            self.insight_generator
                .on_task_assigned(&self.musicians[idx].as_ref().unwrap().get_state().id, &task);

            let event_tx = self.event_tx.clone();
            let project_path = self.config.project_path.clone();
            let musician_id = self.musicians[idx].as_ref().unwrap().get_state().id.clone();
            let task_clone = task.clone();
            let wt_path = worktree_path.clone();
            let br = branch.clone();

            // Create guidance channel for this musician
            let (guidance_tx, guidance_rx) = mpsc::channel::<String>(16);
            self.guidance_channels.insert(musician_id.clone(), guidance_tx);

            // Take the musician out of the Vec for the spawned task.
            // It will be returned via musician_return_tx after execution.
            let mut musician = self.musicians[idx].take()
                .expect("musician should be present when assigned");
            let return_tx = self.musician_return_tx.clone();

            let handle = tokio::spawn(async move {
                let result = musician
                    .execute(
                        task_clone,
                        &wt_path,
                        &br,
                        &project_path,
                        event_tx,
                        false,
                        shared_mem,
                        guidance_rx,
                    )
                    .await;
                // Result is already sent via MusicianComplete event in execute()
                let _ = result;
                // Return musician to orchestra
                let _ = return_tx.send((idx, musician)).await;
            });

            self.active_musicians.insert(musician_id.clone(), handle);
            self.active_start_times
                .insert(musician_id, std::time::Instant::now());
        }

        self.broadcast_state();
    }

    /// Sync task results from musician completion events and run verification.
    async fn sync_task_results(&mut self) {
        let mut newly_completed: Vec<(usize, Option<String>)> = Vec::new();
        let mut phase_syncs: Vec<(usize, TaskStatus, Option<TaskResult>)> = Vec::new();

        for ms in &self.musician_states {
            if ms.status == MusicianStatus::Completed || ms.status == MusicianStatus::Failed {
                if let Some(ref task) = ms.current_task {
                    if let Some(t) = self.tasks.iter_mut().find(|t| t.index == task.index) {
                        if t.status == TaskStatus::InProgress {
                            t.status = if ms.status == MusicianStatus::Completed {
                                TaskStatus::Completed
                            } else {
                                TaskStatus::Failed
                            };
                            phase_syncs.push((t.index, t.status.clone(), t.result.clone()));
                            // Track newly completed tasks for verification
                            if t.status == TaskStatus::Completed {
                                if let Some(snap) = self
                                    .worktree_snapshots
                                    .iter()
                                    .find(|s| s.task_index == t.index)
                                {
                                    newly_completed
                                        .push((t.index, Some(snap.path.clone())));
                                } else {
                                    // No worktree — still track for verification marking
                                    newly_completed.push((t.index, None));
                                }
                            }
                        }
                    }
                }
            }
        }

        // Sync status changes to phase tasks
        for (idx, status, result) in phase_syncs {
            self.sync_task_to_phase(idx, status, result);
        }

        // Run verification on newly completed tasks — collect results first to avoid borrow conflicts
        let mut verification_results: Vec<(usize, Option<(String, String, bool)>)> = Vec::new();
        for (task_index, worktree_path) in &newly_completed {
            if let Some(worktree) = worktree_path {
                let (diff, verification_output, passed) =
                    self.run_verification(worktree).await;
                verification_results.push((*task_index, Some((diff, verification_output, passed))));
            } else {
                verification_results.push((*task_index, None));
            }
        }

        // Apply verification results to tasks
        let mut post_verify_syncs: Vec<(usize, TaskStatus, Option<TaskResult>)> = Vec::new();
        for (task_index, result) in verification_results {
            if let Some(t) = self.tasks.iter_mut().find(|t| t.index == task_index) {
                match result {
                    Some((diff, verification_output, passed)) => {
                        if let Some(ref mut r) = t.result {
                            r.diff = Some(diff);
                            r.verification_output = Some(verification_output);
                            r.verification_passed = Some(passed);
                        }
                    }
                    None => {
                        tracing::warn!(
                            task_index,
                            title = %t.title,
                            "task completed without a worktree — verification skipped"
                        );
                        if let Some(ref mut r) = t.result {
                            r.verification_output =
                                Some("Skipped: no worktree available".into());
                            r.verification_passed = None;
                        }
                    }
                }
                post_verify_syncs.push((task_index, t.status.clone(), t.result.clone()));
            }
        }
        // Sync verification results to phase tasks
        for (idx, status, result) in post_verify_syncs {
            self.sync_task_to_phase(idx, status, result);
        }

        let _ = self.task_store.save_tasks(&self.tasks).await;
        let _ = self.persist_state().await;
    }

    /// Sync a task's status and result back into `self.phases[*].tasks`.
    ///
    /// `self.tasks` is the authoritative flat list, but phase review and
    /// broadcast also read from `self.phases[pi].tasks`. Without this sync
    /// the phase copies stay stale after `sync_task_results` updates them.
    fn sync_task_to_phase(
        &mut self,
        task_index: usize,
        status: TaskStatus,
        result: Option<TaskResult>,
    ) {
        for phase in &mut self.phases {
            if let Some(t) = phase.tasks.iter_mut().find(|t| t.index == task_index) {
                t.status = status;
                t.result = result;
                break;
            }
        }
    }

    // ─── V2: Phase-Based Execution ─────────────────────────────

    async fn execute_phase_loop(&mut self) -> Result<(), CoreError> {
        let mut completed_phases: Vec<Phase> = Vec::new();
        let mut phase_retry_count: HashMap<usize, u32> = HashMap::new();

        // On resume, collect already-completed phases and start from saved index
        let start_index = self.current_phase_index.unwrap_or(0);
        for i in 0..start_index.min(self.phases.len()) {
            if self.phases[i].status == PhaseStatus::Completed {
                completed_phases.push(self.phases[i].clone());
            }
        }

        let mut pi = start_index;
        while pi < self.phases.len() {
            if self.phase == OrchestraPhase::Failed {
                return Ok(());
            }

            // Skip already-completed phases (from previous session)
            if self.phases[pi].status == PhaseStatus::Completed {
                completed_phases.push(self.phases[pi].clone());
                pi += 1;
                continue;
            }

            self.current_phase_index = Some(pi);
            self.phases[pi].status = PhaseStatus::Active;
            self.broadcast_state();

            // 1. Detail phase tasks if empty
            if self.phases[pi].tasks.is_empty() {
                self.set_phase(OrchestraPhase::PhaseDetailing);
                let title = self.phases[pi].title.clone();
                self.push_conductor_output(&format!(
                    "Detailing Phase {}: {}...",
                    pi + 1,
                    title
                ));
                self.broadcast_state();

                let phase_ref = self.phases[pi].clone();
                let completed_ref = completed_phases.clone();
                let detail_result = self.run_conductor_op(move |mut conductor| {
                    Box::pin(async move {
                        let result = conductor.detail_phase(&phase_ref, &completed_ref).await;
                        // On recoverable failure, retry once
                        let result = match result {
                            Err(CoreError::JsonParse { .. }) => {
                                tracing::warn!("phase detailing JSON parse failed, retrying...");
                                conductor.retry_detail_phase().await
                            }
                            Err(CoreError::Bridge(_) | CoreError::Channel(_) | CoreError::Timeout(_)) => {
                                tracing::warn!("phase detailing session error, retrying with fresh session...");
                                conductor.detail_phase(&phase_ref, &completed_ref).await
                            }
                            other => other,
                        };
                        (conductor, result)
                    })
                }).await;

                let detailed_tasks = match detail_result {
                    Ok(tasks) => tasks,
                    Err(CoreError::JsonParse { reason, raw_output }) => {
                        self.push_conductor_output(&format!(
                            "Phase {} detailing failed after retry: {reason}",
                            pi + 1
                        ));
                        if !raw_output.is_empty() {
                            self.push_conductor_output("--- Claude's response ---");
                            self.push_conductor_output(&raw_output);
                            self.push_conductor_output("--- end response ---");
                        }
                        self.phases[pi].status = PhaseStatus::Failed;
                        self.set_phase(OrchestraPhase::Failed);
                        self.broadcast_state();
                        let _ = self.persist_state().await;
                        return Err(CoreError::JsonParse { reason, raw_output });
                    }
                    Err(e) => {
                        self.push_conductor_output(&format!(
                            "Phase {} detailing failed: {e}",
                            pi + 1
                        ));
                        self.phases[pi].status = PhaseStatus::Failed;
                        self.set_phase(OrchestraPhase::Failed);
                        self.broadcast_state();
                        let _ = self.persist_state().await;
                        return Err(e);
                    }
                };

                let total_phase_tasks = detailed_tasks.len();
                let base_index = self.tasks.len();
                for (ti, mut task) in detailed_tasks.into_iter().enumerate() {
                    task.index = base_index + ti;
                    // Guard: filter out-of-phase and self-referential dependencies
                    task.dependencies.retain(|&dep| dep < total_phase_tasks && dep != ti);
                    self.tasks.push(task.clone());
                    self.phases[pi].tasks.push(task);
                }

                let validation = validate_plan(&self.phases[pi].tasks);
                for issue in &validation.issues {
                    self.push_conductor_output(&format!("[{:?}] {}", issue.severity, issue.message));
                }
                self.broadcast_state();

                let _ = self.task_store.save_tasks(&self.tasks).await;
            }

            // 2. Execute tasks within this phase
            self.ensure_musicians(self.config.musician_count);
            self.set_phase(OrchestraPhase::PhaseExecuting);

            // Scope to this phase's tasks
            let phase_task_indices: Vec<usize> =
                self.phases[pi].tasks.iter().map(|t| t.index).collect();
            self.current_phase_tasks = Some(phase_task_indices);
            self.update_task_readiness();
            self.execute_loop().await?;
            self.current_phase_tasks = None;

            // 3. Phase merging checkpoint
            self.set_phase(OrchestraPhase::PhaseMerging);

            // 4. Review this phase
            self.set_phase(OrchestraPhase::PhaseReviewing);

            let mut phase_diffs = HashMap::new();
            for task in &self.phases[pi].tasks {
                if let Some(ref result) = task.result {
                    if let Some(ref diff) = result.diff {
                        phase_diffs.insert(task.index, diff.clone());
                    }
                }
            }

            let phase_ref = self.phases[pi].clone();
            let all_phases = self.phases.clone();
            let retry_count = *phase_retry_count.get(&pi).unwrap_or(&0);

            let review_result = self.run_conductor_op(move |mut conductor| {
                Box::pin(async move {
                    let result = conductor.review_phase(&phase_ref, &all_phases, &phase_diffs, retry_count).await;
                    (conductor, result)
                })
            }).await;

            let review = match review_result {
                Ok(r) => r,
                Err(CoreError::JsonParse { reason, raw_output }) => {
                    self.push_conductor_output(&format!(
                        "Phase {} review parse failed (continuing): {reason}",
                        pi + 1
                    ));
                    if !raw_output.is_empty() {
                        self.push_conductor_output("--- Claude's response ---");
                        self.push_conductor_output(&raw_output);
                        self.push_conductor_output("--- end response ---");
                    }
                    // Default to continue on review parse failure
                    conductor_types::PhaseReviewResult {
                        action: PhaseReviewAction::Continue,
                        summary: format!("Review parse failed, defaulting to continue: {reason}"),
                        task_indices: None,
                        revised_phases: None,
                    }
                }
                Err(e) => {
                    self.push_conductor_output(&format!(
                        "Phase {} review failed (continuing): {e}",
                        pi + 1
                    ));
                    conductor_types::PhaseReviewResult {
                        action: PhaseReviewAction::Continue,
                        summary: format!("Review failed, defaulting to continue: {e}"),
                        task_indices: None,
                        revised_phases: None,
                    }
                }
            };

            self.phases[pi].review_result = Some(review.clone());

            self.insight_generator.add_insight(Insight {
                timestamp: chrono::Utc::now().to_rfc3339(),
                category: InsightCategory::Decision,
                title: format!("Phase {} review: {:?}", pi + 1, review.action),
                body: review.summary.clone(),
                source: "conductor".into(),
            });

            match review.action {
                PhaseReviewAction::Continue => {
                    self.phases[pi].status = PhaseStatus::Completed;
                    completed_phases.push(self.phases[pi].clone());
                }
                PhaseReviewAction::RetryTasks => {
                    let retries = phase_retry_count.entry(pi).or_insert(0);
                    *retries += 1;
                    if *retries > MAX_PHASE_RETRIES {
                        self.push_conductor_output(&format!(
                            "[WARN] Phase {} exceeded max retries ({MAX_PHASE_RETRIES}) — forcing continue",
                            pi + 1
                        ));
                        self.phases[pi].status = PhaseStatus::Completed;
                        completed_phases.push(self.phases[pi].clone());
                    } else {
                        // Reset specified tasks or all non-completed tasks.
                        // Guard: never reset tasks that completed successfully —
                        // the conductor LLM sometimes requests retrying tasks that
                        // actually passed, especially when diffs are missing.
                        let indices_to_retry: Vec<usize> = review
                            .task_indices
                            .unwrap_or_else(|| {
                                self.phases[pi]
                                    .tasks
                                    .iter()
                                    .enumerate()
                                    .filter(|(_, t)| t.status != TaskStatus::Completed)
                                    .map(|(i, _)| i)
                                    .collect()
                            });
                        let mut actually_retried = 0;
                        for idx in indices_to_retry {
                            if let Some(task) = self.phases[pi].tasks.get_mut(idx) {
                                // Skip successfully completed tasks — don't undo good work
                                if task.status == TaskStatus::Completed {
                                    if let Some(ref r) = task.result {
                                        if r.success {
                                            tracing::info!(
                                                task_index = task.index,
                                                title = %task.title,
                                                "skipping retry of successfully completed task"
                                            );
                                            continue;
                                        }
                                    }
                                }
                                task.status = TaskStatus::Queued;
                                task.result = None;
                                actually_retried += 1;
                                // Also update in global tasks
                                if let Some(t) =
                                    self.tasks.iter_mut().find(|t| t.index == task.index)
                                {
                                    t.status = TaskStatus::Queued;
                                    t.result = None;
                                }
                            }
                        }
                        // If no tasks actually need retrying, skip the retry
                        if actually_retried == 0 {
                            self.push_conductor_output(&format!(
                                "Phase {} review requested retry but all tasks succeeded — continuing",
                                pi + 1
                            ));
                            self.phases[pi].status = PhaseStatus::Completed;
                            completed_phases.push(self.phases[pi].clone());
                            pi += 1;
                            continue;
                        }
                        // Retry this phase
                        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                        continue; // Don't increment pi
                    }
                }
                PhaseReviewAction::ReviseRemainingPhases => {
                    self.phases[pi].status = PhaseStatus::Completed;
                    completed_phases.push(self.phases[pi].clone());

                    if let Some(revised) = review.revised_phases {
                        if !revised.is_empty() {
                            let remaining: Vec<Phase> = revised
                                .into_iter()
                                .enumerate()
                                .map(|(ri, p)| Phase {
                                    index: pi + 1 + ri,
                                    ..p
                                })
                                .collect();
                            self.phases.truncate(pi + 1);
                            self.phases.extend(remaining);
                            self.push_conductor_output(&format!(
                                "Revised remaining phases based on Phase {} results",
                                pi + 1
                            ));
                        }
                    }
                }
                PhaseReviewAction::Abort => {
                    self.phases[pi].status = PhaseStatus::Failed;
                    self.set_phase(OrchestraPhase::Failed);
                    self.persist_state().await?;
                    return Err(CoreError::Channel(format!(
                        "Phase {} aborted: {}",
                        pi + 1,
                        review.summary
                    )));
                }
            }

            self.persist_state().await?;
            pi += 1;
        }

        // All phases complete — final review
        self.do_final_review().await
    }

    // ─── Review ────────────────────────────────────────────────

    async fn do_final_review(&mut self) -> Result<(), CoreError> {
        self.set_phase(OrchestraPhase::FinalReview);
        self.do_review().await
    }

    async fn do_review(&mut self) -> Result<(), CoreError> {
        if self.phase != OrchestraPhase::FinalReview {
            self.set_phase(OrchestraPhase::Reviewing);
        }

        let task_results = self
            .tasks
            .iter()
            .map(|t| {
                let r = t.result.as_ref();
                let verify = match r.and_then(|r| r.verification_passed) {
                    Some(true) => "PASS",
                    Some(false) => "FAIL",
                    None => "N/A",
                };
                format!(
                    "Task {}: {}\nStatus: {:?}\nFiles: {}\nVerification: {}\nSummary: {}",
                    t.index + 1,
                    t.title,
                    t.status,
                    r.map(|r| r.files_modified.join(", "))
                        .unwrap_or_else(|| "none".into()),
                    verify,
                    r.map(|r| r.summary.as_str()).unwrap_or("N/A"),
                )
            })
            .collect::<Vec<_>>()
            .join("\n\n");

        let mut diffs = HashMap::new();
        let mut verification_results = HashMap::new();
        for task in &self.tasks {
            if let Some(ref r) = task.result {
                if let Some(ref d) = r.diff {
                    diffs.insert(task.index, d.clone());
                }
                if let Some(ref v) = r.verification_output {
                    verification_results.insert(task.index, v.clone());
                }
            }
        }

        let shared_mem = self.memory.read().await.unwrap_or_default();
        let input = ReviewInput {
            task_results,
            shared_memory: shared_mem,
            diffs: Some(diffs),
            verification_results: Some(verification_results),
        };

        let review_result = self.run_conductor_op(move |mut conductor| {
            Box::pin(async move {
                let result = conductor.review(&input).await;
                (conductor, result)
            })
        }).await;

        let review = match review_result {
            Ok(r) => r,
            Err(CoreError::JsonParse { reason, raw_output }) => {
                self.push_conductor_output(&format!("Final review parse failed: {reason}"));
                if !raw_output.is_empty() {
                    self.push_conductor_output("--- Claude's response ---");
                    self.push_conductor_output(&raw_output);
                    self.push_conductor_output("--- end response ---");
                }
                // Complete anyway — work is done, only review parsing failed
                self.set_phase(OrchestraPhase::Complete);
                self.broadcast_state();
                return Ok(());
            }
            Err(e) => {
                self.push_conductor_output(&format!("Final review failed: {e}"));
                self.set_phase(OrchestraPhase::Complete);
                self.broadcast_state();
                return Ok(());
            }
        };

        self.insight_generator.on_conductor_decision(
            &format!("Review: {}", review.action),
            review.summary.as_deref().unwrap_or("Review completed"),
        );

        match review.action.as_str() {
            "complete" => {
                self.set_phase(OrchestraPhase::Integrating);
                self.worktree_manager.cleanup_all().await;
                self.set_phase(OrchestraPhase::Complete);
                self.persist_state().await?;
                self.caffeinate.stop().await;
            }
            "retry" => {
                self.review_retry_count += 1;
                if self.review_retry_count > MAX_REVIEW_RETRIES {
                    self.push_conductor_output(&format!(
                        "[WARN] Final review exceeded max retries ({MAX_REVIEW_RETRIES}) — completing"
                    ));
                    self.set_phase(OrchestraPhase::Complete);
                    self.persist_state().await?;
                    self.caffeinate.stop().await;
                } else if let Some(indices) = review.task_indices {
                    for idx in indices {
                        if let Some(task) = self.tasks.get_mut(idx) {
                            task.status = TaskStatus::Queued;
                            task.result = None;
                            if let Some(ref instructions) = review.adjusted_instructions {
                                task.description
                                    .push_str(&format!("\n\n[Conductor retry instructions]: {instructions}"));
                            }
                        }
                    }
                    self.start_execution().await?;
                }
            }
            "extend" => {
                if let Some(new_tasks_json) = review.new_tasks {
                    let base_index = self.tasks.len();
                    for (i, t) in new_tasks_json.iter().enumerate() {
                        let new_task = Task {
                            id: Uuid::new_v4().to_string(),
                            index: base_index + i,
                            title: t
                                .get("title")
                                .and_then(|v| v.as_str())
                                .unwrap_or("Extended Task")
                                .to_string(),
                            description: t
                                .get("description")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string(),
                            why: t
                                .get("why")
                                .and_then(|v| v.as_str())
                                .unwrap_or("Added during review")
                                .to_string(),
                            file_scope: t
                                .get("fileScope")
                                .and_then(|v| v.as_array())
                                .map(|a| {
                                    a.iter()
                                        .filter_map(|v| v.as_str().map(String::from))
                                        .collect()
                                })
                                .unwrap_or_default(),
                            dependencies: Vec::new(),
                            acceptance_criteria: Vec::new(),
                            estimated_turns: t
                                .get("estimatedTurns")
                                .and_then(|v| v.as_u64())
                                .unwrap_or(15) as u32,
                            model: t.get("model").and_then(|v| v.as_str()).map(String::from),
                            status: TaskStatus::Queued,
                            assigned_musician: None,
                            result: None,
                        };
                        self.tasks.push(new_task);
                    }
                    self.update_task_readiness();
                    let _ = self.task_store.save_tasks(&self.tasks).await;
                    self.start_execution().await?;
                } else {
                    self.set_phase(OrchestraPhase::Complete);
                    self.persist_state().await?;
                    self.caffeinate.stop().await;
                }
            }
            _ => {
                self.set_phase(OrchestraPhase::Complete);
                self.persist_state().await?;
                self.caffeinate.stop().await;
            }
        }

        Ok(())
    }

    // ─── Guidance ──────────────────────────────────────────────

    async fn queue_guidance(&mut self, message: &str) {
        let timestamp = chrono::Utc::now().to_rfc3339();
        self.guidance_queue.push(GuidanceMessage {
            message: message.to_string(),
            timestamp: timestamp.clone(),
        });
        let _ = self
            .memory
            .append("USER GUIDANCE", &format!("[{timestamp}] {message}"))
            .await;
        self.broadcast_state();
    }

    async fn process_guidance(&mut self) -> Result<GuidanceActions, CoreError> {
        let messages: Vec<GuidanceMessage> = self.guidance_queue.drain(..).collect();
        let current_status = self
            .tasks
            .iter()
            .map(|t| {
                format!(
                    "Task {}: \"{}\" — {:?}{}",
                    t.index,
                    t.title,
                    t.status,
                    t.assigned_musician
                        .as_ref()
                        .map(|m| format!(" ({m})"))
                        .unwrap_or_default()
                )
            })
            .collect::<Vec<_>>()
            .join("\n");

        let shared_mem = self
            .memory
            .read_truncated(2000)
            .await
            .unwrap_or_default();

        let input = GuidanceInput {
            user_messages: messages,
            task_status: current_status,
            shared_memory: shared_mem,
        };

        self.conductor.as_mut().unwrap().review_guidance(&input).await
    }

    async fn apply_guidance_actions(&mut self, actions: GuidanceActions) {
        // Cancel pending tasks
        if let Some(ref cancel_indices) = actions.cancel_tasks {
            for &idx in cancel_indices {
                if let Some(task) = self.tasks.get_mut(idx) {
                    if task.status == TaskStatus::Queued || task.status == TaskStatus::Ready {
                        task.status = TaskStatus::Cancelled;
                        task.result = Some(TaskResult {
                            success: false,
                            files_modified: Vec::new(),
                            summary: "Cancelled by user guidance".into(),
                            error: None,
                            duration_ms: 0,
                            diff: None,
                            verification_output: None,
                            verification_passed: None,
                        });
                    }
                }
            }
        }

        // Add new tasks
        if let Some(ref new_tasks) = actions.add_tasks {
            let base_index = self.tasks.len();
            for (i, spec) in new_tasks.iter().enumerate() {
                let task = Task {
                    id: Uuid::new_v4().to_string(),
                    index: base_index + i,
                    title: spec.title.clone(),
                    description: spec.description.clone(),
                    why: spec.why.clone(),
                    file_scope: spec.file_scope.clone(),
                    dependencies: spec.dependencies.clone(),
                    acceptance_criteria: spec.acceptance_criteria.clone(),
                    estimated_turns: spec.estimated_turns,
                    model: spec.model.clone(),
                    status: TaskStatus::Queued,
                    assigned_musician: None,
                    result: None,
                };
                self.tasks.push(task);
            }
            self.update_task_readiness();
        }

        // Modify pending tasks
        if let Some(ref modifications) = actions.modify_tasks {
            for m in modifications {
                if let Some(task) = self.tasks.get_mut(m.index) {
                    if task.status == TaskStatus::Queued || task.status == TaskStatus::Ready {
                        task.description
                            .push_str(&format!("\n\n[USER GUIDANCE]: {}", m.new_description));
                    }
                }
            }
        }

        // Append general guidance to shared memory
        if let Some(ref guidance) = actions.guidance {
            let _ = self
                .memory
                .append("CONDUCTOR GUIDANCE (from user prompt)", guidance)
                .await;
        }

        let _ = self.task_store.save_tasks(&self.tasks).await;
    }

    // ─── Memory Sync ───────────────────────────────────────────

    async fn sync_memory_to_active_musicians(&mut self) {
        let result = self
            .memory
            .get_entries_since(self.last_memory_sync_offset)
            .await;
        if let Ok((content, new_offset)) = result {
            if content.trim().is_empty() {
                return;
            }
            self.last_memory_sync_offset = new_offset;
            let msg = format!(
                "[MEMORY UPDATE from other musicians]:\n{}",
                &content[..content.len().min(2000)]
            );
            // Send via guidance channels to spawned musicians
            for tx in self.guidance_channels.values() {
                let _ = tx.try_send(msg.clone());
            }
            // Also try local musicians (e.g., not yet spawned)
            for musician in self.musicians.iter_mut().filter_map(|m| m.as_mut()) {
                if musician.is_interactive() {
                    let _ = musician.inject_prompt(&msg).await;
                }
            }
        }
    }

    // ─── Task Scheduling ───────────────────────────────────────

    fn has_remaining_tasks(&self) -> bool {
        let tasks = if let Some(ref indices) = self.current_phase_tasks {
            self.tasks
                .iter()
                .filter(|t| indices.contains(&t.index))
                .collect::<Vec<_>>()
        } else {
            self.tasks.iter().collect()
        };

        tasks.iter().any(|t| {
            matches!(
                t.status,
                TaskStatus::Queued
                    | TaskStatus::Ready
                    | TaskStatus::InProgress
                    | TaskStatus::Blocked
            )
        })
    }

    fn get_next_ready_task(&mut self) -> Option<Task> {
        let indices = self.current_phase_tasks.clone();
        let task = if let Some(ref idx_list) = indices {
            self.tasks
                .iter_mut()
                .find(|t| idx_list.contains(&t.index) && t.status == TaskStatus::Ready)
        } else {
            self.tasks
                .iter_mut()
                .find(|t| t.status == TaskStatus::Ready)
        };
        task.map(|t| {
            t.status = TaskStatus::InProgress;
            t.clone()
        })
    }

    fn update_task_readiness(&mut self) {
        let task_count = self.tasks.len();
        for i in 0..task_count {
            let status = self.tasks[i].status.clone();
            if status != TaskStatus::Blocked && status != TaskStatus::Queued {
                continue;
            }

            let deps = self.tasks[i].dependencies.clone();

            // Check for failed dependencies
            let failed_dep = deps.iter().find(|&&dep| {
                self.tasks
                    .get(dep)
                    .map(|t| t.status == TaskStatus::Failed)
                    .unwrap_or(false)
            });

            if let Some(&dep_idx) = failed_dep {
                let dep_title = self
                    .tasks
                    .get(dep_idx)
                    .map(|t| t.title.clone())
                    .unwrap_or_default();
                self.tasks[i].status = TaskStatus::Failed;
                self.tasks[i].result = Some(TaskResult {
                    success: false,
                    files_modified: Vec::new(),
                    summary: format!(
                        "Skipped — dependency Task {} (\"{}\") failed",
                        dep_idx + 1,
                        dep_title
                    ),
                    error: Some(format!("dependency_failed:{dep_idx}")),
                    duration_ms: 0,
                    diff: None,
                    verification_output: None,
                    verification_passed: None,
                });
                continue;
            }

            let deps_complete = deps.iter().all(|&dep| {
                self.tasks
                    .get(dep)
                    .map(|t| {
                        t.status == TaskStatus::Completed || t.status == TaskStatus::Cancelled
                    })
                    .unwrap_or(true)
            });

            if deps_complete {
                self.tasks[i].status = TaskStatus::Ready;
            } else if !deps.is_empty() {
                self.tasks[i].status = TaskStatus::Blocked;
            }
        }
    }

    // ─── Verification ──────────────────────────────────────────

    async fn run_verification(
        &self,
        worktree_path: &str,
    ) -> (String, String, bool) {
        let mut verification_outputs = Vec::new();
        let mut all_passed = true;

        // Capture git diff
        let diff = match Command::new("sh")
            .args(["-c", "git diff HEAD~1..HEAD 2>/dev/null || git diff"])
            .current_dir(worktree_path)
            .output()
            .await
        {
            Ok(output) => {
                String::from_utf8_lossy(&output.stdout)
                    .chars()
                    .take(5000)
                    .collect()
            }
            Err(_) => "[diff capture failed]".into(),
        };

        // Run verification commands
        let commands = self.config.verification.clone().unwrap_or_else(|| {
            vec![
                "npm run typecheck --if-present 2>&1".into(),
                "npx tsc --noEmit 2>&1".into(),
            ]
        });
        let is_custom = self.config.verification.is_some();

        let timeout = tokio::time::Duration::from_millis(self.verification_timeout_ms);

        for cmd in &commands {
            let cmd_future = Command::new("sh")
                .args(["-c", cmd])
                .current_dir(worktree_path)
                .output();

            match tokio::time::timeout(timeout, cmd_future).await {
                Ok(Ok(output)) if output.status.success() => {
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    verification_outputs.push(format!("✓ {cmd}\n{stdout}"));
                    if !is_custom {
                        break;
                    }
                }
                Ok(Ok(output)) => {
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    verification_outputs.push(format!("✗ {cmd}\n{stdout}{stderr}"));
                    all_passed = false;
                    if !is_custom {
                        break;
                    }
                }
                Ok(Err(e)) => {
                    verification_outputs.push(format!("✗ {cmd}\nError: {e}"));
                    all_passed = false;
                    if !is_custom {
                        break;
                    }
                }
                Err(_) => {
                    verification_outputs.push(format!(
                        "✗ {cmd}\nError: timed out after {}ms",
                        self.verification_timeout_ms
                    ));
                    all_passed = false;
                    if !is_custom {
                        break;
                    }
                }
            }
        }

        let verification_output: String = verification_outputs.join("\n\n");
        let verification_output = verification_output.chars().take(10000).collect();
        (diff, verification_output, all_passed)
    }

    // ─── Persistence ───────────────────────────────────────────

    async fn persist_state(&self) -> Result<(), CoreError> {
        use conductor_types::SessionData;
        let data = SessionData {
            id: self.config.session_id.clone(),
            config: self.config.clone(),
            phase: self.phase.clone(),
            started_at: self.started_at.clone(),
            last_updated_at: chrono::Utc::now().to_rfc3339(),
            tasks: self.tasks.clone(),
            phases: if self.phases.is_empty() {
                None
            } else {
                Some(self.phases.clone())
            },
            current_phase_index: self.current_phase_index,
            worktree_state: if self.worktree_snapshots.is_empty() {
                None
            } else {
                Some(self.worktree_snapshots.clone())
            },
        };
        self.task_store.save_session(&data).await
    }

    // ─── Shutdown ──────────────────────────────────────────────

    /// Graceful shutdown — persist state and cleanup.
    pub async fn shutdown(&mut self) {
        let _ = self.persist_state().await;
        if let Some(ref mut conductor) = self.conductor {
            conductor.close().await;
        }
        self.caffeinate.stop().await;
    }

    // ─── Public Getters ────────────────────────────────────────

    /// Get the current state snapshot.
    pub fn get_state(&self) -> &OrchestraState {
        &self.state
    }

    /// Get the insight generator.
    pub fn insight_generator(&self) -> &InsightGenerator {
        &self.insight_generator
    }

    /// Send a message to a specific musician's active session.
    pub async fn chat_with_musician(&mut self, musician_id: &str, message: &str) -> bool {
        // Try guidance channel first (for spawned musicians)
        if let Some(tx) = self.guidance_channels.get(musician_id) {
            return tx.try_send(message.to_string()).is_ok();
        }
        // Fall back to direct injection (for local musicians)
        if let Some(m) = self.musicians.iter_mut().filter_map(|m| m.as_mut()).find(|m| m.get_state().id == musician_id) {
            m.inject_prompt(message).await
        } else {
            false
        }
    }

    /// Get list of musician IDs that currently accept prompt injection.
    pub fn get_interactive_musicians(&self) -> Vec<String> {
        let mut ids: Vec<String> = self.guidance_channels.keys().cloned().collect();
        // Also include local interactive musicians
        for m in self.musicians.iter().filter_map(|m| m.as_ref()) {
            if m.is_interactive() && !ids.contains(&m.get_state().id) {
                ids.push(m.get_state().id.clone());
            }
        }
        ids
    }
}

/// Compute elapsed milliseconds since an ISO timestamp.
fn elapsed_since(iso_timestamp: &str) -> u64 {
    chrono::DateTime::parse_from_rfc3339(iso_timestamp)
        .map(|dt| {
            let now = chrono::Utc::now();
            now.signed_duration_since(dt).num_milliseconds().max(0) as u64
        })
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_config() -> OrchestraConfig {
        OrchestraConfig {
            project_path: "/tmp/test-project".into(),
            task_description: "Build a thing".into(),
            musician_count: 3,
            conductor_model: "opus".into(),
            musician_model: "sonnet".into(),
            max_turns: 50,
            dry_run: false,
            session_id: "test-session".into(),
            reference_session_id: None,
            verification: None,
            headless: false,
        }
    }

    #[test]
    fn new_orchestra_creates_channels() {
        let (orchestra, _state_rx, _action_tx) = Orchestra::new(sample_config());
        assert_eq!(orchestra.phase, OrchestraPhase::Init);
        assert!(orchestra.tasks.is_empty());
        assert!(orchestra.musicians.is_empty());
        assert!(orchestra.plan.is_none());
    }

    #[test]
    fn ensure_musicians_creates_correct_count() {
        let (mut orchestra, _, _) = Orchestra::new(sample_config());
        orchestra.ensure_musicians(3);
        assert_eq!(orchestra.musicians.len(), 3);
        assert_eq!(orchestra.musician_states.len(), 3);
        assert_eq!(orchestra.musicians[0].as_ref().unwrap().get_state().id, "m1");
        assert_eq!(orchestra.musicians[2].as_ref().unwrap().get_state().id, "m3");
    }

    #[test]
    fn ensure_musicians_is_idempotent() {
        let (mut orchestra, _, _) = Orchestra::new(sample_config());
        orchestra.ensure_musicians(3);
        orchestra.ensure_musicians(3);
        assert_eq!(orchestra.musicians.len(), 3);
    }

    #[test]
    fn ensure_musicians_can_grow() {
        let (mut orchestra, _, _) = Orchestra::new(sample_config());
        orchestra.ensure_musicians(2);
        assert_eq!(orchestra.musicians.len(), 2);
        orchestra.ensure_musicians(5);
        assert_eq!(orchestra.musicians.len(), 5);
    }

    #[test]
    fn update_task_readiness_marks_ready() {
        let (mut orchestra, _, _) = Orchestra::new(sample_config());
        orchestra.tasks = vec![Task {
            id: "t1".into(),
            index: 0,
            title: "Test".into(),
            description: "Do it".into(),
            why: "Testing".into(),
            file_scope: vec![],
            dependencies: vec![],
            acceptance_criteria: vec![],
            estimated_turns: 5,
            model: None,
            status: TaskStatus::Queued,
            assigned_musician: None,
            result: None,
        }];
        orchestra.update_task_readiness();
        assert_eq!(orchestra.tasks[0].status, TaskStatus::Ready);
    }

    #[test]
    fn update_task_readiness_blocks_on_deps() {
        let (mut orchestra, _, _) = Orchestra::new(sample_config());
        orchestra.tasks = vec![
            Task {
                id: "t0".into(),
                index: 0,
                title: "First".into(),
                description: "".into(),
                why: "".into(),
                file_scope: vec![],
                dependencies: vec![],
                acceptance_criteria: vec![],
                estimated_turns: 5,
                model: None,
                status: TaskStatus::InProgress,
                assigned_musician: None,
                result: None,
            },
            Task {
                id: "t1".into(),
                index: 1,
                title: "Second".into(),
                description: "".into(),
                why: "".into(),
                file_scope: vec![],
                dependencies: vec![0],
                acceptance_criteria: vec![],
                estimated_turns: 5,
                model: None,
                status: TaskStatus::Queued,
                assigned_musician: None,
                result: None,
            },
        ];
        orchestra.update_task_readiness();
        assert_eq!(orchestra.tasks[1].status, TaskStatus::Blocked);
    }

    #[test]
    fn update_task_readiness_cascades_failure() {
        let (mut orchestra, _, _) = Orchestra::new(sample_config());
        orchestra.tasks = vec![
            Task {
                id: "t0".into(),
                index: 0,
                title: "First".into(),
                description: "".into(),
                why: "".into(),
                file_scope: vec![],
                dependencies: vec![],
                acceptance_criteria: vec![],
                estimated_turns: 5,
                model: None,
                status: TaskStatus::Failed,
                assigned_musician: None,
                result: None,
            },
            Task {
                id: "t1".into(),
                index: 1,
                title: "Second".into(),
                description: "".into(),
                why: "".into(),
                file_scope: vec![],
                dependencies: vec![0],
                acceptance_criteria: vec![],
                estimated_turns: 5,
                model: None,
                status: TaskStatus::Queued,
                assigned_musician: None,
                result: None,
            },
        ];
        orchestra.update_task_readiness();
        assert_eq!(orchestra.tasks[1].status, TaskStatus::Failed);
        assert!(orchestra.tasks[1]
            .result
            .as_ref()
            .unwrap()
            .summary
            .contains("dependency"));
    }

    #[test]
    fn extract_session_reference_works() {
        assert_eq!(
            extract_session_reference("fix session 0481dc issues"),
            Some("0481dc".into())
        );
        assert_eq!(
            extract_session_reference("improve on 0481dc2d"),
            Some("0481dc2d".into())
        );
        assert_eq!(extract_session_reference("just do the thing"), None);
    }

    #[test]
    fn set_phase_updates_state() {
        let (mut orchestra, _, _) = Orchestra::new(sample_config());
        orchestra.set_phase(OrchestraPhase::Exploring);
        assert_eq!(orchestra.phase, OrchestraPhase::Exploring);
    }

    #[test]
    fn push_conductor_output_caps_at_100() {
        let (mut orchestra, _, _) = Orchestra::new(sample_config());
        for i in 0..150 {
            orchestra.push_conductor_output(&format!("line {i}"));
        }
        assert_eq!(orchestra.conductor_output.len(), 100);
        assert!(orchestra.conductor_output.last().unwrap().contains("149"));
    }

    #[test]
    fn has_remaining_tasks_checks_correctly() {
        let (mut orchestra, _, _) = Orchestra::new(sample_config());
        assert!(!orchestra.has_remaining_tasks());

        orchestra.tasks.push(Task {
            id: "t1".into(),
            index: 0,
            title: "Test".into(),
            description: "".into(),
            why: "".into(),
            file_scope: vec![],
            dependencies: vec![],
            acceptance_criteria: vec![],
            estimated_turns: 5,
            model: None,
            status: TaskStatus::Ready,
            assigned_musician: None,
            result: None,
        });
        assert!(orchestra.has_remaining_tasks());
    }
}
