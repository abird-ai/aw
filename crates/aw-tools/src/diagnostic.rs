//! Shared bounded diagnostics for caller-controlled text.

pub const MAX_DIAGNOSTIC_BYTES: usize = 16 * 1024;

/// Bound one diagnostic on a UTF-8 character boundary.
///
/// This is defense in depth for user-controlled text that may reach a
/// transport error. Semantic parsing should still reject invalid input early.
pub fn bounded_diagnostic(mut message: String) -> String {
    if message.len() <= MAX_DIAGNOSTIC_BYTES {
        return message;
    }
    let mut end = MAX_DIAGNOSTIC_BYTES.saturating_sub('…'.len_utf8());
    while end > 0 && !message.is_char_boundary(end) {
        end -= 1;
    }
    message.truncate(end);
    message.push('…');
    message
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostics_are_bounded_on_utf8_boundary() {
        let message = format!("bad ref {}", "雪".repeat(MAX_DIAGNOSTIC_BYTES));
        let bounded = bounded_diagnostic(message);
        assert!(bounded.len() <= MAX_DIAGNOSTIC_BYTES);
        assert!(bounded.ends_with('…'));
        assert!(core::str::from_utf8(bounded.as_bytes()).is_ok());
    }
}
