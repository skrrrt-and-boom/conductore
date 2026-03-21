use thiserror::Error;

pub mod parse;
pub mod session;
pub mod validate;

pub use parse::{is_rate_limit_error, parse_claude_event};
pub use session::{ClaudeSession, ClaudeSessionOptions};
pub use validate::{validate_claude_cli, validate_model, VALID_MODELS};

#[derive(Debug, Error)]
pub enum BridgeError {
    #[error(
        "Claude Code CLI not found. Install it with: npm install -g @anthropic-ai/claude-code\n\
         Then ensure `claude` is on your PATH."
    )]
    CliNotFound,

    #[error("Unknown model \"{0}\". Valid models: {1}")]
    UnknownModel(String, String),

    #[error("Claude process exited with code {exit_code}: {stderr}")]
    ProcessFailed { exit_code: i32, stderr: String },

    #[error("JSON parse error: {0}")]
    JsonParse(#[from] serde_json::Error),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Session is closed")]
    SessionClosed,
}
