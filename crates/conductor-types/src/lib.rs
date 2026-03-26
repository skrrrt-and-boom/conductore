pub mod events;
pub mod image_utils;
pub mod state;

pub use events::*;
pub use image_utils::{extract_image_paths, ExtractedInput};
pub use state::*;

/// Truncate a string to at most `max_bytes`, ensuring the cut falls on a char boundary.
pub fn truncate_str(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

/// Return the last `max_bytes` of a string, ensuring the cut falls on a char boundary.
pub fn truncate_str_tail(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut start = s.len() - max_bytes;
    while start < s.len() && !s.is_char_boundary(start) {
        start += 1;
    }
    &s[start..]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_str_ascii() {
        assert_eq!(truncate_str("hello world", 5), "hello");
        assert_eq!(truncate_str("hello", 10), "hello");
        assert_eq!(truncate_str("", 5), "");
    }

    #[test]
    fn truncate_str_multibyte() {
        // Each emoji is 4 bytes
        let s = "ab\u{1F600}cd"; // 2 + 4 + 2 = 8 bytes
        assert_eq!(truncate_str(s, 6), "ab\u{1F600}");
        // Cutting inside the emoji should back up to before it
        assert_eq!(truncate_str(s, 4), "ab"); // 4 is inside the emoji (bytes 2..6)
        assert_eq!(truncate_str(s, 3), "ab"); // 3 is inside emoji
        assert_eq!(truncate_str(s, 5), "ab"); // 5 is inside emoji (bytes 2..6)
    }

    #[test]
    fn truncate_str_tail_ascii() {
        assert_eq!(truncate_str_tail("hello world", 5), "world");
        assert_eq!(truncate_str_tail("hello", 10), "hello");
    }

    #[test]
    fn truncate_str_tail_multibyte() {
        let s = "ab\u{1F600}cd"; // 2 + 4 + 2 = 8 bytes
        assert_eq!(truncate_str_tail(s, 6), "\u{1F600}cd");
        // Cutting inside the emoji should skip forward past it
        assert_eq!(truncate_str_tail(s, 5), "cd"); // start lands inside emoji
        assert_eq!(truncate_str_tail(s, 3), "cd"); // start lands inside emoji
    }
}
