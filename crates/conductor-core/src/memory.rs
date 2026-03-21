//! Shared memory manager using a markdown file.
//!
//! Workers inject shared memory into their system prompts.
//! This file is human-readable and serves as a lightweight coordination
//! mechanism — like a team wiki that every worker reads before starting
//! and writes to after finishing.
//!
//! All writes use atomic rename (write tmp → rename) so concurrent
//! readers never see a half-written file.

use std::path::{Path, PathBuf};
use tokio::fs;

use crate::CoreError;

/// Shared memory file manager.
pub struct SharedMemory {
    file_path: PathBuf,
}

impl SharedMemory {
    /// Create a new shared memory manager for the given file path.
    pub fn new(file_path: impl Into<PathBuf>) -> Self {
        Self {
            file_path: file_path.into(),
        }
    }

    /// Initialize the shared memory file if it doesn't exist.
    pub async fn init(&self) -> Result<(), CoreError> {
        if !self.file_path.exists() {
            if let Some(parent) = self.file_path.parent() {
                fs::create_dir_all(parent).await?;
            }
            fs::write(&self.file_path, "# Shared Memory\n\n").await?;
        }
        Ok(())
    }

    /// Read the full shared memory content.
    pub async fn read(&self) -> Result<String, CoreError> {
        if !self.file_path.exists() {
            return Ok(String::new());
        }
        Ok(fs::read_to_string(&self.file_path).await?)
    }

    /// Append a section to shared memory.
    ///
    /// If a section with the same header already exists, it is replaced.
    /// Uses atomic write (tmp file + rename) for safety.
    pub async fn append(&self, section: &str, content: &str) -> Result<(), CoreError> {
        let existing = self.read().await?;

        let section_header = format!("## {section}");
        let updated = if let Some(start) = existing.find(&section_header) {
            // Find the end of this section (next ## header or end of file)
            let after_header = start + section_header.len();
            let end = existing[after_header..]
                .find("\n## ")
                .map(|pos| after_header + pos)
                .unwrap_or(existing.len());
            // Replace the section content
            let mut result = existing[..start].to_string();
            result.push_str(&section_header);
            result.push('\n');
            result.push_str(content);
            result.push('\n');
            result.push_str(&existing[end..]);
            result
        } else {
            format!("{}\n\n{section_header}\n{content}\n", existing.trim_end())
        };

        atomic_write(&self.file_path, &updated).await?;
        Ok(())
    }

    /// Read shared memory, truncating at section boundaries to fit within max_chars.
    ///
    /// Keeps the most recent complete sections that fit within the limit.
    pub async fn read_truncated(&self, max_chars: usize) -> Result<String, CoreError> {
        let full = self.read().await?;
        if full.len() <= max_chars {
            return Ok(full);
        }

        // Split into sections
        let parts: Vec<&str> = full.split("\n## ").collect();
        if parts.len() <= 1 {
            // No section boundaries — hard slice from end
            let start = full.len().saturating_sub(max_chars);
            return Ok(format!(
                "[...earlier entries truncated]\n\n{}",
                &full[start..]
            ));
        }

        // Build result from the end, keeping complete sections
        let mut result = String::new();
        for part in parts.iter().rev() {
            let candidate = if result.is_empty() {
                format!("\n## {part}")
            } else {
                format!("\n## {part}{result}")
            };
            if candidate.len() > max_chars && !result.is_empty() {
                break;
            }
            result = candidate;
        }

        if result.len() < full.len() {
            result = format!("[...earlier entries truncated]{result}");
        }
        Ok(result)
    }

    /// Get section count and total size for TUI display.
    pub async fn stats(&self) -> Result<MemoryStats, CoreError> {
        let content = self.read().await?;
        let sections = content.matches("\n## ").count() + content.starts_with("## ") as usize;
        Ok(MemoryStats {
            sections,
            size_bytes: content.len(),
        })
    }

