//! Multi-root secured filesystem capability set for hosts such as dotlink.
//!
//! Product policy remains outside this module. The host supplies only admitted
//! read/write roots plus its canonical relative-path base. Every filesystem
//! effect is then routed to a pinned `Root` and reuses the same shared engines
//! as the single-root AW adapter.
use super::{port, Result, Root, ScopedError, StagedWrite};
use aw_tools::filesystem::{
    backend::{
        CommitFailure, ExplicitPathBackend, MetadataBackend, MutationAccess, MutationExtent,
        MutationPath, SecuredFilesystem,
    },
    walk, EntryMeta, Kind,
};
use std::{
    collections::BTreeMap,
    io,
    path::{Path, PathBuf},
    sync::Arc,
};

#[derive(Clone, Debug)]
pub struct GrantSpec {
    pub path: PathBuf,
    pub read: bool,
    pub write: bool,
}
impl GrantSpec {
    pub fn read(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            read: true,
            write: false,
        }
    }
    pub fn write(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            read: false,
            write: true,
        }
    }
    pub fn read_write(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            read: true,
            write: true,
        }
    }
}

pub struct PreparedGrant {
    pub spec: GrantSpec,
    pub private: Arc<aw_tools::filesystem::private_artifact::Registry>,
}
impl PreparedGrant {
    pub fn new(
        spec: GrantSpec,
        private: Arc<aw_tools::filesystem::private_artifact::Registry>,
    ) -> Self {
        Self { spec, private }
    }
}

#[derive(Clone, Debug)]
pub struct ExclusionSpec {
    pub path: PathBuf,
    pub read: bool,
    pub write: bool,
}
impl ExclusionSpec {
    pub fn read(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            read: true,
            write: false,
        }
    }
    pub fn write(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            read: false,
            write: true,
        }
    }
    pub fn read_write(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            read: true,
            write: true,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GrantIdentity {
    pub path: PathBuf,
    pub volume: u64,
    pub object: u64,
    pub read: bool,
    pub write: bool,
}

struct Grant {
    path: PathBuf,
    root: Root,
    read: bool,
    write: bool,
}

struct Exclusion {
    path: PathBuf,
    read: bool,
    write: bool,
}

pub struct CapabilitySet {
    base: PathBuf,
    grants: Vec<Grant>,
    exclusions: Vec<Exclusion>,
}

#[derive(Clone, Copy)]
enum Need {
    Read,
    Write,
    Both,
    Coordination,
}
struct Routed<'a> {
    grant: &'a Grant,
    index: usize,
    relative: String,
    absolute: PathBuf,
}

pub struct CapabilityMutationGuard<'a> {
    _guards: Vec<port::MutationGuard<'a>>,
}

fn error_kind(error: &ScopedError) -> walk::BackendErrorKind {
    use walk::BackendErrorKind as K;
    match error {
        ScopedError::Io(error) if error.kind() == io::ErrorKind::NotFound => K::Missing,
        ScopedError::Io(_) | ScopedError::Denied(_) => K::Unavailable,
        ScopedError::Limit(_) => K::Limit,
        ScopedError::Cancelled => K::Other,
    }
}

fn join_model(root: &str, suffix: &str) -> String {
    if suffix.is_empty() {
        return root.to_owned();
    }
    match root {
        "." => suffix.to_owned(),
        "/" => format!("/{suffix}"),
        _ if root.ends_with(":/") => format!("{root}{suffix}"),
        _ => format!("{root}/{suffix}"),
    }
}

fn suffix<'a>(entry: &'a str, relative_root: &str) -> &'a str {
    if relative_root.is_empty() {
        entry
    } else {
        entry
            .strip_prefix(relative_root)
            .and_then(|rest| rest.strip_prefix('/'))
            .unwrap_or(entry)
    }
}

impl CapabilitySet {
    pub fn open(base: &Path, grants: impl IntoIterator<Item = GrantSpec>) -> Result<Self> {
        Self::open_with_exclusions(base, grants, std::iter::empty())
    }

    pub fn open_with_exclusions(
        base: &Path,
        grants: impl IntoIterator<Item = GrantSpec>,
        exclusions: impl IntoIterator<Item = ExclusionSpec>,
    ) -> Result<Self> {
        let base = base.canonicalize()?;
        let mut merged = BTreeMap::<PathBuf, (bool, bool)>::new();
        for grant in grants {
            if !grant.read && !grant.write {
                return Err(ScopedError::Denied("capability grant has no access"));
            }
            let path = grant.path.canonicalize()?;
            if !path.is_dir() {
                return Err(ScopedError::Denied("capability root is not a directory"));
            }
            let access = merged.entry(path).or_default();
            access.0 |= grant.read;
            access.1 |= grant.write;
        }
        let prepared = merged.into_iter().map(|(path, (read, write))| {
            PreparedGrant::new(
                GrantSpec { path, read, write },
                aw_tools::filesystem::private_artifact::Registry::memory(),
            )
        });
        Self::open_prepared_with_exclusions(&base, prepared, exclusions)
    }

    pub fn open_prepared(
        base: &Path,
        grants: impl IntoIterator<Item = PreparedGrant>,
    ) -> Result<Self> {
        Self::open_prepared_with_exclusions(base, grants, std::iter::empty())
    }

