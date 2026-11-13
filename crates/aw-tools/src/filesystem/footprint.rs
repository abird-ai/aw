//! Conservative filesystem scopes used only for deterministic native-node
//! scheduling. They never authorize an operation or replace native locks.
use sha2::{Digest as _, Sha256};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Access {
    Read,
    Write,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Extent {
    Exact,
    Subtree,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Scope {
    pub path: String,
    pub access: Access,
    pub extent: Extent,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Footprint {
    scopes: Vec<Scope>,
    unknown_read: bool,
    unknown_write: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    InvalidPath,
    TooManyScopes,
}

pub const MAX_SCOPES: usize = 128;

impl Footprint {
    pub fn new(
        mut scopes: Vec<Scope>,
        unknown_read: bool,
        unknown_write: bool,
    ) -> Result<Self, Error> {
        if scopes.len() > MAX_SCOPES {
            return Err(Error::TooManyScopes);
        }
        if scopes
            .iter()
            .any(|scope| super::path::validate_scheduling(&scope.path).is_err())
        {
            return Err(Error::InvalidPath);
        }
        scopes.sort();
        scopes.dedup();
        Ok(Self {
            scopes,
            unknown_read,
            unknown_write,
        })
    }

    pub fn none() -> Self {
        Self::default()
    }

    pub fn unknown(read: bool, write: bool) -> Self {
        Self {
            scopes: Vec::new(),
            unknown_read: read,
            unknown_write: write,
        }
    }

    pub fn scopes(&self) -> &[Scope] {
        &self.scopes
    }

    /// Conservative union for execution-route planning. This can only add
    /// scopes/unknown hazards; it never removes semantic footprint evidence.
    pub fn union(&self, other: &Self) -> Result<Self, Error> {
        let mut scopes = self.scopes.clone();
        scopes.extend(other.scopes.iter().cloned());
        Self::new(
            scopes,
            self.unknown_read || other.unknown_read,
            self.unknown_write || other.unknown_write,
        )
    }

    pub fn unknown_read(&self) -> bool {
        self.unknown_read
    }

    pub fn unknown_write(&self) -> bool {
        self.unknown_write
    }

    fn has_read(&self) -> bool {
        self.unknown_read || self.scopes.iter().any(|scope| scope.access == Access::Read)
    }

    fn has_write(&self) -> bool {
        self.unknown_write
            || self
                .scopes
                .iter()
                .any(|scope| scope.access == Access::Write)
    }

    pub fn conflicts(&self, other: &Self) -> bool {
        // Unknown is conservative only when one side can write. Two read-only
        // unknown scopes are observational and may run together.
        if (self.unknown_write && (other.has_read() || other.has_write()))
            || (other.unknown_write && (self.has_read() || self.has_write()))
            || (self.unknown_read && other.has_write())
            || (other.unknown_read && self.has_write())
        {
            return true;
        }
        self.scopes.iter().any(|left| {
            other.scopes.iter().any(|right| {
                (left.access == Access::Write || right.access == Access::Write)
                    && scopes_overlap(left, right)
            })
        })
    }

    pub fn fingerprint(&self) -> [u8; 32] {
        let mut hash = Sha256::new();
        hash.update(b"aw-tools-footprint-v1\0");
        hash.update([u8::from(self.unknown_read), u8::from(self.unknown_write)]);
        hash.update((self.scopes.len() as u64).to_le_bytes());
        for scope in &self.scopes {
            hash.update([scope.access as u8, scope.extent as u8]);
            hash.update((scope.path.len() as u64).to_le_bytes());
            hash.update(scope.path.as_bytes());
        }
        hash.finalize().into()
    }
}

fn under(path: &str, root: &str) -> bool {
    if path == root {
        return true;
    }
    if root == "." {
        return !super::path::is_absolute(path);
    }
    if root == "/" {
        return path.starts_with('/') && !super::path::is_unc(path);
    }
    if root.len() == 3 && root.as_bytes()[1] == b':' && root.ends_with('/') {
        return path.starts_with(root);
    }
    path.strip_prefix(root)
        .is_some_and(|suffix| suffix.starts_with('/'))
}

fn scopes_overlap(left: &Scope, right: &Scope) -> bool {
    match (left.extent, right.extent) {
        (Extent::Exact, Extent::Exact) => left.path == right.path,
        (Extent::Exact, Extent::Subtree) => under(&left.path, &right.path),
        (Extent::Subtree, Extent::Exact) => under(&right.path, &left.path),
        (Extent::Subtree, Extent::Subtree) => {
            under(&left.path, &right.path) || under(&right.path, &left.path)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn scope(path: &str, access: Access, extent: Extent) -> Scope {
        Scope {
            path: path.into(),
            access,
            extent,
        }
    }

    #[test]
    fn exact_subtree_and_access_define_conflicts() {
        let write_src = Footprint::new(
            vec![scope("src", Access::Write, Extent::Subtree)],
            false,
            false,
        )
        .unwrap();
        let read_file = Footprint::new(
            vec![scope("src/a.rs", Access::Read, Extent::Exact)],
            false,
            false,
        )
        .unwrap();
        let read_docs = Footprint::new(
            vec![scope("docs/a.md", Access::Read, Extent::Exact)],
            false,
            false,
        )
        .unwrap();
        let read_src = Footprint::new(
            vec![scope("src/b.rs", Access::Read, Extent::Exact)],
            false,
            false,
        )
        .unwrap();
        assert!(write_src.conflicts(&read_file));
        assert!(!write_src.conflicts(&read_docs));
        assert!(!read_file.conflicts(&read_src));
    }

    #[test]
    fn unknown_read_only_does_not_serialize_other_reads_but_unknown_write_does() {
        let read_unknown = Footprint::unknown(true, false);
        let write_unknown = Footprint::unknown(false, true);
        let read = Footprint::new(
            vec![scope("src", Access::Read, Extent::Subtree)],
            false,
            false,
        )
        .unwrap();
        let write = Footprint::new(
            vec![scope("src", Access::Write, Extent::Exact)],
            false,
            false,
        )
        .unwrap();
        assert!(!read_unknown.conflicts(&read));
        assert!(read_unknown.conflicts(&write));
        assert!(write_unknown.conflicts(&read));
    }

    #[test]
    fn absolute_and_unc_scopes_use_canonical_model_path_namespaces() {
        let posix_root = Footprint::new(
            vec![scope("/", Access::Write, Extent::Subtree)],
            false,
            false,
        )
        .unwrap();
        let posix_child = Footprint::new(
            vec![scope("/srv/repo/a", Access::Read, Extent::Exact)],
            false,
            false,
        )
        .unwrap();
        let unc_child = Footprint::new(
            vec![scope("//server/share/a", Access::Read, Extent::Exact)],
            false,
            false,
        )
        .unwrap();
        assert!(posix_root.conflicts(&posix_child));
        assert!(!posix_root.conflicts(&unc_child));

        let drive_root = Footprint::new(
            vec![scope("C:/", Access::Write, Extent::Subtree)],
            false,
            false,
        )
        .unwrap();
        let drive_child = Footprint::new(
            vec![scope("C:/repo/a", Access::Read, Extent::Exact)],
            false,
            false,
        )
        .unwrap();
        let drive_sibling = Footprint::new(
            vec![scope("D:/repo/a", Access::Read, Extent::Exact)],
            false,
            false,
        )
        .unwrap();
        assert!(drive_root.conflicts(&drive_child));
        assert!(!drive_root.conflicts(&drive_sibling));

        let unc_root = Footprint::new(
            vec![scope("//server/share", Access::Write, Extent::Subtree)],
            false,
            false,
        )
        .unwrap();
        assert!(unc_root.conflicts(&unc_child));
        assert!(!unc_root.conflicts(&posix_child));
    }

    #[test]
    fn relative_root_does_not_alias_an_absolute_namespace() {
        let relative = Footprint::new(
            vec![scope(".", Access::Write, Extent::Subtree)],
            false,
            false,
        )
        .unwrap();
        let child = Footprint::new(
            vec![scope("src/a", Access::Read, Extent::Exact)],
            false,
            false,
        )
        .unwrap();
        let absolute = Footprint::new(
            vec![scope("/src/a", Access::Read, Extent::Exact)],
            false,
            false,
        )
        .unwrap();
        assert!(relative.conflicts(&child));
        assert!(!relative.conflicts(&absolute));
    }

    #[test]
    fn encoded_scheduler_key_can_exceed_model_path_budget_without_becoming_unbounded() {
        let encoded = format!(
            "~{}",
            "a".repeat(super::super::path::MAX_MODEL_PATH_BYTES + 8)
        );
        assert!(super::super::path::validate(&encoded).is_err());
        assert!(Footprint::new(
            vec![scope(&encoded, Access::Write, Extent::Exact)],
            false,
            false,
        )
        .is_ok());
    }

    #[test]
    fn fingerprint_is_order_independent_after_normalization() {
        let a = scope("a", Access::Read, Extent::Exact);
        let b = scope("b", Access::Write, Extent::Subtree);
        let one = Footprint::new(vec![a.clone(), b.clone()], false, false).unwrap();
        let two = Footprint::new(vec![b, a], false, false).unwrap();
        assert_eq!(one.fingerprint(), two.fingerprint());
    }
}
