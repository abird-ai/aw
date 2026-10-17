//! Exact, retained read capabilities. Paths and labels never manufacture grants.
use super::*;

const MAX_VIEW_BYTES: usize = 4 * 1024 * 1024;
const MAX_CONTEXTS: usize = 16;
const MAX_INVENTORIES: usize = 12;
const MAX_REVIEWS: usize = 8;

#[derive(Serialize)]
struct ViewEnvelope<'a> {
    source_id: &'a str,
    scope_id: &'a str,
    result: &'a Value,
}

impl SurfaceHost {
    fn check_view_budget(&self, scope: &str, result: &Value) -> Result<()> {
        let envelope = ViewEnvelope {
            source_id: &self.source_id,
            scope_id: scope,
            result,
        };
        if serde_json::to_vec(&envelope)
            .map_err(|_| Error::Unavailable("view encoding"))?
            .len()
            > MAX_VIEW_BYTES
        {
            return Err(Error::Limit("workspace view exceeds four MiB"));
        }
        Ok(())
    }

    pub(super) fn validate_root(&self) -> Result<()> {
        if !self
            .root_identity
            .is_current(&self.options.root)
            .unwrap_or(false)
        {
            return Err(Error::ScopeChanged(
                "workspace root changed; restart owner explicitly",
            ));
        }
        Ok(())
    }

    pub(super) fn files_list(&self, path: &str, context: &str) -> Result<Value> {
        if path.len() > 8192 {
            return Err(Error::Limit("path"));
        }
        self.validate_root()?;
        let list = self
            .files
            .list(path)
            .map_err(|_| Error::Denied("directory not readable in this root"))?;
        let entries = entries(list.entries);
        self.validate_root()?;
        Ok(
            json!({"path":list.path,"entries":entries,"incomplete":list.incomplete,"context":context}),
        )
    }

    pub(super) fn file_search(&self, query: &str, limit: usize, context: &str) -> Result<Value> {
        self.validate_root()?;
        let found = self
            .files
            .search(query, limit)
            .map_err(|_| Error::Denied("file search unavailable in this root"))?;
        self.validate_root()?;
        Ok(json!({"query":query,"entries":entries(found),"context":context}))
    }

    pub(super) fn file_read(&self, path: &str, context: &str) -> Result<Value> {
        if path.len() > 8192 {
            return Err(Error::Limit("path"));
        }
        self.validate_root()?;
        let text = self
            .files
            .read(path)
            .map_err(|_| Error::Denied("file not readable in this root"))?;
        self.validate_root()?;
        Ok(json!({"path":path,"text":text,"read_only":true,"context":context}))
    }

    pub(super) fn view(
        &self,
        inner: &mut Inner,
        source: &str,
        scope: &str,
        request: ViewRequest,
    ) -> Result<Value> {
        contract::id_check(source)?;
        contract::id_check(scope)?;
        if source != self.source_id {
            return Err(Error::SourceChanged);
        }
        // The legacy root alias is deliberately not a scoped capability ID.
        if !inner.contexts.contains_key(scope) {
            return Err(Error::ScopeChanged("unknown view scope"));
        }
        let files = matches!(
            &request,
            ViewRequest::Files { .. } | ViewRequest::File { .. } | ViewRequest::FileSearch { .. }
        );
        if files && scope != self.root_scope {
            return Err(Error::Denied("Files requires the granted launch scope"));
        }
        let result = match request {
            ViewRequest::Files { path } => self.files_list(&path, scope)?,
            ViewRequest::File { path } => self.file_read(&path, scope)?,
            ViewRequest::FileSearch { query, limit } => self.file_search(&query, limit, scope)?,
            ViewRequest::GitList { mode } => self.git_list(inner, scope, mode, true)?,
            ViewRequest::GitOpen {
                inventory_id,
                item_id,
            } => self.git_open(inner, Some(scope), Some(&inventory_id), &item_id)?,
            ViewRequest::Review { review_id } => self.review(inner, Some(scope), &review_id)?,
        };
        self.check_view_budget(scope, &result)?;
        let response = json!({"source_id":source,"scope_id":scope,"result":result});
        // Keep deterministic replacement/revocation checks after projection too.
        if files {
            self.validate_root()?;
        } else {
            self.repo(inner, scope)?;
        }
        if response["result"]["kind"] == "context" {
            if let Some(selected) = response["result"]["context"]["id"].as_str() {
                self.repo(inner, selected).map_err(|_| {
                    Error::Conflict("selected worktree changed; refresh the inventory")
                })?;
            }
        }
        Ok(response)
    }