    pub fn open_prepared_with_exclusions(
        base: &Path,
        grants: impl IntoIterator<Item = PreparedGrant>,
        exclusions: impl IntoIterator<Item = ExclusionSpec>,
    ) -> Result<Self> {
        let base = base.canonicalize()?;
        if !base.is_dir() {
            return Err(ScopedError::Denied("capability base is not a directory"));
        }
        let mut merged = BTreeMap::<
            PathBuf,
            (
                bool,
                bool,
                Arc<aw_tools::filesystem::private_artifact::Registry>,
            ),
        >::new();
        for prepared in grants {
            let GrantSpec { path, read, write } = prepared.spec;
            if !read && !write {
                return Err(ScopedError::Denied("capability grant has no access"));
            }
            let path = path.canonicalize()?;
            if !path.is_dir() {
                return Err(ScopedError::Denied("capability root is not a directory"));
            }
            match merged.get_mut(&path) {
                Some((old_read, old_write, registry)) => {
                    if !Arc::ptr_eq(registry, &prepared.private) {
                        return Err(ScopedError::Denied(
                            "duplicate capability root has different private artifact registry",
                        ));
                    }
                    *old_read |= read;
                    *old_write |= write;
                }
                None => {
                    merged.insert(path, (read, write, prepared.private));
                }
            }
        }
        if merged.is_empty() {
            return Err(ScopedError::Denied("capability set has no roots"));
        }
        let mut opened = Vec::with_capacity(merged.len());
        let mut objects = std::collections::BTreeSet::new();
        for (path, (read, write, private)) in merged {
            let root = Root::open_with_private_artifacts(&path, private)?;
            let path_meta = std::fs::metadata(&path)?;
            let root_meta = root.directory.metadata()?;
            use std::os::unix::fs::MetadataExt as _;
            let identity = (root_meta.dev(), root_meta.ino());
            if (path_meta.dev(), path_meta.ino()) != identity {
                return Err(ScopedError::Denied("capability root changed while opening"));
            }
            if !objects.insert(identity) {
                return Err(ScopedError::Denied(
                    "multiple capability roots alias the same directory object",
                ));
            }
            opened.push(Grant {
                path,
                root,
                read,
                write,
            });
        }
        let mut denied = BTreeMap::<PathBuf, (bool, bool)>::new();
        for exclusion in exclusions {
            if !exclusion.read && !exclusion.write {
                return Err(ScopedError::Denied(
                    "capability exclusion has no access bit",
                ));
            }
            let path = Self::prepared_absolute(&base, &exclusion.path)?;
            let access = denied.entry(path).or_default();
            access.0 |= exclusion.read;
            access.1 |= exclusion.write;
        }
        let exclusions = denied
            .into_iter()
            .map(|(path, (read, write))| Exclusion { path, read, write })
            .collect();
        Ok(Self {
            base,
            grants: opened,
            exclusions,
        })
    }

    fn prepared_absolute(base: &Path, path: &Path) -> Result<PathBuf> {
        let joined = if path.is_absolute() {
            path.to_path_buf()
        } else {
            base.join(path)
        };
        let mut normalized = PathBuf::new();
        for component in joined.components() {
            match component {
                std::path::Component::RootDir => normalized.push("/"),
                std::path::Component::Normal(part) => normalized.push(part),
                std::path::Component::CurDir => {}
                std::path::Component::ParentDir => {
                    return Err(ScopedError::Denied(
                        "prepared exclusion contains parent traversal",
                    ));
                }
                std::path::Component::Prefix(_) => {
                    return Err(ScopedError::Denied("invalid Unix exclusion prefix"));
                }
            }
        }
        if !normalized.is_absolute() {
            return Err(ScopedError::Denied("prepared exclusion is not absolute"));
        }
        Ok(normalized)
    }

    fn exclusion_applies(exclusion: &Exclusion, need: Need) -> bool {
        match need {
            Need::Read => exclusion.read,
            Need::Write => exclusion.write,
            Need::Both => exclusion.read || exclusion.write,
            Need::Coordination => false,
        }
    }

    fn excluded(&self, absolute: &Path, need: Need) -> bool {
        self.exclusions.iter().any(|exclusion| {
            Self::exclusion_applies(exclusion, need) && absolute.starts_with(&exclusion.path)
        })
    }

    fn overlaps_exclusion(&self, absolute: &Path, need: Need) -> bool {
        self.exclusions.iter().any(|exclusion| {
            Self::exclusion_applies(exclusion, need)
                && (absolute.starts_with(&exclusion.path) || exclusion.path.starts_with(absolute))
        })
    }

    pub fn reconcile_private_artifacts(&self) -> Result<()> {
        for grant in &self.grants {
            grant.root.reconcile_private_artifacts()?;
        }
        Ok(())
    }

    pub fn base(&self) -> &Path {
        &self.base
    }

    pub fn identities(&self) -> Result<Vec<GrantIdentity>> {
        use std::os::unix::fs::MetadataExt as _;
        self.grants
            .iter()
            .map(|grant| {
                let meta = grant.root.directory.metadata()?;
                Ok(GrantIdentity {
                    path: grant.path.clone(),
                    volume: meta.dev(),
                    object: meta.ino(),
                    read: grant.read,
                    write: grant.write,
                })
            })
            .collect()
    }

    fn absolute(&self, model: &str) -> Result<PathBuf> {
        aw_tools::filesystem::path::validate(model)
            .map_err(|_| ScopedError::Denied("invalid model path"))?;
        if model == "." {
            return Ok(self.base.clone());
        }
        if aw_tools::filesystem::path::is_unc(model) {
            return Err(ScopedError::Denied(
                "Windows UNC model path is not valid on this Unix host",
            ));
        }
        if model.starts_with('/') {
            return Ok(PathBuf::from(model));
        }
        if aw_tools::filesystem::path::is_absolute(model) {
            return Err(ScopedError::Denied(
                "Windows drive path is not valid on this Unix host",
            ));
        }
        Ok(self.base.join(model))
    }

