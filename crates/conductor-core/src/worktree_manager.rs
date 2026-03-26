//! Git worktree isolation for parallel musician execution.
//!
//! Each worker gets its own worktree on a unique branch, preventing
//! file conflicts during parallel execution. After completion, the
//! worktree branch is merged back to the main branch.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use tokio::process::Command;

use conductor_types::truncate_str;

use crate::CoreError;

/// Result of a merge operation.
#[derive(Debug, Clone)]
pub struct MergeResult {
    pub success: bool,
    pub conflicts: Vec<String>,
    pub resolved: bool,
}

/// Context for a task being merged, used for conflict resolution.
#[derive(Debug, Clone)]
pub struct TaskContext {
    pub title: String,
    pub summary: String,
}

/// Extended task context for batch merge ordering.
#[derive(Debug, Clone)]
pub struct BatchTaskContext {
    pub title: String,
    pub summary: String,
    pub files_modified: Vec<String>,
}

/// Internal worktree tracking info.
#[derive(Debug, Clone)]
struct WorktreeInfo {
    path: PathBuf,
    branch: String,
}

/// Manages git worktrees for worker isolation.
pub struct WorktreeManager {
    project_path: PathBuf,
    worktrees: HashMap<String, WorktreeInfo>,
    is_git_repo: Option<bool>,
}

impl WorktreeManager {
    pub fn new(project_path: impl Into<PathBuf>) -> Self {
        Self {
            project_path: project_path.into(),
            worktrees: HashMap::new(),
            is_git_repo: None,
        }
    }

    /// Check if the project path is inside a git repository.
    pub async fn is_git_repo(&mut self) -> bool {
        if let Some(cached) = self.is_git_repo {
            return cached;
        }
        let result = Command::new("git")
            .args(["rev-parse", "--git-dir"])
            .current_dir(&self.project_path)
            .output()
            .await;
        let is_repo = result.map(|o| o.status.success()).unwrap_or(false);
        self.is_git_repo = Some(is_repo);
        is_repo
    }

    /// Get the current branch name.
    pub async fn get_current_branch(&self) -> Result<String, CoreError> {
        let output = git_cmd(&self.project_path, &["rev-parse", "--abbrev-ref", "HEAD"]).await?;
        Ok(output.trim().to_string())
    }

    /// Create a worktree for a worker.
    pub async fn create(
        &mut self,
        worker_id: &str,
        task_slug: &str,
    ) -> Result<(PathBuf, String), CoreError> {
        let branch = format!("conductor/{worker_id}/{task_slug}");
        let worktree_path = self
            .project_path
            .join(".conductor-worktrees")
            .join(format!("{worker_id}-{task_slug}"));

        // Create branch from current HEAD
        if git_cmd(&self.project_path, &["branch", &branch]).await.is_err() {
            // Branch may already exist — delete and retry
            let _ = git_cmd(&self.project_path, &["branch", "-D", &branch]).await;
            git_cmd(&self.project_path, &["branch", &branch]).await.map_err(|e| {
                CoreError::Git(format!("Failed to create branch {branch}: {e}"))
            })?;
        }

        // Remove stale worktree if it exists from a previous crashed run
        if worktree_path.exists() {
            let _ =
                git_cmd(&self.project_path, &["worktree", "remove", &path_str(&worktree_path), "--force"])
                    .await;
            if worktree_path.exists() {
                tokio::fs::remove_dir_all(&worktree_path).await?;
            }
            let _ = git_cmd(&self.project_path, &["worktree", "prune"]).await;
        }

        // Create worktree
        git_cmd(
            &self.project_path,
            &["worktree", "add", &path_str(&worktree_path), &branch],
        )
        .await?;

        // Symlink node_modules from the main project
        self.symlink_node_modules(&worktree_path).await;

        self.worktrees.insert(
            worker_id.to_string(),
            WorktreeInfo {
                path: worktree_path.clone(),
                branch: branch.clone(),
            },
        );

        Ok((worktree_path, branch))
    }

