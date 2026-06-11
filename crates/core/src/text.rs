//! Unicode-safe text preview helpers shared across CLI, server, and core code.

use std::borrow::Cow;

/// Return the first `max_chars` user-visible Unicode scalar values.
pub fn prefix_chars(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

/// Return a borrowed string when `value` is within the limit, otherwise an owned prefix.
pub fn prefix_chars_cow(value: &str, max_chars: usize) -> Cow<'_, str> {
    if value.chars().count() <= max_chars {
        Cow::Borrowed(value)
    } else {
        Cow::Owned(prefix_chars(value, max_chars))
    }
}

/// Return the first `prefix_chars` characters and append `...` only when truncated.
pub fn prefix_chars_with_ellipsis(value: &str, prefix_chars: usize) -> String {
    let mut chars = value.chars();
    let mut out: String = chars.by_ref().take(prefix_chars).collect();
    if chars.next().is_some() {
        out.push_str("...");
    }
    out
}

/// Return a string whose total character count is at most `max_chars`.
pub fn ellipsize_chars(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }

    if max_chars <= 3 {
        return ".".repeat(max_chars);
    }

    prefix_chars_with_ellipsis(value, max_chars - 3)
}

/// Build a one-line preview by truncating by character count and replacing newlines.
pub fn single_line_prefix_with_ellipsis(value: &str, prefix_chars: usize) -> String {
    prefix_chars_with_ellipsis(value, prefix_chars).replace('\n', " ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefix_chars_preserves_short_text() {
        assert_eq!(prefix_chars("hello", 10), "hello");
    }

    #[test]
    fn prefix_chars_truncates_multibyte_text() {
        assert_eq!(prefix_chars("✅✅✅", 2), "✅✅");
    }

    #[test]
    fn prefix_chars_cow_borrows_short_text() {
        assert!(matches!(
            prefix_chars_cow("hello", 10),
            Cow::Borrowed("hello")
        ));
    }

    #[test]
    fn prefix_chars_cow_owns_truncated_text() {
        assert!(matches!(prefix_chars_cow("✅✅✅", 2), Cow::Owned(s) if s == "✅✅"));
    }

    #[test]
    fn prefix_chars_with_ellipsis_only_when_truncated() {
        assert_eq!(prefix_chars_with_ellipsis("✅✅", 2), "✅✅");
        assert_eq!(prefix_chars_with_ellipsis("✅✅✅", 2), "✅✅...");
    }

    #[test]
    fn ellipsize_chars_keeps_total_limit() {
        assert_eq!(ellipsize_chars("✅✅✅✅✅", 4), "✅...");
        assert_eq!(ellipsize_chars("hello", 10), "hello");
    }

    #[test]
    fn ellipsize_chars_handles_small_limits() {
        assert_eq!(ellipsize_chars("hello", 0), "");
        assert_eq!(ellipsize_chars("hello", 2), "..");
        assert_eq!(ellipsize_chars("hello", 3), "...");
    }

    #[test]
    fn single_line_prefix_with_ellipsis_flattens_newlines() {
        assert_eq!(
            single_line_prefix_with_ellipsis("line one\nline two", 50),
            "line one line two"
        );
    }
}
