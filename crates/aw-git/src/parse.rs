use crate::{
    Branch, Change, ChangeGroup, Error, RepoPath, Result, Status, Worktree, MAX_ROWS,
    MAX_WIRE_BYTES,
};
use std::path::PathBuf;
fn utf8(bytes: &[u8]) -> Result<&str> {
    std::str::from_utf8(bytes).map_err(|_| Error::Invalid("invalid UTF-8 metadata"))
}
fn check(bytes: &[u8]) -> Result<()> {
    if bytes.len() > MAX_WIRE_BYTES {
        return Err(Error::Limit("Git output"));
    }
    if !bytes.is_empty() && !bytes.ends_with(&[0]) {
        return Err(Error::Invalid("truncated NUL record"));
    }
    Ok(())
}
fn oid(value: &str) -> bool {
    matches!(value.len(), 40 | 64) && value.bytes().all(|b| b.is_ascii_hexdigit())
}
pub fn parse_status(bytes: &[u8]) -> Result<Status> {
    check(bytes)?;
    let mut out = Status::default();
    let mut records = bytes.split(|b| *b == 0).peekable();
    while let Some(record) = records.next() {
        if record.is_empty() {
            continue;
        }
        if let Some(head) = record.strip_prefix(b"# branch.head ") {
            out.branch = Some(utf8(head)?.into());
            continue;
        }
        if let Some(head) = record.strip_prefix(b"# branch.oid ") {
            let value = utf8(head)?;
            if value != "(initial)" {
                if !oid(value) {
                    return Err(Error::Invalid("object ID"));
                }
                out.head = Some(value.into());
            }
            continue;
        }
        if record.starts_with(b"# ") {
            continue;
        }
        if let Some(path) = record.strip_prefix(b"? ") {
            out.changes.push(Change {
                group: ChangeGroup::Untracked,
                path: RepoPath::new(path.to_vec())?,
                old_path: None,
                code: '?',
                submodule: false,
            });
        } else if record.starts_with(b"! ") {
            continue;
        } else {
            let kind = record[0];
            let count = match kind {
                b'1' => 9,
                b'2' => 10,
                b'u' => 11,
                _ => return Err(Error::Invalid("unknown status record")),
            };
            let f = record.splitn(count, |b| *b == b' ').collect::<Vec<_>>();
            if f.len() != count || f[1].len() != 2 || f[2].len() != 4 {
                return Err(Error::Invalid("status fields"));
            }
            let path = RepoPath::new(f[count - 1].to_vec())?;
            let old = if kind == b'2' {
                Some(RepoPath::new(
                    records
                        .next()
                        .ok_or(Error::Invalid("rename source missing"))?
                        .to_vec(),
                )?)
            } else {
                None
            };
            let sub = f[2][0] == b'S';
            if kind == b'u' {
                out.changes.push(Change {
                    group: ChangeGroup::Conflicts,
                    path,
                    old_path: old,
                    code: 'U',
                    submodule: sub,
                });
            } else {
                for (index, group) in [(0, ChangeGroup::Staged), (1, ChangeGroup::Unstaged)] {
                    let c = f[1][index];
                    if c != b'.' {
                        if !b"MADRCUT".contains(&c) {
                            return Err(Error::Invalid("change code"));
                        }
                        out.changes.push(Change {
                            group,
                            path: path.clone(),
                            old_path: old.clone(),
                            code: char::from(c),
                            submodule: sub,
                        });
                    }
                }
            }
        }
        if out.changes.len() > MAX_ROWS {
            return Err(Error::Limit("too many changed entries"));
        }
    }
    out.changes
        .sort_by(|a, b| (a.group, &a.path).cmp(&(b.group, &b.path)));
    Ok(out)
}
pub fn parse_worktrees(bytes: &[u8]) -> Result<Vec<Worktree>> {
    check(bytes)?;
    let mut out = Vec::new();
    let mut item: Option<Worktree> = None;
    for field in bytes.split(|b| *b == 0) {
        if field.is_empty() {
            if let Some(w) = item.take() {
                out.push(w);
            }
            continue;
        }
        if let Some(path) = field.strip_prefix(b"worktree ") {
            if item.is_some() {
                return Err(Error::Invalid("worktree delimiter"));
            }
            let path =
                PathBuf::from(aw_platform::os_string_from_bytes(path.to_vec()).map_err(Error::Io)?);
            if !path.is_absolute() {
                return Err(Error::Invalid("absolute worktree path"));
            }
            item = Some(Worktree {
                path,
                head: None,
                branch: None,
                detached: false,
                bare: false,
                locked: false,
                prunable: false,
            });
        } else {
            let w = item
                .as_mut()
                .ok_or(Error::Invalid("worktree header missing"))?;
            if let Some(h) = field.strip_prefix(b"HEAD ") {
                let h = utf8(h)?;
                if !oid(h) {
                    return Err(Error::Invalid("worktree OID"));
                }
                w.head = Some(h.into());
            } else if let Some(b) = field.strip_prefix(b"branch ") {
                w.branch = Some(utf8(b)?.into());
            } else if field == b"detached" {
                w.detached = true;
            } else if field == b"bare" {
                w.bare = true;
            } else if field == b"locked" || field.starts_with(b"locked ") {
                w.locked = true;
            } else if field == b"prunable" || field.starts_with(b"prunable ") {
                w.prunable = true;
            }
        }
        if out.len() > 1024 {
            return Err(Error::Limit("worktree count"));
        }
    }
    if item.is_some() {
        return Err(Error::Invalid("incomplete worktree record"));
    }
    if out.len() > 1024 {
        return Err(Error::Limit("worktree count"));
    }
    Ok(out)
}
pub fn parse_branches(bytes: &[u8]) -> Result<Vec<Branch>> {
    if bytes.len() > MAX_WIRE_BYTES {
        return Err(Error::Limit("branches"));
    }
    let mut out = Vec::new();
    for record in bytes.split(|b| *b == b'\n').filter(|r| !r.is_empty()) {
        let f = record.split(|b| *b == 0).collect::<Vec<_>>();
        if f.len() != 4 || !f[3].is_empty() {
            return Err(Error::Invalid("branch fields"));
        }
        let reference = utf8(f[0])?;
        let object = utf8(f[1])?;
        if !(reference.starts_with("refs/heads/") || reference.starts_with("refs/remotes/"))
            || reference.chars().any(char::is_control)
            || !oid(object)
        {
            return Err(Error::Invalid("branch ref/OID"));
        }
        out.push(Branch {
            reference: reference.into(),
            object: object.into(),
            current: f[2] == b"*",
        });
    }
    if out.len() > 1024 {
        return Err(Error::Limit("branches; use explicit external filtering"));
    }
    Ok(out)
}