    /// Merge a worker's branch back to the main branch, with automatic conflict resolution.
    pub async fn merge(
        &self,
        worker_id: &str,
        task_context: Option<&TaskContext>,
    ) -> Result<MergeResult, CoreError> {
        let worktree = match self.worktrees.get(worker_id) {
            Some(wt) => wt,
            None => {
                return Ok(MergeResult {
                    success: false,
                    conflicts: vec!["No worktree found".to_string()],
                    resolved: false,
                });
            }
        };

        let merge_msg = format!("conductor: merge {}", worktree.branch);
        let merge_result = git_cmd(
            &self.project_path,
            &["merge", "--no-ff", &worktree.branch, "-m", &merge_msg],
        )
        .await;

        if merge_result.is_ok() {
            return Ok(MergeResult {
                success: true,
                conflicts: vec![],
                resolved: false,
            });
        }

        // Merge conflict detected — attempt automatic resolution
        let conflict_details = match &merge_result {
            Err(CoreError::Git(msg)) => msg.clone(),
            Err(e) => e.to_string(),
            _ => String::new(),
        };

        // Get list of conflicted files
        let conflicted_files = match git_cmd(
            &self.project_path,
            &["diff", "--name-only", "--diff-filter=U"],
        )
        .await
        {
            Ok(stdout) => stdout
                .trim()
                .split('\n')
                .filter(|s| !s.is_empty())
                .map(String::from)
                .collect::<Vec<_>>(),
            Err(_) => {
                let _ = git_cmd(&self.project_path, &["merge", "--abort"]).await;
                return Ok(MergeResult {
                    success: false,
                    conflicts: vec![conflict_details],
                    resolved: false,
                });
            }
        };

        if conflicted_files.is_empty() {
            let _ = git_cmd(&self.project_path, &["merge", "--abort"]).await;
            return Ok(MergeResult {
                success: false,
                conflicts: vec![conflict_details],
                resolved: false,
            });
        }

        // Attempt auto-resolution using Claude
        match self
            .resolve_conflicts(&conflicted_files, &worktree.branch, task_context)
            .await
        {
            Ok(true) => {
                // Stage resolved files and complete the merge
                let mut args = vec!["add".to_string()];
                args.extend(conflicted_files.clone());
                let arg_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
                if git_cmd(&self.project_path, &arg_refs).await.is_ok()
                    && git_cmd(&self.project_path, &["commit", "--no-edit"])
                        .await
                        .is_ok()
                {
                    return Ok(MergeResult {
                        success: true,
                        conflicts: conflicted_files,
                        resolved: true,
                    });
                }
            }
            Ok(false) => {}
            Err(e) => {
                tracing::error!("Merge conflict resolution failed: {e}");
            }
        }

        // Resolution failed — abort the merge
        let _ = git_cmd(&self.project_path, &["merge", "--abort"]).await;
        let conflicts = if !conflicted_files.is_empty() {
            conflicted_files
                .iter()
                .map(|f| format!("CONFLICT: {f}"))
                .collect()
        } else {
            vec![conflict_details]
        };

        Ok(MergeResult {
            success: false,
            conflicts,
            resolved: false,
        })
    }

    /// Attempt to resolve merge conflicts using Claude (Sonnet for speed).
    async fn resolve_conflicts(
        &self,
        files: &[String],
        branch_name: &str,
        task_context: Option<&TaskContext>,
    ) -> Result<bool, CoreError> {
        let mut all_resolved = true;

        for file in files {
            let file_path = self.project_path.join(file);
            if !file_path.exists() {
                all_resolved = false;
                continue;
            }

            let conflicted_content = tokio::fs::read_to_string(&file_path).await?;

            // Only attempt resolution if we see actual conflict markers
            if !conflicted_content.contains("<<<<<<<") || !conflicted_content.contains(">>>>>>>") {
                all_resolved = false;
                continue;
            }

            let task_info = match task_context {
                Some(ctx) => format!(
                    "\nThe branch \"{}\" was implementing: \"{}\"\nSummary: {}",
                    branch_name, ctx.title, ctx.summary
                ),
                None => format!("\nThe branch being merged is: \"{branch_name}\""),
            };

            // Truncate content to 8000 chars like the TS version
            let content_slice = truncate_str(&conflicted_content, 8000);

            let prompt = format!(
                "Resolve the merge conflicts in this file. Output ONLY the resolved file content \
                 with no explanation, no markdown fences, no commentary.{task_info}\n\n\
                 The goal is to keep BOTH sides' changes integrated correctly. If the changes are \
                 in different parts of the file, include both. If they modify the same code, merge \
                 the intent of both changes.\n\n\
                 File: {file}\n```\n{content_slice}\n```\n\n\
                 Output the complete resolved file content:"
            );

            match resolve_with_claude(&self.project_path, &prompt).await {
                Ok(result) => {
                    // Validate the result doesn't still contain conflict markers
                    if result.contains("<<<<<<<")
                        || result.contains(">>>>>>>")
                        || result.contains("=======")
                    {
                        all_resolved = false;
                        continue;
                    }
                    tokio::fs::write(&file_path, &result).await?;
                }
                Err(_) => {
                    all_resolved = false;
                }
            }
        }

        Ok(all_resolved)
    }

