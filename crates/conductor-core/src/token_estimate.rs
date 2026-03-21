//! Lightweight token estimation from streamed text.
//!
//! Claude Code's stream-json format only reports actual token counts in the
//! final `result` event. During streaming we estimate from content length
//! using a calibrated characters/token ratio.
//!
//! The ratio starts at 4.0 (standard for English + code) and is refined
//! after each result event reports actual token counts.

use std::sync::Mutex;

struct CalibrationState {
    ratio: f64,
    total_chars: u64,
    total_tokens: u64,
}

static CALIBRATION: Mutex<CalibrationState> = Mutex::new(CalibrationState {
    ratio: 4.0,
    total_chars: 0,
    total_tokens: 0,
});

/// Estimate token count from text length using the current calibration ratio.
pub fn estimate_tokens(text: &str) -> u64 {
    if text.is_empty() {
        return 0;
    }
    let state = CALIBRATION.lock().unwrap();
    (text.len() as f64 / state.ratio).ceil() as u64
}

/// Calibrate the estimation ratio using actual token counts from a result event.
///
/// Call after receiving actual usage data to improve future estimates.
/// The ratio is clamped to the range \[2.5, 6.0\] chars/token.
pub fn calibrate_token_estimate(actual_chars: u64, actual_tokens: u64) {
    if actual_tokens == 0 || actual_chars == 0 {
        return;
    }
    let mut state = CALIBRATION.lock().unwrap();
    state.total_chars += actual_chars;
    state.total_tokens += actual_tokens;
    state.ratio = state.total_chars as f64 / state.total_tokens as f64;
    state.ratio = state.ratio.clamp(2.5, 6.0);
}

/// Get the current calibration ratio (for debugging/display).
pub fn get_calibration_ratio() -> f64 {
    CALIBRATION.lock().unwrap().ratio
}

/// Reset calibration state (for testing).
pub fn reset_calibration() {
    let mut state = CALIBRATION.lock().unwrap();
    state.ratio = 4.0;
    state.total_chars = 0;
    state.total_tokens = 0;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() {
        reset_calibration();
    }

    #[test]
    fn returns_zero_for_empty_string() {
        setup();
        assert_eq!(estimate_tokens(""), 0);
    }

    #[test]
    fn estimates_based_on_character_length() {
        setup();
        // Default ratio is 4.0
        assert_eq!(estimate_tokens("abcd"), 1); // 4/4 = 1
        assert_eq!(estimate_tokens("abcdefgh"), 2); // 8/4 = 2
    }

    #[test]
    fn rounds_up() {
        setup();
        assert_eq!(estimate_tokens("abc"), 1); // ceil(3/4) = 1
    }

    #[test]
    fn adjusts_ratio_based_on_actual_data() {
        setup();
        assert!((get_calibration_ratio() - 4.0).abs() < f64::EPSILON);

        // 100 chars / 50 tokens = 2.0 → clamped to 2.5
        calibrate_token_estimate(100, 50);
        assert!((get_calibration_ratio() - 2.5).abs() < f64::EPSILON);

        reset_calibration();

        // 300 chars / 100 tokens = 3.0
        calibrate_token_estimate(300, 100);
        assert!((get_calibration_ratio() - 3.0).abs() < f64::EPSILON);
    }

    #[test]
    fn clamps_ratio_to_reasonable_range() {
        setup();
        // 700/100 = 7.0 → clamped to 6.0
        calibrate_token_estimate(700, 100);
        assert!((get_calibration_ratio() - 6.0).abs() < f64::EPSILON);
    }

    #[test]
    fn ignores_invalid_inputs() {
        setup();
        calibrate_token_estimate(0, 100);
        assert!((get_calibration_ratio() - 4.0).abs() < f64::EPSILON);
        calibrate_token_estimate(100, 0);
        assert!((get_calibration_ratio() - 4.0).abs() < f64::EPSILON);
    }
}