    pub(super) fn git_list(
        &self,
        inner: &mut Inner,
        context: &str,
        mode: GitMode,
        scoped: bool,
    ) -> Result<Value> {
        contract::id_check(context)?;
        let repo = self.repo(inner, context)?;
        let scope = self.context_key(context).to_owned();
        let mut inventory = Inventory {
            scope,
            items: vec![],
            targets: BTreeMap::new(),
        };
        let (branch, head) = match mode {
            GitMode::Changes => {
                let status = repo.status().map_err(|_| {
                    Error::Unavailable("Git status unsupported or bounded limit exceeded")
                })?;
                for change in status.changes {
                    let id = fresh("item")?;
                    let group = change.group.label().to_string();
                    inventory.items.push(Item {
                        id: id.clone(),
                        key: format!("{group}:{}", change.path.label()),
                        label: change.path.label(),
                        path: Some(change.path.label()),
                        kind: "change".into(),
                        group: Some(group),
                        status: Some(change.code.to_string()),
                    });
                    inventory
                        .targets
                        .insert(id, Selected::Change(repo.clone(), change));
                }
                (status.branch, status.head)
            }
            GitMode::Worktrees => {
                for worktree in repo
                    .worktrees()
                    .map_err(|_| Error::Unavailable("worktree inventory unavailable"))?
                {
                    if worktree.bare || worktree.prunable {
                        continue;
                    }
                    let identity = Arc::new(
                        aw_platform::DirectoryIdentity::open(&worktree.path).map_err(|_| {
                            Error::Conflict("worktree changed during inventory; refresh")
                        })?,
                    );
                    let id = fresh("item")?;
                    inventory.items.push(Item {
                        id: id.clone(),
                        key: worktree.path.display().to_string(),
                        label: worktree
                            .branch
                            .clone()
                            .unwrap_or_else(|| "(detached)".into()),
                        kind: "worktree".into(),
                        group: None,
                        path: Some(worktree.path.display().to_string()),
                        status: Some(
                            if worktree.locked {
                                "locked"
                            } else {
                                "existing"
                            }
                            .into(),
                        ),
                    });
                    inventory
                        .targets
                        .insert(id, Selected::Worktree(repo.clone(), worktree, identity));
                }
                (None, None)
            }
            GitMode::Branches => {
                for branch in repo
                    .branches()
                    .map_err(|_| Error::Unavailable("branch inventory unavailable"))?
                {
                    let id = fresh("item")?;
                    inventory.items.push(Item {
                        id: id.clone(),
                        key: branch.reference.clone(),
                        label: branch.reference.clone(),
                        kind: "branch".into(),
                        group: None,
                        path: None,
                        status: Some(if branch.current { "current" } else { "compare" }.into()),
                    });
                    inventory
                        .targets
                        .insert(id, Selected::Branch(repo.clone(), branch));
                }
                (None, None)
            }
        };
        repo.validate_binding()
            .map_err(|_| Error::ScopeChanged("repository binding changed; reopen explicitly"))?;
        let id = fresh("inventory")?;
        let result = json!({"inventory_id":id,"context":context,"root":view_label(&repo.root().display().to_string()),"mode":mode,"branch":branch,"head":head,"items":inventory.items});
        if scoped {
            self.check_view_budget(&inventory.scope, &result)?;
        }
        while inner.inventories.len() >= MAX_INVENTORIES {
            let expired = inner
                .inventory_order
                .pop_front()
                .expect("bounded inventory order");
            inner.inventories.remove(&expired);
        }
        inner.inventory_order.push_back(id.clone());
        inner.inventories.insert(id, inventory);
        Ok(result)
    }