    /// Symlink node_modules from the main project into the worktree.
    async fn symlink_node_modules(&self, worktree_path: &Path) {
        let main_node_modules = self.project_path.join("node_modules");
        let wt_node_modules = worktree_path.join("node_modules");

        if main_node_modules.exists() && !wt_node_modules.exists() {
            #[cfg(unix)]
            let _ = tokio::fs::symlink(&main_node_modules, &wt_node_modules).await;
            #[cfg(windows)]
            let _ = tokio::fs::symlink_dir(&main_node_modules, &wt_node_modules).await;
        }

        // Handle monorepo subdirectories
        if let Ok(mut entries) = tokio::fs::read_dir(worktree_path).await {
            while let Ok(Some(entry)) = entries.next_entry().await {
                let name = entry.file_name();
                let name_str = name.to_string_lossy();
                if name_str == "node_modules" || name_str.starts_with('.') {
                    continue;
                }
                let Ok(file_type) = entry.file_type().await else {
                    continue;
                };
                if !file_type.is_dir() {
                    continue;
                }

                let sub_dir = worktree_path.join(&name);
                if !sub_dir.join("package.json").exists() {
                    continue;
                }

                let sub_nm = self.project_path.join(&name).join("node_modules");
                let wt_sub_nm = sub_dir.join("node_modules");
                if sub_nm.exists() && !wt_sub_nm.exists() {
                    #[cfg(unix)]
                    let _ = tokio::fs::symlink(&sub_nm, &wt_sub_nm).await;
                    #[cfg(windows)]
                    let _ = tokio::fs::symlink_dir(&sub_nm, &wt_sub_nm).await;
                }
            }
        }
    }

    /// Remove a worktree and its branch.
    pub async fn cleanup(&mut self, worker_id: &str) {
        let worktree = match self.worktrees.remove(worker_id) {
            Some(wt) => wt,
            None => return,
        };

        // Remove worktree
        if git_cmd(
            &self.project_path,
            &["worktree", "remove", &path_str(&worktree.path), "--force"],
        )
        .await
        .is_err()
        {
            if worktree.path.exists() {
                let _ = tokio::fs::remove_dir_all(&worktree.path).await;
            }
            let _ = git_cmd(&self.project_path, &["worktree", "prune"]).await;
        }

        // Delete branch
        let _ = git_cmd(&self.project_path, &["branch", "-D", &worktree.branch]).await;
    }

    /// Clean up all worktrees.
    pub async fn cleanup_all(&mut self) {
        let worker_ids: Vec<String> = self.worktrees.keys().cloned().collect();
        for id in worker_ids {
            self.cleanup(&id).await;
        }
    }

    /// Get worktree info for a worker.
    pub fn get_worktree(&self, worker_id: &str) -> Option<(&Path, &str)> {
        self.worktrees
            .get(worker_id)
            .map(|wt| (wt.path.as_path(), wt.branch.as_str()))
    }

    /// Check if a worktree path still exists on disk.
    pub fn worktree_exists(&self, worker_id: &str) -> bool {
        self.worktrees
            .get(worker_id)
            .map(|wt| wt.path.exists())
            .unwrap_or(false)
    }

    /// Rebuild the in-memory worktree map from session snapshots.
    /// Called on resume to restore state lost during a crash.
    /// Only registers worktrees whose directories still exist on disk.
    pub fn restore_from_snapshots(
        &mut self,
        snapshots: &[conductor_types::WorktreeSnapshot],
    ) {
        for snap in snapshots {
            if snap.status != conductor_types::WorktreeStatus::Active {
                continue;
            }
            let path = PathBuf::from(&snap.path);
            if !path.exists() {
                continue;
            }
            self.worktrees.insert(
                snap.worker_id.clone(),
                WorktreeInfo {
                    path,
                    branch: snap.branch.clone(),
                },
            );
        }
    }

    /// Batch merge multiple worker branches at a phase boundary.
    /// Orders merges by file overlap score to minimize conflicts.
    pub async fn batch_merge(
        &self,
        worker_ids: &[String],
        task_contexts: &HashMap<String, BatchTaskContext>,
    ) -> HashMap<String, MergeResult> {
        let mut results = HashMap::new();

        // Score each worker by file overlap with other workers — lower overlap = merge first
        let mut scored: Vec<(&String, usize)> = worker_ids
            .iter()
            .map(|id| {
                let my_files: std::collections::HashSet<&str> = task_contexts
                    .get(id)
                    .map(|ctx| ctx.files_modified.iter().map(|s| s.as_str()).collect())
                    .unwrap_or_default();
                let mut overlap_score = 0usize;
                for other_id in worker_ids {
                    if other_id == id {
                        continue;
                    }
                    if let Some(other_ctx) = task_contexts.get(other_id) {
                        for f in &other_ctx.files_modified {
                            if my_files.contains(f.as_str()) {
                                overlap_score += 1;
                            }
                        }
                    }
                }
                (id, overlap_score)
            })
            .collect();

        scored.sort_by_key(|&(_, score)| score);

        for (id, _) in scored {
            let ctx = task_contexts.get(id).map(|c| TaskContext {
                title: c.title.clone(),
                summary: c.summary.clone(),
            });
            match self.merge(id, ctx.as_ref()).await {
                Ok(result) => {
                    results.insert(id.clone(), result);
                }
                Err(e) => {
                    results.insert(
                        id.clone(),
                        MergeResult {
                            success: false,
                            conflicts: vec![format!("Merge threw: {e}")],
                            resolved: false,
                        },
                    );
                }
            }
        }

        results
    }
}

