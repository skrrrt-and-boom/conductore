//! Session and task persistence to disk.
//!
//! Sessions are stored at `~/.conductor/sessions/{session_id}/` with:
//! - `session.json` — session metadata
//! - `tasks/` — individual task YAML files
//! - `logs/` — per-musician log files
//! - `memory/` — shared memory files

use std::path::{Path, PathBuf};
use tokio::fs;

use conductor_types::{SessionData, Task};

use crate::CoreError;

/// Base directory for conductor sessions.
fn conductor_home() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".conductor")
        .join("sessions")
}

/// Task and session persistence manager.
pub struct TaskStore {
    session_id: String,
    session_dir: PathBuf,
}

impl TaskStore {
    /// Create a task store for the given session ID.
    pub fn new(session_id: &str) -> Self {
        let session_dir = conductor_home().join(session_id);
        Self {
            session_id: session_id.to_string(),
            session_dir,
        }
    }

    /// Resolve a partial session ID (prefix) to the full ID.
    ///
    /// Returns `None` if no match or ambiguous (multiple matches).
    pub async fn resolve_id(partial_id: &str) -> Option<String> {
        let home = conductor_home();
        // Exact match
        if home.join(partial_id).exists() {
            return Some(partial_id.to_string());
        }
        // Prefix match
        let mut entries = match fs::read_dir(&home).await {
            Ok(e) => e,
            Err(_) => return None,
        };
        let mut matches = Vec::new();
        while let Ok(Some(entry)) = entries.next_entry().await {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with(partial_id) {
                matches.push(name);
            }
        }
        if matches.len() == 1 {
            Some(matches.into_iter().next().unwrap())
        } else {
            None // ambiguous or not found
        }
    }

    /// Get the base session directory path.
    pub fn base_path(&self) -> &Path {
        &self.session_dir
    }

    /// Get the tasks subdirectory path.
    pub fn tasks_dir(&self) -> PathBuf {
        self.session_dir.join("tasks")
    }

    /// Get the logs subdirectory path.
    pub fn logs_dir(&self) -> PathBuf {
        self.session_dir.join("logs")
    }

    /// Get the memory subdirectory path.
    pub fn memory_dir(&self) -> PathBuf {
        self.session_dir.join("memory")
    }

    /// Ensure all session directories exist.
    pub async fn init(&self) -> Result<(), CoreError> {
        for dir in [self.tasks_dir(), self.logs_dir(), self.memory_dir()] {
            fs::create_dir_all(&dir).await?;
        }
        Ok(())
    }

    /// Save session metadata to disk.
    pub async fn save_session(&self, data: &SessionData) -> Result<(), CoreError> {
        let path = self.session_dir.join("session.json");
        fs::create_dir_all(&self.session_dir).await?;
        let json = serde_json::to_string_pretty(data)?;
        fs::write(&path, json).await?;
        Ok(())
    }

    /// Load session metadata from disk.
    pub async fn load_session(&self) -> Result<Option<SessionData>, CoreError> {
        let path = self.session_dir.join("session.json");
        if !path.exists() {
            return Ok(None);
        }
        let content = fs::read_to_string(&path).await?;
        let data: SessionData = serde_json::from_str(&content)?;
        Ok(Some(data))
    }

    /// Save a single task to disk.
    pub async fn save_task(&self, task: &Task) -> Result<(), CoreError> {
        let filename = format!("{:03}-{}.json", task.index, slugify(&task.title));
        let path = self.tasks_dir().join(filename);
        fs::create_dir_all(self.tasks_dir()).await?;
        let json = serde_json::to_string_pretty(task)?;
        fs::write(&path, json).await?;
        Ok(())
    }

    /// Save all tasks to disk.
    pub async fn save_tasks(&self, tasks: &[Task]) -> Result<(), CoreError> {
        for task in tasks {
            self.save_task(task).await?;
        }
        Ok(())
    }

