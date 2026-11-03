//! Canonical model-visible filesystem path grammar.
//!
//! This is intentionally distinct from a secured backend's root-relative path
//! grammar. A host such as dotlink may expose several pinned roots and accept
//! either launch-directory-relative or absolute model paths; after host policy
//! preparation the backend still receives only a clean root-relative path.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Invalid,
}

pub const MAX_MODEL_PATH_BYTES: usize = 4096;
/// Scheduler-only namespace keys may encode platform identity (for example
/// invariant UTF-16 Windows folding) and can therefore be larger than the
/// model spelling. Keep this independently bounded without narrowing a valid
/// model path after authorization.
pub const MAX_SCHEDULING_PATH_BYTES: usize = 64 * 1024;

fn drive_absolute(path: &str) -> bool {
    let bytes = path.as_bytes();
    bytes.len() >= 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && bytes[2] == b'/'
}

pub fn is_unc(path: &str) -> bool {
    if !path.starts_with("//") {
        return false;
    }
    let mut parts = path[2..].split('/');
    matches!((parts.next(), parts.next()), (Some(server), Some(share)) if !server.is_empty() && !share.is_empty())
}

pub fn is_absolute(path: &str) -> bool {
    path.starts_with('/') || drive_absolute(path)
}

pub fn is_root(path: &str) -> bool {
    if matches!(path, "." | "/") || (drive_absolute(path) && path.len() == 3) {
        return true;
    }
    if !is_unc(path) {
        return false;
    }
    path[2..].split('/').count() == 2
}

/// Validate one canonical UTF-8 model path. `/` is the only separator on every
/// platform; `.`/`..`, NUL, repeated separators and bare roots are rejected.
/// POSIX `/a/b` and Windows-style `C:/a/b` absolute spellings are accepted so
/// host adapters can prepare them against pinned grants without rewriting the
/// user/model namespace.
fn validate_bounded(path: &str, max_bytes: usize) -> Result<(), Error> {
    if path.is_empty() || path.len() > max_bytes || path.contains(['\\', '\0']) {
        return Err(Error::Invalid);
    }
    if path == "." || path == "/" || (drive_absolute(path) && path.len() == 3) {
        return Ok(());
    }
    if let Some(body) = path.strip_prefix("//") {
        if !is_unc(path) {
            return Err(Error::Invalid);
        }
        if body
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
        {
            return Err(Error::Invalid);
        }
        return Ok(());
    }
    let body = if let Some(rest) = path.strip_prefix('/') {
        rest
    } else if drive_absolute(path) {
        &path[3..]
    } else {
        path
    };
    if body
        .split('/')
        .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(Error::Invalid);
    }
    Ok(())
}

pub fn validate(path: &str) -> Result<(), Error> {
    validate_bounded(path, MAX_MODEL_PATH_BYTES)
}

/// Validate an internal scheduler namespace key. It intentionally shares the
/// model path's structural grammar while allowing bounded platform encodings to
/// be longer than their already-validated source spelling. This is never an
/// authorization or model-input validator.
pub fn validate_scheduling(path: &str) -> Result<(), Error> {
    validate_bounded(path, MAX_SCHEDULING_PATH_BYTES)
}

/// Secured single-root backends consume only relative paths after host
/// preparation. This helper lets adapters/backends state that invariant without
/// maintaining a second grammar.
pub fn validate_relative(path: &str) -> Result<(), Error> {
    validate(path)?;
    if is_absolute(path) {
        return Err(Error::Invalid);
    }
    Ok(())
}

/// Join one canonical model path without consulting the host platform path
/// grammar. An absolute child keeps its own namespace; `.` behaves as the
/// lexical identity. The result is revalidated so the 4096-byte bound and
/// structural-component rules remain single-sourced here.
pub fn join(base: &str, child: &str) -> Result<String, Error> {
    validate(base)?;
    validate(child)?;
    if is_absolute(child) || base == "." {
        return Ok(child.to_owned());
    }
    if child == "." {
        return Ok(base.to_owned());
    }
    let joined = if base.ends_with('/') {
        format!("{base}{child}")
    } else {
        format!("{base}/{child}")
    };
    validate(&joined)?;
    Ok(joined)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_paths_accept_clean_relative_and_absolute_spellings() {
        for path in [
            ".",
            "/",
            "C:/",
            "//server/share",
            "//server/share/path/a.rs",
            "src/a.rs",
            "/srv/repo/a.rs",
            "C:/repo/a.rs",
            "a:b/c",
        ] {
            assert_eq!(validate(path), Ok(()), "{path}");
        }
        assert!(is_absolute("/srv/repo/a.rs"));
        assert!(is_absolute("C:/repo/a.rs"));
        assert!(!is_absolute("src/a.rs"));
        assert!(is_unc("//server/share/path"));
        assert!(!is_unc("//server"));
        for root in [".", "/", "C:/", "//server/share"] {
            assert!(is_root(root), "{root}");
        }
        assert!(!is_root("//server/share/path"));
        assert_eq!(validate_relative("src/a.rs"), Ok(()));
        assert_eq!(validate_relative("/srv/a"), Err(Error::Invalid));
    }

    #[test]
    fn model_path_join_is_host_independent_and_preserves_absolute_children() {
        for (base, child, expected) in [
            (".", "src/a.rs", "src/a.rs"),
            ("src", "a.rs", "src/a.rs"),
            ("/srv/repo", "a.rs", "/srv/repo/a.rs"),
            ("/", "a.rs", "/a.rs"),
            ("C:/repo", "a.rs", "C:/repo/a.rs"),
            ("C:/", "a.rs", "C:/a.rs"),
            ("//server/share", "a.rs", "//server/share/a.rs"),
            ("src", "/elsewhere/a.rs", "/elsewhere/a.rs"),
            ("src", "D:/elsewhere/a.rs", "D:/elsewhere/a.rs"),
            ("src", "//other/share/a.rs", "//other/share/a.rs"),
            ("src", ".", "src"),
        ] {
            assert_eq!(
                join(base, child).as_deref(),
                Ok(expected),
                "{base} + {child}"
            );
        }
        let long = "x".repeat(4091);
        assert_eq!(join(&long, "child"), Err(Error::Invalid));
    }

    #[test]
    fn scheduler_keys_have_an_independent_bounded_encoding_budget() {
        let expanded = "x".repeat(MAX_MODEL_PATH_BYTES + 1);
        assert_eq!(validate(&expanded), Err(Error::Invalid));
        assert_eq!(validate_scheduling(&expanded), Ok(()));
        let too_large = "x".repeat(MAX_SCHEDULING_PATH_BYTES + 1);
        assert_eq!(validate_scheduling(&too_large), Err(Error::Invalid));
        assert_eq!(validate_scheduling("a/../b"), Err(Error::Invalid));
    }

    #[test]
    fn model_paths_reject_ambiguous_or_structural_components() {
        for path in [
            "",
            "./a",
            "a/./b",
            "../a",
            "a/../b",
            "a//b",
            "//server",
            "///server/share",
            "//server//share",
            "a\\b",
            "a\0b",
        ] {
            assert_eq!(validate(path), Err(Error::Invalid), "{path:?}");
        }
    }
}