/// Run a git command and return stdout on success.
async fn git_cmd(cwd: &Path, args: &[&str]) -> Result<String, CoreError> {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .await
        .map_err(|e| CoreError::Git(format!("failed to run git {}: {e}", args.join(" "))))?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(CoreError::Git(format!(
            "git {} failed: {}",
            args.join(" "),
            stderr.trim()
        )))
    }
}

/// Resolve merge conflicts by spawning claude CLI with sonnet model.
async fn resolve_with_claude(cwd: &Path, prompt: &str) -> Result<String, CoreError> {
    let output = Command::new("claude")
        .args(["-p", prompt, "--model", "sonnet", "--max-turns", "1"])
        .current_dir(cwd)
        .output()
        .await
        .map_err(|e| CoreError::Git(format!("failed to run claude for conflict resolution: {e}")))?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(CoreError::Git(format!(
            "claude conflict resolution failed: {}",
            stderr.trim()
        )))
    }
}

/// Convert a PathBuf to a string for git command arguments.
fn path_str(p: &Path) -> String {
    p.to_string_lossy().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_creates_empty_manager() {
        let mgr = WorktreeManager::new("/tmp/test");
        assert!(mgr.worktrees.is_empty());
        assert!(mgr.is_git_repo.is_none());
    }

    #[test]
    fn get_worktree_returns_none_for_unknown() {
        let mgr = WorktreeManager::new("/tmp/test");
        assert!(mgr.get_worktree("unknown").is_none());
    }

    #[test]
    fn worktree_exists_returns_false_for_unknown() {
        let mgr = WorktreeManager::new("/tmp/test");
        assert!(!mgr.worktree_exists("unknown"));
    }

    #[test]
    fn restore_from_snapshots_only_restores_active() {
        use conductor_types::{WorktreeSnapshot, WorktreeStatus};

        let mut mgr = WorktreeManager::new("/tmp/test");
        let snapshots = vec![
            WorktreeSnapshot {
                worker_id: "m1".to_string(),
                task_index: 0,
                branch: "conductor/m1/task".to_string(),
                path: "/tmp/nonexistent-path-xyz".to_string(),
                last_commit_sha: "abc123".to_string(),
                status: WorktreeStatus::Active,
            },
            WorktreeSnapshot {
                worker_id: "m2".to_string(),
                task_index: 1,
                branch: "conductor/m2/task".to_string(),
                path: "/tmp".to_string(), // exists
                last_commit_sha: "def456".to_string(),
                status: WorktreeStatus::Active,
            },
            WorktreeSnapshot {
                worker_id: "m3".to_string(),
                task_index: 2,
                branch: "conductor/m3/task".to_string(),
                path: "/tmp".to_string(),
                last_commit_sha: "ghi789".to_string(),
                status: WorktreeStatus::Completed,
            },
        ];

        mgr.restore_from_snapshots(&snapshots);

        // m1: active but path doesn't exist — not restored
        assert!(mgr.get_worktree("m1").is_none());
        // m2: active and path exists — restored
        assert!(mgr.get_worktree("m2").is_some());
        let (path, branch) = mgr.get_worktree("m2").unwrap();
        assert_eq!(path, Path::new("/tmp"));
        assert_eq!(branch, "conductor/m2/task");
        // m3: completed — not restored
        assert!(mgr.get_worktree("m3").is_none());
    }

    #[test]
    fn merge_result_defaults() {
        let result = MergeResult {
            success: true,
            conflicts: vec![],
            resolved: false,
        };
        assert!(result.success);
        assert!(result.conflicts.is_empty());
        assert!(!result.resolved);
    }

    #[tokio::test]
    async fn cleanup_is_idempotent() {
        let mut mgr = WorktreeManager::new("/tmp/test");
        // Cleaning up a non-existent worker should not panic
        mgr.cleanup("nonexistent").await;
        assert!(mgr.worktrees.is_empty());
    }

    #[tokio::test]
    async fn cleanup_all_on_empty() {
        let mut mgr = WorktreeManager::new("/tmp/test");
        mgr.cleanup_all().await;
        assert!(mgr.worktrees.is_empty());
    }
}
