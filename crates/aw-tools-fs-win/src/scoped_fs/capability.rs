//! Multi-root secured filesystem capability set for Windows hosts.
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
    fs, io,
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
    model_path: String,
    exact_segments: Vec<String>,
    root: Root,
    read: bool,
    write: bool,
}

struct Exclusion {
    folded_segments: Vec<Vec<u16>>,
    read: bool,
    write: bool,
}

pub struct CapabilitySet {
    base: PathBuf,
    base_model: String,
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
    absolute_model: String,
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

fn portable_canonical(path: &Path) -> Result<String> {
    let value = path
        .to_str()
        .ok_or(ScopedError::Denied("capability root is not UTF-8"))?;
    let model = if let Some(rest) = value.strip_prefix(r"\\?\UNC\") {
        format!("//{}", rest.replace('\\', "/"))
    } else if let Some(rest) = value.strip_prefix(r"\\") {
        format!("//{}", rest.replace('\\', "/"))
    } else {
        value
            .strip_prefix(r"\\?\")
            .unwrap_or(value)
            .replace('\\', "/")
    };
    aw_tools::filesystem::path::validate(&model)
        .map_err(|_| ScopedError::Denied("capability root has no canonical model spelling"))?;
    if aw_tools::filesystem::path::is_unc(&model) {
        return Ok(model.trim_end_matches('/').to_owned());
    }
    if !aw_tools::filesystem::path::is_absolute(&model) || model.starts_with('/') {
        return Err(ScopedError::Denied(
            "capability root is not a Windows absolute path",
        ));
    }
    if model.len() == 3 && model.as_bytes()[1] == b':' && model.ends_with('/') {
        Ok(model)
    } else {
        Ok(model.trim_end_matches('/').to_owned())
    }
}

fn fold_segment(value: &str) -> Result<Vec<u16>> {
    super::port::folded_component_units(value)
}

fn model_components(model: &str) -> Result<Vec<String>> {
    aw_tools::filesystem::path::validate(model)
        .map_err(|_| ScopedError::Denied("invalid model path"))?;
    if aw_tools::filesystem::path::is_unc(model) {
        let mut values = vec!["//".to_owned()];
        values.extend(
            model[2..]
                .trim_end_matches('/')
                .split('/')
                .map(str::to_owned),
        );
        return Ok(values);
    }
    if !aw_tools::filesystem::path::is_absolute(model) || model.starts_with('/') {
        return Err(ScopedError::Denied("model path is not Windows absolute"));
    }
    Ok(model
        .trim_end_matches('/')
        .split('/')
        .map(str::to_owned)
        .collect())
}

fn scheduling_model(model: &str) -> Result<String> {
    model_components(model)?
        .iter()
        .map(|part| super::port::folded_component_label(part))
        .collect::<Result<Vec<_>>>()
        .map(|parts| parts.join("/"))
}

fn exact_segments(model: &str) -> Result<Vec<String>> {
    model_components(model)
}

fn exact_prefix(prefix: &[String], full: &[String]) -> bool {
    prefix.len() <= full.len() && prefix.iter().zip(full).all(|(a, b)| a == b)
}

fn segments(model: &str) -> Result<Vec<Vec<u16>>> {
    model_components(model)?
        .iter()
        .map(|part| fold_segment(part))
        .collect()
}

fn segment_prefix(prefix: &[Vec<u16>], full: &[Vec<u16>]) -> bool {
    prefix.len() <= full.len() && prefix.iter().zip(full).all(|(a, b)| a == b)
}

fn join_model(root: &str, suffix: &str) -> String {
    if suffix.is_empty() {
        return root.to_owned();
    }
    if root.ends_with(":/") {
        format!("{root}{suffix}")
    } else {
        format!("{root}/{suffix}")
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
        let base_model = portable_canonical(&base)?;
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
        let mut objects = BTreeMap::<(u32, u64), PathBuf>::new();
        for (path, (read, write, private)) in merged {
            let root = Root::open_with_private_artifacts(&path, private)?;
            if root.root != path {
                return Err(ScopedError::Denied("capability root changed while opening"));
            }
            let handle = root
                ._directories
                .last()
                .ok_or(ScopedError::Denied("capability root handle missing"))?;
            let identity = crate::file_identity(handle)?;
            if objects.insert(identity, path.clone()).is_some() {
                return Err(ScopedError::Denied(
                    "multiple capability roots alias the same directory object",
                ));
            }
            let model_path = portable_canonical(&root.root)?;
            let exact_segments = exact_segments(&model_path)?;
            opened.push(Grant {
                path: root.root.clone(),
                model_path,
                exact_segments,
                root,
                read,
                write,
            });
        }
        let mut denied = BTreeMap::<Vec<Vec<u16>>, (bool, bool)>::new();
        for exclusion in exclusions {
            if !exclusion.read && !exclusion.write {
                return Err(ScopedError::Denied(
                    "capability exclusion has no access bit",
                ));
            }
            let model = Self::prepared_exclusion_model(&base, &exclusion.path)?;
            let key = segments(&model)?;
            let access = denied.entry(key).or_default();
            access.0 |= exclusion.read;
            access.1 |= exclusion.write;
        }
        let exclusions = denied
            .into_iter()
            .map(|(folded_segments, (read, write))| Exclusion {
                folded_segments,
                read,
                write,
            })
            .collect();
        Ok(Self {
            base,
            base_model,
            grants: opened,
            exclusions,
        })
    }

    pub fn base(&self) -> &Path {
        &self.base
    }

    fn prepared_exclusion_model(base: &Path, path: &Path) -> Result<String> {
        let joined = if path.is_absolute() {
            path.to_path_buf()
        } else {
            base.join(path)
        };
        let model = portable_canonical(&joined)?;
        segments(&model)?;
        Ok(model)
    }

    fn exclusion_applies(exclusion: &Exclusion, need: Need) -> bool {
        match need {
            Need::Read => exclusion.read,
            Need::Write => exclusion.write,
            Need::Both => exclusion.read || exclusion.write,
            Need::Coordination => false,
        }
    }

    fn excluded(&self, full: &[Vec<u16>], need: Need) -> bool {
        self.exclusions.iter().any(|exclusion| {
            Self::exclusion_applies(exclusion, need)
                && segment_prefix(&exclusion.folded_segments, full)
        })
    }

    fn overlaps_exclusion(&self, full: &[Vec<u16>], need: Need) -> bool {
        self.exclusions.iter().any(|exclusion| {
            Self::exclusion_applies(exclusion, need)
                && (segment_prefix(&exclusion.folded_segments, full)
                    || segment_prefix(full, &exclusion.folded_segments))
        })
    }

    fn relative_for_grant(grant: &Grant, absolute_model: &str) -> Result<Option<String>> {
        let full = exact_segments(absolute_model)?;
        if !exact_prefix(&grant.exact_segments, &full) {
            return Ok(None);
        }
        let relative = full[grant.exact_segments.len()..].join("/");
        if !relative.is_empty() {
            super::validate_path(&relative)?;
        }
        Ok(Some(relative))
    }

    fn private_absolute(&self, absolute_model: &str) -> Result<bool> {
        for grant in &self.grants {
            let Some(relative) = Self::relative_for_grant(grant, absolute_model)? else {
                continue;
            };
            if !relative.is_empty() && grant.root.is_private_path(&relative)? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn private_subtree_overlap(&self, absolute_model: &str) -> Result<bool> {
        let subtree = segments(absolute_model)?;
        for grant in &self.grants {
            let records = grant.root.private_artifacts().records().map_err(|_| {
                ScopedError::Denied("private mutation artifact registry unavailable")
            })?;
            for record in records {
                let relative = if record.parent_path.is_empty() {
                    record.name.clone()
                } else {
                    format!("{}/{}", record.parent_path, record.name)
                };
                super::validate_path(&relative)?;
                let candidate = join_model(&grant.model_path, &relative);
                if segment_prefix(&subtree, &segments(&candidate)?)
                    && grant.root.is_private_path(&relative)?
                {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }

    pub fn reconcile_private_artifacts(&self) -> Result<()> {
        for grant in &self.grants {
            grant.root.reconcile_private_artifacts()?;
        }
        Ok(())
    }

    pub fn identities(&self) -> Result<Vec<GrantIdentity>> {
        self.grants
            .iter()
            .map(|grant| {
                let handle = grant
                    .root
                    ._directories
                    .last()
                    .ok_or(ScopedError::Denied("capability root handle missing"))?;
                let (volume, object) = crate::file_identity(handle)?;
                Ok(GrantIdentity {
                    path: grant.path.clone(),
                    volume: u64::from(volume),
                    object,
                    read: grant.read,
                    write: grant.write,
                })
            })
            .collect()
    }

    fn absolute_model(&self, model: &str) -> Result<String> {
        aw_tools::filesystem::path::validate(model)
            .map_err(|_| ScopedError::Denied("invalid model path"))?;
        if model == "." {
            return Ok(self.base_model.clone());
        }
        if aw_tools::filesystem::path::is_unc(model) {
            return Ok(model.trim_end_matches('/').to_owned());
        }
        if model.starts_with('/') {
            return Err(ScopedError::Denied(
                "POSIX absolute path is not valid on Windows",
            ));
        }
        if aw_tools::filesystem::path::is_absolute(model) {
            if model.len() == 3 && model.as_bytes()[1] == b':' && model.ends_with('/') {
                return Ok(model.to_owned());
            }
            return Ok(model.trim_end_matches('/').to_owned());
        }
        Ok(join_model(&self.base_model, model))
    }

    fn route(&self, model: &str, need: Need) -> Result<Routed<'_>> {
        let absolute_model = self.absolute_model(model)?;
        let exact = exact_segments(&absolute_model)?;
        let folded = segments(&absolute_model)?;
        if !matches!(need, Need::Coordination) && self.excluded(&folded, need) {
            return Err(ScopedError::Denied(
                "path is excluded by prepared capability policy",
            ));
        }
        if matches!(need, Need::Both) {
            if !self
                .grants
                .iter()
                .any(|g| g.read && exact_prefix(&g.exact_segments, &exact))
            {
                return Err(ScopedError::Denied("path is not covered by a read grant"));
            }
            if !self
                .grants
                .iter()
                .any(|g| g.write && exact_prefix(&g.exact_segments, &exact))
            {
                return Err(ScopedError::Denied("path is not covered by a write grant"));
            }
        }
        let mut best = None::<(usize, usize)>;
        for (index, grant) in self.grants.iter().enumerate() {
            let access = match need {
                Need::Read => grant.read,
                Need::Write => grant.write,
                Need::Both => grant.write,
                Need::Coordination => true,
            };
            if access && exact_prefix(&grant.exact_segments, &exact) {
                let depth = grant.exact_segments.len();
                if best.is_none_or(|(_, old)| depth > old) {
                    best = Some((index, depth));
                }
            }
        }
        let (index, depth) = best.ok_or(ScopedError::Denied(match need {
            Need::Read => "path is not covered by a read grant",
            Need::Write => "path is not covered by a write grant",
            Need::Both => "path is not covered by read and write grants",
            Need::Coordination => "path is not covered by a filesystem grant",
        }))?;
        let grant = &self.grants[index];
        let original_parts = model_components(&absolute_model)?;
        let relative = original_parts[depth..].join("/");
        if !relative.is_empty() {
            super::validate_path(&relative)?;
        }
        if !matches!(need, Need::Coordination) && self.private_absolute(&absolute_model)? {
            return Err(ScopedError::Denied(
                "private mutation artifact is not externally addressable",
            ));
        }
        Ok(Routed {
            grant,
            index,
            relative,
            absolute_model,
        })
    }
    fn route_lock(&self, model: &str) -> Result<Routed<'_>> {
        self.route(model, Need::Coordination)
    }

    fn map_listing(&self, model: &str, routed: &Routed<'_>, max: usize) -> Result<walk::Listing> {
        let listing = <Root as walk::Backend>::list(&routed.grant.root, &routed.relative, max)?;
        let mut entries = Vec::with_capacity(listing.entries.len());
        for entry in listing.entries {
            let model_path = if model == "." {
                suffix(&entry.path, &routed.relative).to_owned()
            } else {
                join_model(model, suffix(&entry.path, &routed.relative))
            };
            let absolute = self.absolute_model(&model_path)?;
            if self.excluded(&segments(&absolute)?, Need::Read)
                || self.private_absolute(&absolute)?
            {
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

    fn is_descendant(source: &Routed<'_>, destination: &Routed<'_>, directory: bool) -> Result<()> {
        if !directory {
            return Ok(());
        }
        let a = segments(&source.absolute_model)?;
        let b = segments(&destination.absolute_model)?;
        if b.len() > a.len() && segment_prefix(&a, &b) {
            return Err(ScopedError::Denied("destination is inside source"));
        }
        Ok(())
    }

    fn resolve_final(
        &self,
        path: &str,
        allow_missing: bool,
        need: Need,
        directory: bool,
        label: &'static str,
    ) -> Result<String> {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_DELETE,
            FILE_SHARE_READ, FILE_SHARE_WRITE,
        };
        let lexical = self.route(path, need)?;
        if lexical.relative.is_empty() {
            return if directory {
                Ok(path.to_owned())
            } else {
                Err(ScopedError::Denied(label))
            };
        }
        lexical.grant.root.ensure_public_path(&lexical.relative)?;
        let (_parent, target) = match lexical.grant.root.parent(&lexical.relative) {
            Ok(v) => v,
            Err(ScopedError::Io(e)) if allow_missing && e.kind() == io::ErrorKind::NotFound => {
                return Ok(path.to_owned())
            }
            Err(e) => return Err(e),
        };
        let entry = match fs::symlink_metadata(&target) {
            Ok(v) => v,
            Err(e) if allow_missing && e.kind() == io::ErrorKind::NotFound => {
                return Ok(path.to_owned())
            }
            Err(e) => return Err(e.into()),
        };
        if !super::is_reparse(&entry) {
            if entry.is_dir() == directory && (entry.is_dir() || entry.is_file()) {
                return Ok(path.to_owned());
            }
            return Err(ScopedError::Denied(label));
        }
        if !entry.file_type().is_symlink() {
            return Err(ScopedError::Denied("unsupported final reparse point"));
        }
        let followed = fs::OpenOptions::new()
            .access_mode(0)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
            .open(&target)
            .map_err(|e| {
                if e.kind() == io::ErrorKind::NotFound {
                    ScopedError::Denied("dangling final symlink is not a missing target")
                } else {
                    ScopedError::Io(e)
                }
            })?;
        let followed_meta = followed.metadata()?;
        if followed_meta.is_dir() != directory
            || (!followed_meta.is_dir() && !followed_meta.is_file())
        {
            return Err(ScopedError::Denied(label));
        }
        let followed_id = crate::file_identity(&followed)?;
        let canonical = target.canonicalize().map_err(|e| {
            if e.kind() == io::ErrorKind::NotFound {
                ScopedError::Denied("dangling final symlink is not a missing target")
            } else {
                ScopedError::Io(e)
            }
        })?;
        let canonical_model = portable_canonical(&canonical)?;
        let resolved = self.route(&canonical_model, need)?;
        let verified = if resolved.relative.is_empty() {
            let handle = resolved
                .grant
                .root
                ._directories
                .last()
                .ok_or(ScopedError::Denied("capability root handle missing"))?;
            crate::file_identity(handle)?
        } else {
            resolved.grant.root.ensure_public_path(&resolved.relative)?;
            let (_p, t) = resolved.grant.root.parent(&resolved.relative)?;
            let file = fs::OpenOptions::new()
                .access_mode(0)
                .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
                .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS)
                .open(&t)?;
            crate::file_identity(&file)?
        };
        if verified != followed_id {
            return Err(ScopedError::Denied(
                "final symlink target changed during resolution",
            ));
        }
        Ok(join_model(&resolved.grant.model_path, &resolved.relative))
    }
}

impl walk::Backend for CapabilitySet {
    type Error = ScopedError;
    fn list(&self, path: &str, max: usize) -> Result<walk::Listing> {
        let r = self.route(path, Need::Read)?;
        self.map_listing(path, &r, max)
    }
    fn read_bytes(&self, path: &str, max: usize) -> Result<Vec<u8>> {
        let r = self.route(path, Need::Read)?;
        Root::read_bytes(&r.grant.root, &r.relative, max)
    }
    fn classify(&self, error: &Self::Error) -> walk::BackendErrorKind {
        error_kind(error)
    }
    fn private_entry(&self, path: &str) -> Result<bool> {
        let absolute = self.absolute_model(path)?;
        if self.private_absolute(&absolute)? {
            return Ok(true);
        }
        let r = self.route(path, Need::Read)?;
        Ok(!r.relative.is_empty() && r.grant.root.is_private_path(&r.relative)?)
    }
}
fn root_entry_meta(root: &Root) -> Result<EntryMeta> {
    let handle = root._directories.last().ok_or(ScopedError::Denied(
        "prepared capability root is not pinned",
    ))?;
    let meta = handle.metadata()?;
    Ok(EntryMeta {
        kind: aw_tools::filesystem::Kind::Dir,
        size: None,
        mtime: meta.modified().ok(),
        ctime: None,
        btime: meta.created().ok(),
        mode: None,
    })
}

impl MetadataBackend for CapabilitySet {
    fn entry_meta(&self, path: &str) -> Result<EntryMeta> {
        let r = self.route(path, Need::Read)?;
        if r.relative.is_empty() {
            return root_entry_meta(&r.grant.root);
        }
        Root::entry_meta(&r.grant.root, &r.relative)
    }

    fn scheduling_path(&self, path: &str) -> Result<String> {
        scheduling_model(&self.absolute_model(path)?)
    }
}
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
            false,
            "final symlink target is not a regular file",
        )
    }
    fn resolve_directory_path(&self, path: &str) -> Result<String> {
        self.resolve_final(
            path,
            false,
            Need::Read,
            true,
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
            if path.extent == MutationExtent::Subtree {
                if self.overlaps_exclusion(&segments(&routed.absolute_model)?, need) {
                    return Err(ScopedError::Denied(
                        "mutation subtree overlaps a prepared capability exclusion",
                    ));
                }
                if self.private_subtree_overlap(&routed.absolute_model)? {
                    return Err(ScopedError::Denied(
                        "mutation subtree overlaps a private mutation artifact",
                    ));
                }
            }
        }
        Ok(())
    }

    fn mutation_meta(&self, path: &str) -> Result<EntryMeta> {
        let r = self.route(path, Need::Write)?;
        if r.relative.is_empty() {
            return Err(ScopedError::Denied("mutation path must name an entry"));
        }
        Root::entry_meta(&r.grant.root, &r.relative)
    }
    fn mutation_path_key(&self, path: &str) -> Result<Vec<u8>> {
        let r = self.route(path, Need::Write)?;
        if r.relative.is_empty() {
            return Err(ScopedError::Denied("mutation path must name an entry"));
        }
        aw_tools::filesystem::backend::SecuredFilesystem::mutation_path_key(
            &r.grant.root,
            &r.relative,
        )
    }
    fn lock<'a>(&'a self, paths: &[String], cancel: &dyn Fn() -> bool) -> Result<Self::Guard<'a>> {
        let mut groups = BTreeMap::<String, (usize, Vec<String>)>::new();
        for model in paths {
            let r = self.route_lock(model)?;
            if r.relative.is_empty() {
                return Err(ScopedError::Denied("mutation path must name an entry"));
            }
            groups
                .entry(r.grant.model_path.clone())
                .or_insert_with(|| (r.index, Vec::new()))
                .1
                .push(r.relative);
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
        let r = self.route(path, Need::Write)?;
        Root::stage_create(&r.grant.root, &r.relative, bytes, cancel)
    }
    fn stage_create_many(
        &self,
        files: &[aw_tools::filesystem::backend::CreateFile<'_>],
        cancel: &dyn Fn() -> bool,
    ) -> Result<Vec<aw_tools::filesystem::backend::CreateStage<Self::Stage>>> {
        let routed = files
            .iter()
            .map(|file| self.route(file.path, Need::Write))
            .collect::<Result<Vec<_>>>()?;
        let folded = routed
            .iter()
            .map(|route| segments(&route.absolute_model))
            .collect::<Result<Vec<_>>>()?;
        for left in 0..folded.len() {
            for right in left + 1..folded.len() {
                let a = &folded[left];
                let b = &folded[right];
                if (a.len() <= b.len() && segment_prefix(a, b))
                    || (b.len() <= a.len() && segment_prefix(b, a))
                {
                    return Err(ScopedError::Denied("create paths overlap"));
                }
            }
        }
        let mut groups = std::collections::BTreeMap::<usize, Vec<(usize, String, &[u8])>>::new();
        for (index, (route, file)) in routed.iter().zip(files).enumerate() {
            if route.relative.is_empty() {
                return Err(ScopedError::Denied("mutation path must name an entry"));
            }
            groups.entry(route.index).or_default().push((
                index,
                route.relative.clone(),
                file.bytes,
            ));
        }
        let mut staged = Vec::new();
        for (grant, entries) in groups {
            let local = entries
                .iter()
                .map(
                    |(_, path, bytes)| aw_tools::filesystem::backend::CreateFile {
                        path: path.as_str(),
                        bytes,
                    },
                )
                .collect::<Vec<_>>();
            for unit in Root::stage_create_many(&self.grants[grant].root, &local, cancel)? {
                let first = entries
                    .get(unit.first)
                    .ok_or(ScopedError::Denied("invalid staged create unit index"))?
                    .0;
                let inputs = unit
                    .inputs
                    .iter()
                    .map(|local_index| {
                        entries
                            .get(*local_index)
                            .map(|entry| entry.0)
                            .ok_or(ScopedError::Denied("invalid staged create unit index"))
                    })
                    .collect::<Result<Vec<_>>>()?;
                staged.push(aw_tools::filesystem::backend::CreateStage {
                    first,
                    inputs,
                    stage: unit.stage,
                });
            }
        }
        staged.sort_by_key(|unit| unit.first);
        Ok(staged)
    }
    fn stage_replace(
        &self,
        path: &str,
        bytes: &[u8],
        cancel: &dyn Fn() -> bool,
    ) -> Result<Self::Stage> {
        let r = self.route(path, Need::Write)?;
        Root::stage_replace(&r.grant.root, &r.relative, bytes, cancel)
    }
    fn stage_update(
        &self,
        path: &str,
        expected: &[u8],
        bytes: &[u8],
        cancel: &dyn Fn() -> bool,
    ) -> Result<Self::Stage> {
        let r = self.route(path, Need::Both)?;
        Root::stage_update(&r.grant.root, &r.relative, expected, bytes, cancel)
    }
    fn open_regular_for_update(&self, path: &str) -> Result<std::fs::File> {
        let routed = self.route(path, Need::Both)?;
        Root::open_regular(&routed.grant.root, &routed.relative)
    }
    fn stage_stream_update(
        &self,
        path: &str,
        expected: (u64, [u8; 32]),
        writer: &mut dyn FnMut(
            &mut std::fs::File,
        ) -> std::result::Result<
            (),
            aw_tools::filesystem::backend::StreamWriteError,
        >,
        cancel: &dyn Fn() -> bool,
    ) -> Result<Self::Stage> {
        let routed = self.route(path, Need::Both)?;
        Root::stage_stream_update(
            &routed.grant.root,
            &routed.relative,
            expected,
            |output| {
                writer(output).map_err(|error| match error {
                    aw_tools::filesystem::backend::StreamWriteError::Io(error) => {
                        ScopedError::Io(error)
                    }
                    aw_tools::filesystem::backend::StreamWriteError::Cancelled => {
                        ScopedError::Cancelled
                    }
                    aw_tools::filesystem::backend::StreamWriteError::Refused(reason) => {
                        ScopedError::Denied(reason)
                    }
                })
            },
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
        let s = self.route(source, Need::Read)?;
        let d = self.route(destination, Need::Write)?;
        if self.overlaps_exclusion(&segments(&s.absolute_model)?, Need::Read)
            || self.overlaps_exclusion(&segments(&d.absolute_model)?, Need::Write)
        {
            return Err(ScopedError::Denied(
                "copy overlaps a prepared capability exclusion",
            ));
        }
        if self.private_subtree_overlap(&s.absolute_model)?
            || self.private_subtree_overlap(&d.absolute_model)?
        {
            return Err(ScopedError::Denied(
                "copy overlaps a private mutation artifact",
            ));
        }
        let m = Root::entry_meta(&s.grant.root, &s.relative)?;
        Self::is_descendant(&s, &d, m.kind == Kind::Dir)?;
        port::lifecycle::copy_between(
            &s.grant.root,
            &s.relative,
            &d.grant.root,
            &d.relative,
            true,
            cancel,
        )
    }
    fn move_exact(&self, source: &str, destination: &str, cancel: &dyn Fn() -> bool) -> Result<()> {
        let s = self.route(source, Need::Write)?;
        let d = self.route(destination, Need::Write)?;
        if self.overlaps_exclusion(&segments(&s.absolute_model)?, Need::Write)
            || self.overlaps_exclusion(&segments(&d.absolute_model)?, Need::Write)
        {
            return Err(ScopedError::Denied(
                "move overlaps a prepared capability exclusion",
            ));
        }
        if self.private_subtree_overlap(&s.absolute_model)?
            || self.private_subtree_overlap(&d.absolute_model)?
        {
            return Err(ScopedError::Denied(
                "move overlaps a private mutation artifact",
            ));
        }
        let m = Root::entry_meta(&s.grant.root, &s.relative)?;
        Self::is_descendant(&s, &d, m.kind == Kind::Dir)?;
        port::lifecycle::move_between(
            &s.grant.root,
            &s.relative,
            &d.grant.root,
            &d.relative,
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
            .map(|p| self.route(p, Need::Write))
            .collect::<Result<Vec<_>>>()?;
        for path in &routed {
            if self.overlaps_exclusion(&segments(&path.absolute_model)?, Need::Write) {
                return Err(ScopedError::Denied(
                    "remove overlaps a prepared capability exclusion",
                ));
            }
            if self.private_subtree_overlap(&path.absolute_model)? {
                return Err(ScopedError::Denied(
                    "remove overlaps a private mutation artifact",
                ));
            }
        }
        let refs = routed
            .iter()
            .map(|p| (&p.grant.root, p.relative.as_str()))
            .collect::<Vec<_>>();
        port::lifecycle::remove_between(&refs, recursive, cancel)
    }
    fn read_page(
        &self,
        path: &str,
        request: aw_tools::read::PageRequest,
        cancel: &dyn Fn() -> bool,
    ) -> Result<aw_tools::read::Page> {
        let r = self.route(path, Need::Read)?;
        Root::read_page_with_cancel(&r.grant.root, &r.relative, request, cancel)
    }
    fn read_binary_window(
        &self,
        path: &str,
        offset: u64,
        limit: usize,
        cancel: &dyn Fn() -> bool,
    ) -> Result<(Vec<u8>, u64, bool)> {
        let routed = self.route(path, Need::Read)?;
        Root::read_binary_window_with_cancel(
            &routed.grant.root,
            &routed.relative,
            offset,
            limit,
            cancel,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scheduling_model_folds_case_but_preserves_hierarchy_and_namespace() {
        assert_eq!(
            scheduling_model("C:/Repo/Foo").unwrap(),
            scheduling_model("c:/repo/foo").unwrap()
        );
        assert_ne!(
            scheduling_model("C:/Repo/Foo").unwrap(),
            scheduling_model("D:/Repo/Foo").unwrap()
        );
        let parent = scheduling_model("//Server/Share/Dir").unwrap();
        let child = scheduling_model("//server/share/dir/File").unwrap();
        assert!(child
            .strip_prefix(&parent)
            .is_some_and(|suffix| suffix.starts_with('/')));
    }

    #[test]
    fn portable_canonical_maps_unc_to_model_namespace() {
        assert_eq!(
            portable_canonical(Path::new(r"\\?\UNC\server\share\Dir\File.txt")).unwrap(),
            "//server/share/Dir/File.txt"
        );
        assert_eq!(
            portable_canonical(Path::new(r"\\server\share\Dir")).unwrap(),
            "//server/share/Dir"
        );
        assert!(aw_tools::filesystem::path::is_unc(
            &portable_canonical(Path::new(r"\\server\share")).unwrap()
        ));
    }

    #[test]
    fn portable_canonical_strips_extended_drive_prefix() {
        let path = Path::new(r"\\?\C:\Repo\Data");
        assert_eq!(portable_canonical(path).unwrap(), "C:/Repo/Data");
    }
    #[test]
    fn portable_drive_root_remains_absolute() {
        let path = Path::new(r"\\?\C:\");
        assert_eq!(portable_canonical(path).unwrap(), "C:/");
        assert!(aw_tools::filesystem::path::is_absolute(
            &portable_canonical(path).unwrap()
        ));
    }

    #[test]
    fn unc_grant_components_preserve_exact_authority_and_fold_for_safety() {
        let grant = exact_segments("//Server/Share/Foo").unwrap();
        assert!(exact_prefix(
            &grant,
            &exact_segments("//Server/Share/Foo/file.txt").unwrap()
        ));
        assert!(!exact_prefix(
            &grant,
            &exact_segments("//server/share/Foo/file.txt").unwrap()
        ));
        assert!(segment_prefix(
            &segments("//Server/Share/Foo").unwrap(),
            &segments("//server/share/foo/file.txt").unwrap()
        ));
    }

    #[test]
    fn grant_authority_prefixes_are_case_sensitive() {
        let grant = exact_segments("C:/Repo/Foo").unwrap();
        assert!(exact_prefix(
            &grant,
            &exact_segments("C:/Repo/Foo/file.txt").unwrap()
        ));
        assert!(!exact_prefix(
            &grant,
            &exact_segments("C:/Repo/foo/file.txt").unwrap()
        ));
        assert!(!exact_prefix(
            &grant,
            &exact_segments("c:/Repo/Foo/file.txt").unwrap()
        ));
    }

    #[test]
    fn exclusion_prefixes_are_case_insensitive_and_directional() {
        let excluded = segments("C:/Repo/Secret").unwrap();
        let child = segments("c:/repo/secret/file.txt").unwrap();
        let sibling = segments("C:/Repo/Other").unwrap();
        assert!(segment_prefix(&excluded, &child));
        assert!(!segment_prefix(&excluded, &sibling));
    }

    #[test]
    fn segment_routing_is_case_insensitive() {
        assert_eq!(fold_segment("Repo").unwrap(), fold_segment("repo").unwrap());
        assert!(segment_prefix(
            &segments("C:/Repo").unwrap(),
            &segments("c:/repo/File.txt").unwrap()
        ));
    }
}
