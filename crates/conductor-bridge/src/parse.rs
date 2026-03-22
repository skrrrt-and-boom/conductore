use conductor_types::{ClaudeEvent, ClaudeEventType};
use serde_json::Value;

/// Helper to create a ClaudeEvent with all fields set to None.
pub fn empty_event(event_type: ClaudeEventType) -> ClaudeEvent {
    ClaudeEvent {
        event_type,
        subtype: None,
        session_id: None,
        message: None,
        tool_name: None,
        tool_input: None,
        tool_result_content: None,
        result: None,
        duration_ms: None,
        duration_api_ms: None,
        num_turns: None,
        is_error: None,
        resets_at: None,
    }
}

/// Parse a raw NDJSON value into zero or more ClaudeEvents.
///
/// Assistant messages can contain multiple content blocks (thinking, text, tool_use),
/// so one raw JSON object may expand to several events.
pub fn parse_claude_event(raw: &Value) -> Vec<ClaudeEvent> {
    let type_str = raw.get("type").and_then(|v| v.as_str()).unwrap_or("");

    match type_str {
        "system" => {
            let mut ev = empty_event(ClaudeEventType::System);
            ev.subtype = raw.get("subtype").and_then(|v| v.as_str()).map(String::from);
            ev.session_id = raw.get("session_id").and_then(|v| v.as_str()).map(String::from);
            vec![ev]
        }

        "assistant" => {
            let content = raw
                .get("message")
                .and_then(|m| m.get("content"))
                .and_then(|c| c.as_array());
            let content = match content {
                Some(c) if !c.is_empty() => c,
                _ => return vec![],
            };

            let mut events = Vec::new();
            for block in content {
                let block_type = block.get("type").and_then(|v| v.as_str()).unwrap_or("");
                match block_type {
                    "text" => {
                        let mut ev = empty_event(ClaudeEventType::Assistant);
                        ev.subtype = Some("text".into());
                        ev.message = block.get("text").and_then(|v| v.as_str()).map(String::from);
                        events.push(ev);
                    }
                    "thinking" => {
                        let mut ev = empty_event(ClaudeEventType::Assistant);
                        ev.subtype = Some("thinking".into());
                        ev.message = Some("[Thinking...]".into());
                        events.push(ev);
                    }
                    "tool_use" => {
                        let mut ev = empty_event(ClaudeEventType::ToolUse);
                        ev.tool_name =
                            block.get("name").and_then(|v| v.as_str()).map(String::from);
                        ev.tool_input = block.get("input").cloned();
                        events.push(ev);
                    }
                    _ => {}
                }
            }
            events
        }

        "user" => {
            let tool_result = raw.get("tool_use_result");
            match tool_result {
                Some(tr) => {
                    let mut ev = empty_event(ClaudeEventType::ToolResult);
                    ev.tool_result_content = Some(
                        tr.get("stdout")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string(),
                    );
                    vec![ev]
                }
                None => vec![],
            }
        }

        "rate_limit_event" => {
            let info = raw.get("rate_limit_info");
            let status = info
                .and_then(|i| i.get("status"))
                .and_then(|v| v.as_str())
                .unwrap_or("");

            let resets_at = info
                .and_then(|i| i.get("resetsAt"))
                .and_then(|v| v.as_f64())
                .map(|ts| {
                    let secs = ts as i64;
                    let nanos = ((ts - secs as f64) * 1e9) as u32;
                    chrono::DateTime::from_timestamp(secs, nanos)
                        .map(|dt| dt.to_rfc3339())
                        .unwrap_or_default()
                });

            if status == "rate_limited" || status == "blocked" {
                let rate_limit_type = info
                    .and_then(|i| i.get("rateLimitType"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                let mut ev = empty_event(ClaudeEventType::Error);
                ev.subtype = Some("rate_limit".into());
                ev.message = Some(format!("Rate limited ({rate_limit_type})"));
                ev.resets_at = resets_at;
                vec![ev]
            } else {
                let mut ev = empty_event(ClaudeEventType::System);
                ev.subtype = Some("rate_limit_info".into());
                ev.resets_at = resets_at;
                vec![ev]
            }
        }

        "result" => {
            let mut ev = empty_event(ClaudeEventType::Result);
            ev.subtype = raw.get("subtype").and_then(|v| v.as_str()).map(String::from);
            ev.result = raw.get("result").and_then(|v| v.as_str()).map(String::from);
            ev.duration_ms = raw.get("duration_ms").and_then(|v| v.as_u64());
            ev.duration_api_ms = raw.get("duration_api_ms").and_then(|v| v.as_u64());
            ev.num_turns = raw.get("num_turns").and_then(|v| v.as_u64()).map(|n| n as u32);
            ev.is_error = raw.get("is_error").and_then(|v| v.as_bool());
            vec![ev]
        }

        _ => vec![],
    }
}

/// Check if stderr output indicates a rate limit error.
pub fn is_rate_limit_error(stderr: &str) -> bool {
    let lower = stderr.to_lowercase();
    lower.contains("429")
        || lower.contains("rate limit")
        || lower.contains("too many requests")
        || lower.contains("rate_limit")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_system_event() {
        let raw = json!({"type": "system", "subtype": "init", "session_id": "sess-123"});
        let events = parse_claude_event(&raw);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, ClaudeEventType::System);
        assert_eq!(events[0].subtype.as_deref(), Some("init"));
        assert_eq!(events[0].session_id.as_deref(), Some("sess-123"));
    }

    #[test]
    fn parse_assistant_text() {
        let raw = json!({
            "type": "assistant",
            "message": {"content": [{"type": "text", "text": "Hello world"}]}
        });
        let events = parse_claude_event(&raw);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, ClaudeEventType::Assistant);
        assert_eq!(events[0].subtype.as_deref(), Some("text"));
        assert_eq!(events[0].message.as_deref(), Some("Hello world"));
    }

    #[test]
    fn parse_assistant_multiple_blocks() {
        let raw = json!({
            "type": "assistant",
            "message": {"content": [
                {"type": "text", "text": "Here's what I'll do"},
                {"type": "tool_use", "name": "Read", "input": {"path": "/tmp/file"}}
            ]}
        });
        let events = parse_claude_event(&raw);
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].event_type, ClaudeEventType::Assistant);
        assert_eq!(events[1].event_type, ClaudeEventType::ToolUse);
        assert_eq!(events[1].tool_name.as_deref(), Some("Read"));
    }

    #[test]
    fn parse_assistant_thinking() {
        let raw = json!({
            "type": "assistant",
            "message": {"content": [{"type": "thinking", "thinking": "Let me consider..."}]}
        });
        let events = parse_claude_event(&raw);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].subtype.as_deref(), Some("thinking"));
        assert_eq!(events[0].message.as_deref(), Some("[Thinking...]"));
    }

    #[test]
    fn parse_tool_use() {
        let raw = json!({
            "type": "assistant",
            "message": {"content": [
                {"type": "tool_use", "name": "Write", "input": {"path": "/tmp/out", "content": "hi"}}
            ]}
        });
        let events = parse_claude_event(&raw);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, ClaudeEventType::ToolUse);
        assert_eq!(events[0].tool_name.as_deref(), Some("Write"));
        assert!(events[0].tool_input.is_some());
    }

    #[test]
    fn parse_tool_result() {
        let raw = json!({"type": "user", "tool_use_result": {"stdout": "file contents here"}});
        let events = parse_claude_event(&raw);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, ClaudeEventType::ToolResult);
        assert_eq!(
            events[0].tool_result_content.as_deref(),
            Some("file contents here")
        );
    }

    #[test]
    fn parse_result_event() {
        let raw = json!({
            "type": "result",
            "subtype": "success",
            "result": "Task completed",
            "duration_ms": 3000,
            "duration_api_ms": 2500,
            "num_turns": 3,
            "is_error": false
        });
        let events = parse_claude_event(&raw);
        assert_eq!(events.len(), 1);
        let ev = &events[0];
        assert_eq!(ev.event_type, ClaudeEventType::Result);
        assert_eq!(ev.result.as_deref(), Some("Task completed"));
        assert_eq!(ev.duration_ms, Some(3000));
        assert_eq!(ev.num_turns, Some(3));
        assert_eq!(ev.is_error, Some(false));
    }

    #[test]
    fn parse_rate_limit_blocked() {
        let raw = json!({
            "type": "rate_limit_event",
            "rate_limit_info": {
                "status": "rate_limited",
                "rateLimitType": "token",
                "resetsAt": 1700000000
            }
        });
        let events = parse_claude_event(&raw);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, ClaudeEventType::Error);
        assert_eq!(events[0].subtype.as_deref(), Some("rate_limit"));
        assert!(events[0].message.as_deref().unwrap().contains("token"));
        assert!(events[0].resets_at.is_some());
    }

    #[test]
    fn parse_rate_limit_info() {
        let raw = json!({
            "type": "rate_limit_event",
            "rate_limit_info": {"status": "ok"}
        });
        let events = parse_claude_event(&raw);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, ClaudeEventType::System);
        assert_eq!(events[0].subtype.as_deref(), Some("rate_limit_info"));
    }

    #[test]
    fn parse_unknown_type() {
        let raw = json!({"type": "banana"});
        let events = parse_claude_event(&raw);
        assert!(events.is_empty());
    }

    #[test]
    fn test_is_rate_limit_error() {
        assert!(is_rate_limit_error("Error 429: Too Many Requests"));
        assert!(is_rate_limit_error("rate limit exceeded"));
        assert!(is_rate_limit_error("too many requests"));
        assert!(is_rate_limit_error("rate_limit_error"));
        assert!(!is_rate_limit_error("normal error message"));
        assert!(!is_rate_limit_error(""));
    }
}
