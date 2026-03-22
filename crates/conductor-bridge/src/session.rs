use crate::BridgeError;
use base64::Engine;
use conductor_types::{ClaudeEvent, ClaudeEventType};
use std::path::Path;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::mpsc;

/// Options for creating a ClaudeSession.
pub struct ClaudeSessionOptions {
    pub model: String,
    pub max_turns: Option<u32>,
    pub allowed_tools: Option<Vec<String>>,
    pub append_system_prompt: Option<String>,
    pub cwd: Option<String>,
}

/// Persistent bidirectional Claude Code session using stream-json I/O.
///
/// Spawns `claude -p --input-format stream-json --output-format stream-json`
/// with stdin open. Messages are sent via stdin as JSON, responses come
/// back as NDJSON on stdout. The process stays alive between messages,
/// enabling true multi-turn conversations.
pub struct ClaudeSession {
    child: Option<Child>,
    stdin: Option<ChildStdin>,
    closed: bool,
    session_id: Option<String>,
    options: ClaudeSessionOptions,
}

impl ClaudeSession {
    pub fn new(options: ClaudeSessionOptions) -> Self {
        Self {
            child: None,
            stdin: None,
            closed: false,
            session_id: None,
            options,
        }
    }

    pub fn is_closed(&self) -> bool {
        self.closed
    }

    pub fn session_id(&self) -> Option<&str> {
        self.session_id.as_deref()
    }

    /// Start the session with an initial prompt.
    /// The process stays alive after the first turn completes —
    /// use send_message() for follow-up prompts.
    pub async fn start(
        &mut self,
        initial_prompt: &str,
        event_tx: mpsc::Sender<ClaudeEvent>,
    ) -> Result<(), BridgeError> {
        let args = build_args(&self.options);
        let cwd = self.options.cwd.as_deref().unwrap_or(".");

        let mut child = Command::new("claude")
            .args(&args)
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        let stdout = child.stdout.take().expect("stdout piped");
        let stderr = child.stderr.take().expect("stderr piped");
        self.stdin = child.stdin.take();
        self.child = Some(child);

        // Write initial message
        self.write_message(initial_prompt, None).await?;

        // Spawn stderr collector task
        let stderr_tx = event_tx.clone();
        tokio::spawn(async move {
            let mut stderr_buf = String::new();
            let mut reader = BufReader::new(stderr);
            loop {
                let mut line = String::new();
                match reader.read_line(&mut line).await {
                    Ok(0) => break, // EOF
                    Ok(_) => {
                        let trimmed = line.trim();
                        if !trimmed.is_empty() {
                            tracing::debug!(stderr = trimmed, "claude stderr");
                            stderr_buf.push_str(trimmed);
                            stderr_buf.push('\n');
                        }
                    }
                    Err(e) => {
                        tracing::debug!(error = %e, "stderr read error");
                        break;
                    }
                }
            }
            // If we collected stderr and it looks like a rate limit, emit an error event
            if !stderr_buf.is_empty() && crate::parse::is_rate_limit_error(&stderr_buf) {
                let ev = ClaudeEvent {
                    event_type: ClaudeEventType::Error,
                    subtype: Some("rate_limit".into()),
                    message: Some(stderr_buf),
                    session_id: None,
                    tool_name: None,
                    tool_input: None,
                    tool_result_content: None,
                    result: None,
                    duration_ms: None,
                    duration_api_ms: None,
                    num_turns: None,
                    is_error: None,
                    resets_at: None,
                };
                let _ = stderr_tx.send(ev).await;
            }
        });

        // Spawn stdout NDJSON reader task
        tokio::spawn(async move {
            let reader = BufReader::new(stdout);
            let mut lines = reader.lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let trimmed = line.trim().to_string();
                if trimmed.is_empty() {
                    continue;
                }
                match serde_json::from_str::<serde_json::Value>(&trimmed) {
                    Ok(raw) => {
                        let events = crate::parse::parse_claude_event(&raw);
                        for event in events {
                            if event_tx.send(event).await.is_err() {
                                // Receiver dropped — session is being torn down
                                return;
                            }
                        }
                    }
                    Err(e) => {
                        tracing::debug!(line = trimmed, error = %e, "non-JSON stdout line (progress indicator)");
                    }
                }
            }
        });

