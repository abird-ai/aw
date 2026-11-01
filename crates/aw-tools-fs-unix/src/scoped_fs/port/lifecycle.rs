use super::*;

// AI-native exact-path lifecycle operations. Callers hold the existing
// MutationGuard for source/destination (or every removal root).
fn lifecycle_error(error: aw_tools::filesystem::lifecycle::LifecycleError) -> ScopedError {
    use aw_tools::filesystem::lifecycle::LifecycleError as E;
    match error {
        E::TooManyRoots | E::TooManyEntries | E::TooDeep | E::TooManyBytes => {
            ScopedError::Limit("filesystem lifecycle work budget exhausted")
        }
        _ => ScopedError::Denied("invalid or overlapping lifecycle paths"),
    }
}
fn lstat_at(parent: &File, name: &str) -> Result<libc::stat> {
    let name = cstring(name)?;
    let mut info = std::mem::MaybeUninit::<libc::stat>::uninit();
    // SAFETY: pinned directory and clean single-component name.
    if unsafe {
        libc::fstatat(
            parent.as_raw_fd(),
            name.as_ptr(),
            info.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    } != 0
    {
        return Err(io::Error::last_os_error().into());
    }
    Ok(unsafe { info.assume_init() })
}
pub(super) fn rename_exact(
    parent_from: &File,
    from: &str,
    parent_to: &File,
    to: &str,
) -> Result<()> {
    #[cfg(target_os = "linux")]
    {
        let source = cstring(from)?;
        let target = cstring(to)?;
        // SAFETY: both open parent FDs and valid leaf names. Linux atomic
        // no-replace publication is mandatory; never copy+delete on EXDEV.
        let status = unsafe {
            libc::syscall(
                libc::SYS_renameat2,
                parent_from.as_raw_fd(),
                source.as_ptr(),
                parent_to.as_raw_fd(),
                target.as_ptr(),
                libc::RENAME_NOREPLACE,
            )
        };
        if status != 0 {
            let error = io::Error::last_os_error();
            if error.raw_os_error() == Some(libc::EXDEV) {
                return Err(ScopedError::Denied("cross-device move; use cp then rm"));
            }
            return Err(error.into());
        }
        Ok(())
    }
    #[cfg(target_os = "macos")]
    {
        const RENAME_EXCL: libc::c_uint = 0x0000_0004;
        let from = cstring(from)?;
        let to = cstring(to)?;
        // SAFETY: pinned parent FDs and clean, NUL-terminated leaf names.
        // Darwin renameatx_np provides atomic no-replace on supporting FS.
        let status = unsafe {
            libc::renameatx_np(
                parent_from.as_raw_fd(),
                from.as_ptr(),
                parent_to.as_raw_fd(),
                to.as_ptr(),
                RENAME_EXCL,
            )
        };
        if status != 0 {
            let error = io::Error::last_os_error();
            if error.raw_os_error() == Some(libc::EXDEV) {
                return Err(ScopedError::Denied("cross-device move; use cp then rm"));
            }
            return Err(error.into());
        }
        Ok(())
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = (parent_from, from, parent_to, to);
        Err(ScopedError::Denied(
            "atomic no-replace rename unavailable on this Unix target",
        ))
    }
}
/// Bounded cleanup of an unpublished private staged suffix. The temporary
/// Root uses the already pinned parent FD, never an ambient path or symlink.
/// Only an empty freshly staged directory may be removed without a
/// verified inode identity; never recurse through an unverified name.
pub(super) fn remove_empty_stage(parent: &File, temp: &str) {
    if let Ok(name) = cstring(temp) {
        // SAFETY: pinned parent; unlinkat with AT_REMOVEDIR cannot traverse.
        let _ = unsafe { libc::unlinkat(parent.as_raw_fd(), name.as_ptr(), libc::AT_REMOVEDIR) };
    }
}
pub(super) fn cleanup_stage_tree(parent: &File, temp: &str, expected: Option<(u64, u64)>) {
    if let Some((dev, ino)) = expected {
        let Ok(opened) = at(parent, temp, libc::O_RDONLY | libc::O_DIRECTORY, 0) else {
            return;
        };
        let Ok(meta) = opened.metadata() else {
            return;
        };
        if meta.dev() != dev || meta.ino() != ino {
            return;
        }
        drop(opened);
    }
    let Ok(directory) = parent.try_clone() else {
        return;
    };
    let private = Root {
        directory,
        serial: std::sync::Mutex::new(()),
        private: aw_tools::filesystem::private_artifact::Registry::memory(),
    };
    let _ = remove_tree(&private, temp, true, &|| false);
}
fn unique_lifecycle_name() -> String {
    format!(
        ".aw-trash-{}-{:016x}.tmp",
        std::process::id(),
        NEXT_TEMP.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    )
}
fn list_checked(root: &Root, path: &str) -> Result<Vec<super::super::Entry>> {
    let listing = root.list_with_limit(path, 4096)?;
    if listing.incomplete {
        return Err(ScopedError::Limit(
            "recursive lifecycle directory exceeds bounded scan",
        ));
    }
    Ok(listing.entries)
}
fn preflight_tree(
    root: &Root,
    path: &str,
    depth: usize,
    budget: &mut aw_tools::filesystem::lifecycle::WorkBudget,
    cancel: &dyn Fn() -> bool,
) -> Result<()> {
    if cancel() {
        return Err(ScopedError::Cancelled);
    }
    budget.visit(depth).map_err(lifecycle_error)?;
    let (dir, name) = root.parent(path)?;
    let info = lstat_at(&dir, &name)?;
    match info.st_mode & libc::S_IFMT {
        libc::S_IFDIR => {
            for entry in list_checked(root, path)? {
                if root.is_private_path(&entry.path)? {
                    return Err(ScopedError::Denied(
                        "private mutation artifact inside lifecycle tree",
                    ));
                }
                preflight_tree(root, &entry.path, depth + 1, budget, cancel)?;
            }
        }
        libc::S_IFREG => {
            budget
                .copied(info.st_size.max(0) as u64)
                .map_err(lifecycle_error)?;
        }
        libc::S_IFLNK => {}
        _ => return Err(ScopedError::Denied("unsupported file type or hardlink")),
    }
    Ok(())
}
fn copy_symlink(
    from_parent: &File,
    from_name: &str,
    to_parent: &File,
    to_name: &str,
) -> Result<()> {
    let name = cstring(from_name)?;
    let mut bytes = vec![0u8; 4097];
    // SAFETY: lstat-verified link under pinned parent; readlink never follows.
    let n = unsafe {
        libc::readlinkat(
            from_parent.as_raw_fd(),
            name.as_ptr(),
            bytes.as_mut_ptr().cast(),
            bytes.len(),
        )
    };
    if n < 0 {
        return Err(io::Error::last_os_error().into());
    }
    if n as usize == bytes.len() {
        return Err(ScopedError::Limit("symlink target exceeds 4096 bytes"));
    }
    bytes.truncate(n as usize);
    let target =
        std::ffi::CString::new(bytes).map_err(|_| ScopedError::Denied("invalid symlink target"))?;
    let name = cstring(to_name)?;
    if unsafe { libc::symlinkat(target.as_ptr(), to_parent.as_raw_fd(), name.as_ptr()) } != 0 {
        return Err(io::Error::last_os_error().into());
    }
    Ok(())
}
fn copy_stream_error(e: aw_tools::filesystem::lifecycle::CopyStreamError) -> ScopedError {
    use aw_tools::filesystem::lifecycle::CopyStreamError as E;
    match e {
        E::Io(err) => ScopedError::Io(err),
        E::Cancelled => ScopedError::Cancelled,
        E::Changed => ScopedError::Denied("copy source changed during transfer"),
        E::TooManyBytes => ScopedError::Limit("copy exceeds 64 MiB"),
    }
}
fn source_stable(before: &std::fs::Metadata, after: &std::fs::Metadata) -> bool {
    before.dev() == after.dev()
        && before.ino() == after.ino()
        && before.len() == after.len()
        && before.mtime() == after.mtime()
        && before.mtime_nsec() == after.mtime_nsec()
        && before.ctime() == after.ctime()
        && before.ctime_nsec() == after.ctime_nsec()
        && before.mode() == after.mode()
}
fn copy_file(
    root: &Root,
    source: &str,
    target_parent: &File,
    target_name: &str,
    cancel: &dyn Fn() -> bool,
) -> Result<()> {
    let mut input = root.open_regular(source)?;
    let stat = input.metadata()?;
    if !stat.is_file() || stat.len() > MAX_BATCH_FILE as u64 {
        return Err(ScopedError::Limit(
            "copy source exceeds bounded regular file",
        ));
    }
    if stat.mode() & 0o6000 != 0 {
        return Err(ScopedError::Denied(
            "copy of setuid/setgid file requires privileged host",
        ));
    }
    let mut output = at(
        target_parent,
        target_name,
        libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL,
        0o600,
    )?;
    let result = (|| -> Result<()> {
        aw_tools::filesystem::lifecycle::copy_stream(&mut input, &mut output, stat.len(), cancel)
            .map_err(copy_stream_error)?;
        if !source_stable(&stat, &input.metadata()?) {
            return Err(ScopedError::Denied("copy source changed during transfer"));
        }
        // SAFETY: private regular output fd, source mode is non-privileged.
        if unsafe { libc::fchmod(output.as_raw_fd(), stat.mode() & 0o777) } != 0 {
            return Err(io::Error::last_os_error().into());
        }
        output.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        unlink(target_parent, target_name);
    }
    result
}
fn copy_dir_tree(
    root: &Root,
    source: &str,
    target: &File,
    cancel: &dyn Fn() -> bool,
    depth: usize,
    work: &mut aw_tools::filesystem::lifecycle::WorkBudget,
) -> Result<()> {
    if cancel() {
        return Err(ScopedError::Cancelled);
    }
    work.visit(depth).map_err(lifecycle_error)?;
    for entry in list_checked(root, source)? {
        let (src_parent, src_name) = root.parent(&entry.path)?;
        let stat = lstat_at(&src_parent, &src_name)?;
        match stat.st_mode & libc::S_IFMT {
            libc::S_IFDIR => {
                let to = cstring(&entry.name)?;
                if unsafe { libc::mkdirat(target.as_raw_fd(), to.as_ptr(), 0o700) } != 0 {
                    return Err(io::Error::last_os_error().into());
                }
                let next = at(target, &entry.name, libc::O_RDONLY | libc::O_DIRECTORY, 0)?;
                copy_dir_tree(root, &entry.path, &next, cancel, depth + 1, work)?;
                // SAFETY: private destination directory and non-privileged mode.
                if unsafe { libc::fchmod(next.as_raw_fd(), stat.st_mode & 0o777) } != 0 {
                    return Err(io::Error::last_os_error().into());
                }
                next.sync_all()?;
            }
            libc::S_IFREG => {
                work.visit(depth + 1).map_err(lifecycle_error)?;
                work.copied(stat.st_size.max(0) as u64)
                    .map_err(lifecycle_error)?;
                copy_file(root, &entry.path, target, &entry.name, cancel)?;
            }
            libc::S_IFLNK => {
                work.visit(depth + 1).map_err(lifecycle_error)?;
                copy_symlink(&src_parent, &src_name, target, &entry.name)?;
            }
            _ => {
                return Err(ScopedError::Denied(
                    "unsupported entry during recursive copy",
                ))
            }
        }
    }
    Ok(())
}
fn cleanup_owned_private(parent: &File, name: &str, expected: &libc::stat) -> Result<()> {
    let current = lstat_at(parent, name)?;
    if current.st_dev != expected.st_dev || current.st_ino != expected.st_ino {
        return Err(ScopedError::Denied(
            "private tombstone identity changed before cleanup",
        ));
    }
    match current.st_mode & libc::S_IFMT {
        libc::S_IFDIR => {
            cleanup_stage_tree(
                parent,
                name,
                Some((current.st_dev as u64, current.st_ino as u64)),
            );
        }
        libc::S_IFREG | libc::S_IFLNK => {
            let name = cstring(name)?;
            if unsafe { libc::unlinkat(parent.as_raw_fd(), name.as_ptr(), 0) } != 0 {
                return Err(io::Error::last_os_error().into());
            }
        }
        _ => return Err(ScopedError::Denied("unsupported private tombstone type")),
    }
    match lstat_at(parent, name) {
        Err(ScopedError::Io(error)) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Ok(_) => Err(ScopedError::Denied("private tombstone cleanup incomplete")),
        Err(error) => Err(error),
    }
}

fn remove_tree(root: &Root, path: &str, recursive: bool, cancel: &dyn Fn() -> bool) -> Result<()> {
    let mut work = aw_tools::filesystem::lifecycle::WorkBudget::default();
    remove_tree_inner(root, path, recursive, cancel, 0, &mut work)
}
fn remove_tree_inner(
    root: &Root,
    path: &str,
    recursive: bool,
    cancel: &dyn Fn() -> bool,
    depth: usize,
    work: &mut aw_tools::filesystem::lifecycle::WorkBudget,
) -> Result<()> {
    if cancel() {
        return Err(ScopedError::Cancelled);
    }
    work.visit(depth).map_err(lifecycle_error)?;
    let (parent, name) = root.parent(path)?;
    let info = lstat_at(&parent, &name)?;
    if info.st_mode & libc::S_IFMT == libc::S_IFDIR {
        let children = list_checked(root, path)?;
        if !recursive && !children.is_empty() {
            return Err(ScopedError::Denied(
                "non-empty directory; set recursive=true",
            ));
        }
        for child in children {
            remove_tree_inner(root, &child.path, true, cancel, depth + 1, work)?;
        }
        let name = cstring(&name)?;
        if unsafe { libc::unlinkat(parent.as_raw_fd(), name.as_ptr(), libc::AT_REMOVEDIR) } != 0 {
            return Err(io::Error::last_os_error().into());
        }
    } else {
        if info.st_mode & libc::S_IFMT != libc::S_IFLNK
            && info.st_mode & libc::S_IFMT != libc::S_IFREG
        {
            return Err(ScopedError::Denied("unsupported remove target"));
        }
        let name = cstring(&name)?;
        if unsafe { libc::unlinkat(parent.as_raw_fd(), name.as_ptr(), 0) } != 0 {
            return Err(io::Error::last_os_error().into());
        }
    }
    Ok(())
}
/// Source identity and attributes remain bound to the same authorized
/// descriptor-rooted preflight across staged suffix publication.
struct CopySource<'a> {
    root: &'a Root,
    path: &'a str,
    parent: &'a File,
    name: &'a str,
    stat: &'a libc::stat,
}
pub(in crate::scoped_fs) fn copy_between(
    source_root: &Root,
    from: &str,
    destination_root: &Root,
    to: &str,
    recursive: bool,
    cancel: &dyn Fn() -> bool,
) -> Result<()> {
    use aw_tools::filesystem::lifecycle as life;
    life::exact_path(from).map_err(lifecycle_error)?;
    life::exact_path(to).map_err(lifecycle_error)?;
    source_root.ensure_public_path(from)?;
    destination_root.ensure_public_path(to)?;
    let (source_parent, source_name) = source_root.parent(from)?;
    let stat = lstat_at(&source_parent, &source_name)?;
    let directory = stat.st_mode & libc::S_IFMT == libc::S_IFDIR;
    if std::ptr::eq(source_root, destination_root) {
        life::pair(from, to, directory).map_err(lifecycle_error)?;
    } else {
        life::exact_path(from).map_err(lifecycle_error)?;
        life::exact_path(to).map_err(lifecycle_error)?;
    }
    if directory && !recursive {
        return Err(ScopedError::Denied(
            "directory copy requires recursive=true",
        ));
    }
    let mut budget = life::WorkBudget::default();
    preflight_tree(source_root, from, 0, &mut budget, cancel)?;
    if let Some((existing, missing, leaf)) = destination_root.missing_parent(to)? {
        if stat.st_mode & libc::S_IFMT == libc::S_IFREG {
            let staged = destination_root.stage_stream_create(
                to,
                |output| {
                    let mut input = source_root.open_regular(from)?;
                    let before = input.metadata()?;
                    if before.dev() != stat.st_dev as u64 || before.ino() != stat.st_ino as u64 {
                        return Err(ScopedError::Denied("source identity changed"));
                    }
                    aw_tools::filesystem::lifecycle::copy_stream(
                        &mut input,
                        output,
                        stat.st_size.max(0) as u64,
                        cancel,
                    )
                    .map_err(copy_stream_error)?;
                    if !source_stable(&before, &input.metadata()?) {
                        return Err(ScopedError::Denied("copy source changed"));
                    }
                    if unsafe { libc::fchmod(output.as_raw_fd(), stat.st_mode & 0o777) } != 0 {
                        return Err(io::Error::last_os_error().into());
                    }
                    Ok(())
                },
                cancel,
            )?;
            return staged.commit();
        }
        return destination_root.copy_missing_suffix(
            to,
            CopySource {
                root: source_root,
                path: from,
                parent: &source_parent,
                name: &source_name,
                stat: &stat,
            },
            existing,
            missing,
            leaf,
            cancel,
        );
    }
    let (target_parent, target) = destination_root.parent(to)?;
    match lstat_at(&target_parent, &target) {
        Ok(_) => return Err(ScopedError::Denied("destination exists")),
        Err(ScopedError::Io(ref e)) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    match stat.st_mode & libc::S_IFMT {
        libc::S_IFREG => {
            destination_root
                .stage_stream_create(
                    to,
                    |output| {
                        let mut input = source_root.open_regular(from)?;
                        let before = input.metadata()?;
                        if before.dev() != stat.st_dev as u64 || before.ino() != stat.st_ino as u64
                        {
                            return Err(ScopedError::Denied("copy source identity changed"));
                        }
                        aw_tools::filesystem::lifecycle::copy_stream(
                            &mut input,
                            output,
                            stat.st_size.max(0) as u64,
                            cancel,
                        )
                        .map_err(copy_stream_error)?;
                        if !source_stable(&before, &input.metadata()?) {
                            return Err(ScopedError::Denied("copy source changed during transfer"));
                        }
                        if unsafe { libc::fchmod(output.as_raw_fd(), stat.st_mode & 0o777) } != 0 {
                            return Err(io::Error::last_os_error().into());
                        }
                        Ok(())
                    },
                    cancel,
                )?
                .commit()?;
        }
        libc::S_IFLNK => {
            // symlinkat creates the complete stored link payload atomically
            // and refuses an occupied destination; no private staging name
            // is needed for this single-object publication.
            copy_symlink(&source_parent, &source_name, &target_parent, &target)?;
            let _ = target_parent.sync_all();
        }
        libc::S_IFDIR => {
            let temp = unique_lifecycle_name();
            let name = cstring(&temp)?;
            if unsafe { libc::mkdirat(target_parent.as_raw_fd(), name.as_ptr(), 0o700) } != 0 {
                return Err(io::Error::last_os_error().into());
            }
            let dir = at(&target_parent, &temp, libc::O_RDONLY | libc::O_DIRECTORY, 0)?;
            let meta = dir.metadata()?;
            let identity = (meta.dev(), meta.ino());
            let artifact = match destination_root.reserve_private_artifact(
                &target_parent,
                super::parent_model_path(to).to_owned(),
                &temp,
                aw_tools::filesystem::private_artifact::ObjectIdentity {
                    volume: identity.0,
                    object: identity.1,
                },
                aw_tools::filesystem::private_artifact::Kind::Stage,
            ) {
                Ok(artifact) => artifact,
                Err(error) => {
                    cleanup_stage_tree(&target_parent, &temp, Some(identity));
                    return Err(error);
                }
            };
            let stage = (|| -> Result<()> {
                let mut stage_budget = life::WorkBudget::default();
                copy_dir_tree(source_root, from, &dir, cancel, 0, &mut stage_budget)?;
                if unsafe { libc::fchmod(dir.as_raw_fd(), stat.st_mode & 0o777) } != 0 {
                    return Err(io::Error::last_os_error().into());
                }
                dir.sync_all()?;
                rename_exact(&target_parent, &temp, &target_parent, &target)?;
                let _ = target_parent.sync_all();
                Ok(())
            })();
            if stage.is_err() {
                cleanup_stage_tree(&target_parent, &temp, Some(identity));
            }
            if super::stat_leaf_missing(&target_parent, &temp) {
                let _ = artifact.retire();
            }
            stage?;
        }
        _ => return Err(ScopedError::Denied("unsupported copy source")),
    }
    Ok(())
}

pub(in crate::scoped_fs) fn move_between(
    source_root: &Root,
    from: &str,
    destination_root: &Root,
    to: &str,
    cancel: &dyn Fn() -> bool,
) -> Result<()> {
    use aw_tools::filesystem::lifecycle as life;
    let (source_parent, source_name) = source_root.parent(from)?;
    let stat = lstat_at(&source_parent, &source_name)?;
    let directory = stat.st_mode & libc::S_IFMT == libc::S_IFDIR;
    if std::ptr::eq(source_root, destination_root) {
        life::pair(from, to, directory).map_err(lifecycle_error)?;
    } else {
        life::exact_path(from).map_err(lifecycle_error)?;
        life::exact_path(to).map_err(lifecycle_error)?;
    }
    let mut budget = life::WorkBudget::default();
    preflight_tree(source_root, from, 0, &mut budget, cancel)?;
    if cancel() {
        return Err(ScopedError::Cancelled);
    }
    // One rename or no change; no eager destination mkdir.
    let (target_parent, target) = destination_root.parent(to).map_err(|e| match e {
        ScopedError::Io(ref io) if io.kind() == io::ErrorKind::NotFound => {
            ScopedError::Denied("destination parent does not exist")
        }
        other => other,
    })?;
    match lstat_at(&target_parent, &target) {
        Ok(_) => return Err(ScopedError::Denied("destination exists")),
        Err(ScopedError::Io(ref err)) if err.kind() == io::ErrorKind::NotFound => {}
        Err(err) => return Err(err),
    }
    rename_exact(&source_parent, &source_name, &target_parent, &target)?;
    let _ = target_parent.sync_all();
    Ok(())
}

struct PreparedRemoval {
    parent: File,
    name: String,
    source: libc::stat,
    private: String,
    artifact: Option<ArtifactLease>,
}

fn retire_unpublished_removals(items: &mut [PreparedRemoval]) {
    for item in items {
        if let Some(artifact) = item.artifact.take() {
            let _ = artifact.retire();
        }
    }
}

fn same_object(left: &libc::stat, right: &libc::stat) -> bool {
    left.st_dev == right.st_dev && left.st_ino == right.st_ino
}

pub(in crate::scoped_fs) fn remove_between(
    paths: &[(&Root, &str)],
    recursive: bool,
    cancel: &dyn Fn() -> bool,
) -> Result<aw_tools::filesystem::lifecycle::RemoveOutcome> {
    use aw_tools::filesystem::lifecycle as life;
    if paths.is_empty() || paths.len() > life::MAX_LIFECYCLE_ROOTS {
        return Err(ScopedError::Denied("invalid removal paths"));
    }
    let mut budget = life::WorkBudget::default();
    let mut identities = std::collections::BTreeSet::new();
    let mut source_identities = Vec::with_capacity(paths.len());
    let mut namespace_targets = Vec::with_capacity(paths.len());
    // Full cross-root preflight before the first namespace detach.
    for (root, original) in paths {
        root.ensure_public_path(original)?;
        let (parent, name) = root.parent(original).map_err(|error| match error {
            ScopedError::Io(ref io) if io.kind() == io::ErrorKind::NotFound => {
                ScopedError::Denied("remove target does not exist")
            }
            other => other,
        })?;
        namespace_targets.push(super::super::descriptor_path(&parent)?.join(&name));
        let info = lstat_at(&parent, &name).map_err(|error| match error {
            ScopedError::Io(ref io) if io.kind() == io::ErrorKind::NotFound => {
                ScopedError::Denied("remove target does not exist")
            }
            other => other,
        })?;
        let identity = (info.st_dev, info.st_ino);
        if !identities.insert(identity) {
            return Err(ScopedError::Denied(
                "remove paths alias the same filesystem object",
            ));
        }
        source_identities.push(identity);
        match preflight_tree(root, original, 0, &mut budget, cancel) {
            Err(ScopedError::Io(ref err)) if err.kind() == io::ErrorKind::NotFound => {
                return Err(ScopedError::Denied("remove target does not exist"));
            }
            other => other?,
        }
        if !recursive {
            let (parent, name) = root.parent(original)?;
            if lstat_at(&parent, &name)?.st_mode & libc::S_IFMT == libc::S_IFDIR
                && !list_checked(root, original)?.is_empty()
            {
                return Err(ScopedError::Denied(
                    "non-empty directory; set recursive=true",
                ));
            }
        }
    }
    for (index, target) in namespace_targets.iter().enumerate() {
        for (other_index, other) in namespace_targets.iter().enumerate() {
            if index != other_index && target != other && target.starts_with(other) {
                return Err(ScopedError::Denied(
                    "remove paths overlap in the filesystem namespace",
                ));
            }
        }
    }

    // Reserve every exact tombstone durably before the first namespace
    // detach. A later store/capacity failure therefore cannot partially remove
    // earlier request roots. Reserved-but-unpublished records are retired on
    // every early exit.
    let mut prepared = Vec::with_capacity(paths.len());
    for ((root, original), expected) in paths.iter().copied().zip(source_identities) {
        if cancel() {
            retire_unpublished_removals(&mut prepared);
            return Err(ScopedError::Cancelled);
        }
        let (parent, name) = match root.parent(original) {
            Ok(value) => value,
            Err(error) => {
                retire_unpublished_removals(&mut prepared);
                return Err(error);
            }
        };
        let source = match lstat_at(&parent, &name) {
            Ok(value) => value,
            Err(error) => {
                retire_unpublished_removals(&mut prepared);
                return Err(error);
            }
        };
        if (source.st_dev, source.st_ino) != expected {
            retire_unpublished_removals(&mut prepared);
            return Err(ScopedError::Denied(
                "remove target changed after lifecycle preflight",
            ));
        }
        let private = unique_lifecycle_name();
        let artifact = match root.reserve_private_artifact(
            &parent,
            super::parent_model_path(original).to_owned(),
            &private,
            aw_tools::filesystem::private_artifact::ObjectIdentity {
                volume: source.st_dev as u64,
                object: source.st_ino as u64,
            },
            aw_tools::filesystem::private_artifact::Kind::Tombstone,
        ) {
            Ok(value) => value,
            Err(error) => {
                retire_unpublished_removals(&mut prepared);
                return Err(error);
            }
        };
        prepared.push(PreparedRemoval {
            parent,
            name,
            source,
            private,
            artifact: Some(artifact),
        });
    }

    let total = prepared.len();
    let mut completed = 0usize;
    for idx in 0..total {
        let mut published = false;
        let step = (|| -> Result<()> {
            let item = &mut prepared[idx];
            let artifact = item
                .artifact
                .take()
                .expect("preflight reserved one artifact per removal root");
            if cancel() {
                let _ = artifact.retire();
                return Err(ScopedError::Cancelled);
            }
            let current = match lstat_at(&item.parent, &item.name) {
                Ok(value) => value,
                Err(error) => {
                    let _ = artifact.retire();
                    return Err(error);
                }
            };
            if !same_object(&current, &item.source) {
                let _ = artifact.retire();
                return Err(ScopedError::Denied(
                    "remove target changed after tombstone reservation",
                ));
            }
            if let Err(error) = rename_exact(&item.parent, &item.name, &item.parent, &item.private)
            {
                let _ = artifact.retire();
                return Err(error);
            }
            published = true;
            let cleanup = if cancel() {
                Err(ScopedError::Cancelled)
            } else {
                cleanup_owned_private(&item.parent, &item.private, &item.source)
            };
            let _ = item.parent.sync_all();
            if cleanup.is_ok() && super::stat_leaf_missing(&item.parent, &item.private) {
                let _ = artifact.retire();
            } else {
                // Drop deliberately retains the durable record for recovery.
                drop(artifact);
            }
            cleanup
        })();
        match step {
            Ok(()) => completed += 1,
            Err(_) if published => {
                retire_unpublished_removals(&mut prepared[idx + 1..]);
                return Ok(life::RemoveOutcome::CleanupPending {
                    ok: completed + 1,
                    total,
                    at: idx + 1,
                });
            }
            Err(error) if completed > 0 => {
                retire_unpublished_removals(&mut prepared[idx + 1..]);
                return Ok(life::RemoveOutcome::Partial {
                    ok: completed,
                    total,
                    at: idx + 1,
                    reason: match error {
                        ScopedError::Cancelled => "cancelled",
                        ScopedError::Denied(_) => "denied",
                        ScopedError::Limit(_) => "limit",
                        ScopedError::Io(_) => "io",
                    },
                });
            }
            Err(error) => {
                retire_unpublished_removals(&mut prepared[idx + 1..]);
                return Err(error);
            }
        }
    }
    Ok(life::RemoveOutcome::Complete(completed))
}

impl Root {
    fn copy_missing_suffix(
        &self,
        destination: &str,
        source: CopySource<'_>,
        existing: File,
        missing: Vec<String>,
        leaf: String,
        cancel: &dyn Fn() -> bool,
    ) -> Result<()> {
        let temp = format!(
            ".aw-stage-{}-{:016x}.tmp",
            std::process::id(),
            NEXT_TEMP.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        );
        let ctemp = cstring(&temp)?;
        if unsafe { libc::mkdirat(existing.as_raw_fd(), ctemp.as_ptr(), 0o700) } != 0 {
            return Err(io::Error::last_os_error().into());
        }
        let identity = (|| -> Result<(u64, u64)> {
            let pinned = at(&existing, &temp, libc::O_RDONLY | libc::O_DIRECTORY, 0)?;
            let meta = pinned.metadata()?;
            Ok((meta.dev(), meta.ino()))
        })()
        .inspect_err(|_| {
            remove_empty_stage(&existing, &temp);
        })?;
        let artifact = match self.reserve_private_artifact(
            &existing,
            super::existing_parent_model_path(destination, missing.len()),
            &temp,
            aw_tools::filesystem::private_artifact::ObjectIdentity {
                volume: identity.0,
                object: identity.1,
            },
            aw_tools::filesystem::private_artifact::Kind::Stage,
        ) {
            Ok(artifact) => artifact,
            Err(error) => {
                cleanup_stage_tree(&existing, &temp, Some(identity));
                return Err(error);
            }
        };
        let result = (|| -> Result<()> {
            let mut parent = at(&existing, &temp, libc::O_RDONLY | libc::O_DIRECTORY, 0)?;
            let mut pinned = vec![parent.try_clone()?];
            for next in missing.iter().skip(1) {
                if cancel() {
                    return Err(ScopedError::Cancelled);
                }
                let name = cstring(next)?;
                if unsafe { libc::mkdirat(parent.as_raw_fd(), name.as_ptr(), 0o755) } != 0 {
                    return Err(io::Error::last_os_error().into());
                }
                parent = at(&parent, next, libc::O_RDONLY | libc::O_DIRECTORY, 0)?;
                pinned.push(parent.try_clone()?);
            }
            match source.stat.st_mode & libc::S_IFMT {
                libc::S_IFDIR => {
                    let name = cstring(&leaf)?;
                    if unsafe { libc::mkdirat(parent.as_raw_fd(), name.as_ptr(), 0o700) } != 0 {
                        return Err(io::Error::last_os_error().into());
                    }
                    let dir = at(&parent, &leaf, libc::O_RDONLY | libc::O_DIRECTORY, 0)?;
                    let mut budget = aw_tools::filesystem::lifecycle::WorkBudget::default();
                    copy_dir_tree(source.root, source.path, &dir, cancel, 0, &mut budget)?;
                    if unsafe { libc::fchmod(dir.as_raw_fd(), source.stat.st_mode & 0o777) } != 0 {
                        return Err(io::Error::last_os_error().into());
                    }
                    dir.sync_all()?;
                }
                libc::S_IFLNK => {
                    copy_symlink(source.parent, source.name, &parent, &leaf)?;
                }
                _ => {
                    return Err(ScopedError::Denied(
                        "only directory/symlink use staged suffix tree",
                    ))
                }
            }
            if cancel() {
                return Err(ScopedError::Cancelled);
            }
            for dir in pinned.into_iter().rev() {
                dir.sync_all()?;
            }
            rename_exact(&existing, &temp, &existing, &missing[0])?;
            // One atomic publication of first missing directory component.
            // Later fsync errors never invite a duplicate copy.
            let _ = existing.sync_all();
            Ok(())
        })();
        if result.is_err() {
            cleanup_stage_tree(&existing, &temp, Some(identity));
        }
        if super::stat_leaf_missing(&existing, &temp) {
            let _ = artifact.retire();
        }
        result
    }
    pub fn copy_path(
        &self,
        from: &str,
        to: &str,
        recursive: bool,
        cancel: &dyn Fn() -> bool,
    ) -> Result<()> {
        copy_between(self, from, self, to, recursive, cancel)
    }
    pub fn move_path(&self, from: &str, to: &str, cancel: &dyn Fn() -> bool) -> Result<()> {
        move_between(self, from, self, to, cancel)
    }
    pub fn remove_paths(
        &self,
        paths: &[String],
        recursive: bool,
        cancel: &dyn Fn() -> bool,
    ) -> Result<aw_tools::filesystem::lifecycle::RemoveOutcome> {
        use aw_tools::filesystem::lifecycle as life;
        let originals =
            life::removals(paths.iter().map(String::as_str)).map_err(lifecycle_error)?;
        let routed = originals
            .into_iter()
            .map(|path| (self, path))
            .collect::<Vec<_>>();
        remove_between(&routed, recursive, cancel)
    }
}

#[cfg(test)]
mod cross_root_tests {
    use super::*;
    use std::{
        fs,
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
    };

    static NEXT: AtomicU64 = AtomicU64::new(1);
    struct Trees {
        base: PathBuf,
        source: PathBuf,
        destination: PathBuf,
    }
    impl Trees {
        fn new() -> Self {
            let base = std::env::temp_dir().join(format!(
                "aw-cross-root-lifecycle-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            let source = base.join("source");
            let destination = base.join("destination");
            fs::create_dir_all(&source).unwrap();
            fs::create_dir_all(&destination).unwrap();
            Self {
                base,
                source,
                destination,
            }
        }
    }
    impl Drop for Trees {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.base);
        }
    }

    #[test]
    fn copy_between_disjoint_roots_reuses_one_lifecycle_engine() {
        let trees = Trees::new();
        fs::write(trees.source.join("file.txt"), b"cross-root").unwrap();
        fs::create_dir(trees.source.join("tree")).unwrap();
        fs::write(trees.source.join("tree/child.txt"), b"child").unwrap();
        std::os::unix::fs::symlink("child.txt", trees.source.join("tree/link")).unwrap();

        let source = Root::open(&trees.source).unwrap();
        let destination = Root::open(&trees.destination).unwrap();
        copy_between(
            &source,
            "file.txt",
            &destination,
            "file-copy.txt",
            true,
            &|| false,
        )
        .unwrap();
        copy_between(&source, "tree", &destination, "tree-copy", true, &|| false).unwrap();

        assert_eq!(
            fs::read(trees.destination.join("file-copy.txt")).unwrap(),
            b"cross-root"
        );
        assert_eq!(
            fs::read(trees.destination.join("tree-copy/child.txt")).unwrap(),
            b"child"
        );
        assert_eq!(
            fs::read_link(trees.destination.join("tree-copy/link")).unwrap(),
            PathBuf::from("child.txt")
        );
        assert_eq!(
            fs::read(trees.source.join("file.txt")).unwrap(),
            b"cross-root"
        );
    }

    #[test]
    fn move_between_disjoint_roots_is_one_atomic_rename_or_fail() {
        use std::os::unix::fs::MetadataExt as _;
        let trees = Trees::new();
        fs::write(trees.source.join("move.txt"), b"move").unwrap();
        let source = Root::open(&trees.source).unwrap();
        let destination = Root::open(&trees.destination).unwrap();
        let same_device = fs::metadata(&trees.source).unwrap().dev()
            == fs::metadata(&trees.destination).unwrap().dev();
        let result = move_between(&source, "move.txt", &destination, "moved.txt", &|| false);
        if same_device {
            result.unwrap();
            assert!(!trees.source.join("move.txt").exists());
            assert_eq!(
                fs::read(trees.destination.join("moved.txt")).unwrap(),
                b"move"
            );
        } else {
            assert!(result.is_err());
            assert!(trees.source.join("move.txt").exists());
            assert!(!trees.destination.join("moved.txt").exists());
        }
    }
}