    /// Load all tasks from disk, sorted by filename.
    pub async fn load_tasks(&self) -> Result<Vec<Task>, CoreError> {
        let tasks_dir = self.tasks_dir();
        if !tasks_dir.exists() {
            return Ok(vec![]);
        }
        let mut entries = fs::read_dir(&tasks_dir).await?;
        let mut files = Vec::new();
        while let Ok(Some(entry)) = entries.next_entry().await {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.ends_with(".json") {
                files.push(entry.path());
            }
        }
        files.sort();

        let mut tasks = Vec::new();
        for path in files {
            let content = fs::read_to_string(&path).await?;
            let task: Task = serde_json::from_str(&content)?;
            tasks.push(task);
        }
        Ok(tasks)
    }

    /// Append a line to a log file.
    pub async fn append_log(&self, name: &str, line: &str) -> Result<(), CoreError> {
        let path = self.logs_dir().join(format!("{name}.log"));
        fs::create_dir_all(self.logs_dir()).await?;
        let mut content = if path.exists() {
            fs::read_to_string(&path).await.unwrap_or_default()
        } else {
            String::new()
        };
        content.push_str(line);
        content.push('\n');
        fs::write(&path, content).await?;
        Ok(())
    }

    /// List all sessions, sorted by last_updated_at (newest first).
    pub async fn list_sessions() -> Result<Vec<SessionData>, CoreError> {
        let home = conductor_home();
        if !home.exists() {
            return Ok(vec![]);
        }
        let mut entries = fs::read_dir(&home).await?;
        let mut sessions = Vec::new();
        while let Ok(Some(entry)) = entries.next_entry().await {
            let session_path = entry.path().join("session.json");
            if session_path.exists() {
                if let Ok(content) = fs::read_to_string(&session_path).await {
                    if let Ok(data) = serde_json::from_str::<SessionData>(&content) {
                        sessions.push(data);
                    }
                }
            }
        }
        sessions.sort_by(|a, b| b.last_updated_at.cmp(&a.last_updated_at));
        Ok(sessions)
    }

    /// Delete a session directory.
    pub async fn clean_session(session_id: &str) -> Result<(), CoreError> {
        let dir = conductor_home().join(session_id);
        if dir.exists() {
            fs::remove_dir_all(&dir).await?;
        }
        Ok(())
    }

    /// Delete all sessions. Returns count removed.
    pub async fn clean_all() -> Result<usize, CoreError> {
        let home = conductor_home();
        if !home.exists() {
            return Ok(0);
        }
        let mut entries = fs::read_dir(&home).await?;
        let mut count = 0;
        while let Ok(Some(entry)) = entries.next_entry().await {
            if entry.path().is_dir() {
                fs::remove_dir_all(entry.path()).await?;
                count += 1;
            }
        }
        Ok(count)
    }

    /// Delete sessions older than `max_age_days`. Returns count removed.
    pub async fn clean_older_than(max_age_days: u64) -> Result<usize, CoreError> {
        let sessions = Self::list_sessions().await?;
        let cutoff = chrono::Utc::now()
            - chrono::Duration::days(max_age_days as i64);
        let cutoff_str = cutoff.to_rfc3339();
        let mut removed = 0;
        for s in sessions {
            if s.last_updated_at < cutoff_str {
                Self::clean_session(&s.id).await?;
                removed += 1;
            }
        }
        Ok(removed)
    }

    /// Keep only the N most recent sessions. Returns count removed.
    pub async fn keep_recent(max_count: usize) -> Result<usize, CoreError> {
        let sessions = Self::list_sessions().await?; // already sorted newest-first
        let mut removed = 0;
        for s in sessions.iter().skip(max_count) {
            Self::clean_session(&s.id).await?;
            removed += 1;
        }
        Ok(removed)
    }