    /// Get entries added since a given character offset.
    ///
    /// Returns the new content and the updated offset for next call.
    pub async fn get_entries_since(
        &self,
        char_offset: usize,
    ) -> Result<(String, usize), CoreError> {
        let full = self.read().await?;
        if full.len() <= char_offset {
            return Ok((String::new(), char_offset));
        }
        let content = full[char_offset..].to_string();
        Ok((content, full.len()))
    }
}

/// Memory stats for TUI display.
pub struct MemoryStats {
    pub sections: usize,
    pub size_bytes: usize,
}

/// Atomic file write: writes to a temp file then renames.
///
/// `rename()` is atomic on POSIX filesystems, so concurrent readers
/// always see either the old or new complete content.
async fn atomic_write(path: &Path, content: &str) -> Result<(), CoreError> {
    let dir = path.parent().unwrap_or(Path::new("."));
    let tmp_name = format!(
        ".{}.{}.tmp",
        std::process::id(),
        rand_suffix()
    );
    let tmp_path = dir.join(tmp_name);
    fs::write(&tmp_path, content).await?;
    fs::rename(&tmp_path, path).await?;
    Ok(())
}

fn rand_suffix() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos();
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{nanos:x}-{seq}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn temp_path() -> PathBuf {
        let dir = std::env::temp_dir().join("conductor-test-memory");
        std::fs::create_dir_all(&dir).ok();
        dir.join(format!("shared-{}.md", rand_suffix()))
    }

    #[tokio::test]
    async fn init_creates_file() {
        let path = temp_path();
        let mem = SharedMemory::new(&path);
        mem.init().await.unwrap();
        assert!(path.exists());
        let content = fs::read_to_string(&path).await.unwrap();
        assert!(content.contains("# Shared Memory"));
        fs::remove_file(&path).await.ok();
    }

    #[tokio::test]
    async fn read_returns_empty_when_missing() {
        let mem = SharedMemory::new("/tmp/nonexistent-conductor-test.md");
        let content = mem.read().await.unwrap();
        assert!(content.is_empty());
    }

    #[tokio::test]
    async fn append_adds_section() {
        let path = temp_path();
        let mem = SharedMemory::new(&path);
        mem.init().await.unwrap();
        mem.append("Worker 1", "Did something useful").await.unwrap();
        let content = mem.read().await.unwrap();
        assert!(content.contains("## Worker 1"));
        assert!(content.contains("Did something useful"));
        fs::remove_file(&path).await.ok();
    }

    #[tokio::test]
    async fn append_replaces_existing_section() {
        let path = temp_path();
        let mem = SharedMemory::new(&path);
        mem.init().await.unwrap();
        mem.append("Worker 1", "First result").await.unwrap();
        mem.append("Worker 1", "Updated result").await.unwrap();
        let content = mem.read().await.unwrap();
        assert!(content.contains("Updated result"));
        // Should NOT contain duplicate headers (may still have the first one in replacement)
        let header_count = content.matches("## Worker 1").count();
        assert_eq!(header_count, 1);
        fs::remove_file(&path).await.ok();
    }

    #[tokio::test]
    async fn stats_counts_sections() {
        let path = temp_path();
        let mem = SharedMemory::new(&path);
        mem.init().await.unwrap();
        mem.append("Section A", "content a").await.unwrap();
        mem.append("Section B", "content b").await.unwrap();
        let stats = mem.stats().await.unwrap();
        assert_eq!(stats.sections, 2);
        fs::remove_file(&path).await.ok();
    }

    #[tokio::test]
    async fn get_entries_since_returns_new_content() {
        let path = temp_path();
        let mem = SharedMemory::new(&path);
        mem.init().await.unwrap();
        mem.append("First", "data 1").await.unwrap();
        let full = mem.read().await.unwrap();
        let offset = full.len();
        mem.append("Second", "data 2").await.unwrap();
        let (new_content, new_offset) = mem.get_entries_since(offset).await.unwrap();
        assert!(new_content.contains("Second"));
        assert!(new_offset > offset);
        fs::remove_file(&path).await.ok();
    }
}
