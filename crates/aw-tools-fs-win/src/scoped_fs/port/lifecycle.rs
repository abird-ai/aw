use super::*;

// AI-native exact lifecycle tools. The AW host holds MutationGuard for all
// source/destination paths; Windows pinned parents deny rename/delete races.
fn lifecycle_error(error: aw_tools_core::filesystem::lifecycle::LifecycleError) -> ScopedError {
    use aw_tools_core::filesystem::lifecycle::LifecycleError as E;
    match error {
        E::TooManyRoots | E::TooManyEntries | E::TooDeep | E::TooManyBytes => {
            ScopedError::Limit("filesystem lifecycle work budget exhausted")
        }
        _ => ScopedError::Denied("invalid or overlapping lifecycle paths"),
    }
}
fn clean_entry(path: &std::path::Path) -> Result<fs::Metadata> {
    let meta = fs::symlink_metadata(path)?;
    if is_reparse(&meta) && !meta.file_type().is_symlink() {
        return Err(ScopedError::Denied("unsupported reparse point"));
    }
    Ok(meta)
}
fn no_replace(source: &std::path::Path, dest: &std::path::Path) -> Result<()> {
    // MoveFileW is a same-filesystem native move without replace semantics.
    // Parent handles remain pinned by callers; no copy/delete fallback.
    let old = wide(source);
    let new = wide(dest);
    if unsafe { MoveFileW(old.as_ptr(), new.as_ptr()) } == 0 {
        let error = io::Error::last_os_error();
        if error.raw_os_error() == Some(17) {
            return Err(ScopedError::Denied("cross-device move; use cp then rm"));
        }
        return Err(error.into());
    }
    Ok(())
}
fn private_name() -> String {
    format!(
        ".aw-trash-{}-{:016x}.tmp",
        std::process::id(),
        TEMP.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    )
}
fn limited_children(root: &Root, path: &str) -> Result<Vec<super::super::Entry>> {
    let listing = root.list_with_limit(path, 4096)?;
    if listing.incomplete {
        return Err(ScopedError::Limit(
            "recursive lifecycle directory scan truncated",
        ));
    }
    Ok(listing.entries)
}
fn preflight(
    root: &Root,
    path: &str,
    depth: usize,
    work: &mut aw_tools_core::filesystem::lifecycle::WorkBudget,
    cancel: &dyn Fn() -> bool,
) -> Result<()> {
    use aw_tools_core::filesystem::lifecycle as life;
    if cancel() {
        return Err(ScopedError::Cancelled);
    }
    work.visit(depth).map_err(lifecycle_error)?;
    let (_guard, target) = root.parent(path)?;
    let stat = clean_entry(&target)?;
    if stat.is_dir() {
        for item in limited_children(root, path)? {
            if life::is_reserved_name(&item.name) {
                return Err(ScopedError::Denied(
                    "protected staging entry in lifecycle tree",
                ));
            }
            preflight(root, &item.path, depth + 1, work, cancel)?;
        }
    } else if stat.is_file() {
        let file = root.open_regular(path)?;
        work.copied(regular(&file)?).map_err(lifecycle_error)?;
    } else if !stat.file_type().is_symlink() {
        return Err(ScopedError::Denied("unsupported lifecycle object"));
    }
    Ok(())
}
fn stream_copy_error(error: aw_tools_core::filesystem::lifecycle::CopyStreamError) -> ScopedError {
    use aw_tools_core::filesystem::lifecycle::CopyStreamError as E;
    match error {
        E::Io(error) => ScopedError::Io(error),
        E::Cancelled => ScopedError::Cancelled,
        E::Changed => ScopedError::Denied("copy source changed during transfer"),
        E::TooManyBytes => ScopedError::Limit("copy exceeds 64 MiB"),
    }
}
fn source_stable(before: &fs::Metadata, after: &fs::Metadata) -> bool {
    before.len() == after.len()
        && before.modified().ok() == after.modified().ok()
        && before.created().ok() == after.created().ok()
}
fn build_tree(
    root: &Root,
    source: &str,
    dest: &std::path::Path,
    cancel: &dyn Fn() -> bool,
    depth: usize,
    work: &mut aw_tools_core::filesystem::lifecycle::WorkBudget,
) -> Result<()> {
    if cancel() {
        return Err(ScopedError::Cancelled);
    }
    work.visit(depth).map_err(lifecycle_error)?;
    for item in limited_children(root, source)? {
        let next = dest.join(&item.name);
        let (_guard, src) = root.parent(&item.path)?;
        let stat = clean_entry(&src)?;
        if stat.is_dir() {
            fs::create_dir(&next)?;
            build_tree(root, &item.path, &next, cancel, depth + 1, work)?;
        } else if stat.is_file() {
            work.visit(depth + 1).map_err(lifecycle_error)?;
            work.copied(stat.len()).map_err(lifecycle_error)?;
            let mut input = root.open_regular(&item.path)?;
            let mut output = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&next)?;
            let before = input.metadata()?;
            aw_tools_core::filesystem::lifecycle::copy_stream(
                &mut input,
                &mut output,
                stat.len(),
                cancel,
            )
            .map_err(stream_copy_error)?;
            if !source_stable(&before, &input.metadata()?) {
                return Err(ScopedError::Denied("copy source changed during transfer"));
            }
            output.sync_all()?;
        } else if stat.file_type().is_symlink() {
            work.visit(depth + 1).map_err(lifecycle_error)?;
            let link = fs::read_link(&src)?;
            use std::os::windows::fs::{symlink_dir, symlink_file, MetadataExt};
            if stat.file_attributes()
                & windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_DIRECTORY
                != 0
            {
                symlink_dir(link, &next)?;
            } else {
                symlink_file(link, &next)?;
            }
        } else {
            return Err(ScopedError::Denied("unsupported copy object"));
        }
    }
    Ok(())
}
/// Bounded cleanup of a published private tombstone, without following
/// reparse points, including directory symlinks.
pub(super) fn cleanup_private(
    path: &std::path::Path,
    depth: usize,
    work: &mut aw_tools_core::filesystem::lifecycle::WorkBudget,
    cancelled: &dyn Fn() -> bool,
) -> Result<()> {
    if cancelled() {
        return Err(ScopedError::Cancelled);
    }
    work.visit(depth).map_err(lifecycle_error)?;
    let stat = clean_entry(path)?;
    if stat.file_type().is_symlink() {
        use std::os::windows::fs::MetadataExt;
        if stat.file_attributes()
            & windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_DIRECTORY
            != 0
        {
            fs::remove_dir(path)?;
        } else {
            fs::remove_file(path)?;
        }
        return Ok(());
    }
    if stat.is_dir() {
        let mut entries = 0usize;
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            entries += 1;
            if entries > aw_tools_core::filesystem::lifecycle::MAX_LIFECYCLE_ENTRIES {
                return Err(ScopedError::Limit("tombstone cleanup entry budget"));
            }
            let name = entry.file_name();
            let name = name
                .to_str()
                .ok_or(ScopedError::Denied("non-UTF8 name in tombstone cleanup"))?;
            super::super::validate_path(name)?;
            cleanup_private(&entry.path(), depth + 1, work, cancelled)?;
        }
        fs::remove_dir(path)?;
    } else if stat.is_file() {
        fs::remove_file(path)?;
    } else {
        return Err(ScopedError::Denied("unsafe object in tombstone cleanup"));
    }
    Ok(())
}
/// Never remove a private staged name that has been replaced by a
/// different filesystem object after the original stage handle closed.
pub(super) fn cleanup_stage_tree(path: &std::path::Path, identity: (u32, u64)) {
    let Ok(opened) = pin_directory(path) else {
        return;
    };
    if directory_identity(&opened).ok() != Some(identity) {
        return;
    }
    drop(opened);
    let mut work = aw_tools_core::filesystem::lifecycle::WorkBudget::default();
    let _ = cleanup_private(path, 0, &mut work, &|| false);
}
impl Root {
    fn copy_missing_suffix(
        &self,
        from: &str,
        source: &std::path::Path,
        stat: &fs::Metadata,
        parent: Parent,
        missing: Vec<String>,
        leaf: String,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<()> {
        let temp = parent.path.join(format!(
            ".aw-stage-{}-{:016x}.tmp",
            std::process::id(),
            TEMP.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        fs::create_dir(&temp)?;
        let opened = pin_directory(&temp)?;
        let identity = directory_identity(&opened)?;
        drop(opened);
        let result = (|| -> Result<()> {
            let mut path = temp.clone();
            let mut pins = Vec::with_capacity(missing.len());
            for part in missing.iter().skip(1) {
                if cancelled() {
                    return Err(ScopedError::Cancelled);
                }
                path.push(part);
                fs::create_dir(&path)?;
                pins.push(pin_directory(&path)?);
            }
            let target = path.join(&leaf);
            if stat.is_dir() {
                fs::create_dir(&target)?;
                let mut work = aw_tools_core::filesystem::lifecycle::WorkBudget::default();
                build_tree(self, from, &target, cancelled, 0, &mut work)?;
            } else if stat.file_type().is_symlink() {
                let link = fs::read_link(source)?;
                use std::os::windows::fs::{symlink_dir, symlink_file, MetadataExt};
                if stat.file_attributes()
                    & windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_DIRECTORY
                    != 0
                {
                    symlink_dir(link, &target)?;
                } else {
                    symlink_file(link, &target)?;
                }
            } else {
                return Err(ScopedError::Denied("invalid staged copy source"));
            }
            if cancelled() {
                return Err(ScopedError::Cancelled);
            }
            drop(pins);
            no_replace(&temp, &parent.path.join(&missing[0]))?;
            Ok(())
        })();
        if result.is_err() {
            cleanup_stage_tree(&temp, identity);
        }
        result
    }
    pub fn copy_path(
        &self,
        from: &str,
        to: &str,
        recursive: bool,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<()> {
        use aw_tools_core::filesystem::lifecycle as life;
        life::exact_path(from).map_err(lifecycle_error)?;
        life::exact_path(to).map_err(lifecycle_error)?;
        let (_source, source) = self.parent(from)?;
        let stat = clean_entry(&source)?;
        life::pair(from, to, stat.is_dir()).map_err(lifecycle_error)?;
        if stat.is_dir() && !recursive {
            return Err(ScopedError::Denied(
                "directory copy requires recursive=true",
            ));
        }
        let mut work = life::WorkBudget::default();
        preflight(self, from, 0, &mut work, cancelled)?;
        // Existing file/symlink/tree source must be staged privately first.
        // A missing parent suffix is one atomic directory publication.
        if stat.is_file() {
            self.stage_stream_create(
                to,
                |out| {
                    let mut input = self.open_regular(from)?;
                    let before = input.metadata()?;
                    aw_tools_core::filesystem::lifecycle::copy_stream(
                        &mut input,
                        out,
                        stat.len(),
                        cancelled,
                    )
                    .map_err(stream_copy_error)?;
                    if !source_stable(&before, &input.metadata()?) {
                        return Err(ScopedError::Denied("copy source changed during transfer"));
                    }
                    Ok(())
                },
                cancelled,
            )?
            .commit()?;
            return Ok(());
        }
        if let Some((parent, missing, leaf)) = self.missing_parent(to)? {
            return self
                .copy_missing_suffix(from, &source, &stat, parent, missing, leaf, cancelled);
        }
        let (existing, target) = self.parent(to)?;
        if fs::symlink_metadata(&target).is_ok() {
            return Err(ScopedError::Denied("destination exists"));
        }
        let prepared = PreparedParent {
            parent: Some(existing),
        };
        let temp = prepared.path().join(private_name());
        let build = (|| -> Result<()> {
            if stat.is_dir() {
                fs::create_dir(&temp)?;
                let mut stage_budget = life::WorkBudget::default();
                build_tree(self, from, &temp, cancelled, 0, &mut stage_budget)?;
            } else if stat.file_type().is_symlink() {
                let target_link = fs::read_link(&source)?;
                use std::os::windows::fs::{symlink_dir, symlink_file, MetadataExt};
                if stat.file_attributes()
                    & windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_DIRECTORY
                    != 0
                {
                    symlink_dir(target_link, &temp)?;
                } else {
                    symlink_file(target_link, &temp)?;
                }
            } else {
                return Err(ScopedError::Denied("unsupported copy source"));
            }
            no_replace(&temp, &target)?;
            Ok(())
        })();
        if build.is_err() {
            let _ = if temp.is_dir() {
                fs::remove_dir_all(&temp)
            } else {
                fs::remove_file(&temp)
            };
        }
        build?;
        drop(prepared);
        Ok(())
    }
    pub fn move_path(&self, from: &str, to: &str, cancelled: &dyn Fn() -> bool) -> Result<()> {
        use aw_tools_core::filesystem::lifecycle as life;
        let (_guard, source) = self.parent(from)?;
        let stat = clean_entry(&source)?;
        life::pair(from, to, stat.is_dir()).map_err(lifecycle_error)?;
        let mut work = life::WorkBudget::default();
        preflight(self, from, 0, &mut work, cancelled)?;
        if cancelled() {
            return Err(ScopedError::Cancelled);
        }
        // Missing destination parents are never materialized by move.
        let (_parent, target) = self.parent(to).map_err(|e| match e {
            ScopedError::Io(ref io) if io.kind() == io::ErrorKind::NotFound => {
                ScopedError::Denied("destination parent does not exist")
            }
            other => other,
        })?;
        if fs::symlink_metadata(&target).is_ok() {
            return Err(ScopedError::Denied("destination exists"));
        }
        no_replace(&source, &target)?;
        Ok(())
    }
    pub fn remove_paths(
        &self,
        paths: &[String],
        recursive: bool,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<aw_tools_core::filesystem::lifecycle::RemoveOutcome> {
        use aw_tools_core::filesystem::lifecycle as life;
        let paths = life::removals(paths.iter().map(String::as_str)).map_err(lifecycle_error)?;
        let mut work = life::WorkBudget::default();
        let mut identities = std::collections::BTreeSet::new();
        for path in &paths {
            use std::os::windows::fs::OpenOptionsExt;
            use windows_sys::Win32::Storage::FileSystem::{
                FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ,
                FILE_SHARE_WRITE,
            };
            let (_parent, target) = self.parent(path)?;
            let opened = fs::OpenOptions::new()
                .access_mode(0)
                .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
                .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS)
                .open(&target)
                .map_err(|error| match error.kind() {
                    io::ErrorKind::NotFound => ScopedError::Denied("remove target does not exist"),
                    _ => ScopedError::Io(error),
                })?;
            let identity = directory_identity(&opened)?;
            if !identities.insert(identity) {
                return Err(ScopedError::Denied(
                    "remove paths alias the same filesystem object",
                ));
            }
            preflight(self, path, 0, &mut work, cancelled)?;
            let (_guard, target) = self.parent(path)?;
            if !recursive
                && clean_entry(&target)?.is_dir()
                && !limited_children(self, path)?.is_empty()
            {
                return Err(ScopedError::Denied(
                    "non-empty directory; set recursive=true",
                ));
            }
        }
        let total = paths.len();
        let mut completed = 0usize;
        for (idx, path) in paths.into_iter().enumerate() {
            let mut published = false;
            let step = (|| -> Result<()> {
                if cancelled() {
                    return Err(ScopedError::Cancelled);
                }
                let (parent, source) = self.parent(path)?;
                let tombstone = parent.path.join(private_name());
                no_replace(&source, &tombstone)?;
                published = true;
                let mut work = life::WorkBudget::default();
                cleanup_private(&tombstone, 0, &mut work, cancelled)?;
                Ok(())
            })();
            match step {
                Ok(()) => completed += 1,
                Err(_) if published => {
                    return Ok(life::RemoveOutcome::CleanupPending {
                        ok: completed + 1,
                        total,
                        at: idx + 1,
                    })
                }
                Err(error) if completed > 0 => {
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
                    })
                }
                Err(error) => return Err(error),
            }
        }
        Ok(life::RemoveOutcome::Complete(completed))
    }
}
