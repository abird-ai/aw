//! Canonical single-file read/plan/stage/commit operations. Authorization,
//! task journaling, model replies and OS handles belong to the host;
//! every backend call uses the original pinned filesystem capability.
use super::{
    backend::{
        CreateFile, ExplicitPathBackend, MutationAccess, MutationExtent, MutationPath,
        SecuredFilesystem, StagedPublication,
    },
    exact_edit::{self, Replacement},
    lifecycle::{self, LifecycleError},
    status::Primary,
    walk::BackendErrorKind,
};

#[derive(Debug)]
pub enum Error<E> {
    InvalidPath(LifecycleError),
    TooLarge,
    InvalidText,
    Edit(exact_edit::EditError),
    BinaryPlan(crate::binary::PlanError),
    BinaryPrecondition,
    BinarySourceChanged,
    Patch(String),
    DestinationExists,
    Cancelled,
    PartialCancelled {
        /// Exact 1-based patch sections already published.
        published: Vec<usize>,
    },
    PatchCommitUnknown {
        /// Exact 1-based patch sections in the publication unit whose commit
        /// outcome is unknown.
        sections: Vec<usize>,
        error: E,
    },
    PartialCommit {
        /// Exact 1-based patch sections already known published.
        published: Vec<usize>,
        /// Exact sections in the unit that failed/has unknown commit outcome.
        failed: Vec<usize>,
        error: E,
        commit_unknown: bool,
    },
    Backend(E),
    /// Commit may have published the bytes before returning an error. Do
    /// not replay automatically, even when the host's error looks like IO.
    UnknownCommit(E),
}
/// Commit error classification is owned by the exact native backend.
fn publish<B: SecuredFilesystem>(backend: &B, stage: B::Stage) -> Result<(), Error<B::Error>> {
    stage
        .commit()
        .map_err(|error| match backend.classify_publication_failure(&error) {
            super::backend::CommitFailure::NoPublication => Error::Backend(error),
            super::backend::CommitFailure::Unknown => Error::UnknownCommit(error),
        })
}
fn admitted<E>(path: &str) -> Result<(), Error<E>> {
    lifecycle::exact_path(path).map_err(Error::InvalidPath)
}

/// Canonical full-byte publication. No equality probe: Write authority
/// alone never reveals whether old bytes matched. The pinned backend keeps
/// existing target access protection during staged publication.
pub fn write_bytes<B: SecuredFilesystem>(
    backend: &B,
    path: &str,
    data: &[u8],
    max_write: usize,
    cancel: &dyn Fn() -> bool,
) -> Result<Primary, Error<B::Error>> {
    admitted(path)?;
    if data.len() > max_write {
        return Err(Error::TooLarge);
    }
    if cancel() {
        return Err(Error::Cancelled);
    }
    backend
        .preflight_mutation(&[MutationPath {
            path,
            access: MutationAccess::Write,
            extent: MutationExtent::Exact,
        }])
        .map_err(Error::Backend)?;
    let _guard = backend
        .lock(&[path.to_owned()], cancel)
        .map_err(Error::Backend)?;
    if cancel() {
        return Err(Error::Cancelled);
    }
    let staged = backend
        .stage_replace(path, data, cancel)
        .map_err(Error::Backend)?;
    if cancel() {
        return Err(Error::Cancelled);
    }
    publish(backend, staged)?;
    Ok(Primary::Ok)
}
pub fn write_text<B: SecuredFilesystem>(
    backend: &B,
    path: &str,
    data: &str,
    max_write: usize,
    cancel: &dyn Fn() -> bool,
) -> Result<Primary, Error<B::Error>> {
    if crate::text::has_binary_control(data.as_bytes()) {
        return Err(Error::InvalidText);
    }
    write_bytes(backend, path, data.as_bytes(), max_write, cancel)
}

