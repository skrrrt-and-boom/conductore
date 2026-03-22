//! Persistent prompt history stored at `~/.conductor/prompt_history.jsonl`.
//!
//! Each line is a JSON object: `{"text": "...", "timestamp": "..."}`.
//! History is loaded on TUI startup and appended after each submission.

use std::path::PathBuf;

use tokio::fs;

use crate::CoreError;

const MAX_HISTORY: usize = 500;

fn history_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".conductor")
        .join("prompt_history.jsonl")
}

/// Load prompt history from disk (most recent last). Returns up to MAX_HISTORY entries.
pub async fn load_history() -> Result<Vec<String>, CoreError> {
    let path = history_path();
    if !path.exists() {
        return Ok(vec![]);
    }
    let content = fs::read_to_string(&path).await?;
    let mut prompts: Vec<String> = content
        .lines()
        .filter_map(|line| {
            serde_json::from_str::<serde_json::Value>(line)
                .ok()
                .and_then(|v| v.get("text")?.as_str().map(|s| s.to_string()))
        })
        .collect();

    // Keep only the most recent entries
    if prompts.len() > MAX_HISTORY {
        prompts = prompts.split_off(prompts.len() - MAX_HISTORY);
    }
    Ok(prompts)
}

/// Append a prompt to the history file on disk.
pub async fn save_prompt(text: &str) -> Result<(), CoreError> {
    let path = history_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await?;
    }
    let entry = serde_json::json!({
        "text": text,
        "timestamp": chrono::Utc::now().to_rfc3339(),
    });
    let mut line = serde_json::to_string(&entry)?;
    line.push('\n');

    use tokio::io::AsyncWriteExt;
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .await?;
    file.write_all(line.as_bytes()).await?;
    Ok(())
}
