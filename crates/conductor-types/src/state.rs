use serde::{Deserialize, Serialize};

// ─── Orchestra State Machine ─────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum OrchestraPhase {
    Init,
    /// Legacy: kept for backward compat during initial plan
    Planning,
    Exploring,
    Analyzing,
    Decomposing,
    PlanReview,
    PhaseDetailing,
    PhaseExecuting,
    PhaseMerging,
    PhaseReviewing,
    /// Legacy: flat execution (kept for backward compat)
    Executing,
    /// Legacy: end-of-run review
    Reviewing,
    FinalReview,
    Integrating,
    Paused,
    Probing,
    Complete,
    Failed,
}

impl Default for OrchestraPhase {
    fn default() -> Self {
        Self::Init
    }
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

impl OrchestraState {
    pub fn new(config: OrchestraConfig) -> Self {
        Self {
            phase: OrchestraPhase::Init,
            config,
            tasks: Vec::new(),
            plan: None,
            phases: Vec::new(),
            current_phase_index: -1,
            musicians: Vec::new(),
            analysts: Vec::new(),
            analysis_results: Vec::new(),
            rate_limit: RateLimitState::default(),
            started_at: String::new(),
            elapsed_ms: 0,
            conductor_output: Vec::new(),
            conductor_prompts: Vec::new(),
            tokens: TokenUsage::default(),
            tokens_estimated: false,
            guidance_queue_size: 0,
            plan_validation: None,
            refinement_history: Vec::new(),
            insights: Vec::new(),
            total_cost_usd: 0.0,
        }
    }
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

impl Default for TaskStatus {
    fn default() -> Self {
        Self::Queued
    }
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

impl Default for PhaseStatus {
    fn default() -> Self {
        Self::Pending
    }
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

impl Default for WorktreeStatus {
    fn default() -> Self {
        Self::Active
    }
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

impl Default for MusicianStatus {
    fn default() -> Self {
        Self::Idle
    }
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
pub struct GuidanceInput {
    pub user_messages: Vec<GuidanceMessage>,
    pub task_status: String,
    pub shared_memory: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuidanceMessage {
    pub message: String,
    pub timestamp: String,
}

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

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip<T: serde::Serialize + serde::de::DeserializeOwned + std::fmt::Debug>(val: &T) {
        let json = serde_json::to_string(val).expect("serialize");
        let _: T = serde_json::from_str(&json).expect("deserialize");
    }

    fn sample_config() -> OrchestraConfig {
        OrchestraConfig {
            project_path: "/tmp/project".into(),
            task_description: "Build a thing".into(),
            musician_count: 3,
            conductor_model: "opus".into(),
            musician_model: "sonnet".into(),
            max_turns: 50,
            dry_run: false,
            session_id: "sess-001".into(),
            reference_session_id: None,
            verification: None,
        }
    }

    #[test]
    fn orchestra_state_round_trip() {
        let state = OrchestraState::new(sample_config());
        round_trip(&state);
    }

    #[test]
    fn task_round_trip() {
        let task = Task {
            id: "t1".into(),
            index: 0,
            title: "Do something".into(),
            description: "Details".into(),
            why: "Because".into(),
            file_scope: vec!["src/main.rs".into()],
            dependencies: vec![],
            acceptance_criteria: vec!["It works".into()],
            estimated_turns: 5,
            model: None,
            status: TaskStatus::Queued,
            assigned_musician: None,
            result: None,
        };
        round_trip(&task);
    }

    #[test]
    fn task_result_round_trip() {
        let result = TaskResult {
            success: true,
            files_modified: vec!["src/lib.rs".into()],
            summary: "Done".into(),
            error: None,
            tokens_used: 1000,
            duration_ms: 5000,
            diff: Some("+added line".into()),
            verification_output: Some("ok".into()),
            verification_passed: Some(true),
        };
        round_trip(&result);
    }

    #[test]
    fn claude_event_round_trip() {
        let event = ClaudeEvent {
            event_type: ClaudeEventType::Result,
            subtype: None,
            session_id: Some("s1".into()),
            message: None,
            tool_name: None,
            tool_input: None,
            tool_result_content: None,
            result: Some("done".into()),
            cost_usd: Some(0.05),
            total_cost_usd: Some(0.10),
            duration_ms: Some(3000),
            duration_api_ms: Some(2500),
            num_turns: Some(3),
            is_error: Some(false),
            resets_at: None,
            usage: Some(TokenUsage { input: 100, output: 50, cache_read: 0, cache_creation: 0 }),
        };
        round_trip(&event);
    }

    #[test]
    fn session_data_round_trip() {
        let session = SessionData {
            id: "sess-001".into(),
            config: sample_config(),
            phase: OrchestraPhase::Init,
            started_at: "2026-01-01T00:00:00Z".into(),
            last_updated_at: "2026-01-01T00:01:00Z".into(),
            tokens: TokenUsage::default(),
            tokens_estimated: false,
            tasks: vec![],
            phases: None,
            current_phase_index: None,
            worktree_state: None,
        };
        round_trip(&session);
    }

    #[test]
    fn phase_round_trip() {
        let phase = Phase {
            id: "p1".into(),
            index: 0,
            title: "Phase 1".into(),
            description: "First phase".into(),
            status: PhaseStatus::Pending,
            tasks: vec![],
            review_result: None,
            token_used: 0,
        };
        round_trip(&phase);
    }

    #[test]
    fn enum_defaults() {
        assert_eq!(OrchestraPhase::default(), OrchestraPhase::Init);
        assert_eq!(TaskStatus::default(), TaskStatus::Queued);
        assert_eq!(PhaseStatus::default(), PhaseStatus::Pending);
        assert_eq!(MusicianStatus::default(), MusicianStatus::Idle);
        assert_eq!(WorktreeStatus::default(), WorktreeStatus::Active);
    }

    #[test]
    fn token_usage_default_is_zero() {
        let t = TokenUsage::default();
        assert_eq!(t.input, 0);
        assert_eq!(t.output, 0);
        assert_eq!(t.cache_read, 0);
        assert_eq!(t.cache_creation, 0);
    }

    #[test]
    fn orchestra_state_new_defaults() {
        let state = OrchestraState::new(sample_config());
        assert_eq!(state.phase, OrchestraPhase::Init);
        assert_eq!(state.current_phase_index, -1);
        assert!(state.tasks.is_empty());
        assert!(state.musicians.is_empty());
        assert_eq!(state.total_cost_usd, 0.0);
    }
}
