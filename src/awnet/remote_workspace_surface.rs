use super::{
    RemoteWorkspaceErrorCode, RemoteWorkspaceGitMode, RemoteWorkspaceProjection,
    RemoteWorkspaceProjectionError, RemoteWorkspaceViewRequest,
};
use aw_store::RemoteViewPermission;
use aw_surfaces::{Command, Error as SurfaceError, GitMode, SurfaceHost, ViewRequest};
use serde_json::{json, Value};
use std::{fmt, sync::Arc};

#[derive(Clone)]
pub struct SurfaceRemoteWorkspaceProjection {
    host: Arc<SurfaceHost>,
    actor: String,
}

impl fmt::Debug for SurfaceRemoteWorkspaceProjection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SurfaceRemoteWorkspaceProjection")
            .field("actor", &self.actor)
            .finish_non_exhaustive()
    }
}

impl SurfaceRemoteWorkspaceProjection {
    pub fn new(host: Arc<SurfaceHost>, actor: impl Into<String>) -> Self {
        Self {
            host,
            actor: actor.into(),
        }
    }
}

fn map_error(error: SurfaceError) -> RemoteWorkspaceProjectionError {
    match error {
        SurfaceError::SourceChanged => RemoteWorkspaceProjectionError {
            code: RemoteWorkspaceErrorCode::SourceChanged,
            message: "remote Workspace source changed; refresh capabilities explicitly",
        },
        SurfaceError::ScopeChanged(_) => RemoteWorkspaceProjectionError {
            code: RemoteWorkspaceErrorCode::ScopeChanged,
            message: "remote Workspace scope changed; refresh the exact target",
        },
        SurfaceError::Denied(_) => RemoteWorkspaceProjectionError {
            code: RemoteWorkspaceErrorCode::Denied,
            message: "remote Workspace read denied",
        },
        SurfaceError::Invalid(_) => RemoteWorkspaceProjectionError {
            code: RemoteWorkspaceErrorCode::Invalid,
            message: "remote Workspace request invalid",
        },
        SurfaceError::Conflict(_) => RemoteWorkspaceProjectionError {
            code: RemoteWorkspaceErrorCode::Conflict,
            message: "remote Workspace snapshot changed; refresh explicitly",
        },
        SurfaceError::Limit(_) => RemoteWorkspaceProjectionError {
            code: RemoteWorkspaceErrorCode::Limit,
            message: "remote Workspace bounded view limit reached",
        },
        SurfaceError::Unavailable(_) => RemoteWorkspaceProjectionError {
            code: RemoteWorkspaceErrorCode::Unavailable,
            message: "remote Workspace view unavailable",
        },
    }
}

fn request(value: &RemoteWorkspaceViewRequest) -> ViewRequest {
    match value {
        RemoteWorkspaceViewRequest::Files { path } => ViewRequest::Files { path: path.clone() },
        RemoteWorkspaceViewRequest::FileSearch { query, limit } => ViewRequest::FileSearch {
            query: query.clone(),
            limit: *limit,
        },
        RemoteWorkspaceViewRequest::File { path } => ViewRequest::File { path: path.clone() },
        RemoteWorkspaceViewRequest::GitList { mode } => ViewRequest::GitList {
            mode: match mode {
                RemoteWorkspaceGitMode::Changes => GitMode::Changes,
                RemoteWorkspaceGitMode::Worktrees => GitMode::Worktrees,
                RemoteWorkspaceGitMode::Branches => GitMode::Branches,
            },
        },
        RemoteWorkspaceViewRequest::GitOpen {
            inventory_id,
            item_id,
        } => ViewRequest::GitOpen {
            inventory_id: inventory_id.clone(),
            item_id: item_id.clone(),
        },
        RemoteWorkspaceViewRequest::Review { review_id } => ViewRequest::Review {
            review_id: review_id.clone(),
        },
    }
}

impl RemoteWorkspaceProjection for SurfaceRemoteWorkspaceProjection {
    fn capabilities(
        &self,
        permission: RemoteViewPermission,
    ) -> std::result::Result<Value, RemoteWorkspaceProjectionError> {
        let caps = self.host.capabilities();
        let files = permission == RemoteViewPermission::Files;
        let git = permission == RemoteViewPermission::Git;
        Ok(json!({
            "schema": caps.schema,
            "root": caps.root,
            "source_id": caps.source_id,
            "view_root": caps.view_root,
            "files": files,
            "git_read": git,
            "git_write": false,
            "terminals": false,
            "sites": [],
            "max_terminals": 0,
            "terminal_lease_seconds": 0
        }))
    }