/// One immutable original UTF-8 preimage, independent replacement ranges,
/// one atomic publication, and one authoritatively shared root lock.
pub fn edit_text<B: SecuredFilesystem>(
    backend: &B,
    path: &str,
    edits: &[Replacement<'_>],
    max_file: usize,
    cancel: &dyn Fn() -> bool,
) -> Result<Primary, Error<B::Error>> {
    admitted(path)?;
    if cancel() {
        return Err(Error::Cancelled);
    }
    backend
        .preflight_mutation(&[MutationPath {
            path,
            access: MutationAccess::ReadWrite,
            extent: MutationExtent::Exact,
        }])
        .map_err(Error::Backend)?;
    let _guard = backend
        .lock(&[path.to_owned()], cancel)
        .map_err(Error::Backend)?;
    if cancel() {
        return Err(Error::Cancelled);
    }
    let original = backend.read_bytes(path, max_file).map_err(Error::Backend)?;
    let source = crate::text::text_bytes(&original).ok_or(Error::InvalidText)?;
    let replacement = exact_edit::apply(source, edits, max_file).map_err(Error::Edit)?;
    if replacement.as_bytes() == original {
        return Ok(Primary::Unchanged);
    }
    if cancel() {
        return Err(Error::Cancelled);
    }
    let staged = backend
        .stage_update(path, &original, replacement.as_bytes(), cancel)
        .map_err(Error::Backend)?;
    if cancel() {
        return Err(Error::Cancelled);
    }
    publish(backend, staged)?;
    Ok(Primary::Ok)
}

/// Stream one canonical original-relative binary edit under one immutable
/// preimage. The backend revalidates the digest both before staging and at
/// commit; no-op edits re-open and verify the whole preimage before reporting
/// unchanged, so unrelated source changes cannot be hidden by touched ranges.
pub fn edit_binary<B: SecuredFilesystem>(
    backend: &B,
    path: &str,
    edits: Vec<crate::binary::RequestedSplice>,
    expected_sha256: Option<[u8; 32]>,
    max_file: usize,
    cancel: &dyn Fn() -> bool,
) -> Result<Primary, Error<B::Error>> {
    use crate::binary::{
        digest_limited, plan_splices, splices_are_unchanged, stream_splice_with_stop,
        verify_preimage,
    };
    use std::io::{Seek as _, SeekFrom};

    admitted(path)?;
    if cancel() {
        return Err(Error::Cancelled);
    }
    backend
        .preflight_mutation(&[MutationPath {
            path,
            access: MutationAccess::ReadWrite,
            extent: MutationExtent::Exact,
        }])
        .map_err(Error::Backend)?;
    let _guard = backend
        .lock(&[path.to_owned()], cancel)
        .map_err(Error::Backend)?;
    if cancel() {
        return Err(Error::Cancelled);
    }

    let mut source = backend
        .open_regular_for_update(path)
        .map_err(Error::Backend)?;
    if source
        .metadata()
        .map_err(|_| Error::BinarySourceChanged)?
        .len()
        > max_file as u64
    {
        return Err(Error::TooLarge);
    }
    let (size, digest) =
        digest_limited(&mut source, max_file).map_err(|_| Error::BinarySourceChanged)?;
    if expected_sha256.is_some_and(|expected| expected != digest) {
        return Err(Error::BinaryPrecondition);
    }
    let plan = plan_splices(edits, size, max_file).map_err(Error::BinaryPlan)?;
    let splices = plan
        .edits
        .iter()
        .map(crate::binary::OwnedByteSplice::borrowed)
        .collect::<Vec<_>>();
    let unchanged = size == plan.output_len
        && splices_are_unchanged(&mut source, &splices).map_err(|_| Error::BinarySourceChanged)?;
    if unchanged {
        if cancel() {
            return Err(Error::Cancelled);
        }
        let mut current = backend
            .open_regular_for_update(path)
            .map_err(Error::Backend)?;
        verify_preimage(&mut current, (size, digest), max_file)
            .map_err(|_| Error::BinarySourceChanged)?;
        return Ok(Primary::Unchanged);
    }
    if cancel() {
        return Err(Error::Cancelled);
    }

    let mut writer = |output: &mut std::fs::File| {
        source.seek(SeekFrom::Start(0))?;
        stream_splice_with_stop(&mut source, output, (size, digest), &splices, cancel)
            .map(|_| ())
            .map_err(|_| {
                if cancel() {
                    super::backend::StreamWriteError::Cancelled
                } else {
                    super::backend::StreamWriteError::Refused(
                        "binary source changed during staged edit",
                    )
                }
            })
    };
    let staged = backend
        .stage_stream_update(path, (size as u64, digest), &mut writer, cancel)
        .map_err(Error::Backend)?;
    if cancel() {
        return Err(Error::Cancelled);
    }
    publish(backend, staged)?;
    Ok(Primary::Ok)
}

/// Execute the canonical strict Codex Update/Add patch as one fully planned
/// mutation. Compatibility-only directory prefixes/best-effort behavior belong
/// in host adapters; this path is the portable semantics shared by AW/dotlink.
pub fn patch_text<B: SecuredFilesystem + ExplicitPathBackend>(
    backend: &B,
    patch: &str,
    max_file: usize,
    cancel: &dyn Fn() -> bool,
) -> Result<Primary, Error<B::Error>> {
    use crate::{
        mutation::PlanBudget,
        patch::{apply_patch_hunks, parse_patch, PatchOp},
    };
    use std::collections::BTreeSet;

    #[derive(Debug)]
    struct Plan {
        section: usize,
        path: String,
        expected: Option<Vec<u8>>,
        replacement: Vec<u8>,
    }

    let ops = parse_patch(patch).map_err(|error| Error::Patch(error.to_string()))?;
    if cancel() {
        return Err(Error::Cancelled);
    }
    let total_hunks = ops
        .iter()
        .map(|op| match op {
            PatchOp::Update { hunks, .. } => hunks.len(),
            PatchOp::Add { .. } => 0,
        })
        .sum::<usize>();

    // Resolve every final content symlink and prove complete authority before
    // any mutation lock or private staging object exists.
    let mut next_hunk = 0usize;
    let mut prepared = Vec::with_capacity(ops.len());
    let mut claims = BTreeSet::<Vec<u8>>::new();
    for (section, op) in ops.into_iter().enumerate() {
        let (lexical, access, first_hunk) = match &op {
            PatchOp::Update { path, hunks } => {
                let first = next_hunk;
                next_hunk += hunks.len();
                (path.as_str(), MutationAccess::ReadWrite, first)
            }
            PatchOp::Add { path, .. } => (path.as_str(), MutationAccess::Write, next_hunk),
        };
        admitted::<B::Error>(lexical)?;
        let effective = match &op {
            PatchOp::Update { .. } => backend
                .resolve_content_path(lexical, false)
                .map_err(Error::Backend)?,
            PatchOp::Add { .. } => lexical.to_owned(),
        };
        admitted::<B::Error>(&effective)?;
        let claim = backend
            .mutation_path_key(&effective)
            .map_err(Error::Backend)?;
        if !claims.insert(claim) {
            return Err(Error::Patch(format!(
                "patch path appears in more than one section: {lexical}"
            )));
        }
        prepared.push((section, op, effective, first_hunk, access));
    }
    let preflight = prepared
        .iter()
        .map(|(_, _, path, _, access)| MutationPath {
            path: path.as_str(),
            access: *access,
            extent: MutationExtent::Exact,
        })
        .collect::<Vec<_>>();
    backend
        .preflight_mutation(&preflight)
        .map_err(Error::Backend)?;
    let paths = prepared
        .iter()
        .map(|(_, _, path, _, _)| path.clone())
        .collect::<Vec<_>>();
    let _guard = backend.lock(&paths, cancel).map_err(Error::Backend)?;
    if cancel() {
        return Err(Error::Cancelled);
    }

    // Build every replacement from immutable preimages before reserving any
    // private publication object. PlanBudget bounds aggregate original+working
    // memory independently of the per-file ceiling.
    let mut work = PlanBudget::default();
    let mut plans = Vec::<Plan>::with_capacity(prepared.len());
    for (section, op, path, first_hunk, _) in prepared {
        if cancel() {
            return Err(Error::Cancelled);
        }
        match op {
            PatchOp::Update { hunks, .. } => {
                let cap = work
                    .copy_read_limit(max_file)
                    .map_err(|error| Error::Patch(error.to_string()))?;
                let original = backend.read_bytes(&path, cap).map_err(Error::Backend)?;
                let text = crate::text::text_bytes(&original).ok_or_else(|| {
                    Error::Patch(format!("patch: {path}: update target is binary; use bedit"))
                })?;
                let (replacement, _, _) =
                    apply_patch_hunks(text, &hunks, &path, first_hunk, total_hunks)
                        .map_err(|error| Error::Patch(error.to_string()))?;
                if replacement.len() > max_file {
                    return Err(Error::TooLarge);
                }
                if crate::text::has_binary_control(replacement.as_bytes()) {
                    return Err(Error::InvalidText);
                }
                work.add(original.len(), replacement.len())
                    .map_err(|error| Error::Patch(error.to_string()))?;
                plans.push(Plan {
                    section,
                    path,
                    expected: Some(original),
                    replacement: replacement.into_bytes(),
                });
            }
            PatchOp::Add { lines, .. } => {
                let replacement = crate::patch::add_file_bytes(&lines);
                if replacement.len() > max_file {
                    return Err(Error::TooLarge);
                }
                if crate::text::has_binary_control(&replacement) {
                    return Err(Error::InvalidText);
                }
                work.add(0, replacement.len())
                    .map_err(|error| Error::Patch(error.to_string()))?;
                plans.push(Plan {
                    section,
                    path,
                    expected: None,
                    replacement,
                });
            }
        }
    }

    // Stage every publication before the first visible effect. Updates retain
    // one original snapshot; additions are batch-staged so files under one new
    // publication root become visible together by one rename.
    struct Publication<S> {
        first: usize,
        sections: Vec<usize>,
        stage: S,
    }
    let mut staged = Vec::<Publication<B::Stage>>::new();
    let mut adds = Vec::<(usize, &str, &[u8])>::new();
    for plan in &plans {
        match &plan.expected {
            Some(original) if original.as_slice() == plan.replacement.as_slice() => {}
            Some(original) => staged.push(Publication {
                first: plan.section + 1,
                sections: vec![plan.section + 1],
                stage: backend
                    .stage_update(&plan.path, original, &plan.replacement, cancel)
                    .map_err(Error::Backend)?,
            }),
            None => adds.push((
                plan.section,
                plan.path.as_str(),
                plan.replacement.as_slice(),
            )),
        }
    }
    if !adds.is_empty() {
        let create = adds
            .iter()
            .map(|(_, path, bytes)| CreateFile { path, bytes })
            .collect::<Vec<_>>();
        for unit in backend
            .stage_create_many(&create, cancel)
            .map_err(Error::Backend)?
        {
            let first = adds
                .get(unit.first)
                .ok_or_else(|| Error::Patch("invalid staged add publication index".into()))?
                .0
                + 1;
            let sections = unit
                .inputs
                .iter()
                .map(|input| {
                    adds.get(*input)
                        .map(|entry| entry.0 + 1)
                        .ok_or_else(|| Error::Patch("invalid staged add publication index".into()))
                })
                .collect::<Result<Vec<_>, Error<B::Error>>>()?;
            staged.push(Publication {
                first,
                sections,
                stage: unit.stage,
            });
        }
    }
    staged.sort_by_key(|publication| publication.first);
    if staged.is_empty() {
        return Ok(Primary::Unchanged);
    }
    if cancel() {
        return Err(Error::Cancelled);
    }

    let mut published = Vec::<usize>::new();
    for publication in staged {
        if cancel() {
            return if published.is_empty() {
                Err(Error::Cancelled)
            } else {
                Err(Error::PartialCancelled { published })
            };
        }
        match publication.stage.commit() {
            Ok(()) => published.extend(publication.sections),
            Err(error) if published.is_empty() => {
                return Err(match backend.classify_publication_failure(&error) {
                    super::backend::CommitFailure::NoPublication => Error::Backend(error),
                    super::backend::CommitFailure::Unknown => Error::PatchCommitUnknown {
                        sections: publication.sections,
                        error,
                    },
                });
            }
            Err(error) => {
                let commit_unknown = matches!(
                    backend.classify_publication_failure(&error),
                    super::backend::CommitFailure::Unknown
                );
                return Err(Error::PartialCommit {
                    published,
                    failed: publication.sections,
                    error,
                    commit_unknown,
                });
            }
        }
    }
    Ok(Primary::Ok)
}

/// Exact scoped copy. No ambient paths or implicit destination basename;
/// the backend stages the full payload and publishes only by no-replace.
pub fn copy_path<B: SecuredFilesystem>(
    backend: &B,
    source: &str,
    destination: &str,
    recursive: bool,
    cancel: &dyn Fn() -> bool,
) -> Result<Primary, Error<B::Error>> {
    lifecycle::pair(source, destination, true).map_err(Error::InvalidPath)?;
    if cancel() {
        return Err(Error::Cancelled);
    }
    let extent = if recursive {
        MutationExtent::Subtree
    } else {
        MutationExtent::Exact
    };
    backend
        .preflight_mutation(&[
            MutationPath {
                path: source,
                access: MutationAccess::Read,
                extent,
            },
            MutationPath {
                path: destination,
                access: MutationAccess::Write,
                extent,
            },
        ])
        .map_err(Error::Backend)?;
    let _guard = backend
        .lock(&[source.to_owned(), destination.to_owned()], cancel)
        .map_err(Error::Backend)?;
    if cancel() {
        return Err(Error::Cancelled);
    }
    let source_meta = backend.entry_meta(source).map_err(Error::Backend)?;
    if !recursive && source_meta.kind == super::Kind::Dir {
        return Err(Error::InvalidPath(LifecycleError::InvalidPath));
    }
    match backend.mutation_meta(destination) {
        Ok(_) => return Err(Error::DestinationExists),
        Err(error) => {
            if backend.classify(&error) != BackendErrorKind::Missing {
                return Err(Error::Backend(error));
            }
        }
    }
    if cancel() {
        return Err(Error::Cancelled);
    }
    // The generic backend does not yet expose a "published" bit on
    // failed staged copies; treat any ambiguous failure as unknown until the
    // original host can prove its namespace publication state.
    backend
        .copy_exact(source, destination, cancel)
        .map_err(|error| match backend.classify_publication_failure(&error) {
            super::backend::CommitFailure::NoPublication => Error::Backend(error),
            super::backend::CommitFailure::Unknown => Error::UnknownCommit(error),
        })?;
    Ok(Primary::Ok)
}
/// Exact existing-parent native rename; no copy/delete or mkdir fallback.
pub fn move_path<B: SecuredFilesystem>(
    backend: &B,
    source: &str,
    destination: &str,
    cancel: &dyn Fn() -> bool,
) -> Result<Primary, Error<B::Error>> {
    admitted(source)?;
    admitted(destination)?;
    if source != destination {
        lifecycle::pair(source, destination, true).map_err(Error::InvalidPath)?;
    }
    if cancel() {
        return Err(Error::Cancelled);
    }
    let mut preflight = vec![MutationPath {
        path: source,
        access: MutationAccess::Write,
        extent: MutationExtent::Subtree,
    }];
    if source != destination {
        preflight.push(MutationPath {
            path: destination,
            access: MutationAccess::Write,
            extent: MutationExtent::Subtree,
        });
    }
    backend
        .preflight_mutation(&preflight)
        .map_err(Error::Backend)?;
    let mut paths = vec![source.to_owned()];
    if source != destination {
        paths.push(destination.to_owned());
    }
    let _guard = backend.lock(&paths, cancel).map_err(Error::Backend)?;
    backend.mutation_meta(source).map_err(Error::Backend)?;
    if source == destination {
        return Ok(Primary::Unchanged);
    }
    match backend.mutation_meta(destination) {
        Ok(_) => return Err(Error::DestinationExists),
        Err(error) => {
            if backend.classify(&error) != BackendErrorKind::Missing {
                return Err(Error::Backend(error));
            }
        }
    }
    if cancel() {
        return Err(Error::Cancelled);
    }
    // The backend's same-volume rename is a single atomic no-replace
    // syscall: errors report that no public namespace mutation occurred.
    backend
        .move_exact(source, destination, cancel)
        .map_err(Error::Backend)?;
    Ok(Primary::Ok)
}
/// Multi-root preflight and cross-process mutation lock are shared by all
/// consumers. Backends report exact known partial or cleanup outcomes.
pub fn remove_paths<B: SecuredFilesystem>(
    backend: &B,
    paths: &[String],
    recursive: bool,
    cancel: &dyn Fn() -> bool,
) -> Result<lifecycle::RemoveOutcome, Error<B::Error>> {
    lifecycle::removals(paths.iter().map(String::as_str)).map_err(Error::InvalidPath)?;
    if cancel() {
        return Err(Error::Cancelled);
    }
    let extent = if recursive {
        MutationExtent::Subtree
    } else {
        MutationExtent::Exact
    };
    let preflight = paths
        .iter()
        .map(|path| MutationPath {
            path,
            access: MutationAccess::Write,
            extent,
        })
        .collect::<Vec<_>>();
    backend
        .preflight_mutation(&preflight)
        .map_err(Error::Backend)?;
    let _guard = backend.lock(paths, cancel).map_err(Error::Backend)?;
    if cancel() {
        return Err(Error::Cancelled);
    }
    backend
        .remove_exact(paths, recursive, cancel)
        .map_err(Error::Backend)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        filesystem::{
            backend::{MetadataBackend, StagedPublication},
            walk::{Backend as WalkBackend, BackendErrorKind, Listing},
            EntryMeta,
        },
        read::{Page, PageRequest},
    };
    use std::{
        io::{Read as _, Seek as _, SeekFrom, Write as _},
        sync::{
            atomic::{AtomicU64, Ordering},
            Arc, Mutex,
        },
    };
    static NEXT_STREAM_FILE: AtomicU64 = AtomicU64::new(1);
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Fault {
        NotFound,
        Denied,
        AfterCommit,
    }
    #[derive(Default)]
    struct State {
        bytes: Option<Vec<u8>>,
        commits: usize,
        stage_calls: usize,
        read_calls: usize,
        fail_after_commit: bool,
        fail_before_commit: bool,
        fail_commit_attempt: Option<usize>,
        commit_attempts: usize,
        deny_preflight: bool,
        preflight_calls: usize,
        lock_calls: usize,
    }
    struct Memory(Arc<Mutex<State>>);
    struct Stage {
        state: Arc<Mutex<State>>,
        new: Vec<u8>,
    }
    impl StagedPublication for Stage {
        type Error = Fault;
        fn commit(self) -> Result<(), Fault> {
            let mut state = self.state.lock().unwrap();
            state.commit_attempts += 1;
            if state.fail_before_commit || state.fail_commit_attempt == Some(state.commit_attempts)
            {
                return Err(Fault::Denied);
            }
            state.bytes = Some(self.new);
            state.commits += 1;
            if state.fail_after_commit {
                Err(Fault::AfterCommit)
            } else {
                Ok(())
            }
        }
    }
    impl WalkBackend for Memory {
        type Error = Fault;
        fn list(&self, _: &str, _: usize) -> Result<Listing, Fault> {
            Err(Fault::Denied)
        }
        fn read_bytes(&self, _: &str, _: usize) -> Result<Vec<u8>, Fault> {
            let mut state = self.0.lock().unwrap();
            state.read_calls += 1;
            state.bytes.clone().ok_or(Fault::NotFound)
        }
        fn classify(&self, error: &Fault) -> BackendErrorKind {
            if *error == Fault::NotFound {
                BackendErrorKind::Missing
            } else {
                BackendErrorKind::Other
            }
        }
    }
    impl MetadataBackend for Memory {
        fn entry_meta(&self, path: &str) -> Result<EntryMeta, Fault> {
            let data = self.0.lock().unwrap();
            if path == "f.txt" {
                if let Some(ref bytes) = data.bytes {
                    return Ok(EntryMeta {
                        kind: super::super::Kind::File,
                        size: Some(bytes.len() as u64),
                        ..Default::default()
                    });
                }
            }
            Err(Fault::NotFound)
        }
    }
    impl super::super::backend::ExplicitPathBackend for Memory {
        fn resolve_content_path(&self, path: &str, _: bool) -> Result<String, Fault> {
            Ok(path.to_owned())
        }
        fn resolve_directory_path(&self, path: &str) -> Result<String, Fault> {
            Ok(path.to_owned())
        }
    }
    impl SecuredFilesystem for Memory {
        type Guard<'a> = ();
        type Stage = Stage;
        fn preflight_mutation(
            &self,
            _: &[super::super::backend::MutationPath<'_>],
        ) -> Result<(), Fault> {
            let mut state = self.0.lock().unwrap();
            state.preflight_calls += 1;
            if state.deny_preflight {
                Err(Fault::Denied)
            } else {
                Ok(())
            }
        }
        fn lock<'a>(
            &'a self,
            _: &[String],
            _: &dyn Fn() -> bool,
        ) -> Result<Self::Guard<'a>, Fault> {
            self.0.lock().unwrap().lock_calls += 1;
            Ok(())
        }
        fn stage_create(&self, _: &str, data: &[u8], _: &dyn Fn() -> bool) -> Result<Stage, Fault> {
            let mut state = self.0.lock().unwrap();
            state.stage_calls += 1;
            Ok(Stage {
                state: self.0.clone(),
                new: data.to_vec(),
            })
        }
        fn stage_create_many(
            &self,
            files: &[super::super::backend::CreateFile<'_>],
            cancel: &dyn Fn() -> bool,
        ) -> Result<Vec<super::super::backend::CreateStage<Stage>>, Fault> {
            files
                .iter()
                .enumerate()
                .map(|(index, file)| {
                    self.stage_create(file.path, file.bytes, cancel)
                        .map(|stage| super::super::backend::CreateStage {
                            first: index,
                            inputs: vec![index],
                            stage,
                        })
                })
                .collect()
        }
        fn stage_replace(
            &self,
            _: &str,
            data: &[u8],
            _: &dyn Fn() -> bool,
        ) -> Result<Stage, Fault> {
            self.stage_create("", data, &|| false)
        }
        fn stage_update(
            &self,
            _: &str,
            expected: &[u8],
            data: &[u8],
            _: &dyn Fn() -> bool,
        ) -> Result<Stage, Fault> {
            if self.0.lock().unwrap().bytes.as_deref() != Some(expected) {
                return Err(Fault::Denied);
            }
            self.stage_create("", data, &|| false)
        }
        fn open_regular_for_update(&self, _: &str) -> Result<std::fs::File, Fault> {
            let bytes = self
                .0
                .lock()
                .unwrap()
                .bytes
                .clone()
                .ok_or(Fault::NotFound)?;
            let path = std::env::temp_dir().join(format!(
                "aw-tools-stream-test-{}-{}",
                std::process::id(),
                NEXT_STREAM_FILE.fetch_add(1, Ordering::Relaxed)
            ));
            let mut file = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .create_new(true)
                .open(&path)
                .map_err(|_| Fault::Denied)?;
            file.write_all(&bytes).map_err(|_| Fault::Denied)?;
            file.seek(SeekFrom::Start(0)).map_err(|_| Fault::Denied)?;
            let _ = std::fs::remove_file(path);
            Ok(file)
        }
        fn stage_stream_update(
            &self,
            _: &str,
            expected: (u64, [u8; 32]),
            writer: &mut dyn FnMut(
                &mut std::fs::File,
            )
                -> Result<(), super::super::backend::StreamWriteError>,
            _: &dyn Fn() -> bool,
        ) -> Result<Stage, Fault> {
            use sha2::{Digest as _, Sha256};
            let current = self
                .0
                .lock()
                .unwrap()
                .bytes
                .clone()
                .ok_or(Fault::NotFound)?;
            if (
                current.len() as u64,
                <[u8; 32]>::from(Sha256::digest(&current)),
            ) != expected
            {
                return Err(Fault::Denied);
            }
            let path = std::env::temp_dir().join(format!(
                "aw-tools-stream-stage-test-{}-{}",
                std::process::id(),
                NEXT_STREAM_FILE.fetch_add(1, Ordering::Relaxed)
            ));
            let mut file = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .create_new(true)
                .open(&path)
                .map_err(|_| Fault::Denied)?;
            writer(&mut file).map_err(|_| Fault::Denied)?;
            file.seek(SeekFrom::Start(0)).map_err(|_| Fault::Denied)?;
            let mut new = Vec::new();
            file.read_to_end(&mut new).map_err(|_| Fault::Denied)?;
            drop(file);
            let _ = std::fs::remove_file(path);
            self.0.lock().unwrap().stage_calls += 1;
            Ok(Stage {
                state: self.0.clone(),
                new,
            })
        }
        fn classify_publication_failure(
            &self,
            error: &Fault,
        ) -> super::super::backend::CommitFailure {
            use super::super::backend::CommitFailure;
            if *error == Fault::AfterCommit {
                CommitFailure::Unknown
            } else {
                CommitFailure::NoPublication
            }
        }
        fn copy_exact(&self, _: &str, _: &str, _: &dyn Fn() -> bool) -> Result<(), Fault> {
            Err(Fault::Denied)
        }
        fn move_exact(&self, _: &str, _: &str, _: &dyn Fn() -> bool) -> Result<(), Fault> {
            Err(Fault::Denied)
        }
        fn remove_exact(
            &self,
            _: &[String],
            _: bool,
            _: &dyn Fn() -> bool,
        ) -> Result<super::super::lifecycle::RemoveOutcome, Fault> {
            Err(Fault::Denied)
        }
        fn read_page(&self, _: &str, _: PageRequest, _: &dyn Fn() -> bool) -> Result<Page, Fault> {
            Err(Fault::Denied)
        }
        fn read_binary_window(
            &self,
            _: &str,
            _: u64,
            _: usize,
            _: &dyn Fn() -> bool,
        ) -> Result<(Vec<u8>, u64, bool), Fault> {
            Err(Fault::Denied)
        }
    }
    #[test]
    fn mutation_authority_preflight_runs_before_lock_admission() {
        let shared = Arc::new(Mutex::new(State {
            deny_preflight: true,
            ..Default::default()
        }));
        let fs = Memory(shared.clone());
        assert!(matches!(
            write_text(&fs, "f.txt", "new", 1024, &|| false),
            Err(Error::Backend(Fault::Denied))
        ));
        let state = shared.lock().unwrap();
        assert_eq!(state.preflight_calls, 1);
        assert_eq!(state.lock_calls, 0);
        assert_eq!(state.commits, 0);
    }

    #[test]
    fn write_publishes_but_unchanged_edit_does_not() {
        let shared = Arc::new(Mutex::new(State {
            bytes: Some(b"hello world".to_vec()),
            ..Default::default()
        }));
        let fs = Memory(shared.clone());
        assert!(matches!(
            write_text(&fs, "f.txt", "hello world", 1024, &|| false),
            Ok(Primary::Ok)
        ));
        assert!(matches!(
            edit_text(
                &fs,
                "f.txt",
                &[Replacement {
                    old: "world",
                    new: "world",
                    all: false
                }],
                1024,
                &|| false
            ),
            Ok(Primary::Unchanged)
        ));
        let state = shared.lock().unwrap();
        assert_eq!(state.stage_calls, 1);
        assert_eq!(state.commits, 1);
        assert_eq!(
            state.read_calls, 1,
            "only edit may read its original snapshot"
        );
    }
    #[test]
    fn canonical_text_and_binary_writes_never_read_existing_contents() {
        let state = Arc::new(Mutex::new(State {
            bytes: Some(b"private old content".to_vec()),
            ..Default::default()
        }));
        let fs = Memory(state.clone());
        assert!(matches!(
            write_text(&fs, "f.txt", "a", 1024, &|| false),
            Ok(Primary::Ok)
        ));
        assert!(matches!(
            write_bytes(&fs, "f.txt", &[0, 255], 1024, &|| false),
            Ok(Primary::Ok)
        ));
        let result = state.lock().unwrap();
        assert_eq!(
            result.read_calls, 0,
            "write-only operations must not be equality/preimage oracles"
        );
        assert_eq!(result.commits, 2);
    }
    #[test]
    fn known_prepublication_commit_rejection_is_safe_to_report_as_fail() {
        let state = Arc::new(Mutex::new(State {
            fail_before_commit: true,
            ..Default::default()
        }));
        let fs = Memory(state.clone());
        let result = write_text(&fs, "example.txt", "data", 4096, &|| false);
        assert!(matches!(result, Err(Error::Backend(Fault::Denied))));
        assert!(state.lock().unwrap().bytes.is_none());
        let old = Arc::new(Mutex::new(State {
            bytes: Some(b"hello".to_vec()),
            fail_before_commit: true,
            ..Default::default()
        }));
        let fs = Memory(old.clone());
        let result = edit_text(
            &fs,
            "f.txt",
            &[Replacement {
                old: "hello",
                new: "bye",
                all: false,
            }],
            4096,
            &|| false,
        );
        assert!(matches!(result, Err(Error::Backend(Fault::Denied))));
        assert_eq!(
            old.lock().unwrap().bytes.as_deref(),
            Some(b"hello".as_slice())
        );
    }
    #[test]
    fn post_publication_commit_error_is_never_a_safe_failure() {
        let shared = Arc::new(Mutex::new(State {
            fail_after_commit: true,
            ..Default::default()
        }));
        let fs = Memory(shared.clone());
        let result = write_text(&fs, "nested/file.txt", "published", 1024, &|| false);
        assert!(matches!(
            result,
            Err(Error::UnknownCommit(Fault::AfterCommit))
        ));
        let state = shared.lock().unwrap();
        assert_eq!(state.bytes.as_deref(), Some(b"published".as_slice()));
        assert_eq!(state.commits, 1);
    }
    #[test]
    fn text_write_rejects_binary_control_before_any_staging() {
        let state = Arc::new(Mutex::new(State::default()));
        let fs = Memory(state.clone());
        for text in ["before\0after", "escape\u{1b}", "control\u{7f}"] {
            assert!(matches!(
                write_text(&fs, "safe.txt", text, 1024, &|| false),
                Err(Error::InvalidText)
            ));
        }
        let state = state.lock().unwrap();
        assert_eq!(state.stage_calls, 0);
        assert_eq!(state.commits, 0);
    }
    #[test]
    fn all_edit_errors_precede_mutation_and_invalid_bytes_fail_closed() {
        let shared = Arc::new(Mutex::new(State {
            bytes: Some(b"abc".to_vec()),
            ..Default::default()
        }));
        let fs = Memory(shared.clone());
        assert!(matches!(
            edit_text(
                &fs,
                "f.txt",
                &[Replacement {
                    old: "not found",
                    new: "x",
                    all: false
                }],
                1024,
                &|| false
            ),
            Err(Error::Edit(exact_edit::EditError::Missing(1)))
        ));
        shared.lock().unwrap().bytes = Some(b"ab\0c".to_vec());
        assert!(matches!(
            edit_text(
                &fs,
                "f.txt",
                &[Replacement {
                    old: "a",
                    new: "z",
                    all: false
                }],
                1024,
                &|| false
            ),
            Err(Error::InvalidText)
        ));
        assert_eq!(shared.lock().unwrap().commits, 0);
        assert!(matches!(
            write_text(&fs, "../escape", "oops", 1024, &|| false),
            Err(Error::InvalidPath(_))
        ));
    }
    #[test]
    fn streamed_binary_edit_changes_bytes_and_noop_reverifies_preimage() {
        use crate::binary::RequestedSplice;
        let shared = Arc::new(Mutex::new(State {
            bytes: Some(b"abcdef".to_vec()),
            ..Default::default()
        }));
        let fs = Memory(shared.clone());
        assert!(matches!(
            edit_binary(
                &fs,
                "f.txt",
                vec![RequestedSplice {
                    offset: 2,
                    length: Some(2),
                    data: b"XY".to_vec(),
                }],
                None,
                1024,
                &|| false,
            ),
            Ok(Primary::Ok)
        ));
        assert_eq!(
            shared.lock().unwrap().bytes.as_deref(),
            Some(b"abXYef".as_slice())
        );
        assert_eq!(shared.lock().unwrap().commits, 1);

        assert!(matches!(
            edit_binary(
                &fs,
                "f.txt",
                vec![RequestedSplice {
                    offset: 2,
                    length: Some(2),
                    data: b"XY".to_vec(),
                }],
                None,
                1024,
                &|| false,
            ),
            Ok(Primary::Unchanged)
        ));
        assert_eq!(shared.lock().unwrap().commits, 1);
    }

    #[test]
    fn binary_sha_precondition_and_plan_fail_before_staging() {
        use crate::binary::RequestedSplice;
        let shared = Arc::new(Mutex::new(State {
            bytes: Some(b"abcdef".to_vec()),
            ..Default::default()
        }));
        let fs = Memory(shared.clone());
        assert!(matches!(
            edit_binary(
                &fs,
                "f.txt",
                vec![RequestedSplice {
                    offset: 1,
                    length: Some(1),
                    data: b"x".to_vec(),
                }],
                Some([7; 32]),
                1024,
                &|| false,
            ),
            Err(Error::BinaryPrecondition)
        ));
        assert!(matches!(
            edit_binary(
                &fs,
                "f.txt",
                vec![RequestedSplice {
                    offset: 99,
                    length: Some(1),
                    data: b"x".to_vec(),
                }],
                None,
                1024,
                &|| false,
            ),
            Err(Error::BinaryPlan(crate::binary::PlanError::PastEof))
        ));
        let state = shared.lock().unwrap();
        assert_eq!(state.stage_calls, 0);
        assert_eq!(state.commits, 0);
    }

    #[test]
    fn strict_patch_later_publication_failure_is_explicit_partial_commit() {
        let shared = Arc::new(Mutex::new(State {
            fail_commit_attempt: Some(2),
            ..Default::default()
        }));
        let fs = Memory(shared.clone());
        let result = patch_text(
            &fs,
            "*** Begin Patch\n*** Add File: a.txt\n+a\n*** Add File: b.txt\n+b\n*** End Patch\n",
            1024,
            &|| false,
        );
        assert!(matches!(
            result,
            Err(Error::PartialCommit {
                published,
                failed,
                error: Fault::Denied,
                commit_unknown: false,
            }) if published == vec![1] && failed == vec![2]
        ));
        let state = shared.lock().unwrap();
        assert_eq!(state.commit_attempts, 2);
        assert_eq!(state.commits, 1);
    }

    #[test]
    fn shared_lifecycle_admission_is_exact_and_single_owner() {
        let shared = Arc::new(Mutex::new(State {
            bytes: Some(b"safe".to_vec()),
            ..Default::default()
        }));
        let fs = Memory(shared.clone());
        assert!(matches!(
            copy_path(&fs, "f.txt", "f.txt", true, &|| false),
            Err(Error::InvalidPath(_))
        ));
        assert!(matches!(
            copy_path(&fs, "missing", "other", true, &|| false),
            Err(Error::Backend(Fault::NotFound))
        ));
        assert!(matches!(
            copy_path(&fs, "f.txt", "copy/f.txt", true, &|| false),
            Err(Error::Backend(Fault::Denied))
        ));
        assert!(matches!(
            move_path(&fs, "f.txt", "f.txt", &|| false),
            Ok(Primary::Unchanged)
        ));
        assert!(matches!(
            move_path(&fs, "f.txt", "other", &|| false),
            Err(Error::Backend(Fault::Denied))
        ));
        let duplicate = ["f.txt".to_string(), "f.txt".to_string()];
        assert!(matches!(
            remove_paths(&fs, &duplicate, true, &|| false),
            Err(Error::InvalidPath(_))
        ));
        let known = ["f.txt".to_string()];
        assert!(matches!(
            remove_paths(&fs, &known, true, &|| false),
            Err(Error::Backend(Fault::Denied))
        ));
        assert_eq!(shared.lock().unwrap().commits, 0);
        assert_eq!(shared.lock().unwrap().stage_calls, 0);
    }
    #[test]
    fn cancelled_operation_never_reserves_a_new_stage() {
        let shared = Arc::new(Mutex::new(State::default()));
        let fs = Memory(shared.clone());
        assert!(matches!(
            write_text(&fs, "f", "data", 1024, &|| true),
            Err(Error::Cancelled)
        ));
        assert_eq!(shared.lock().unwrap().commits, 0);
    }
}
