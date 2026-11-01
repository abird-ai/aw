//! Canonical single-file read/plan/stage/commit operations. Authorization,
//! task journaling, model replies and OS handles belong to the host;
//! every backend call uses the original pinned filesystem capability.
use super::{
    backend::{MutationAccess, MutationExtent, MutationPath, SecuredFilesystem, StagedPublication},
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
    DestinationExists,
    Cancelled,
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
    use std::sync::{Arc, Mutex};
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
            if state.fail_before_commit {
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
