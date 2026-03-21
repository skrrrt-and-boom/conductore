//! macOS sleep prevention via `caffeinate`.
//!
//! Flags used:
//!   -d  prevent display sleep
//!   -i  prevent idle sleep (critical — no keyboard input during autonomous runs)
//!   -m  prevent disk sleep (worktrees, logs, shared memory I/O)
//!   -s  prevent system sleep on AC power only (battery-safe)
//!   -w  tie lifetime to conductor's PID (auto-cleanup on crash)

use tokio::process::Child;
use tokio::process::Command;

/// Manages a macOS `caffeinate` child process to prevent sleep during long
/// orchestration runs.
pub struct Caffeinate {
    child: Option<Child>,
    warned_non_mac: bool,
}

impl Caffeinate {
    pub fn new() -> Self {
        Self {
            child: None,
            warned_non_mac: false,
        }
    }

    /// Start the caffeinate process if not already running.
    pub async fn start(&mut self) {
        if cfg!(not(target_os = "macos")) {
            if !self.warned_non_mac {
                self.warned_non_mac = true;
                tracing::warn!(
                    "[conductor] Sleep prevention unavailable on this platform. \
                     Consider using `systemd-inhibit` or similar to prevent OS sleep \
                     during long orchestration sessions."
                );
            }
            return;
        }

        if self.is_active() {
            return;
        }

        let pid = std::process::id();
        match Command::new("caffeinate")
            .args(["-dims", "-w", &pid.to_string()])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true)
            .spawn()
        {
            Ok(child) => {
                self.child = Some(child);
            }
            Err(e) => {
                tracing::warn!("[conductor] Failed to spawn caffeinate: {e}");
            }
        }
    }

    /// Stop the caffeinate process if running.
    pub async fn stop(&mut self) {
        if let Some(mut child) = self.child.take() {
            if let Err(e) = child.kill().await {
                tracing::warn!("[conductor] Failed to kill caffeinate: {e}");
            }
        }
    }

    /// Returns true if the caffeinate process is still running.
    pub fn is_active(&mut self) -> bool {
        if let Some(child) = &mut self.child {
            match child.try_wait() {
                Ok(None) => true,    // still running
                Ok(Some(_)) | Err(_) => {
                    self.child = None;
                    false
                }
            }
        } else {
            false
        }
    }
}

impl Default for Caffeinate {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for Caffeinate {
    fn drop(&mut self) {
        if let Some(child) = &mut self.child {
            // start_kill() is sync — suitable for Drop
            let _ = child.start_kill();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_active_returns_false_initially() {
        let mut c = Caffeinate::new();
        assert!(!c.is_active());
    }

    #[tokio::test]
    async fn stop_is_idempotent() {
        let mut c = Caffeinate::new();
        c.stop().await;
        c.stop().await;
        assert!(!c.is_active());
    }

    #[cfg(not(target_os = "macos"))]
    #[tokio::test]
    async fn start_on_non_mac_does_not_spawn_process() {
        let mut c = Caffeinate::new();
        c.start().await;
        assert!(!c.is_active());
        // warned_non_mac should now be set — calling start again should not warn twice
        c.start().await;
        assert!(!c.is_active());
    }
}