    pub(super) fn git_open(
        &self,
        inner: &mut Inner,
        scope: Option<&str>,
        inventory_id: Option<&str>,
        item_id: &str,
    ) -> Result<Value> {
        contract::id_check(item_id)?;
        let scoped = scope.is_some();
        let (origin, selected) = if let (Some(scope), Some(id)) = (scope, inventory_id) {
            contract::id_check(id)?;
            let inventory = inner
                .inventories
                .get(id)
                .ok_or(Error::Conflict("inventory expired; refresh the navigator"))?;
            if inventory.scope != scope {
                return Err(Error::Denied("inventory belongs to another scope"));
            }
            let selected = inventory
                .targets
                .get(item_id)
                .cloned()
                .ok_or(Error::Conflict(
                    "item unavailable in this inventory; refresh",
                ))?;
            (inventory.scope.clone(), selected)
        } else {
            inner
                .inventories
                .values()
                .find_map(|inventory| {
                    inventory
                        .targets
                        .get(item_id)
                        .cloned()
                        .map(|item| (inventory.scope.clone(), item))
                })
                .ok_or(Error::Conflict("inventory expired; refresh the navigator"))?
        };
        self.repo(inner, &origin)?;
        match selected {
            Selected::Worktree(repo, worktree, identity) => {
                if !identity.is_current(&worktree.path).unwrap_or(false) {
                    return Err(Error::Conflict(
                        "listed worktree directory changed; refresh",
                    ));
                }
                let next = repo
                    .select_worktree(&worktree.path)
                    .map_err(|_| Error::Conflict("worktree disappeared or changed"))?;
                next.validate_binding().map_err(|_| {
                    Error::Conflict("worktree binding changed; refresh the inventory")
                })?;
                if !identity.is_current(&worktree.path).unwrap_or(false) {
                    return Err(Error::Conflict(
                        "listed worktree directory changed; refresh",
                    ));
                }
                // Reuse a physically current binding, never overwrite a stale ID
                // merely because its saved pathname is equal.
                let existing = inner.repos.iter().find_map(|(id, existing)| {
                    (existing.root() == next.root()
                        && existing.common_dir() == next.common_dir()
                        && existing.validate_binding().is_ok())
                    .then(|| id.clone())
                });
                let is_new = existing.is_none();
                let id = if let Some(id) = existing {
                    id
                } else {
                    if inner.contexts.len() >= MAX_CONTEXTS {
                        return Err(Error::Limit("retained worktree scopes"));
                    }
                    fresh("scope")?
                };
                let result = json!({"kind":"context","context":Context{id:id.clone(),label:view_label(&next.root().display().to_string())},"agent_unchanged":true});
                if scoped {
                    self.check_view_budget(&origin, &result)?;
                }
                repo.validate_binding().map_err(|_| {
                    Error::ScopeChanged("repository binding changed; reopen explicitly")
                })?;
                next.validate_binding().map_err(|_| {
                    Error::Conflict("selected worktree changed; refresh the inventory")
                })?;
                if is_new {
                    inner.contexts.insert(id.clone(), next.root().to_owned());
                    inner.repos.insert(id, next);
                }
                Ok(result)
            }
            Selected::Change(repo, change) => {
                let review = repo.review(&change).map_err(|_| {
                    Error::Conflict("change stale/unsupported; refresh and inspect")
                })?;
                self.save_review(inner, origin, repo, review, scoped)
            }
            Selected::Branch(repo, branch) => {
                let review = repo
                    .compare(&branch)
                    .map_err(|_| Error::Conflict("comparison unavailable; refresh"))?;
                self.save_review(inner, origin, repo, review, scoped)
            }
        }
    }