    fn covers(grant: &Grant, absolute: &Path, need: Need) -> bool {
        if !absolute.starts_with(&grant.path) {
            return false;
        }
        match need {
            Need::Read => grant.read,
            Need::Write => grant.write,
            Need::Both => grant.write,
            Need::Coordination => true,
        }
    }

    fn route(&self, model: &str, need: Need) -> Result<Routed<'_>> {
        let absolute = self.absolute(model)?;
        if !matches!(need, Need::Coordination) && self.excluded(&absolute, need) {
            return Err(ScopedError::Denied(
                "path is excluded by prepared capability policy",
            ));
        }
        if matches!(need, Need::Both) {
            if !self
                .grants
                .iter()
                .any(|grant| grant.read && absolute.starts_with(&grant.path))
            {
                return Err(ScopedError::Denied("path is not covered by a read grant"));
            }
            if !self
                .grants
                .iter()
                .any(|grant| grant.write && absolute.starts_with(&grant.path))
            {
                return Err(ScopedError::Denied("path is not covered by a write grant"));
            }
        }
        let mut best = None::<(usize, usize)>;
        for (index, grant) in self.grants.iter().enumerate() {
            if Self::covers(grant, &absolute, need) {
                let depth = grant.path.components().count();
                if best.is_none_or(|(_, old)| depth > old) {
                    best = Some((index, depth));
                }
            }
        }
        let (index, _) = best.ok_or(ScopedError::Denied(match need {
            Need::Read => "path is not covered by a read grant",
            Need::Write => "path is not covered by a write grant",
            Need::Both => "path is not covered by read and write grants",
            Need::Coordination => "path is not covered by a filesystem grant",
        }))?;
        let grant = &self.grants[index];
        let relative = absolute
            .strip_prefix(&grant.path)
            .map_err(|_| ScopedError::Denied("path escaped selected capability root"))?;
        let relative = relative
            .to_str()
            .ok_or(ScopedError::Denied("model path is not UTF-8"))?
            .trim_start_matches('/')
            .to_owned();
        if !relative.is_empty() {
            super::validate_path(&relative)?;
        }
        Ok(Routed {
            grant,
            index,
            relative,
            absolute,
        })
    }

    fn route_lock(&self, model: &str) -> Result<Routed<'_>> {
        // Coordination is not authority: choose the deepest retained capability
        // covering this already-authorized model path to avoid nested-root
        // self-locking while keeping one deterministic namespace.
        self.route(model, Need::Coordination)
    }

    fn map_listing(&self, model: &str, routed: &Routed<'_>, max: usize) -> Result<walk::Listing> {
        let listing = <Root as walk::Backend>::list(&routed.grant.root, &routed.relative, max)?;
        let mut entries = Vec::with_capacity(listing.entries.len());
        for entry in listing.entries {
            let model_path = join_model(model, suffix(&entry.path, &routed.relative));
            let absolute = self.absolute(&model_path)?;
            if self.excluded(&absolute, Need::Read) {
                continue;
            }
            entries.push(walk::WalkEntry {
                path: model_path,
                name: entry.name,
                kind: entry.kind,
                hidden: entry.hidden,
            });
        }
        Ok(walk::Listing {
            incomplete: listing.incomplete,
            path_encoding: listing.path_encoding,
            entries,
        })
    }

    fn resolve_final(
        &self,
        path: &str,
        allow_missing: bool,
        need: Need,
        expected_kind: libc::mode_t,
        label: &'static str,
    ) -> Result<String> {
        use std::os::unix::fs::MetadataExt as _;
        let lexical = self.route(path, need)?;
        if lexical.relative.is_empty() {
            return if expected_kind == libc::S_IFDIR {
                Ok(path.to_owned())
            } else {
                Err(ScopedError::Denied(label))
            };
        }
        lexical.grant.root.ensure_public_path(&lexical.relative)?;
        let (parent, name) = match lexical.grant.root.parent(&lexical.relative) {
            Ok(value) => value,
            Err(ScopedError::Io(error))
                if allow_missing && error.kind() == io::ErrorKind::NotFound =>
            {
                return Ok(path.to_owned());
            }
            Err(error) => return Err(error),
        };
        let entry = match super::stat_leaf(&parent, &name, true) {
            Ok(entry) => entry,
            Err(ScopedError::Io(error))
                if allow_missing && error.kind() == io::ErrorKind::NotFound =>
            {
                return Ok(path.to_owned());
            }
            Err(error) => return Err(error),
        };
        let entry_kind = entry.st_mode & libc::S_IFMT;
        if entry_kind == expected_kind {
            return Ok(path.to_owned());
        }
        if entry_kind != libc::S_IFLNK {
            return Err(ScopedError::Denied(label));
        }
        let followed = match super::stat_leaf(&parent, &name, false) {
            Ok(value) => value,
            Err(ScopedError::Io(error)) if error.kind() == io::ErrorKind::NotFound => {
                return Err(ScopedError::Denied(
                    "dangling final symlink is not a missing target",
                ));
            }
            Err(error) => return Err(error),
        };
        if followed.st_mode & libc::S_IFMT != expected_kind {
            return Err(ScopedError::Denied(label));
        }

        let parent_path = super::descriptor_path(&parent)?;
        let canonical = std::fs::canonicalize(parent_path.join(&name)).map_err(|error| {
            if error.kind() == io::ErrorKind::NotFound {
                ScopedError::Denied("dangling final symlink is not a missing target")
            } else {
                ScopedError::Io(error)
            }
        })?;
        let model = canonical
            .to_str()
            .ok_or(ScopedError::Denied("resolved target is not UTF-8"))?;
        let target = self.route(model, need)?;
        let verified = if target.relative.is_empty() {
            let meta = target.grant.root.directory.metadata()?;
            let mode = if meta.is_dir() {
                libc::S_IFDIR
            } else if meta.is_file() {
                libc::S_IFREG
            } else {
                0
            };
            (mode, meta.dev() as libc::dev_t, meta.ino() as libc::ino_t)
        } else {
            target.grant.root.ensure_public_path(&target.relative)?;
            let (target_parent, target_name) = target.grant.root.parent(&target.relative)?;
            let stat = super::stat_leaf(&target_parent, &target_name, true)?;
            (stat.st_mode & libc::S_IFMT, stat.st_dev, stat.st_ino)
        };
        if verified.0 != expected_kind
            || verified.1 != followed.st_dev
            || verified.2 != followed.st_ino
        {
            return Err(ScopedError::Denied(
                "final symlink target changed during resolution",
            ));
        }
        // Return a canonical absolute model spelling so the host can re-run its
        // deny/protected policy against the resolved target before payload IO.
        Ok(model.to_owned())
    }

    fn check_descendant(
        source: &Routed<'_>,
        destination: &Routed<'_>,
        directory: bool,
    ) -> Result<()> {
        if directory
            && destination.absolute != source.absolute
            && destination.absolute.starts_with(&source.absolute)
        {
            return Err(ScopedError::Denied("destination is inside source"));
        }
        Ok(())
    }
}

