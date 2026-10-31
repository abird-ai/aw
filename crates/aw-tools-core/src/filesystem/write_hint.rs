//! Optional shell write-footprint hints are only scheduling metadata.
//! They NEVER authorize paths, sandbox commands or substitute for native
//! cross-process mutation locks.
use std::path::{Component, Path, PathBuf};

pub const MAX_SCOPES: usize = 64;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HintError {
    TooMany,
    InvalidScope,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WriteHint {
    Unknown,
    Known(Vec<PathBuf>),
}
fn lexical(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            Component::Normal(p) => normalized.push(p),
            Component::RootDir => normalized.push(component.as_os_str()),
            Component::Prefix(p) => normalized.push(p.as_os_str()),
        }
    }
    normalized
}
/// No owned string copies are required when scheduling multiple calls.
pub fn normalize_names<'a>(
    cwd: &Path,
    scopes: Option<impl IntoIterator<Item = &'a str>>,
) -> Result<WriteHint, HintError> {
    let Some(scopes) = scopes else {
        return Ok(WriteHint::Unknown);
    };
    if !cwd.is_absolute() {
        return Err(HintError::InvalidScope);
    }
    let mut paths = Vec::new();
    for name in scopes {
        if paths.len() >= MAX_SCOPES {
            return Err(HintError::TooMany);
        }
        if name.len() > 4096
            || name.is_empty()
            || name.chars().any(char::is_control)
            || name.contains(['*', '?', '[', ']', '{', '}', '$', '%'])
        {
            return Err(HintError::InvalidScope);
        }
        let path = Path::new(name);
        let base = if path.is_absolute() {
            path.to_path_buf()
        } else {
            cwd.join(path)
        };
        let clean = lexical(&base);
        if !clean.is_absolute() {
            return Err(HintError::InvalidScope);
        }
        if !paths.contains(&clean) {
            paths.push(clean);
        }
    }
    Ok(WriteHint::Known(paths))
}
pub fn normalize(cwd: &Path, scopes: Option<&[String]>) -> Result<WriteHint, HintError> {
    normalize_names(cwd, scopes.map(|items| items.iter().map(String::as_str)))
}
/// True means conservative scheduling should treat two claimed footprints
/// as overlapping. Unknown conflicts with any possible mutation.
fn paths_may_overlap(left: &Path, right: &Path) -> bool {
    #[cfg(windows)]
    {
        let l = left.to_string_lossy();
        let r = right.to_string_lossy();
        // NTFS Unicode collation varies across filesystem settings.
        // Prove disjointness only for ASCII scopes; non-ASCII stays serial.
        if !l.is_ascii() || !r.is_ascii() {
            return true;
        }
        let a = l.to_ascii_lowercase().replace('\\', "/");
        let b = r.to_ascii_lowercase().replace('\\', "/");
        return a == b
            || a.strip_prefix(&b).is_some_and(|s| s.starts_with('/'))
            || b.strip_prefix(&a).is_some_and(|s| s.starts_with('/'));
    }
    #[cfg(not(windows))]
    {
        left.starts_with(right) || right.starts_with(left)
    }
}
pub fn overlaps(left: &WriteHint, right: &WriteHint) -> bool {
    match (left, right) {
        (WriteHint::Unknown, _) | (_, WriteHint::Unknown) => true,
        (WriteHint::Known(a), WriteHint::Known(b)) => {
            a.iter().any(|p| b.iter().any(|q| paths_may_overlap(p, q)))
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(windows)]
    #[test]
    fn windows_case_fold_and_non_ascii_paths_never_false_negative() {
        let root = PathBuf::from(r"C:\repo");
        let a = normalize(&root, Some(&["SRC/A.rs".into()])).unwrap();
        let alias = normalize(&root, Some(&["src/a.RS".into()])).unwrap();
        let different = normalize(&root, Some(&["src/b.rs".into()])).unwrap();
        assert!(overlaps(&a, &alias));
        assert!(!overlaps(&a, &different));
        let unicode = normalize(&root, Some(&["資料/one".into()])).unwrap();
        assert!(overlaps(&unicode, &different));
    }
    #[test]
    fn unknown_empty_and_ancestor_scopes_are_distinct() {
        let root = if cfg!(windows) {
            PathBuf::from(r"C:\repo")
        } else {
            PathBuf::from("/repo")
        };
        let unknown = normalize(&root, None).unwrap();
        let empty = normalize(&root, Some(&[])).unwrap();
        assert_ne!(unknown, empty);
        let lhs = normalize(&root, Some(&["src".into()])).unwrap();
        let rhs = normalize(&root, Some(&["src/file.rs".into()])).unwrap();
        let other = normalize(&root, Some(&["docs".into()])).unwrap();
        assert!(overlaps(&lhs, &rhs));
        assert!(!overlaps(&lhs, &other));
        assert!(!overlaps(&lhs, &empty));
        assert!(overlaps(&unknown, &empty));
        assert!(matches!(
            normalize(&root, Some(&["src/*.rs".into()])),
            Err(HintError::InvalidScope)
        ));
    }
}