    fn view(
        &self,
        source_id: &str,
        scope_id: &str,
        value: &RemoteWorkspaceViewRequest,
    ) -> std::result::Result<Value, RemoteWorkspaceProjectionError> {
        self.host
            .execute(
                &self.actor,
                Command::View {
                    source_id: source_id.to_owned(),
                    scope_id: scope_id.to_owned(),
                    request: request(value),
                },
            )
            .map_err(map_error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aw_surfaces::Options;
    use std::{
        fs,
        path::{Path, PathBuf},
        process::Command as ProcessCommand,
        time::{SystemTime, UNIX_EPOCH},
    };

    struct FixtureRoot(PathBuf);
    impl FixtureRoot {
        fn path(&self) -> &Path {
            &self.0
        }
    }
    impl Drop for FixtureRoot {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn fixture() -> (FixtureRoot, SurfaceRemoteWorkspaceProjection) {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = FixtureRoot(std::env::temp_dir().join(format!(
            "aw-remote-workspace-{}-{unique}",
            std::process::id()
        )));
        fs::create_dir_all(root.path().join("src")).unwrap();
        fs::write(
            root.path().join("src/lib.rs"),
            "pub fn remote_fixture() {}\n",
        )
        .unwrap();
        let git = std::env::var_os("AW_GIT")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/usr/bin/git"));
        let bash = std::env::var_os("AW_BASH")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/bin/bash"));
        let host = Arc::new(
            SurfaceHost::new(Options {
                root: root.path().to_path_buf(),
                git,
                bash,
                actor: "fixture-owner".into(),
                allow_terminals: true,
                sites: vec![],
            })
            .unwrap(),
        );
        (
            root,
            SurfaceRemoteWorkspaceProjection::new(host, "fixture-owner"),
        )
    }

    #[test]
    fn remote_capabilities_are_permission_scoped_and_never_expose_terminal_or_write() {
        let (_root, projection) = fixture();
        let files = projection
            .capabilities(RemoteViewPermission::Files)
            .unwrap();
        assert_eq!(files["files"], true);
        assert_eq!(files["git_read"], false);
        assert_eq!(files["git_write"], false);
        assert_eq!(files["terminals"], false);
        assert_eq!(files["max_terminals"], 0);
        assert_eq!(files["sites"], json!([]));

        let git = projection.capabilities(RemoteViewPermission::Git).unwrap();
        assert_eq!(git["files"], false);
        assert_eq!(git["git_read"], true);
        assert_eq!(git["terminals"], false);
    }

    fn git(root: &Path, args: &[&str]) {
        let program = std::env::var_os("AW_GIT")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/usr/bin/git"));
        let status = ProcessCommand::new(program)
            .current_dir(root)
            .args(args)
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?} failed");
    }

    #[test]
    fn remote_file_view_reuses_exact_surface_source_and_scope() {
        let (_root, projection) = fixture();
        let caps = projection
            .capabilities(RemoteViewPermission::Files)
            .unwrap();
        let source = caps["source_id"].as_str().unwrap();
        let scope = caps["view_root"]["id"].as_str().unwrap();
        let envelope = projection
            .view(
                source,
                scope,
                &RemoteWorkspaceViewRequest::File {
                    path: "src/lib.rs".into(),
                },
            )
            .unwrap();
        assert_eq!(envelope["source_id"], source);
        assert_eq!(envelope["scope_id"], scope);
        assert_eq!(envelope["result"]["read_only"], true);
        assert!(envelope["result"]["text"]
            .as_str()
            .unwrap()
            .contains("remote_fixture"));

        let wrong = projection
            .view(
                "surface_forged",
                scope,
                &RemoteWorkspaceViewRequest::File {
                    path: "src/lib.rs".into(),
                },
            )
            .unwrap_err();
        assert_eq!(wrong.code, RemoteWorkspaceErrorCode::SourceChanged);
    }

    #[test]
    fn remote_git_projection_retains_inventory_and_immutable_review_identity() {
        let (root, projection) = fixture();
        git(root.path(), &["init", "-q"]);
        git(
            root.path(),
            &["config", "user.email", "fixture@example.invalid"],
        );
        git(root.path(), &["config", "user.name", "Fixture"]);
        git(root.path(), &["add", "src/lib.rs"]);
        git(root.path(), &["commit", "-qm", "base"]);
        fs::write(
            root.path().join("src/lib.rs"),
            "pub fn remote_fixture() { println!(\"changed\"); }\n",
        )
        .unwrap();

        let caps = projection.capabilities(RemoteViewPermission::Git).unwrap();
        let source = caps["source_id"].as_str().unwrap();
        let scope = caps["view_root"]["id"].as_str().unwrap();
        let list = projection
            .view(
                source,
                scope,
                &RemoteWorkspaceViewRequest::GitList {
                    mode: RemoteWorkspaceGitMode::Changes,
                },
            )
            .unwrap();
        let inventory = list["result"]["inventory_id"].as_str().unwrap();
        let item = list["result"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["kind"] == "change")
            .unwrap()["id"]
            .as_str()
            .unwrap();

        let opened = projection
            .view(
                source,
                scope,
                &RemoteWorkspaceViewRequest::GitOpen {
                    inventory_id: inventory.into(),
                    item_id: item.into(),
                },
            )
            .unwrap();
        assert_eq!(opened["result"]["kind"], "review");
        assert_eq!(opened["result"]["read_only"], true);
        let review_id = opened["result"]["id"].as_str().unwrap().to_owned();

        let retained = projection
            .view(
                source,
                scope,
                &RemoteWorkspaceViewRequest::Review {
                    review_id: review_id.clone(),
                },
            )
            .unwrap();
        assert_eq!(retained["result"]["id"], review_id);
        assert_eq!(retained["result"]["kind"], "review");
        assert_eq!(retained["result"]["read_only"], true);
    }
}
