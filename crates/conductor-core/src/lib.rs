//! Core orchestration logic for conductor.
//!
//! Contains pure logic modules (DAG analysis, rate limiting, token estimation),
//! shared memory coordination, and session persistence.

pub mod caffeinate;
pub mod dag;
pub mod insights;
pub mod memory;
pub mod rate_limiter;
pub mod task_store;
pub mod token_estimate;
pub mod tool_summary;

/// Core error type for conductor-core operations.
#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    /// File I/O error.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// JSON serialization/deserialization error.
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

}
