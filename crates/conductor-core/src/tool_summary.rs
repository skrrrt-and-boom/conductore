//! Human-readable summaries of tool invocations for the TUI.
//!
//! Used by both conductor and musician to produce short descriptions
//! of what Claude is doing (e.g., "Reading src/main.rs").

use serde_json::Value;

/// Produce a human-readable summary of a tool invocation.
///
/// # Examples
/// ```
/// use conductor_core::tool_summary::summarize_tool_use;
/// use serde_json::json;
///
/// let input = json!({"file_path": "/src/main.rs"});
/// assert_eq!(summarize_tool_use("Read", Some(&input)), "Reading /src/main.rs");
/// ```
pub fn summarize_tool_use(tool_name: &str, input: Option<&Value>) -> String {
    let get_str = |key: &str| -> Option<&str> {
        input
            .and_then(|v| v.get(key))
            .and_then(|v| v.as_str())
    };

    match tool_name {
        "Read" => {
            let path = get_str("file_path").unwrap_or("file");
            format!("Reading {path}")
        }
        "Write" => {
            let path = get_str("file_path").unwrap_or("file");
            format!("Writing {path}")
        }
        "Edit" => {
            let path = get_str("file_path").unwrap_or("file");
            format!("Editing {path}")
        }
        "Glob" => {
            let pattern = get_str("pattern").unwrap_or("");
            format!("Searching files: {pattern}")
        }
        "Grep" => {
            let pattern = get_str("pattern").unwrap_or("");
            format!("Searching code: {pattern}")
        }
        "Bash" => {
            let cmd = get_str("command").unwrap_or("");
            let truncated: String = cmd.chars().take(60).collect();
            format!("Running: {truncated}")
        }
        _ => tool_name.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn summarizes_read_with_file_path() {
        let input = json!({"file_path": "/src/main.ts"});
        assert_eq!(summarize_tool_use("Read", Some(&input)), "Reading /src/main.ts");
    }

    #[test]
    fn summarizes_write_with_file_path() {
        let input = json!({"file_path": "/src/new.ts"});
        assert_eq!(summarize_tool_use("Write", Some(&input)), "Writing /src/new.ts");
    }

    #[test]
    fn summarizes_edit_with_file_path() {
        let input = json!({"file_path": "/src/fix.ts"});
        assert_eq!(summarize_tool_use("Edit", Some(&input)), "Editing /src/fix.ts");
    }

    #[test]
    fn summarizes_glob_with_pattern() {
        let input = json!({"pattern": "**/*.ts"});
        assert_eq!(summarize_tool_use("Glob", Some(&input)), "Searching files: **/*.ts");
    }

    #[test]
    fn summarizes_grep_with_pattern() {
        let input = json!({"pattern": "TODO"});
        assert_eq!(summarize_tool_use("Grep", Some(&input)), "Searching code: TODO");
    }

    #[test]
    fn summarizes_bash_with_command_truncation() {
        let long_cmd = "a".repeat(100);
        let input = json!({"command": long_cmd});
        let result = summarize_tool_use("Bash", Some(&input));
        assert_eq!(result, format!("Running: {}", "a".repeat(60)));
    }

    #[test]
    fn handles_unknown_tools() {
        let input = json!({});
        assert_eq!(summarize_tool_use("CustomTool", Some(&input)), "CustomTool");
    }

    #[test]
    fn handles_none_input() {
        assert_eq!(summarize_tool_use("Read", None), "Reading file");
    }
}