    /// Load a compact summary of a previous session for reference context.
    pub async fn load_session_summary(session_id: &str) -> Result<Option<String>, CoreError> {
        let resolved = match Self::resolve_id(session_id).await {
            Some(id) => id,
            None => return Ok(None),
        };

        let store = Self::new(&resolved);
        let session = match store.load_session().await? {
            Some(s) => s,
            None => return Ok(None),
        };

        let tasks = store.load_tasks().await?;
        let mut lines = vec![
            format!("## Previous Session: {resolved}"),
            format!("**Task**: {}", session.config.task_description),
            format!("**Phase**: {:?}", session.phase),
            format!("**Date**: {}", session.last_updated_at),
            String::new(),
            "### Tasks and Results".to_string(),
        ];

        for task in &tasks {
            let status_icon = match task.status {
                conductor_types::TaskStatus::Completed => "✓",
                conductor_types::TaskStatus::Failed => "✗",
                _ => "○",
            };
            lines.push(format!(
                "{status_icon} **{}** [{:?}]",
                task.title, task.status
            ));
            lines.push(format!(
                "  Description: {}",
                truncate(&task.description, 200)
            ));
            if let Some(ref result) = task.result {
                lines.push(format!("  Result: {}", truncate(&result.summary, 300)));
                if !result.files_modified.is_empty() {
                    lines.push(format!("  Files: {}", result.files_modified.join(", ")));
                }
                if let Some(ref error) = result.error {
                    lines.push(format!("  Error: {}", truncate(error, 200)));
                }
            }
        }

        // Load shared memory if available
        let shared_mem_path = conductor_home()
            .join(&resolved)
            .join("memory")
            .join("SHARED.md");
        if shared_mem_path.exists() {
            if let Ok(shared) = fs::read_to_string(&shared_mem_path).await {
                if !shared.trim().is_empty() {
                    lines.push(String::new());
                    lines.push("### Shared Memory (musician discoveries)".to_string());
                    lines.push(truncate(&shared, 2000).to_string());
                }
            }
        }

        Ok(Some(lines.join("\n")))
    }

    /// Get the session ID.
    pub fn session_id(&self) -> &str {
        &self.session_id
    }
}

fn slugify(text: &str) -> String {
    text.to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
        .chars()
        .take(50)
        .collect()
}

fn truncate(s: &str, max: usize) -> &str {
    if s.len() <= max {
        s
    } else {
        &s[..max]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugify_works() {
        assert_eq!(slugify("Hello World!"), "hello-world");
        assert_eq!(slugify("foo/bar.ts"), "foo-bar-ts");
    }

    #[test]
    fn truncate_works() {
        assert_eq!(truncate("hello", 10), "hello");
        assert_eq!(truncate("hello world", 5), "hello");
    }

    #[tokio::test]
    async fn init_creates_directories() {
        let store = TaskStore::new("test-init-session");
        store.init().await.unwrap();
        assert!(store.tasks_dir().exists());
        assert!(store.logs_dir().exists());
        assert!(store.memory_dir().exists());
        // Cleanup
        fs::remove_dir_all(store.base_path()).await.ok();
    }

    #[tokio::test]
    async fn save_and_load_session() {
        let store = TaskStore::new("test-save-load");
        store.init().await.unwrap();

        let data = SessionData {
            id: "test-save-load".into(),
            config: conductor_types::OrchestraConfig {
                project_path: "/tmp".into(),
                task_description: "Test task".into(),
                musician_count: 2,
                conductor_model: "opus".into(),
                musician_model: "sonnet".into(),
                max_turns: 10,
                dry_run: false,
                session_id: "test-save-load".into(),
                reference_session_id: None,
                verification: None,
            },
            phase: conductor_types::OrchestraPhase::Init,
            started_at: "2026-01-01T00:00:00Z".into(),
            last_updated_at: "2026-01-01T00:01:00Z".into(),
            tokens: conductor_types::TokenUsage::default(),
            tokens_estimated: false,
            tasks: vec![],
            phases: None,
            current_phase_index: None,
            worktree_state: None,
        };

        store.save_session(&data).await.unwrap();
        let loaded = store.load_session().await.unwrap().unwrap();
        assert_eq!(loaded.id, "test-save-load");
        assert_eq!(loaded.config.task_description, "Test task");

        // Cleanup
        fs::remove_dir_all(store.base_path()).await.ok();
    }

    #[tokio::test]
    async fn append_log_creates_and_appends() {
        let store = TaskStore::new("test-log");
        store.init().await.unwrap();

        store.append_log("musician-0", "Line 1").await.unwrap();
        store.append_log("musician-0", "Line 2").await.unwrap();

        let log_path = store.logs_dir().join("musician-0.log");
        let content = fs::read_to_string(&log_path).await.unwrap();
        assert!(content.contains("Line 1"));
        assert!(content.contains("Line 2"));

        // Cleanup
        fs::remove_dir_all(store.base_path()).await.ok();
    }
}
