//! Shared text/binary distinction for model-facing text tools.
//! Binary transfer is explicit through bread/bwrite/bedit.
pub(crate) fn has_binary_control(bytes: &[u8]) -> bool {
    bytes
        .iter()
        .any(|byte| matches!(byte, 0..=8 | 11..=12 | 14..=31 | 127))
}

pub(crate) fn text_bytes(bytes: &[u8]) -> Option<&str> {
    if has_binary_control(bytes) {
        return None;
    }
    std::str::from_utf8(bytes).ok()
}

pub(crate) fn require_text(bytes: &[u8]) -> crate::Result<&str> {
    text_bytes(bytes).ok_or(aw_agent_core::Error::Denied(
        "binary or control-containing content; use binary file tools",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn utf8_text_accepts_tabs_newlines_and_rejects_controls() {
        assert!(text_bytes("snow 雪\r\n\t".as_bytes()).is_some());
        for v in [b'\0', 0x1b, 0x7f, 0x0b, 0x1f] {
            assert!(text_bytes(&[b'A', v, b'B']).is_none());
        }
        assert!(text_bytes(&[0xff]).is_none());
    }
}
