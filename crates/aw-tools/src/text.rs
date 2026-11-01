//! Text classification and bounded in-place literal replacements.
pub fn has_binary_control(bytes: &[u8]) -> bool {
    bytes
        .iter()
        .any(|&b| matches!(b, 0..=8 | 11..=12 | 14..=31 | 127))
}
pub fn text_bytes(bytes: &[u8]) -> Option<&str> {
    if has_binary_control(bytes) {
        None
    } else {
        std::str::from_utf8(bytes).ok()
    }
}
/// Number of nonoverlapping matches without retaining all candidate offsets.
pub fn count_matches(text: &str, needle: &str) -> usize {
    if needle.is_empty() {
        return 0;
    }
    memchr::memmem::find_iter(text.as_bytes(), needle.as_bytes()).count()
}
/// Predict result size before allocating a growing replacement.
pub fn replacement_len(original: usize, old: usize, new: usize, count: usize) -> Option<usize> {
    original
        .checked_sub(old.checked_mul(count)?)?
        .checked_add(new.checked_mul(count)?)
}
/// Dotlink's in-place UTF-8 compaction for equal/shrinking multi-replacements.
pub fn replace_literal_in_place(text: &mut String, needle: &str, replacement: &str) {
    assert!(!needle.is_empty() && replacement.len() <= needle.len());
    let finder = memchr::memmem::Finder::new(needle.as_bytes());
    let mut bytes = std::mem::take(text).into_bytes();
    let (mut read, mut write) = (0usize, 0usize);
    while read < bytes.len() {
        let at = finder.find(&bytes[read..]).map(|offset| read + offset);
        let Some(at) = at else {
            let left = bytes.len() - read;
            bytes.copy_within(read.., write);
            write += left;
            break;
        };
        bytes.copy_within(read..at, write);
        write += at - read;
        bytes[write..write + replacement.len()].copy_from_slice(replacement.as_bytes());
        write += replacement.len();
        read = at + needle.len();
    }
    bytes.truncate(write);
    *text = String::from_utf8(bytes).expect("UTF-8 spans remain UTF-8");
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn classification_and_inplace_replacements() {
        assert!(text_bytes(b"abc\r\n\t").is_some());
        assert!(text_bytes(b"abc\0").is_none());
        let mut s = "aéaéabc".to_owned();
        replace_literal_in_place(&mut s, "aé", "b");
        assert_eq!(s, "bbabc");
        assert_eq!(count_matches("aaaa", "aa"), 2);
    }
    #[test]
    fn expansion_checked() {
        assert_eq!(replacement_len(4, 1, 3, 4), Some(12));
        assert_eq!(replacement_len(4, 1, usize::MAX, 4), None);
    }
}