        Ok(())
    }

    /// Send a follow-up message to the running session.
    pub async fn send_message(&mut self, text: &str) -> Result<(), BridgeError> {
        self.send_message_with_images(text, None).await
    }

    /// Send a follow-up message with optional image attachments.
    pub async fn send_message_with_images(
        &mut self,
        text: &str,
        images: Option<&[String]>,
    ) -> Result<(), BridgeError> {
        if self.closed || self.stdin.is_none() {
            return Err(BridgeError::SessionClosed);
        }
        self.write_message(text, images).await
    }

    /// Gracefully close the session.
    pub async fn close(&mut self) {
        if self.closed {
            return;
        }
        self.closed = true;

        // Drop stdin to send EOF to the child
        drop(self.stdin.take());

        // Kill child if still running
        if let Some(ref mut child) = self.child {
            let _ = child.start_kill();
            let _ = child.wait().await;
        }
    }

    /// Write a user message to stdin in stream-json format.
    /// Optionally includes base64-encoded image content blocks.
    async fn write_message(
        &mut self,
        text: &str,
        images: Option<&[String]>,
    ) -> Result<(), BridgeError> {
        let stdin = match self.stdin.as_mut() {
            Some(s) => s,
            None => return Err(BridgeError::SessionClosed),
        };

        let mut content = vec![serde_json::json!({"type": "text", "text": text})];

        // Add images as base64 content blocks
        if let Some(image_paths) = images {
            for img_path in image_paths {
                match tokio::fs::read(img_path).await {
                    Ok(data) => {
                        let media_type = mime_type_from_path(img_path);
                        let b64 = base64::engine::general_purpose::STANDARD.encode(&data);
                        content.push(serde_json::json!({
                            "type": "image",
                            "source": {
                                "type": "base64",
                                "media_type": media_type,
                                "data": b64,
                            }
                        }));
                    }
                    Err(e) => {
                        tracing::warn!(path = img_path, error = %e, "could not read image, skipping");
                    }
                }
            }
        }

        let msg = serde_json::json!({
            "type": "user",
            "message": {
                "role": "user",
                "content": content,
            }
        });

        let mut bytes = serde_json::to_vec(&msg)?;
        bytes.push(b'\n');
        stdin.write_all(&bytes).await?;
        stdin.flush().await?;
        Ok(())
    }
}

/// Determine MIME type from file extension.
fn mime_type_from_path(path: &str) -> &'static str {
    match Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .as_deref()
    {
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("bmp") => "image/bmp",
        Some("svg") => "image/svg+xml",
        Some("tif" | "tiff") => "image/tiff",
        _ => "image/png", // fallback
    }
}

impl Drop for ClaudeSession {
    fn drop(&mut self) {
        if let Some(ref mut child) = self.child {
            let _ = child.start_kill();
        }
    }
}

/// Build CLI arguments from session options.
fn build_args(options: &ClaudeSessionOptions) -> Vec<String> {
    let mut args = vec![
        "-p".into(),
        "--input-format".into(),
        "stream-json".into(),
        "--output-format".into(),
        "stream-json".into(),
        "--model".into(),
        options.model.clone(),
        "--permission-mode".into(),
        "bypassPermissions".into(),
    ];

    if let Some(max_turns) = options.max_turns {
        args.push("--max-turns".into());
        args.push(max_turns.to_string());
    }

    if let Some(ref tools) = options.allowed_tools {
        if !tools.is_empty() {
            args.push("--allowedTools".into());
            args.push(tools.join(" "));
        }
    }

    if let Some(ref prompt) = options.append_system_prompt {
        args.push("--append-system-prompt".into());
        args.push(prompt.clone());
    }

    args
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_options(model: &str) -> ClaudeSessionOptions {
        ClaudeSessionOptions {
            model: model.into(),
            max_turns: None,
            allowed_tools: None,
            append_system_prompt: None,
            cwd: None,
        }
    }

    #[test]
    fn build_args_minimal() {
        let args = build_args(&make_options("sonnet"));
        assert!(args.contains(&"-p".to_string()));
        assert!(args.contains(&"stream-json".to_string()));
        assert!(args.contains(&"sonnet".to_string()));
        assert!(args.contains(&"bypassPermissions".to_string()));
        assert!(!args.contains(&"--max-turns".to_string()));
        assert!(!args.contains(&"--allowedTools".to_string()));
    }

    #[test]
    fn build_args_with_max_turns() {
        let mut opts = make_options("opus");
        opts.max_turns = Some(10);
        let args = build_args(&opts);
        let idx = args.iter().position(|a| a == "--max-turns").unwrap();
        assert_eq!(args[idx + 1], "10");
    }

    #[test]
    fn build_args_with_allowed_tools() {
        let mut opts = make_options("opus");
        opts.allowed_tools = Some(vec!["Read".into(), "Write".into()]);
        let args = build_args(&opts);
        let idx = args.iter().position(|a| a == "--allowedTools").unwrap();
        assert_eq!(args[idx + 1], "Read Write");
    }

    #[test]
    fn build_args_with_system_prompt() {
        let mut opts = make_options("haiku");
        opts.append_system_prompt = Some("Be helpful".into());
        let args = build_args(&opts);
        let idx = args
            .iter()
            .position(|a| a == "--append-system-prompt")
            .unwrap();
        assert_eq!(args[idx + 1], "Be helpful");
    }

    #[test]
    fn build_args_all_options() {
        let opts = ClaudeSessionOptions {
            model: "opus".into(),
            max_turns: Some(5),
            allowed_tools: Some(vec!["Bash".into()]),
            append_system_prompt: Some("Test prompt".into()),
            cwd: Some("/tmp".into()),
        };
        let args = build_args(&opts);
        assert!(args.contains(&"--max-turns".to_string()));
        assert!(args.contains(&"--allowedTools".to_string()));
        assert!(args.contains(&"--append-system-prompt".to_string()));
    }

    #[test]
    fn new_session_defaults() {
        let session = ClaudeSession::new(make_options("sonnet"));
        assert!(!session.is_closed());
        assert!(session.session_id().is_none());
    }

    #[test]
    fn send_message_when_closed() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let mut session = ClaudeSession::new(make_options("sonnet"));
            session.closed = true;
            let result = session.send_message("hello").await;
            assert!(result.is_err());
            assert!(matches!(result.unwrap_err(), BridgeError::SessionClosed));
        });
    }
}
