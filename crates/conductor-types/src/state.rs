use serde::{Deserialize, Serialize};

// ─── Orchestra State Machine ─────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum OrchestraPhase {
    Init,
    Exploring,
    Analyzing,
    Decomposing,
    PlanReview,
    PhaseDetailing,
    PhaseExecuting,
    PhaseMerging,
    PhaseReviewing,
    FinalReview,
    Integrating,
    Paused,
    Probing,
    Complete,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrchestraConfig {
    pub project_path: String,
    pub task_description: String,
    pub musician_count: usize,
    pub conductor_model: String,
    pub musician_model: String,
    pub max_turns: u32,
    pub dry_run: bool,
    pub session_id: String,
    pub reference_session_id: Option<String>,
    pub verification: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrchestraState {
    pub phase: OrchestraPhase,
    pub config: OrchestraConfig,
    pub tasks: Vec<Task>,
    pub plan: Option<Plan>,
    pub phases: Vec<Phase>,
    pub current_phase_index: i32,
    pub musicians: Vec<MusicianState>,
    pub analysts: Vec<AnalystState>,
    pub analysis_results: Vec<AnalysisResult>,
    pub rate_limit: RateLimitState,
    pub started_at: String,
    pub elapsed_ms: u64,
    pub conductor_output: Vec<String>,
    pub conductor_prompts: Vec<String>,
    pub tokens: TokenUsage,
    pub tokens_estimated: bool,
    pub guidance_queue_size: usize,
    pub plan_validation: Option<PlanValidation>,
    pub refinement_history: Vec<PlanRefinementMessage>,
    pub insights: Vec<Insight>,
    pub total_cost_usd: f64,
}

// ─── Tasks ───────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum TaskStatus {
    Queued,
    Ready,
    InProgress,
    Review,
    Completed,
    Failed,
    Blocked,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    pub index: usize,
    pub title: String,
    pub description: String,
    pub why: String,
    pub file_scope: Vec<String>,
    pub dependencies: Vec<usize>,
    pub acceptance_criteria: Vec<String>,
    pub estimated_turns: u32,
    pub model: Option<String>,
    pub status: TaskStatus,
    pub assigned_musician: Option<String>,
    pub result: Option<TaskResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskResult {
    pub success: bool,
    pub files_modified: Vec<String>,
    pub summary: String,
    pub error: Option<String>,
    pub tokens_used: u64,
    pub duration_ms: u64,
    pub diff: Option<String>,
    pub verification_output: Option<String>,
    pub verification_passed: Option<bool>,
}

// ─── Phases ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum PhaseStatus {
    Pending,
    Active,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Phase {
    pub id: String,
    pub index: usize,
    pub title: String,
    pub description: String,
    pub status: PhaseStatus,
    pub tasks: Vec<Task>,
    pub review_result: Option<PhaseReviewResult>,
    pub token_used: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhaseReviewResult {
    pub action: PhaseReviewAction,
    pub task_indices: Option<Vec<usize>>,
    pub revised_phases: Option<Vec<Phase>>,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum PhaseReviewAction {
    Continue,
    RetryTasks,
    ReviseRemainingPhases,
    Abort,
}

// ─── Checkpoints ─────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Checkpoint {
    pub turn_number: u32,
    pub files_modified: Vec<String>,
    pub timestamp: String,
    pub token_used: u64,
    pub commit_sha: Option<String>,
}

// ─── Worktree Snapshots ──────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorktreeSnapshot {
    pub worker_id: String,
    pub task_index: usize,
    pub branch: String,
    pub path: String,
    pub last_commit_sha: String,
    pub status: WorktreeStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum WorktreeStatus {
    Active,
    Completed,
    Abandoned,
}

// ─── Analysis ────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalysisDirective {
    pub id: String,
    pub area: String,
    pub question: String,
    pub file_hints: Vec<String>,
    pub estimated_turns: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalysisResult {
    pub directive_id: String,
    pub area: String,
    pub findings: String,
    pub key_files: Vec<String>,
    pub patterns: Vec<String>,
    pub risks: Vec<String>,
    pub tokens_used: u64,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalystState {
    pub id: String,
    pub index: usize,
    pub status: MusicianStatus,
    pub directive: Option<AnalysisDirective>,
    pub output_lines: Vec<String>,
    pub tokens_used: u64,
    pub token_usage: TokenUsage,
    pub tokens_estimated: bool,
    pub started_at: Option<String>,
    pub elapsed_ms: u64,
}

// ─── Musicians ───────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum MusicianStatus {
    Idle,
    Running,
    Waiting,
    Paused,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MusicianState {
    pub id: String,
    pub index: usize,
    pub status: MusicianStatus,
    pub current_task: Option<Task>,
    pub output_lines: Vec<String>,
    pub tokens_used: u64,
    pub token_usage: TokenUsage,
    pub tokens_estimated: bool,
    pub started_at: Option<String>,
    pub elapsed_ms: u64,
    pub worktree_path: Option<String>,
    pub branch: Option<String>,
    pub checkpoint: Option<Checkpoint>,
    pub prompt_sent: Option<String>,
    pub cost_usd: f64,
}

// ─── Codebase Map ────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodebaseMap {
    pub summary: String,
    pub modules: Vec<CodebaseModule>,
    pub patterns: Vec<String>,
    pub conventions: Vec<String>,
    pub analysis_needed: Option<bool>,
    pub analysis_directives: Option<Vec<AnalysisDirective>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodebaseModule {
    pub path: String,
    pub purpose: String,
    pub key_files: Vec<String>,
    pub dependencies: Vec<String>,
}

// ─── Plan ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Plan {
    pub summary: String,
    pub tasks: Vec<Task>,
    pub dependency_graph: String,
    pub musician_assignment: String,
    pub learning_notes: Vec<String>,
    pub estimated_tokens: u64,
    pub estimated_minutes: u32,
    pub insights: Option<Vec<PlanInsight>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanInsight {
    pub category: InsightCategory,
    pub title: String,
    pub body: String,
}

// ─── Plan Refinement ─────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanRefinementMessage {
    pub role: RefinementRole,
    pub text: String,
    pub images: Option<Vec<String>>,
    pub timestamp: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum RefinementRole {
    User,
    Conductor,
}

// ─── Guidance ────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuidanceActions {
    pub add_tasks: Option<Vec<NewTaskSpec>>,
    pub cancel_tasks: Option<Vec<usize>>,
    pub modify_tasks: Option<Vec<TaskModification>>,
    pub guidance: Option<String>,
    pub insights: Option<Vec<PlanInsight>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewTaskSpec {
    pub title: String,
    pub description: String,
    pub why: String,
    pub file_scope: Vec<String>,
    pub dependencies: Vec<usize>,
    pub acceptance_criteria: Vec<String>,
    pub estimated_turns: u32,
    pub model: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskModification {
    pub index: usize,
    pub new_description: String,
}

// ─── Token Usage ─────────────────────────────────────────────────

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TokenUsage {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_creation: u64,
}

// ─── Claude Bridge Events ────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ClaudeEventType {
    System,
    Assistant,
    ToolUse,
    ToolResult,
    Result,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaudeEvent {
    pub event_type: ClaudeEventType,
    pub subtype: Option<String>,
    pub session_id: Option<String>,
    pub message: Option<String>,
    pub tool_name: Option<String>,
    pub tool_input: Option<serde_json::Value>,
    pub tool_result_content: Option<String>,
    pub result: Option<String>,
    pub cost_usd: Option<f64>,
    pub total_cost_usd: Option<f64>,
    pub duration_ms: Option<u64>,
    pub duration_api_ms: Option<u64>,
    pub num_turns: Option<u32>,
    pub is_error: Option<bool>,
    pub resets_at: Option<String>,
    pub usage: Option<TokenUsage>,
}

// ─── Rate Limiting ───────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum RateLimitStatus {
    Ok,
    Warning,
    Limited,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitState {
    pub status: RateLimitStatus,
    pub resets_at: Option<String>,
    pub last_probe_at: Option<String>,
    pub probe_count: u32,
    pub next_probe_in: Option<u64>,
}

impl Default for RateLimitState {
    fn default() -> Self {
        Self {
            status: RateLimitStatus::Ok,
            resets_at: None,
            last_probe_at: None,
            probe_count: 0,
            next_probe_in: None,
        }
    }
}

// ─── Insights ────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum InsightCategory {
    Pattern,
    Architecture,
    Tool,
    Decision,
    Concept,
    Why,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Insight {
    pub timestamp: String,
    pub category: InsightCategory,
    pub title: String,
    pub body: String,
    pub source: String,
}

// ─── Plan Validation ─────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanValidation {
    pub valid: bool,
    pub issues: Vec<ValidationIssue>,
    pub cycles: Option<Vec<Vec<usize>>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationIssue {
    pub severity: ValidationSeverity,
    pub message: String,
    pub task_indices: Option<Vec<usize>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ValidationSeverity {
    Error,
    Warning,
}

// ─── Conductor Config File ───────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConductorProjectConfig {
    pub verification: Option<Vec<String>>,
}

// ─── Session Persistence ─────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionData {
    pub id: String,
    pub config: OrchestraConfig,
    pub phase: OrchestraPhase,
    pub started_at: String,
    pub last_updated_at: String,
    pub tokens: TokenUsage,
    pub tokens_estimated: bool,
    pub tasks: Vec<Task>,
    pub phases: Option<Vec<Phase>>,
    pub current_phase_index: Option<i32>,
    pub worktree_state: Option<Vec<WorktreeSnapshot>>,
}