impl walk::Backend for CapabilitySet {
    type Error = ScopedError;

    fn list(&self, path: &str, max: usize) -> Result<walk::Listing> {
        let routed = self.route(path, Need::Read)?;
        self.map_listing(path, &routed, max)
    }

    fn read_bytes(&self, path: &str, max: usize) -> Result<Vec<u8>> {
        let routed = self.route(path, Need::Read)?;
        Root::read_bytes(&routed.grant.root, &routed.relative, max)
    }

    fn classify(&self, error: &Self::Error) -> walk::BackendErrorKind {
        error_kind(error)
    }

    fn private_entry(&self, path: &str) -> Result<bool> {
        let routed = self.route(path, Need::Read)?;
        if routed.relative.is_empty() {
            return Ok(false);
        }
        routed.grant.root.is_private_path(&routed.relative)
    }
}

impl MetadataBackend for CapabilitySet {
    fn entry_meta(&self, path: &str) -> Result<EntryMeta> {
        let routed = self.route(path, Need::Read)?;
        if routed.relative.is_empty() {
            return Err(ScopedError::Denied("metadata path must name an entry"));
        }
        Root::entry_meta(&routed.grant.root, &routed.relative)
    }
}

/// Final-symlink resolution initially preserves the same-root public contract.
/// Cross-grant final-symlink authorization is performed by the host and will use
/// the explicit prepared-path API added alongside the dotlink adapter cutover.
impl ExplicitPathBackend for CapabilitySet {
    fn resolve_content_path(&self, path: &str, allow_missing: bool) -> Result<String> {
        self.resolve_final(
            path,
            allow_missing,
            if allow_missing {
                Need::Write
            } else {
                Need::Read
            },
            libc::S_IFREG,
            "final symlink target is not a regular file",
        )
    }

    fn resolve_directory_path(&self, path: &str) -> Result<String> {
        self.resolve_final(
            path,
            false,
            Need::Read,
            libc::S_IFDIR,
            "discovery root is not a directory",
        )
    }
}

impl SecuredFilesystem for CapabilitySet {
    type Guard<'a> = CapabilityMutationGuard<'a>;
    type Stage = StagedWrite;

