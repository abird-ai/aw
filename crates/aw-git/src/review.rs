use serde::{Deserialize, Serialize};
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LineKind {
    Header,
    Hunk,
    Context,
    Added,
    Removed,
    Notice,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReviewLine {
    pub kind: LineKind,
    pub old: Option<u64>,
    pub new: Option<u64>,
    pub text: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReviewComment {
    pub review_id: String,
    pub row: usize,
    pub old_line: Option<u64>,
    pub new_line: Option<u64>,
    pub quote: String,
    pub text: String,
}
/// Display projection only: stored raw patch bytes are never altered by escaping
/// or line/byte display limits. Combined/binary diffs remain readable header data.
pub fn parse_diff_lines(patch: &[u8], limit: usize) -> (Vec<ReviewLine>, bool) {
    let mut out = Vec::new();
    let (mut old, mut new) = (None, None);
    let mut truncated = false;
    for raw in patch.split(|b| *b == b'\n') {
        if out.len() >= limit {
            truncated = true;
            break;
        }
        let text = crate::escape_bytes(raw);
        let kind = if raw.starts_with(b"diff --") {
            old = None;
            new = None;
            LineKind::Header
        } else if raw.starts_with(b"@@ ") {
            let words = text.split_whitespace().collect::<Vec<_>>();
            old = words
                .get(1)
                .and_then(|s| s.strip_prefix('-'))
                .and_then(|s| s.split(',').next())
                .and_then(|s| s.parse().ok());
            new = words
                .get(2)
                .and_then(|s| s.strip_prefix('+'))
                .and_then(|s| s.split(',').next())
                .and_then(|s| s.parse().ok());
            LineKind::Hunk
        } else if old.is_some() && new.is_some() {
            match raw.first() {
                Some(b'+') => LineKind::Added,
                Some(b'-') => LineKind::Removed,
                Some(b' ') => LineKind::Context,
                _ => LineKind::Notice,
            }
        } else {
            LineKind::Header
        };
        let (o, n) = match kind {
            LineKind::Added => (None, new),
            LineKind::Removed => (old, None),
            LineKind::Context => (old, new),
            _ => (None, None),
        };
        out.push(ReviewLine {
            kind,
            old: o,
            new: n,
            text,
        });
        if matches!(kind, LineKind::Removed | LineKind::Context) {
            old = old.and_then(|n| n.checked_add(1));
        }
        if matches!(kind, LineKind::Added | LineKind::Context) {
            new = new.and_then(|n| n.checked_add(1));
        }
    }
    (out, truncated)
}