    fn save_review(
        &self,
        inner: &mut Inner,
        scope: String,
        repo: Repository,
        review: Review,
        scoped: bool,
    ) -> Result<Value> {
        repo.validate_binding()
            .map_err(|_| Error::ScopeChanged("repository binding changed; reopen explicitly"))?;
        let result = review_value(&review, &self.options.root);
        if scoped {
            self.check_view_budget(&scope, &result)?;
        }
        let key = (scope.clone(), review.id.clone());
        if inner.reviews.contains_key(&key) {
            inner.review_order.retain(|old| old != &key);
        }
        while inner.reviews.len() >= MAX_REVIEWS && !inner.reviews.contains_key(&key) {
            let expired = inner
                .review_order
                .pop_front()
                .expect("bounded review order");
            inner.reviews.remove(&expired);
        }
        inner.review_order.push_back(key.clone());
        inner.reviews.insert(
            key,
            HeldReview {
                scope,
                repo,
                review,
            },
        );
        Ok(result)
    }

    pub(super) fn held_review<'a>(
        &self,
        inner: &'a Inner,
        scope: Option<&str>,
        id: &str,
    ) -> Result<&'a HeldReview> {
        contract::id_check(id)?;
        let held = if let Some(scope) = scope {
            inner.reviews.get(&(scope.to_owned(), id.to_owned()))
        } else {
            inner.reviews.values().find(|held| held.review.id == id)
        }
        .ok_or(Error::Conflict(
            "review expired or belongs to another scope; refresh",
        ))?;
        if let Some(scope) = scope {
            if held.scope != scope {
                return Err(Error::Denied("review belongs to another scope"));
            }
        }
        held.repo
            .validate_binding()
            .map_err(|_| Error::ScopeChanged("repository binding changed; reopen explicitly"))?;
        Ok(held)
    }

    pub(super) fn review(&self, inner: &Inner, scope: Option<&str>, id: &str) -> Result<Value> {
        let held = self.held_review(inner, scope, id)?;
        let result = review_value(&held.review, &self.options.root);
        held.repo
            .validate_binding()
            .map_err(|_| Error::ScopeChanged("repository binding changed; reopen explicitly"))?;
        Ok(result)
    }
}

