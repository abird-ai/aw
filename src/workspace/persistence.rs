//! Opt-in per-user view persistence. Never restores credentials or execution.
//! Platform-specific safe file publication/no-follow reads live in aw-platform.
use aw::{Error, Result};
use aw_workspace::Workspace;
use serde::{Deserialize, Serialize};
use std::{
    io::Read,
    path::{Path, PathBuf},
};
#[cfg(test)]
use std::sync::atomic::{AtomicU64, Ordering};

#[cfg(test)]
static SERIAL: AtomicU64 = AtomicU64::new(1);

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Saved {
    version: u32,
    root: PathBuf,
    workspace: Workspace,
}

const LIMIT: usize = 64 * 1024;

pub fn load(path: &Path, root: &Path) -> Result<Option<Workspace>> {
    if !path.try_exists()? {
        return Ok(None);
    }
    if aw_platform::path_is_link_like(path)? {
        return Err(Error::Denied("layout file may not be a link/reparse point"));
    }
    let mut file = aw_platform::open_nofollow_read(path)?;
    if !file.metadata()?.is_file() || aw_platform::file_link_count(&file)? != 1 {
        return Err(Error::Denied(
            "layout file must be a singly-linked regular file",
        ));
    }
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take((LIMIT + 1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() > LIMIT {
        return Err(Error::Limit("layout exceeds 64 KiB"));
    }
    let saved: Saved = serde_json::from_slice(&bytes)
        .map_err(|_| Error::Config("invalid workspace layout JSON".into()))?;
    if saved.version != 1 || saved.root != root {
        return Err(Error::Denied(
            "layout version/root does not match this workspace",
        ));
    }
    saved
        .workspace
        .validate()
        .map_err(|e| Error::Config(e.to_string()))?;
    Ok(Some(saved.workspace))
}

pub fn save(path: &Path, root: &Path, workspace: &Workspace) -> Result<()> {
    workspace
        .validate()
        .map_err(|e| Error::Config(e.to_string()))?;
    let bytes = serde_json::to_vec_pretty(&Saved {
        version: 1,
        root: root.to_owned(),
        workspace: workspace.clone(),
    })
    .map_err(|_| Error::Config("layout serialization failed".into()))?;
    if bytes.len() > LIMIT {
        return Err(Error::Limit("layout exceeds 64 KiB"));
    }
    if path.try_exists()? && aw_platform::path_is_link_like(path)? {
        return Err(Error::Denied(
            "refusing to replace a layout link/reparse point",
        ));
    }
    aw_platform::atomic_write_user_metadata(path, &bytes)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path() -> PathBuf {
        std::env::temp_dir().join(format!(
            "aw-layout-test-{}-{}",
            std::process::id(),
            SERIAL.fetch_add(1, Ordering::Relaxed)
        ))
    }

    #[test]
    fn missing_is_not_corruption() {
        assert!(load(&path(), Path::new("/tmp")).unwrap().is_none());
    }

    #[cfg(unix)]
    #[test]
    fn roundtrip_is_bound_to_root() {
        use std::os::unix::fs::PermissionsExt;
        let d = path();
        std::fs::create_dir(&d).unwrap();
        std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o700)).unwrap();
        let f = d.join("view.json");
        save(&f, &d, &Workspace::default()).unwrap();
        assert_eq!(load(&f, &d).unwrap(), Some(Workspace::default()));
        assert!(load(&f, Path::new("/different")).is_err());
        std::fs::remove_dir_all(d).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn symlink_file_is_rejected() {
        let d = path();
        std::fs::create_dir(&d).unwrap();
        std::fs::write(d.join("actual"), b"{}").unwrap();
        std::os::unix::fs::symlink("actual", d.join("alias")).unwrap();
        assert!(load(&d.join("alias"), &d).is_err());
        std::fs::remove_dir_all(d).unwrap();
    }
}