    fn preflight_mutation(&self, paths: &[MutationPath<'_>]) -> Result<()> {
        for path in paths {
            let need = match path.access {
                MutationAccess::Read => Need::Read,
                MutationAccess::Write => Need::Write,
                MutationAccess::ReadWrite => Need::Both,
            };
            let routed = self.route(path.path, need)?;
            if path.extent == MutationExtent::Subtree
                && self.overlaps_exclusion(&routed.absolute, need)
            {
                return Err(ScopedError::Denied(
                    "mutation subtree overlaps a prepared capability exclusion",
                ));
            }
        }
        Ok(())
    }

    fn mutation_meta(&self, path: &str) -> Result<EntryMeta> {
        let routed = self.route(path, Need::Write)?;
        if routed.relative.is_empty() {
            return Err(ScopedError::Denied("mutation path must name an entry"));
        }
        Root::entry_meta(&routed.grant.root, &routed.relative)
    }

    fn mutation_path_key(&self, path: &str) -> Result<Vec<u8>> {
        let routed = self.route(path, Need::Write)?;
        if routed.relative.is_empty() {
            return Err(ScopedError::Denied("mutation path must name an entry"));
        }
        aw_tools::filesystem::backend::SecuredFilesystem::mutation_path_key(
            &routed.grant.root,
            &routed.relative,
        )
    }

    fn lock<'a>(&'a self, paths: &[String], cancel: &dyn Fn() -> bool) -> Result<Self::Guard<'a>> {
        let mut groups = BTreeMap::<PathBuf, (usize, Vec<String>)>::new();
        for model in paths {
            let routed = self.route_lock(model)?;
            if routed.relative.is_empty() {
                return Err(ScopedError::Denied("mutation path must name an entry"));
            }
            groups
                .entry(routed.grant.path.clone())
                .or_insert_with(|| (routed.index, Vec::new()))
                .1
                .push(routed.relative);
        }
        let mut guards = Vec::with_capacity(groups.len());
        for (_, (index, relative)) in groups {
            guards.push(
                self.grants[index]
                    .root
                    .lock_mutations_with_cancel(&relative, cancel)?,
            );
        }
        Ok(CapabilityMutationGuard { _guards: guards })
    }

    fn stage_create(
        &self,
        path: &str,
        bytes: &[u8],
        cancel: &dyn Fn() -> bool,
    ) -> Result<Self::Stage> {
        let routed = self.route(path, Need::Write)?;
        Root::stage_create(&routed.grant.root, &routed.relative, bytes, cancel)
    }

    fn stage_replace(
        &self,
        path: &str,
        bytes: &[u8],
        cancel: &dyn Fn() -> bool,
    ) -> Result<Self::Stage> {
        let routed = self.route(path, Need::Write)?;
        Root::stage_replace(&routed.grant.root, &routed.relative, bytes, cancel)
    }

    fn stage_update(
        &self,
        path: &str,
        expected: &[u8],
        bytes: &[u8],
        cancel: &dyn Fn() -> bool,
    ) -> Result<Self::Stage> {
        let routed = self.route(path, Need::Both)?;
        Root::stage_update(
            &routed.grant.root,
            &routed.relative,
            expected,
            bytes,
            cancel,
        )
    }

    fn classify_publication_failure(&self, error: &Self::Error) -> CommitFailure {
        match error {
            ScopedError::Denied(_) | ScopedError::Limit(_) | ScopedError::Cancelled => {
                CommitFailure::NoPublication
            }
            ScopedError::Io(_) => CommitFailure::Unknown,
        }
    }

    fn copy_exact(&self, source: &str, destination: &str, cancel: &dyn Fn() -> bool) -> Result<()> {
        let source = self.route(source, Need::Read)?;
        let destination = self.route(destination, Need::Write)?;
        if self.overlaps_exclusion(&source.absolute, Need::Read)
            || self.overlaps_exclusion(&destination.absolute, Need::Write)
        {
            return Err(ScopedError::Denied(
                "copy overlaps a prepared capability exclusion",
            ));
        }
        let meta = Root::entry_meta(&source.grant.root, &source.relative)?;
        Self::check_descendant(&source, &destination, meta.kind == Kind::Dir)?;
        port::lifecycle::copy_between(
            &source.grant.root,
            &source.relative,
            &destination.grant.root,
            &destination.relative,
            true,
            cancel,
        )
    }

    fn move_exact(&self, source: &str, destination: &str, cancel: &dyn Fn() -> bool) -> Result<()> {
        let source = self.route(source, Need::Write)?;
        let destination = self.route(destination, Need::Write)?;
        if self.overlaps_exclusion(&source.absolute, Need::Write)
            || self.overlaps_exclusion(&destination.absolute, Need::Write)
        {
            return Err(ScopedError::Denied(
                "move overlaps a prepared capability exclusion",
            ));
        }
        let meta = Root::entry_meta(&source.grant.root, &source.relative)?;
        Self::check_descendant(&source, &destination, meta.kind == Kind::Dir)?;
        port::lifecycle::move_between(
            &source.grant.root,
            &source.relative,
            &destination.grant.root,
            &destination.relative,
            cancel,
        )
    }

    fn remove_exact(
        &self,
        paths: &[String],
        recursive: bool,
        cancel: &dyn Fn() -> bool,
    ) -> Result<aw_tools::filesystem::lifecycle::RemoveOutcome> {
        let routed = paths
            .iter()
            .map(|path| self.route(path, Need::Write))
            .collect::<Result<Vec<_>>>()?;
        if routed
            .iter()
            .any(|path| self.overlaps_exclusion(&path.absolute, Need::Write))
        {
            return Err(ScopedError::Denied(
                "remove overlaps a prepared capability exclusion",
            ));
        }
        let refs = routed
            .iter()
            .map(|path| (&path.grant.root, path.relative.as_str()))
            .collect::<Vec<_>>();
        port::lifecycle::remove_between(&refs, recursive, cancel)
    }

    fn read_page(
        &self,
        path: &str,
        request: aw_tools::read::PageRequest,
        cancel: &dyn Fn() -> bool,
    ) -> Result<aw_tools::read::Page> {
        let routed = self.route(path, Need::Read)?;
        Root::read_page_with_cancel(&routed.grant.root, &routed.relative, request, cancel)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aw_tools::filesystem::{
        operations,
        private_artifact::{Error as ArtifactError, Record, Registry, Store},
        walk::Backend as _,
    };
    use std::{
        fs,
        sync::{
            atomic::{AtomicU64, AtomicUsize, Ordering},
            Arc, Mutex,
        },
    };

    static NEXT: AtomicU64 = AtomicU64::new(1);

    #[derive(Default)]
    struct DurableTestStore {
        records: Mutex<Vec<Record>>,
        reserve_calls: AtomicUsize,
        fail_reserve: Option<usize>,
    }
    impl DurableTestStore {
        fn fail_on_reserve(call: usize) -> Self {
            Self {
                fail_reserve: Some(call),
                ..Default::default()
            }
        }
    }
    impl Store for DurableTestStore {
        fn load(&self) -> std::result::Result<Vec<Record>, ArtifactError> {
            Ok(self.records.lock().unwrap().clone())
        }
        fn reserve(&self, record: &Record) -> std::result::Result<(), ArtifactError> {
            let call = self.reserve_calls.fetch_add(1, Ordering::SeqCst) + 1;
            if self.fail_reserve == Some(call) {
                return Err(ArtifactError::Store("injected reservation failure".into()));
            }
            self.records.lock().unwrap().push(record.clone());
            Ok(())
        }
        fn retire(&self, record: &Record) -> std::result::Result<(), ArtifactError> {
            let mut records = self.records.lock().unwrap();
            let Some(at) = records.iter().position(|current| current == record) else {
                return Ok(());
            };
            records.remove(at);
            Ok(())
        }
    }
    fn durable_registry(store: Arc<DurableTestStore>) -> Arc<Registry> {
        Registry::open(store).unwrap()
    }
    struct Fixture {
        base: PathBuf,
        read: PathBuf,
        write: PathBuf,
        other: PathBuf,
    }
    impl Fixture {
        fn new() -> Self {
            let base = std::env::temp_dir().join(format!(
                "aw-capability-set-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            let read = base.join("read");
            let write = base.join("write");
            let other = base.join("other");
            fs::create_dir_all(&read).unwrap();
            fs::create_dir_all(&write).unwrap();
            fs::create_dir_all(&other).unwrap();
            Self {
                base,
                read,
                write,
                other,
            }
        }
        fn set(&self) -> CapabilitySet {
            CapabilitySet::open(
                &self.base,
                [
                    GrantSpec::read(&self.read),
                    GrantSpec::write(&self.write),
                    GrantSpec::write(&self.other),
                ],
            )
            .unwrap()
        }
        fn durable_set(&self, registry: Arc<Registry>) -> CapabilitySet {
            CapabilitySet::open_prepared(
                &self.base,
                [
                    PreparedGrant::new(GrantSpec::read(&self.read), registry.clone()),
                    PreparedGrant::new(GrantSpec::write(&self.write), registry.clone()),
                    PreparedGrant::new(GrantSpec::write(&self.other), registry),
                ],
            )
            .unwrap()
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.base);
        }
    }

    #[test]
    fn write_only_destination_supports_shared_cross_root_copy_without_read_oracle() {
        let f = Fixture::new();
        fs::write(f.read.join("source.txt"), b"payload").unwrap();
        let set = f.set();
        assert!(set.entry_meta("write/copied.txt").is_err());
        assert!(set.mutation_meta("write/copied.txt").is_err()); // missing is expected, but through write authority
        assert_eq!(
            operations::copy_path(&set, "read/source.txt", "write/copied.txt", true, &|| false)
                .unwrap(),
            aw_tools::filesystem::status::Primary::Ok
        );
        assert_eq!(fs::read(f.write.join("copied.txt")).unwrap(), b"payload");
        assert!(
            set.entry_meta("write/copied.txt").is_err(),
            "write-only grant must not become readable"
        );
        assert!(set.mutation_meta("write/copied.txt").is_ok());
    }

    #[test]
    fn absolute_and_relative_paths_share_the_same_pinned_capabilities() {
        let f = Fixture::new();
        fs::write(f.read.join("a.txt"), b"a").unwrap();
        let set = f.set();
        assert_eq!(set.read_bytes("read/a.txt", 16).unwrap(), b"a");
        let absolute = f.read.join("a.txt").to_string_lossy().into_owned();
        assert_eq!(set.read_bytes(&absolute, 16).unwrap(), b"a");
        let identities = set.identities().unwrap();
        assert_eq!(identities.len(), 3);
        assert!(identities
            .iter()
            .all(|identity| identity.volume != 0 && identity.object != 0));
    }

    #[test]
    fn multi_root_remove_preflights_then_removes_in_request_order() {
        let f = Fixture::new();
        fs::write(f.write.join("a.txt"), b"a").unwrap();
        fs::write(f.other.join("b.txt"), b"b").unwrap();
        let store = Arc::new(DurableTestStore::default());
        let registry = durable_registry(store);
        let set = f.durable_set(registry.clone());
        let outcome = operations::remove_paths(
            &set,
            &["write/a.txt".into(), "other/b.txt".into()],
            false,
            &|| false,
        )
        .unwrap();
        assert_eq!(outcome.compact(), "ok 2");
        assert!(!f.write.join("a.txt").exists());
        assert!(!f.other.join("b.txt").exists());
        assert!(registry.records().unwrap().is_empty());
    }

    #[test]
    fn volatile_remove_fails_before_namespace_detach() {
        let f = Fixture::new();
        fs::write(f.write.join("keep.txt"), b"keep").unwrap();
        let set = f.set();
        assert!(
            operations::remove_paths(&set, &["write/keep.txt".into()], false, &|| false,).is_err()
        );
        assert_eq!(fs::read(f.write.join("keep.txt")).unwrap(), b"keep");
    }

    #[test]
    fn all_tombstone_reservations_precede_the_first_detach() {
        let f = Fixture::new();
        fs::write(f.write.join("a.txt"), b"a").unwrap();
        fs::write(f.other.join("b.txt"), b"b").unwrap();
        let store = Arc::new(DurableTestStore::fail_on_reserve(2));
        let registry = durable_registry(store.clone());
        let set = f.durable_set(registry.clone());
        assert!(operations::remove_paths(
            &set,
            &["write/a.txt".into(), "other/b.txt".into()],
            false,
            &|| false,
        )
        .is_err());
        assert_eq!(fs::read(f.write.join("a.txt")).unwrap(), b"a");
        assert_eq!(fs::read(f.other.join("b.txt")).unwrap(), b"b");
        assert!(registry.records().unwrap().is_empty());
        assert!(store.records.lock().unwrap().is_empty());
        assert_eq!(store.reserve_calls.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn remove_rejects_relative_and_absolute_ancestor_aliases_before_detach() {
        let f = Fixture::new();
        let tree = f.write.join("tree");
        fs::create_dir(&tree).unwrap();
        fs::write(tree.join("child.txt"), b"child").unwrap();
        let set = f.set();
        let absolute_child = tree.join("child.txt").to_string_lossy().into_owned();
        let error =
            operations::remove_paths(&set, &["write/tree".into(), absolute_child], true, &|| {
                false
            })
            .unwrap_err();
        assert!(matches!(error, operations::Error::Backend(_)));
        assert!(tree.exists());
        assert_eq!(fs::read(tree.join("child.txt")).unwrap(), b"child");
    }

    #[test]
    fn multi_root_remove_failure_preflights_every_root_before_detach() {
        let f = Fixture::new();
        fs::write(f.write.join("keep.txt"), b"keep").unwrap();
        let set = f.set();
        let error = operations::remove_paths(
            &set,
            &["write/keep.txt".into(), "other/missing.txt".into()],
            false,
            &|| false,
        )
        .unwrap_err();
        assert!(matches!(error, operations::Error::Backend(_)));
        assert_eq!(fs::read(f.write.join("keep.txt")).unwrap(), b"keep");
    }

    #[test]
    fn final_symlink_can_cross_admitted_read_roots_and_returns_absolute_target() {
        let f = Fixture::new();
        let aliases = f.base.join("aliases");
        let targets = f.base.join("targets");
        fs::create_dir(&aliases).unwrap();
        fs::create_dir(&targets).unwrap();
        fs::write(targets.join("data.txt"), b"target").unwrap();
        std::os::unix::fs::symlink("../targets/data.txt", aliases.join("link")).unwrap();
        let set = CapabilitySet::open(
            &f.base,
            [GrantSpec::read(&aliases), GrantSpec::read(&targets)],
        )
        .unwrap();
        let resolved = set.resolve_content_path("aliases/link", false).unwrap();
        assert_eq!(PathBuf::from(&resolved), targets.join("data.txt"));
        assert_eq!(set.read_bytes(&resolved, 32).unwrap(), b"target");
    }

    #[test]
    fn write_only_final_symlink_target_requires_write_not_read_authority() {
        let f = Fixture::new();
        let aliases = f.base.join("write-aliases");
        let targets = f.base.join("write-targets");
        fs::create_dir(&aliases).unwrap();
        fs::create_dir(&targets).unwrap();
        fs::write(targets.join("data.txt"), b"target").unwrap();
        std::os::unix::fs::symlink("../write-targets/data.txt", aliases.join("link")).unwrap();
        let set = CapabilitySet::open(
            &f.base,
            [GrantSpec::write(&aliases), GrantSpec::write(&targets)],
        )
        .unwrap();
        let resolved = set
            .resolve_content_path("write-aliases/link", true)
            .unwrap();
        assert_eq!(PathBuf::from(&resolved), targets.join("data.txt"));
        assert!(set.entry_meta(&resolved).is_err());
        assert!(set.mutation_meta(&resolved).is_ok());
    }

    #[test]
    fn final_symlink_target_outside_admitted_grants_fails_closed() {
        let f = Fixture::new();
        let aliases = f.base.join("read-aliases");
        let outside = f.base.join("outside");
        fs::create_dir(&aliases).unwrap();
        fs::create_dir(&outside).unwrap();
        fs::write(outside.join("data.txt"), b"outside").unwrap();
        std::os::unix::fs::symlink("../outside/data.txt", aliases.join("link")).unwrap();
        let set = CapabilitySet::open(&f.base, [GrantSpec::read(&aliases)]).unwrap();
        assert!(set
            .resolve_content_path("read-aliases/link", false)
            .is_err());
    }

    #[test]
    fn read_nested_under_write_uses_write_grant_registry_for_read_write_stage() {
        use aw_tools::filesystem::{backend::SecuredFilesystem as _, private_artifact::Registry};
        let f = Fixture::new();
        let nested = f.write.join("nested");
        fs::create_dir(&nested).unwrap();
        fs::write(nested.join("target.txt"), b"old").unwrap();
        let write_registry = Registry::memory();
        let read_registry = Registry::memory();
        let set = CapabilitySet::open_prepared(
            &f.base,
            [
                PreparedGrant::new(GrantSpec::write(&f.write), write_registry.clone()),
                PreparedGrant::new(GrantSpec::read(&nested), read_registry.clone()),
            ],
        )
        .unwrap();
        let staged = set
            .stage_update("write/nested/target.txt", b"old", b"new", &|| false)
            .unwrap();
        assert_eq!(write_registry.records().unwrap().len(), 1);
        assert!(read_registry.records().unwrap().is_empty());
        drop(staged);
    }

    #[test]
    fn read_exclusion_is_omitted_from_discovery_and_rejected_directly() {
        let f = Fixture::new();
        fs::write(f.read.join("visible.txt"), b"visible").unwrap();
        fs::create_dir(f.read.join("secret")).unwrap();
        fs::write(f.read.join("secret/data.txt"), b"secret").unwrap();
        let set = CapabilitySet::open_with_exclusions(
            &f.base,
            [GrantSpec::read(&f.read)],
            [ExclusionSpec::read(f.read.join("secret"))],
        )
        .unwrap();
        let listing = set.list("read", 16).unwrap();
        assert!(listing
            .entries
            .iter()
            .any(|entry| entry.path == "read/visible.txt"));
        assert!(!listing
            .entries
            .iter()
            .any(|entry| entry.path.contains("secret")));
        assert!(set.read_bytes("read/secret/data.txt", 32).is_err());
    }

    #[test]
    fn direct_copy_backend_enforces_read_and_write_exclusion_overlap() {
        use aw_tools::filesystem::backend::SecuredFilesystem as _;
        let f = Fixture::new();
        let tree = f.read.join("tree");
        fs::create_dir(&tree).unwrap();
        fs::write(tree.join("visible.txt"), b"v").unwrap();
        fs::create_dir(tree.join("secret")).unwrap();
        fs::write(tree.join("secret/data.txt"), b"s").unwrap();
        let set = CapabilitySet::open_with_exclusions(
            &f.base,
            [GrantSpec::read(&f.read), GrantSpec::write(&f.write)],
            [ExclusionSpec::read(tree.join("secret"))],
        )
        .unwrap();
        assert!(set
            .copy_exact("read/tree", "write/copied", &|| false)
            .is_err());
        assert!(!f.write.join("copied").exists());
    }

    #[test]
    fn direct_move_backend_enforces_write_exclusion_overlap() {
        use aw_tools::filesystem::backend::SecuredFilesystem as _;
        let f = Fixture::new();
        let tree = f.write.join("tree");
        fs::create_dir(&tree).unwrap();
        fs::write(tree.join("keep.txt"), b"keep").unwrap();
        let set = CapabilitySet::open_with_exclusions(
            &f.base,
            [GrantSpec::write(&f.write), GrantSpec::write(&f.other)],
            [ExclusionSpec::write(tree.join("keep.txt"))],
        )
        .unwrap();
        assert!(set
            .move_exact("write/tree", "other/moved", &|| false)
            .is_err());
        assert!(tree.exists());
        assert!(!f.other.join("moved").exists());
    }

    #[test]
    fn recursive_lifecycle_rejects_exclusion_overlap_before_effect() {
        let f = Fixture::new();
        let source = f.read.join("tree");
        fs::create_dir(&source).unwrap();
        fs::write(source.join("visible.txt"), b"v").unwrap();
        fs::create_dir(source.join("secret")).unwrap();
        fs::write(source.join("secret/data.txt"), b"s").unwrap();
        let set = CapabilitySet::open_with_exclusions(
            &f.base,
            [GrantSpec::read(&f.read), GrantSpec::write(&f.write)],
            [ExclusionSpec::read(source.join("secret"))],
        )
        .unwrap();
        assert!(
            operations::copy_path(&set, "read/tree", "write/copied", true, &|| false,).is_err()
        );
        assert!(!f.write.join("copied").exists());
    }

    #[test]
    fn write_exclusion_blocks_remove_of_ancestor_before_any_detach() {
        let f = Fixture::new();
        let tree = f.write.join("tree");
        fs::create_dir(&tree).unwrap();
        fs::write(tree.join("keep.txt"), b"keep").unwrap();
        let set = CapabilitySet::open_with_exclusions(
            &f.base,
            [GrantSpec::write(&f.write)],
            [ExclusionSpec::write(tree.join("keep.txt"))],
        )
        .unwrap();
        assert!(operations::remove_paths(&set, &["write/tree".into()], true, &|| false,).is_err());
        assert_eq!(fs::read(tree.join("keep.txt")).unwrap(), b"keep");
    }

    #[test]
    fn prepared_registry_survives_capability_set_reconstruction_and_reconciles_orphan() {
        use aw_tools::filesystem::{backend::SecuredFilesystem as _, private_artifact::Registry};
        let f = Fixture::new();
        fs::write(f.write.join("target.txt"), b"old").unwrap();
        let registry = Registry::memory();
        let set = CapabilitySet::open_prepared(
            &f.base,
            [PreparedGrant::new(
                GrantSpec::write(&f.write),
                registry.clone(),
            )],
        )
        .unwrap();
        let staged = set
            .stage_replace("write/target.txt", b"new", &|| false)
            .unwrap();
        let record = registry.records().unwrap().into_iter().next().unwrap();
        let private = f.write.join(&record.name);
        assert!(private.exists());
        std::mem::forget(staged);
        drop(set);

        let recovered = CapabilitySet::open_prepared(
            &f.base,
            [PreparedGrant::new(
                GrantSpec::write(&f.write),
                registry.clone(),
            )],
        )
        .unwrap();
        recovered.reconcile_private_artifacts().unwrap();
        assert!(!private.exists());
        assert!(registry.records().unwrap().is_empty());
        assert_eq!(fs::read(f.write.join("target.txt")).unwrap(), b"old");
    }

    #[test]
    fn duplicate_prepared_root_requires_the_same_registry_object() {
        use aw_tools::filesystem::private_artifact::Registry;
        let f = Fixture::new();
        let first = Registry::memory();
        let second = Registry::memory();
        assert!(CapabilitySet::open_prepared(
            &f.base,
            [
                PreparedGrant::new(GrantSpec::read(&f.read), first),
                PreparedGrant::new(GrantSpec::write(&f.read), second),
            ],
        )
        .is_err());
        let shared = Registry::memory();
        let set = CapabilitySet::open_prepared(
            &f.base,
            [
                PreparedGrant::new(GrantSpec::read(&f.read), shared.clone()),
                PreparedGrant::new(GrantSpec::write(&f.read), shared),
            ],
        )
        .unwrap();
        let identity = set.identities().unwrap();
        assert_eq!(identity.len(), 1);
        assert!(identity[0].read && identity[0].write);
    }

    #[test]
    fn discovery_keeps_the_callers_relative_or_absolute_model_namespace() {
        let f = Fixture::new();
        fs::write(f.read.join("a.txt"), b"a").unwrap();
        let set = f.set();
        let relative = set.list("read", 16).unwrap();
        assert!(relative
            .entries
            .iter()
            .any(|entry| entry.path == "read/a.txt"));
        let absolute_root = f.read.to_string_lossy().into_owned();
        let absolute = set.list(&absolute_root, 16).unwrap();
        assert!(absolute
            .entries
            .iter()
            .any(|entry| entry.path == format!("{absolute_root}/a.txt")));
    }
}
