//! Core orchestration logic for conductor.
//!
//! Contains pure logic modules (DAG analysis, rate limiting, token estimation),
//! shared memory coordination, and session persistence.

pub mod caffeinate;
pub mod conductor_agent;
pub mod dag;
pub mod insights;
pub mod memory;
pub mod musician;
pub mod orchestra;
pub mod rate_limiter;
pub mod task_store;
pub mod tool_summary;
pub mod worktree_manager;

/// Core error type for conductor-core operations.
#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    /// File I/O error.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// JSON serialization/deserialization error.
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// JSON parsing failed on LLM output — carries the raw output for display.
    #[error("Failed to parse JSON from LLM output: {reason}")]
    JsonParse {
        reason: String,
        raw_output: String,
    },

    /// Git command or worktree operation failed.
    #[error("git error: {0}")]
    Git(String),

    /// Channel send/receive failure.
    #[error("channel error: {0}")]
    Channel(String),

    /// Operation timed out.
    #[error("timeout: {0}")]
    Timeout(String),

    /// Bridge (Claude CLI session) error.
    #[error("bridge error: {0}")]
    Bridge(String),
}
