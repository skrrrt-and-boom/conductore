//! Rate limit detection and recovery.
//!
//! Detects rate limits from 3 signals:
//! 1. `rate_limit_event` in NDJSON stream (primary)
//! 2. stderr pattern matching (429, rate limit)
//! 3. `result` event with error containing rate limit message
//!
//! On detection: pauses workers, notifies orchestra via channel,
//! and tracks probe countdown for TUI display.

use conductor_types::{ClaudeEvent, ClaudeEventType, RateLimitState, RateLimitStatus};

/// Default probe interval in milliseconds (30 seconds).
pub const DEFAULT_PROBE_INTERVAL_MS: u64 = 30_000;

/// Rate limit detector and state tracker.
///
/// The orchestra owns this and calls `handle_event()` / `handle_stderr()`
/// for each musician's Claude events. When a rate limit is detected,
/// the state transitions to `Limited` and the orchestra can read the
/// state to pause musicians and start probing.
pub struct RateLimiter {
    state: RateLimitState,
    probe_interval_ms: u64,
}

impl RateLimiter {
    /// Create a new rate limiter with the given probe interval.
    pub fn new(probe_interval_ms: Option<u64>) -> Self {
        Self {
            state: RateLimitState::default(),
            probe_interval_ms: probe_interval_ms.unwrap_or(DEFAULT_PROBE_INTERVAL_MS),
        }
    }

    /// Get the current rate limit state.
    pub fn state(&self) -> &RateLimitState {
        &self.state
    }

    /// Returns true if currently rate limited.
    pub fn is_limited(&self) -> bool {
        self.state.status == RateLimitStatus::Limited
    }

    /// Process a Claude event and check for rate limit signals.
    ///
    /// Returns `true` if a NEW rate limit was detected (state transitioned
    /// from ok/warning to limited).
    pub fn handle_event(&mut self, event: &ClaudeEvent) -> bool {
        // Signal 1: explicit rate_limit error subtype
        if event.event_type == ClaudeEventType::Error
            && event.subtype.as_deref() == Some("rate_limit")
        {
            return self.on_limited(event.resets_at.clone());
        }

        // Signal 2: result with rate limit error message
        if event.event_type == ClaudeEventType::Result
            && event.is_error == Some(true)
            && event
                .result
                .as_deref()
                .is_some_and(|r| is_rate_limit_message(r))
        {
            return self.on_limited(None);
        }

        false
    }

    /// Handle stderr output from a Claude process.
    ///
    /// Returns `true` if a NEW rate limit was detected.
    pub fn handle_stderr(&mut self, stderr: &str) -> bool {
        if is_rate_limit_message(stderr) {
            return self.on_limited(None);
        }
        false
    }

    /// Mark rate limit as resolved (probe succeeded).
    pub fn mark_available(&mut self) {
        self.state.status = RateLimitStatus::Ok;
        self.state.resets_at = None;
        self.state.next_probe_in = None;
    }

    /// Record a failed probe attempt.
    pub fn record_probe_failure(&mut self) {
        self.state.probe_count += 1;
        self.state.last_probe_at = Some(chrono::Utc::now().to_rfc3339());
        self.state.next_probe_in = Some(self.probe_interval_ms);
    }

    /// Record a successful probe — resets to ok state.
    pub fn record_probe_success(&mut self) {
        self.state.probe_count += 1;
        self.state.last_probe_at = Some(chrono::Utc::now().to_rfc3339());
        self.mark_available();
    }

    /// Update the countdown timer (called from TUI tick).
    pub fn tick(&mut self, delta_ms: u64) {
        if let Some(ref mut remaining) = self.state.next_probe_in {
            *remaining = remaining.saturating_sub(delta_ms);
        }
    }

    /// Get the probe interval in milliseconds.
    pub fn probe_interval_ms(&self) -> u64 {
        self.probe_interval_ms
    }

    fn on_limited(&mut self, resets_at: Option<String>) -> bool {
        if self.state.status == RateLimitStatus::Limited {
            return false; // Already limited
        }

        self.state = RateLimitState {
            status: RateLimitStatus::Limited,
            resets_at,
            last_probe_at: None,
            probe_count: 0,
            next_probe_in: Some(self.probe_interval_ms),
        };

        true
    }
}

/// Check if a text message indicates a rate limit.
pub fn is_rate_limit_message(text: &str) -> bool {
    let lower = text.to_lowercase();
    lower.contains("429")
        || lower.contains("rate limit")
        || lower.contains("too many requests")
        || lower.contains("rate_limit")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_429_status_code() {
        assert!(is_rate_limit_message("Error 429: Too Many Requests"));
    }

    #[test]
    fn detects_rate_limit_text() {
        assert!(is_rate_limit_message("You have been rate limited"));
    }

    #[test]
    fn detects_too_many_requests() {
        assert!(is_rate_limit_message("Too many requests, please slow down"));
    }

    #[test]
    fn detects_rate_limit_underscore_variant() {
        assert!(is_rate_limit_message("rate_limit_exceeded"));
    }

    #[test]
    fn is_case_insensitive() {
        assert!(is_rate_limit_message("RATE LIMIT exceeded"));
    }

    #[test]
    fn returns_false_for_normal_errors() {
        assert!(!is_rate_limit_message("Internal server error"));
        assert!(!is_rate_limit_message("Connection refused"));
        assert!(!is_rate_limit_message(""));
    }

    #[test]
    fn handle_event_detects_rate_limit_error() {
        let mut rl = RateLimiter::new(None);
        let event = ClaudeEvent {
            event_type: ClaudeEventType::Error,
            subtype: Some("rate_limit".into()),
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
            resets_at: Some("2026-01-01T00:05:00Z".into()),
        };
        assert!(rl.handle_event(&event));
        assert!(rl.is_limited());
        assert_eq!(rl.state().resets_at.as_deref(), Some("2026-01-01T00:05:00Z"));
    }

    #[test]
    fn handle_event_does_not_double_trigger() {
        let mut rl = RateLimiter::new(None);
        let event = ClaudeEvent {
            event_type: ClaudeEventType::Error,
            subtype: Some("rate_limit".into()),
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
        };
        assert!(rl.handle_event(&event));  // first time → true
        assert!(!rl.handle_event(&event)); // already limited → false
    }

    #[test]
    fn probe_success_resets_state() {
        let mut rl = RateLimiter::new(None);
        // Trigger limit
        assert!(rl.handle_stderr("Error 429"));
        assert!(rl.is_limited());
        // Probe success
        rl.record_probe_success();
        assert!(!rl.is_limited());
        assert_eq!(rl.state().status, RateLimitStatus::Ok);
    }

    #[test]
    fn tick_decrements_countdown() {
        let mut rl = RateLimiter::new(Some(10_000));
        rl.handle_stderr("429");
        assert_eq!(rl.state().next_probe_in, Some(10_000));
        rl.tick(3_000);
        assert_eq!(rl.state().next_probe_in, Some(7_000));
        rl.tick(10_000);
        assert_eq!(rl.state().next_probe_in, Some(0));
    }
}
