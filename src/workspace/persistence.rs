//! Opt-in per-user view persistence. Never restores credentials or execution.
//! Platform-specific safe file publication/no-follow reads live in aw-platform.
use aw::{Error, Result};
use aw_workspace::Workspace;
use serde::{Deserialize, Serialize};
#[cfg(test)]
use std::sync::atomic::{AtomicU64, Ordering};
use std::{
    io::Read,
    path::{Path, PathBuf},
};

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
    if saved.root != root
        || !matches!(
            (saved.version, saved.workspace.version),
            (1, 1) | (Workspace::VERSION, Workspace::VERSION)
        )
    {
        return Err(Error::Denied(
            "layout version/root does not match this workspace",
        ));
    }
    let workspace = saved
        .workspace
        .migrate()
        .map_err(|e| Error::Config(e.to_string()))?;
    Ok(Some(workspace))
}

pub fn save(path: &Path, root: &Path, workspace: &Workspace) -> Result<()> {
    workspace
        .validate()
        .map_err(|e| Error::Config(e.to_string()))?;
    let bytes = serde_json::to_vec_pretty(&Saved {
        version: Workspace::VERSION,
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

    #[cfg(unix)]
    fn directory() -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let d = path();
        std::fs::create_dir(&d).unwrap();
        std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o700)).unwrap();
        d
    }

    #[cfg(unix)]
    fn write_saved(path: &Path, root: &Path, version: u32, workspace: Workspace) -> Vec<u8> {
        let bytes = serde_json::to_vec(&Saved {
            version,
            root: root.to_owned(),
            workspace,
        })
        .unwrap();
        std::fs::write(path, &bytes).unwrap();
        bytes
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

    #[cfg(unix)]
    #[test]
    fn consistent_v1_migrates_in_memory_and_saves_only_as_v2() {
        use aw_workspace::{Action, Side, Target};
        let d = directory();
        let f = d.join("view.json");
        let mut expected = Workspace::default();
        expected
            .apply(Action::SplitGroup {
                pane: 2,
                side: Side::Below,
            })
            .unwrap();
        let group = expected.focused;
        expected
            .apply(Action::Open {
                group,
                target: Target::File {
                    path: "src/lib.rs".into(),
                },
                pinned: true,
            })
            .unwrap();
        expected.apply(Action::MoveTab { tab: 3, group }).unwrap();
        let mut legacy = expected.clone();
        legacy.version = 1;
        let original = write_saved(&f, &d, 1, legacy);
        let loaded = load(&f, &d).unwrap().unwrap();
        assert_eq!(loaded, expected);
        assert_eq!(std::fs::read(&f).unwrap(), original);
        save(&f, &d, &loaded).unwrap();
        let saved: Saved = serde_json::from_slice(&std::fs::read(&f).unwrap()).unwrap();
        assert_eq!(saved.version, 2);
        assert_eq!(saved.workspace, expected);
        std::fs::remove_dir_all(d).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn mixed_unsupported_versions_and_save_downgrades_are_rejected() {
        let d = directory();
        let f = d.join("view.json");
        for (outer, inner) in [(1, 2), (2, 1), (0, 0), (3, 3), (1, 3), (3, 2)] {
            let mut workspace = Workspace::default();
            workspace.version = inner;
            let bytes = write_saved(&f, &d, outer, workspace);
            assert!(load(&f, &d).is_err(), "versions {outer}/{inner}");
            assert_eq!(std::fs::read(&f).unwrap(), bytes);
        }
        save(&f, &d, &Workspace::default()).unwrap();
        let original = std::fs::read(&f).unwrap();
        let mut legacy = Workspace::default();
        legacy.version = 1;
        assert!(save(&f, &d, &legacy).is_err());
        assert_eq!(std::fs::read(&f).unwrap(), original);
        std::fs::remove_dir_all(d).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn v1_new_targets_and_invalid_migrated_graph_are_rejected() {
        let d = directory();
        let f = d.join("view.json");
        let mut legacy = Workspace::default();
        legacy.version = 1;
        legacy.tabs.get_mut(&3).unwrap().target = aw_workspace::Target::AgentView {
            store_id: "store".into(),
            session_id: "session".into(),
            branch_id: "branch".into(),
        };
        write_saved(&f, &d, 1, legacy);
        assert!(load(&f, &d).is_err());
        let mut invalid = Workspace::default();
        invalid.version = 1;
        invalid.focused = 999;
        write_saved(&f, &d, 1, invalid);
        assert!(load(&f, &d).is_err());
        std::fs::remove_dir_all(d).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn v2_exact_agent_view_descriptor_roundtrips() {
        let d = directory();
        let f = d.join("view.json");
        let mut workspace = Workspace::default();
        workspace.tabs.get_mut(&3).unwrap().target = aw_workspace::Target::AgentView {
            store_id: "store:α".into(),
            session_id: "session#1".into(),
            branch_id: "branch/.. exact".into(),
        };
        save(&f, &d, &workspace).unwrap();
        assert_eq!(load(&f, &d).unwrap(), Some(workspace));
        std::fs::remove_dir_all(d).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn layout_size_limit_is_enforced_before_deserialization() {
        let d = directory();
        let f = d.join("view.json");
        let expected = Workspace::default();
        let mut bytes = write_saved(&f, &d, 2, expected.clone());
        bytes.resize(LIMIT, b' ');
        std::fs::write(&f, &bytes).unwrap();
        assert_eq!(load(&f, &d).unwrap(), Some(expected));
        bytes.push(b' ');
        std::fs::write(&f, bytes).unwrap();
        assert!(matches!(
            load(&f, &d),
            Err(Error::Limit("layout exceeds 64 KiB"))
        ));
        std::fs::remove_dir_all(d).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn oversized_save_preserves_the_existing_layout() {
        let d = directory();
        let f = d.join("view.json");
        save(&f, &d, &Workspace::default()).unwrap();
        let original = std::fs::read(&f).unwrap();
        let mut workspace = Workspace::conversation_only();
        for n in 1..aw_workspace::MAX_TABS {
            workspace
                .apply(aw_workspace::Action::Open {
                    group: 1,
                    target: aw_workspace::Target::File {
                        path: format!("{n:02}{}", "a".repeat(4094)),
                    },
                    pinned: true,
                })
                .unwrap();
        }
        assert!(matches!(
            save(&f, &d, &workspace),
            Err(Error::Limit("layout exceeds 64 KiB"))
        ));
        assert_eq!(std::fs::read(&f).unwrap(), original);
        std::fs::remove_dir_all(d).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn hardlinked_layout_is_rejected() {
        let d = directory();
        let f = d.join("view.json");
        save(&f, &d, &Workspace::default()).unwrap();
        std::fs::hard_link(&f, d.join("alias.json")).unwrap();
        assert!(matches!(load(&f, &d), Err(Error::Denied(_))));
        std::fs::remove_dir_all(d).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn save_never_replaces_a_symlink_or_its_target() {
        let d = directory();
        let actual = d.join("actual.json");
        let alias = d.join("alias.json");
        std::fs::write(&actual, b"original").unwrap();
        std::os::unix::fs::symlink("actual.json", &alias).unwrap();
        assert!(save(&alias, &d, &Workspace::default()).is_err());
        assert!(alias.is_symlink());
        assert_eq!(std::fs::read(&actual).unwrap(), b"original");
        std::fs::remove_dir_all(d).unwrap();
    }
}