fn entries(values: Vec<aw_tools::browse::Entry>) -> Vec<Value> {
    values.into_iter().map(|entry| json!({"name":entry.name,"path":entry.path,"kind":match entry.kind { aw_tools::browse::EntryKind::Directory=>"directory",aw_tools::browse::EntryKind::File=>"file",_=>"unsupported" }})).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, path::Path, process::Command as Process};

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let path = std::env::temp_dir().join(fresh("aw-surface-view-test").unwrap());
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
        fn options(&self, root: &Path) -> Options {
            Options {
                root: root.to_owned(),
                git: git_program(),
                bash: self.0.join("never-execute-shell"),
                actor: "viewer".into(),
                allow_terminals: false,
                sites: vec![],
            }
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn git_program() -> PathBuf {
        std::env::var_os("AW_GIT")
            .map(PathBuf::from)
            .expect("pinned shell provides AW_GIT")
    }
    fn git(root: &Path, args: &[&str]) -> String {
        let output = Process::new(git_program())
            .args(["-C"])
            .arg(root)
            .args(args)
            .env("GIT_TERMINAL_PROMPT", "0")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "Git fixture: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap().trim().into()
    }
    fn scoped(host: &SurfaceHost, scope: &str, request: ViewRequest) -> Result<Value> {
        host.execute(
            "viewer",
            Command::View {
                source_id: host.source_id.clone(),
                scope_id: scope.into(),
                request,
            },
        )
    }
    fn list(host: &SurfaceHost, scope: &str, mode: GitMode) -> Value {
        scoped(host, scope, ViewRequest::GitList { mode }).unwrap()["result"].clone()
    }
    fn open(host: &SurfaceHost, scope: &str, inventory: &Value, item: &Value) -> Result<Value> {
        scoped(
            host,
            scope,
            ViewRequest::GitOpen {
                inventory_id: inventory["inventory_id"].as_str().unwrap().into(),
                item_id: item["id"].as_str().unwrap().into(),
            },
        )
    }
    fn linked() -> (Fixture, PathBuf, PathBuf, SurfaceHost) {
        let fixture = Fixture::new();
        let a = fixture.0.join("a");
        let b = fixture.0.join("b");
        fs::create_dir_all(a.join("src")).unwrap();
        git(&a, &["init", "-b", "main"]);
        git(&a, &["config", "user.name", "Fixture"]);
        git(&a, &["config", "user.email", "fixture@example.invalid"]);
        fs::write(a.join("src/same.rs"), "base α\n").unwrap();
        git(&a, &["add", "src/same.rs"]);
        git(&a, &["commit", "-m", "fixture"]);
        git(&a, &["worktree", "add", "-b", "other", b.to_str().unwrap()]);
        fs::write(a.join("src/same.rs"), "only-A α\n").unwrap();
        fs::write(b.join("src/same.rs"), "only-B 界\n").unwrap();
        let host = SurfaceHost::new(fixture.options(&a)).unwrap();
        (fixture, a, b, host)
    }
    fn selected_worktree(host: &SurfaceHost, scope: &str, root: &Path) -> (Value, Value) {
        let inventory = list(host, scope, GitMode::Worktrees);
        let item = inventory["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["path"].as_str() == root.to_str())
            .unwrap()
            .clone();
        (inventory, item)
    }
    fn index(root: &Path) -> Vec<u8> {
        fs::read(git(
            root,
            &["rev-parse", "--path-format=absolute", "--git-path", "index"],
        ))
        .unwrap()
    }

    #[test]
    fn scoped_identity_and_strict_enum_never_grant_another_root_or_operation() {
        let fixture = Fixture::new();
        let root = fixture.0.join("launch");
        fs::create_dir(&root).unwrap();
        fs::write(root.join("same.txt"), "LAUNCH").unwrap();
        let host = SurfaceHost::new(fixture.options(&root)).unwrap();
        let value = scoped(
            &host,
            &host.root_scope,
            ViewRequest::File {
                path: "same.txt".into(),
            },
        )
        .unwrap();
        assert_eq!(value["source_id"], host.source_id);
        assert_eq!(value["scope_id"], host.root_scope);
        assert_eq!(value["result"]["context"], host.root_scope);
        assert_eq!(value["result"]["text"], "LAUNCH");
        for scope in ["root", "unknown"] {
            assert!(matches!(
                scoped(
                    &host,
                    scope,
                    ViewRequest::Files {
                        path: String::new()
                    }
                ),
                Err(Error::ScopeChanged(_))
            ));
        }
        assert!(matches!(
            host.execute(
                "viewer",
                Command::View {
                    source_id: "prior_owner".into(),
                    scope_id: host.root_scope.clone(),
                    request: ViewRequest::Files {
                        path: String::new()
                    }
                }
            ),
            Err(Error::SourceChanged)
        ));
        assert!(matches!(
            scoped(
                &host,
                &host.root_scope,
                ViewRequest::File {
                    path: "../outside".into()
                }
            ),
            Err(Error::Denied(_))
        ));
        for body in [
            json!({"op":"view","source_id":host.source_id,"scope_id":host.root_scope,"request":{"op":"file","path":"same.txt","context":"root"}}),
            json!({"op":"view","source_id":host.source_id,"scope_id":host.root_scope,"request":{"op":"terminal_start","confirmed":true}}),
            json!({"op":"view","source_id":host.source_id,"scope_id":host.root_scope,"request":{"op":"review_feedback","review_id":"x","row":0,"text":"x"}}),
            json!({"op":"view","source_id":host.source_id,"scope_id":host.root_scope,"request":{"op":"files","root":"/other"}}),
        ] {
            assert!(serde_json::from_value::<Command>(body).is_err());
        }
        assert!(host.inner.lock().unwrap().terminals.is_empty());
        assert!(host.inner.lock().unwrap().starts.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn retained_launch_files_and_shell_admission_fail_closed_after_root_replacement() {
        let fixture = Fixture::new();
        let root = fixture.0.join("launch");
        fs::create_dir(&root).unwrap();
        fs::write(root.join("same.txt"), "LAUNCH").unwrap();
        let mut options = fixture.options(&root);
        options.allow_terminals = true;
        let host = SurfaceHost::new(options).unwrap();
        assert_eq!(
            host.execute(
                "viewer",
                Command::File {
                    path: "same.txt".into()
                }
            )
            .unwrap()["text"],
            "LAUNCH"
        );
        fs::rename(&root, fixture.0.join("retired")).unwrap();
        fs::create_dir(&root).unwrap();
        fs::write(root.join("same.txt"), "REPLACEMENT").unwrap();
        // The scoped descriptor really is retained; the independent binding
        // check refuses even its original bytes after pathname replacement.
        assert_eq!(host.files.read("same.txt").unwrap(), "LAUNCH");
        for command in [
            Command::File {
                path: "same.txt".into(),
            },
            Command::Files {
                path: String::new(),
            },
            Command::FileSearch {
                query: "same".into(),
                limit: 20,
            },
            Command::View {
                source_id: host.source_id.clone(),
                scope_id: host.root_scope.clone(),
                request: ViewRequest::File {
                    path: "same.txt".into(),
                },
            },
        ] {
            assert!(matches!(
                host.execute("viewer", command),
                Err(Error::ScopeChanged(_))
            ));
        }
        assert!(matches!(
            host.execute(
                "viewer",
                Command::TerminalStart {
                    request_id: "denied".into(),
                    context: "root".into(),
                    confirmed: true,
                    rows: 24,
                    cols: 80
                }
            ),
            Err(Error::ScopeChanged(_))
        ));
        assert!(host.inner.lock().unwrap().terminals.is_empty());
        assert!(host.inner.lock().unwrap().starts.is_empty());
        let fresh_host = SurfaceHost::new(fixture.options(&root)).unwrap();
        assert_ne!(fresh_host.source_id, host.source_id);
        assert_ne!(fresh_host.root_scope, host.root_scope);
        assert!(matches!(
            fresh_host.execute(
                "viewer",
                Command::View {
                    source_id: host.source_id.clone(),
                    scope_id: host.root_scope.clone(),
                    request: ViewRequest::File {
                        path: "same.txt".into()
                    }
                }
            ),
            Err(Error::SourceChanged)
        ));
    }

    #[test]
    fn two_worktrees_keep_independent_inventories_reviews_and_unchanged_indexes() {
        let (_fixture, a, b, host) = linked();
        let root = host.root_scope.clone();
        let before = (
            git(&a, &["rev-parse", "HEAD"]),
            git(&b, &["rev-parse", "HEAD"]),
            index(&a),
            index(&b),
        );
        let (worktrees, item) = selected_worktree(&host, &root, &b);
        let opened = open(&host, &root, &worktrees, &item).unwrap();
        assert_eq!(opened["scope_id"], root);
        let other = opened["result"]["context"]["id"]
            .as_str()
            .unwrap()
            .to_owned();
        assert_ne!(other, root);
        assert!(matches!(
            scoped(
                &host,
                &other,
                ViewRequest::File {
                    path: "src/same.rs".into()
                }
            ),
            Err(Error::Denied(_))
        ));
        let a_inventory = list(&host, &root, GitMode::Changes);
        let b_inventory = list(&host, &other, GitMode::Changes);
        let a_item = a_inventory["items"][0].clone();
        let b_item = b_inventory["items"][0].clone();
        let a_review = open(&host, &root, &a_inventory, &a_item).unwrap()["result"].clone();
        let b_review = open(&host, &other, &b_inventory, &b_item).unwrap()["result"].clone();
        assert!(a_review.to_string().contains("only-A"));
        assert!(!a_review.to_string().contains("only-B"));
        assert!(b_review.to_string().contains("only-B"));
        assert!(!b_review.to_string().contains("only-A"));
        let refreshed = list(&host, &root, GitMode::Changes);
        assert_ne!(refreshed["inventory_id"], a_inventory["inventory_id"]);
        assert_eq!(
            open(&host, &root, &a_inventory, &a_item).unwrap()["result"]["id"],
            a_review["id"],
            "another navigator's refresh preserves the old exact inventory"
        );
        assert!(matches!(
            open(&host, &root, &a_inventory, &b_item),
            Err(Error::Conflict(_))
        ));
        assert!(open(&host, &other, &a_inventory, &a_item).is_err());
        assert!(scoped(
            &host,
            &other,
            ViewRequest::Review {
                review_id: a_review["id"].as_str().unwrap().into()
            }
        )
        .is_err());
        assert_eq!(
            scoped(
                &host,
                &root,
                ViewRequest::Review {
                    review_id: a_review["id"].as_str().unwrap().into()
                }
            )
            .unwrap()["result"],
            a_review
        );
        let (again, item) = selected_worktree(&host, &root, &b);
        assert_eq!(
            open(&host, &root, &again, &item).unwrap()["result"]["context"]["id"],
            other
        );
        assert_eq!(
            before,
            (
                git(&a, &["rev-parse", "HEAD"]),
                git(&b, &["rev-parse", "HEAD"]),
                index(&a),
                index(&b)
            )
        );
        assert!(host.inner.lock().unwrap().terminals.is_empty());
        assert!(host.inner.lock().unwrap().starts.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn stale_worktree_item_cannot_authorize_copied_git_pointer_replacement() {
        let (fixture, _a, b, host) = linked();
        let root = host.root_scope.clone();
        let (inventory, item) = selected_worktree(&host, &root, &b);
        let pointer = fs::read(b.join(".git")).unwrap();
        fs::rename(&b, fixture.0.join("retired-b")).unwrap();
        fs::create_dir(&b).unwrap();
        fs::write(b.join(".git"), pointer).unwrap();
        fs::write(b.join("replacement.txt"), "REPLACEMENT").unwrap();
        assert!(matches!(
            open(&host, &root, &inventory, &item),
            Err(Error::Conflict(_))
        ));
        assert_eq!(host.inner.lock().unwrap().contexts.len(), 1);
        assert!(
            scoped(
                &host,
                &root,
                ViewRequest::File {
                    path: "src/same.rs".into()
                }
            )
            .is_ok(),
            "origin scope remains valid"
        );
    }

    #[cfg(unix)]
    #[test]
    fn retained_worktree_review_scope_and_shell_never_retarget_a_replacement() {
        let (fixture, _a, b, mut host) = linked();
        host.options.allow_terminals = true;
        let root = host.root_scope.clone();
        let (inventory, item) = selected_worktree(&host, &root, &b);
        let scope = open(&host, &root, &inventory, &item).unwrap()["result"]["context"]["id"]
            .as_str()
            .unwrap()
            .to_owned();
        let changes = list(&host, &scope, GitMode::Changes);
        let review = open(&host, &scope, &changes, &changes["items"][0]).unwrap()["result"]["id"]
            .as_str()
            .unwrap()
            .to_owned();
        let pointer = fs::read(b.join(".git")).unwrap();
        fs::rename(&b, fixture.0.join("retired-b")).unwrap();
        fs::create_dir_all(b.join("src")).unwrap();
        fs::write(b.join(".git"), pointer).unwrap();
        fs::write(b.join("src/same.rs"), "REPLACEMENT").unwrap();
        assert!(matches!(
            scoped(
                &host,
                &scope,
                ViewRequest::GitList {
                    mode: GitMode::Changes
                }
            ),
            Err(Error::ScopeChanged(_))
        ));
        assert!(matches!(
            scoped(&host, &scope, ViewRequest::Review { review_id: review }),
            Err(Error::ScopeChanged(_))
        ));
        assert!(matches!(
            host.execute(
                "viewer",
                Command::TerminalStart {
                    request_id: "denied".into(),
                    context: scope,
                    confirmed: true,
                    rows: 24,
                    cols: 80
                }
            ),
            Err(Error::ScopeChanged(_))
        ));
        assert!(host.inner.lock().unwrap().terminals.is_empty());
        assert!(host.inner.lock().unwrap().starts.is_empty());
        assert!(scoped(
            &host,
            &root,
            ViewRequest::File {
                path: "src/same.rs".into()
            }
        )
        .is_ok());
    }

    #[test]
    fn bounded_inventory_eviction_is_explicit_and_does_not_rebind_contexts() {
        let (_fixture, _a, _b, host) = linked();
        let root = host.root_scope.clone();
        let oldest = list(&host, &root, GitMode::Changes);
        let item = oldest["items"][0].clone();
        for _ in 0..MAX_INVENTORIES {
            list(&host, &root, GitMode::Changes);
        }
        assert!(matches!(
            open(&host, &root, &oldest, &item),
            Err(Error::Conflict(_))
        ));
        let inner = host.inner.lock().unwrap();
        assert_eq!(inner.inventories.len(), MAX_INVENTORIES);
        assert_eq!(inner.contexts.len(), 1);
        assert!(inner.reviews.is_empty());
    }

    #[test]
    fn oversized_inventory_preserves_previously_admitted_inventory_and_scopes() {
        let (_fixture, a, _b, host) = linked();
        let root = host.root_scope.clone();
        let oldest = list(&host, &root, GitMode::Changes);
        let item = oldest["items"][0].clone();
        for _ in 1..MAX_INVENTORIES {
            list(&host, &root, GitMode::Changes);
        }
        let before = {
            let inner = host.inner.lock().unwrap();
            (
                inner.inventory_order.clone(),
                inner.contexts.clone(),
                inner.repos.keys().cloned().collect::<Vec<_>>(),
            )
        };
        // Raw porcelain fits the SDK's four-MiB limit, while repeated path
        // labels in the public JSON projection exceed the encoded view limit.
        let mut directory = a.clone();
        for _ in 0..5 {
            directory.push("x".repeat(180));
        }
        fs::create_dir_all(&directory).unwrap();
        for number in 0..1800 {
            fs::write(directory.join(format!("item-{number:04}.txt")), "x").unwrap();
        }
        assert!(matches!(
            scoped(
                &host,
                &root,
                ViewRequest::GitList {
                    mode: GitMode::Changes
                }
            ),
            Err(Error::Limit(_))
        ));
        {
            let inner = host.inner.lock().unwrap();
            assert_eq!(inner.inventories.len(), MAX_INVENTORIES);
            assert_eq!(inner.inventory_order, before.0);
            assert_eq!(inner.contexts, before.1);
            assert_eq!(inner.repos.keys().cloned().collect::<Vec<_>>(), before.2);
        }
        assert!(open(&host, &root, &oldest, &item).is_ok());
    }

    #[test]
    fn oversized_review_preserves_previously_admitted_reviews_and_scopes() {
        let (_fixture, a, _b, host) = linked();
        let root = host.root_scope.clone();
        let mut oldest = None;
        for number in 0..MAX_REVIEWS {
            fs::write(a.join("src/same.rs"), format!("review-{number} α\n")).unwrap();
            let inventory = list(&host, &root, GitMode::Changes);
            let review = open(&host, &root, &inventory, &inventory["items"][0]).unwrap();
            oldest.get_or_insert_with(|| review["result"].clone());
        }
        // A text patch stays below two MiB; escaping control characters into
        // readable JSON expands it beyond the full response budget.
        fs::write(a.join("large.txt"), vec![1u8; 900 * 1024]).unwrap();
        let inventory = list(&host, &root, GitMode::Changes);
        let item = inventory["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["path"] == "large.txt")
            .unwrap();
        let before = {
            let inner = host.inner.lock().unwrap();
            (
                inner.review_order.clone(),
                inner.contexts.clone(),
                inner.repos.keys().cloned().collect::<Vec<_>>(),
            )
        };
        assert!(matches!(
            open(&host, &root, &inventory, item),
            Err(Error::Limit(_))
        ));
        {
            let inner = host.inner.lock().unwrap();
            assert_eq!(inner.reviews.len(), MAX_REVIEWS);
            assert_eq!(inner.review_order, before.0);
            assert_eq!(inner.contexts, before.1);
            assert_eq!(inner.repos.keys().cloned().collect::<Vec<_>>(), before.2);
        }
        let oldest = oldest.unwrap();
        assert_eq!(
            scoped(
                &host,
                &root,
                ViewRequest::Review {
                    review_id: oldest["id"].as_str().unwrap().into()
                }
            )
            .unwrap()["result"],
            oldest
        );
    }
}
