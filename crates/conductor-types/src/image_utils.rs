//! Image path extraction from user input.
//!
//! When users drag-and-drop images into the terminal, the full file path
//! gets pasted as text. This module extracts those paths and separates
//! them from the message text.

use std::path::Path;

/// Image file extensions we recognise.
const IMAGE_EXTENSIONS: &[&str] = &[
    "png", "jpg", "jpeg", "gif", "webp", "bmp", "svg", "tif", "tiff",
];

/// Result of extracting image paths from user input.
#[derive(Debug, Clone)]
pub struct ExtractedInput {
    /// The text portion of the input (image paths removed).
    pub text: String,
    /// Absolute image file paths found in the input.
    pub images: Vec<String>,
}

/// Extract image file paths from user input.
///
/// Handles drag-and-drop (terminal pastes full path) and explicit paths.
/// Image paths are removed from the text and returned separately.
/// If only images were provided with no text, a default analysis prompt is used.
pub fn extract_image_paths(input: &str) -> ExtractedInput {
    let mut images = Vec::new();
    let mut text_parts = Vec::new();

    // Tokenise respecting quoted strings (handles paths with spaces)
    for token in tokenize_respecting_quotes(input) {
        // Clean up drag-and-drop artifacts (some terminals add quotes or escape spaces)
        let cleaned = token
            .trim_matches('"')
            .trim_matches('\'')
            .replace("\\ ", " ");

        if cleaned.starts_with('/') && is_image_path(&cleaned) {
            images.push(cleaned);
        } else {
            text_parts.push(token);
        }
    }

    let text = text_parts.join(" ");
    let text = if text.trim().is_empty() && !images.is_empty() {
        "Analyze the attached image(s) and adjust the plan accordingly.".to_string()
    } else {
        text
    };

    ExtractedInput { text, images }
}

/// Split input into tokens, keeping quoted strings as single tokens.
fn tokenize_respecting_quotes(input: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut in_quote: Option<char> = None;

    for ch in input.chars() {
        match (ch, in_quote) {
            ('"' | '\'', None) => {
                in_quote = Some(ch);
                current.push(ch);
            }
            (q, Some(open)) if q == open => {
                current.push(ch);
                in_quote = None;
            }
            (c, None) if c.is_whitespace() => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
            }
            (c, _) => {
                current.push(c);
            }
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

/// Check if a path looks like an image file based on extension.
fn is_image_path(path: &str) -> bool {
    Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .map(|ext| IMAGE_EXTENSIONS.contains(&ext.to_lowercase().as_str()))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_only() {
        let r = extract_image_paths("fix the bug in main.rs");
        assert_eq!(r.text, "fix the bug in main.rs");
        assert!(r.images.is_empty());
    }

    #[test]
    fn image_only() {
        let r = extract_image_paths("/tmp/screenshot.png");
        assert_eq!(
            r.text,
            "Analyze the attached image(s) and adjust the plan accordingly."
        );
        assert_eq!(r.images, vec!["/tmp/screenshot.png"]);
    }

    #[test]
    fn mixed_text_and_images() {
        let r =
            extract_image_paths("use this design /Users/me/mockup.jpg for the login page");
        assert_eq!(r.text, "use this design for the login page");
        assert_eq!(r.images, vec!["/Users/me/mockup.jpg"]);
    }

    #[test]
    fn multiple_images() {
        let r = extract_image_paths("/a/b.png /c/d.webp");
        assert_eq!(r.images, vec!["/a/b.png", "/c/d.webp"]);
    }

    #[test]
    fn quoted_paths() {
        let r = extract_image_paths("\"/tmp/my screenshot.png\"");
        assert_eq!(r.images, vec!["/tmp/my screenshot.png"]);
    }

    #[test]
    fn non_image_absolute_path() {
        let r = extract_image_paths("read /etc/hosts");
        assert_eq!(r.text, "read /etc/hosts");
        assert!(r.images.is_empty());
    }

    #[test]
    fn all_supported_extensions() {
        for ext in IMAGE_EXTENSIONS {
            let input = format!("/tmp/test.{ext}");
            let r = extract_image_paths(&input);
            assert_eq!(r.images.len(), 1, "failed for .{ext}");
        }
    }
}
