//! Human-only Files capabilities. Layout IDs request bindings; only explicitly
//! opened local roots supply them. Jobs retain the exact authorized root.
use super::background::Executor;
use aw::{Error, Result};
use aw_tools::browse::{Entry, Listing, WorkspaceFiles};
use aw_workspace::ScopeId;
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};
use tokio::sync::mpsc as async_channel;

const MAX_SCOPES: usize = 8;

/// Opened by an explicit human action, never deserialized from a layout.
pub struct Grant {
    root: PathBuf,
    files: WorkspaceFiles,
}
impl Grant {
    fn open(path: &Path) -> Result<Self> {
        let root = path.canonicalize()?;
        let files = WorkspaceFiles::open(&root)?;
        Ok(Self { root, files })
    }
}
impl fmt::Debug for Grant {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Grant").finish_non_exhaustive()
    }
}

struct Binding {
    root: PathBuf,
    files: WorkspaceFiles,
    valid: AtomicBool,
}

/// Exact owned request binding; closing/reconnecting invalidates every clone.
#[derive(Clone)]
pub struct Access {
    scope_id: ScopeId,
    epoch: u64,
    binding: Arc<Binding>,
}
impl Access {
    pub fn scope_id(&self) -> &ScopeId {
        &self.scope_id
    }
    #[cfg(test)]
    fn epoch(&self) -> u64 {
        self.epoch
    }
    pub fn is_valid(&self) -> bool {
        self.binding.valid.load(Ordering::Acquire)
    }
    pub fn same_binding(&self, other: &Self) -> bool {
        self.is_valid()
            && other.is_valid()
            && self.scope_id == other.scope_id
            && self.epoch == other.epoch
            && Arc::ptr_eq(&self.binding, &other.binding)
    }
    fn revoke(&self) {
        self.binding.valid.store(false, Ordering::Release);
    }
}
impl fmt::Debug for Access {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Access")
            .field("scope_id", &self.scope_id)
            .field("epoch", &self.epoch)
            .field("valid", &self.is_valid())
            .finish()
    }
}

/// UI-owned capability registry, retained by Host across presentation switches.
/// Its paths never enter layouts or grant Agent/provider/execution authority.
pub struct Scopes {
    launch_root: PathBuf,
    bindings: BTreeMap<ScopeId, Access>,
    next_epoch: u64,
}
impl Scopes {
    pub fn new(root: PathBuf) -> Result<Self> {
        let grant = Grant::open(&root)?;
        let mut scopes = Self {
            launch_root: grant.root.clone(),
            bindings: BTreeMap::new(),
            next_epoch: 0,
        };
        scopes.bind(ScopeId::launch(), grant)?;
        Ok(scopes)
    }
    pub fn launch_root(&self) -> &Path {
        &self.launch_root
    }
    pub fn access(&self, scope_id: &ScopeId) -> Option<Access> {
        self.bindings
            .get(scope_id)
            .filter(|access| access.is_valid())
            .cloned()
    }
    pub fn label(&self, scope_id: &ScopeId) -> Option<&Path> {
        self.bindings
            .get(scope_id)
            .filter(|access| access.is_valid())
            .map(|access| access.binding.root.as_path())
    }
    pub fn install(&mut self, grant: Grant) -> Result<ScopeId> {
        let mut nonce = [0u8; 16];
        getrandom::fill(&mut nonce)
            .map_err(|_| Error::Storage("Files scope randomness unavailable"))?;
        let id = ScopeId(nonce.iter().map(|byte| format!("{byte:02x}")).collect());
        if self.bindings.contains_key(&id) {
            return Err(Error::Conflict(
                "Files scope ID collision; retry explicitly",
            ));
        }
        self.bind(id.clone(), grant)?;
        Ok(id)
    }
    /// Explicit human reconnect supplies a freshly opened Grant. An unknown
    /// saved ID alone cannot recover a path or install any capability.
    pub fn rebind(&mut self, scope_id: &ScopeId, grant: Grant) -> Result<()> {
        if *scope_id == ScopeId::launch() && grant.root != self.launch_root {
            return Err(Error::Denied(
                "the launch Files root stays fixed; open another Files container",
            ));
        }
        self.bind(scope_id.clone(), grant)
    }
    fn bind(&mut self, scope_id: ScopeId, grant: Grant) -> Result<()> {
        if !scope_id.is_valid() {
            return Err(Error::Denied("invalid Files scope ID"));
        }
        if self.bindings.len() >= MAX_SCOPES && !self.bindings.contains_key(&scope_id) {
            return Err(Error::Limit("at most eight authorized Files scopes"));
        }
        let epoch = self
            .next_epoch
            .checked_add(1)
            .ok_or(Error::Limit("Files scope epoch exhausted"))?;
        let access = Access {
            scope_id: scope_id.clone(),
            epoch,
            binding: Arc::new(Binding {
                root: grant.root,
                files: grant.files,
                valid: AtomicBool::new(true),
            }),
        };
        if let Some(previous) = self.bindings.insert(scope_id, access) {
            previous.revoke();
        }
        self.next_epoch = epoch;
        Ok(())
    }
    pub fn close(&mut self, scope_id: &ScopeId) -> bool {
        if *scope_id == ScopeId::launch() {
            return false;
        }
        if let Some(access) = self.bindings.remove(scope_id) {
            access.revoke();
            true
        } else {
            false
        }
    }
    pub fn retain(&mut self, referenced: &BTreeSet<ScopeId>) {
        self.bindings.retain(|scope_id, access| {
            let keep = *scope_id == ScopeId::launch() || referenced.contains(scope_id);
            if !keep {
                access.revoke();
            }
            keep
        });
    }
}
impl Drop for Scopes {
    fn drop(&mut self) {
        for access in self.bindings.values() {
            access.revoke();
        }
    }
}

#[derive(Debug)]
pub enum Request {
    List(String),
    Read(String),
    Search {
        query: String,
        limit: usize,
        cancel: Arc<AtomicBool>,
    },
}
#[derive(Debug)]
pub enum Content {
    Authorized(Grant),
    Directory(Listing),
    File { path: String, text: String },
    Search { query: String, entries: Vec<Entry> },
}
#[derive(Debug)]
pub struct Response {
    pub ticket: u64,
    pub access: Option<Access>,
    pub result: std::result::Result<Content, String>,
}
pub struct Browser {
    background: Executor,
    reply_tx: async_channel::Sender<Response>,
    pub replies: async_channel::Receiver<Response>,
}
impl Browser {
    pub fn new(background: Executor) -> Self {
        let (reply_tx, replies) = async_channel::channel(32);
        Self {
            background,
            reply_tx,
            replies,
        }
    }
    /// Only for an explicit local human Files-open/reconnect command.
    pub fn authorize(&self, ticket: u64, path: PathBuf) -> Result<()> {
        let out = self.reply_tx.clone();
        let panic_out = out.clone();
        self.background.try_submit_with_panic(
            "files",
            move || {
                let result = Grant::open(&path)
                    .map(Content::Authorized)
                    .map_err(|error| error.to_string());
                let _ = out.blocking_send(Response {
                    ticket,
                    access: None,
                    result,
                });
            },
            move || {
                let _ = panic_out.blocking_send(Response {
                    ticket,
                    access: None,
                    result: Err("file root opening panicked; retry explicitly".into()),
                });
            },
        )
    }
    pub fn request(&self, ticket: u64, access: &Access, request: Request) -> Result<()> {
        if !access.is_valid() {
            return Err(Error::Denied("Files scope is no longer authorized"));
        }
        let access = access.clone();
        let panic_access = access.clone();
        let cancel = match &request {
            Request::Search { cancel, .. } => Some(cancel.clone()),
            _ => None,
        };
        let out = self.reply_tx.clone();
        let panic_out = out.clone();
        self.background.try_submit_with_panic(
            "files",
            move || {
                let value = (|| -> Result<Content> {
                    if !access.is_valid() {
                        return Err(Error::Denied("Files scope is no longer authorized"));
                    }
                    let files = &access.binding.files;
                    let content = match request {
                        Request::List(path) => Content::Directory(files.list(&path)?),
                        Request::Read(path) => Content::File {
                            text: files.read(&path)?,
                            path,
                        },
                        Request::Search {
                            query,
                            limit,
                            cancel,
                        } => Content::Search {
                            entries: files.search_with_cancel(&query, limit, || {
                                !access.is_valid() || cancel.load(Ordering::Acquire)
                            })?,
                            query,
                        },
                    };
                    if !access.is_valid() {
                        return Err(Error::Denied("Files scope is no longer authorized"));
                    }
                    Ok(content)
                })()
                .map_err(|error| error.to_string());
                if cancel
                    .as_ref()
                    .is_some_and(|cancel| cancel.load(Ordering::Acquire))
                {
                    return;
                }
                let _ = out.blocking_send(Response {
                    ticket,
                    access: Some(access),
                    result: value,
                });
            },
            move || {
                let _ = panic_out.blocking_send(Response {
                    ticket,
                    access: Some(panic_access),
                    result: Err("file background job panicked; retry explicitly".into()),
                });
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        sync::{atomic::AtomicU64, Barrier},
        time::{Duration, Instant},
    };

    static SERIAL: AtomicU64 = AtomicU64::new(1);
    struct Roots(PathBuf);
    impl Roots {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "aw-files-scopes-{}-{}",
                std::process::id(),
                SERIAL.fetch_add(1, Ordering::Relaxed),
            ));
            fs::create_dir(&path).unwrap();
            for (name, text) in [("alpha", "Alpha 雪"), ("beta", "Beta λ")] {
                fs::create_dir_all(path.join(name).join("src")).unwrap();
                fs::write(path.join(name).join("src/lib.rs"), text).unwrap();
                fs::write(path.join(name).join(format!("{name}-only.txt")), text).unwrap();
            }
            Self(path)
        }
        fn root(&self, name: &str) -> PathBuf {
            self.0.join(name)
        }
    }
    impl Drop for Roots {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn reply(browser: &mut Browser) -> Response {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            match browser.replies.try_recv() {
                Ok(reply) => return reply,
                Err(async_channel::error::TryRecvError::Empty) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(1));
                }
                other => panic!("Files worker did not complete: {other:?}"),
            }
        }
    }

    fn text(response: Response) -> String {
        match response.result.unwrap() {
            Content::File { path, text } => {
                assert_eq!(path, "src/lib.rs");
                text
            }
            other => panic!("expected file, got {other:?}"),
        }
    }

    #[test]
    fn explicit_roots_keep_same_relative_read_list_and_search_separate() {
        let roots = Roots::new();
        let mut scopes = Scopes::new(roots.root("alpha")).unwrap();
        let mut browser = Browser::new(Executor::new().unwrap());
        let unknown = ScopeId("saved-but-not-authorized".into());
        assert!(scopes.access(&unknown).is_none());
        browser.authorize(1, roots.root("beta")).unwrap();
        let authorized = reply(&mut browser);
        assert_eq!(authorized.ticket, 1);
        assert!(authorized.access.is_none());
        let Content::Authorized(grant) = authorized.result.unwrap() else {
            panic!()
        };
        let beta = scopes.install(grant).unwrap();
        assert_ne!(beta, ScopeId::launch());
        assert_eq!(beta.0.len(), 32);
        for (index, (scope, expected, name)) in [
            (ScopeId::launch(), "Alpha 雪", "alpha-only.txt"),
            (beta, "Beta λ", "beta-only.txt"),
        ]
        .into_iter()
        .enumerate()
        {
            let access = scopes.access(&scope).unwrap();
            let ticket = index as u64 * 3 + 2;
            browser
                .request(ticket, &access, Request::Read("src/lib.rs".into()))
                .unwrap();
            let response = reply(&mut browser);
            assert_eq!(response.ticket, ticket);
            assert!(response.access.as_ref().unwrap().same_binding(&access));
            assert_eq!(text(response), expected);
            browser
                .request(ticket + 1, &access, Request::List(String::new()))
                .unwrap();
            let Content::Directory(list) = reply(&mut browser).result.unwrap() else {
                panic!()
            };
            assert!(list.entries.iter().any(|entry| entry.path == name));
            assert_eq!(list.entries.len(), 2);
            browser
                .request(
                    ticket + 2,
                    &access,
                    Request::Search {
                        query: "only".into(),
                        limit: 80,
                        cancel: Arc::new(AtomicBool::new(false)),
                    },
                )
                .unwrap();
            let Content::Search { entries, .. } = reply(&mut browser).result.unwrap() else {
                panic!()
            };
            assert_eq!(entries.len(), 1);
            assert_eq!(entries[0].path, name);
        }
        assert!(scopes.access(&unknown).is_none());
    }

    #[test]
    fn pruning_retains_referenced_capabilities_and_releases_scope_capacity() {
        let roots = Roots::new();
        let mut scopes = Scopes::new(roots.root("alpha")).unwrap();
        let keep = scopes
            .install(Grant::open(&roots.root("beta")).unwrap())
            .unwrap();
        let keep_access = scopes.access(&keep).unwrap();
        let mut removed = Vec::new();
        for _ in 2..MAX_SCOPES {
            let id = scopes
                .install(Grant::open(&roots.root("alpha")).unwrap())
                .unwrap();
            removed.push(scopes.access(&id).unwrap());
        }
        scopes.retain(&BTreeSet::from([keep.clone()]));
        assert!(removed.iter().all(|access| !access.is_valid()));
        assert!(keep_access.same_binding(&scopes.access(&keep).unwrap()));
        assert!(scopes.access(&ScopeId::launch()).is_some());
        assert!(scopes
            .install(Grant::open(&roots.root("alpha")).unwrap())
            .is_ok());
    }

    #[test]
    fn reconnect_and_close_revoke_completed_and_queued_bindings() {
        let roots = Roots::new();
        let mut scopes = Scopes::new(roots.root("alpha")).unwrap();
        let id = scopes
            .install(Grant::open(&roots.root("alpha")).unwrap())
            .unwrap();
        let old = scopes.access(&id).unwrap();
        let executor = Executor::new().unwrap();
        let mut browser = Browser::new(executor.clone());
        browser
            .request(1, &old, Request::Read("src/lib.rs".into()))
            .unwrap();
        let completed = reply(&mut browser);
        assert!(completed.access.as_ref().unwrap().same_binding(&old));

        // Occupy every actual worker so revocation precedes execution, without
        // relying on sleeps or filesystem timing to manufacture the race.
        let workers = std::thread::available_parallelism()
            .map_or(2, usize::from)
            .clamp(2, 4);
        let release = Arc::new(Barrier::new(workers + 1));
        let (started_tx, started_rx) = std::sync::mpsc::channel();
        for _ in 0..workers {
            let release = release.clone();
            let started = started_tx.clone();
            executor
                .try_submit("files", move || {
                    started.send(()).unwrap();
                    release.wait();
                })
                .unwrap();
        }
        for _ in 0..workers {
            started_rx.recv_timeout(Duration::from_secs(10)).unwrap();
        }
        browser
            .request(2, &old, Request::Read("src/lib.rs".into()))
            .unwrap();
        scopes
            .rebind(&id, Grant::open(&roots.root("beta")).unwrap())
            .unwrap();
        let current = scopes.access(&id).unwrap();
        assert!(current.epoch() > old.epoch());
        assert!(!old.is_valid());
        assert!(!completed.access.as_ref().unwrap().same_binding(&current));
        release.wait();
        let queued = reply(&mut browser);
        assert_eq!(queued.ticket, 2);
        assert!(queued.result.is_err());
        assert!(!queued.access.unwrap().same_binding(&current));
        assert!(browser
            .request(3, &old, Request::Read("src/lib.rs".into()))
            .is_err());
        browser
            .request(4, &current, Request::Read("src/lib.rs".into()))
            .unwrap();
        assert_eq!(text(reply(&mut browser)), "Beta λ");
        assert!(scopes.close(&id));
        assert!(!current.is_valid());
        assert!(scopes.access(&id).is_none());
        scopes
            .rebind(&id, Grant::open(&roots.root("alpha")).unwrap())
            .unwrap();
        let regranted = scopes.access(&id).unwrap();
        assert!(!current.same_binding(&regranted));
        assert!(regranted.epoch() > current.epoch());
        drop(scopes);
        assert!(!regranted.is_valid());
    }

    #[cfg(unix)]
    #[test]
    fn root_path_replacement_never_retargets_a_retained_capability() {
        let roots = Roots::new();
        let scopes = Scopes::new(roots.root("alpha")).unwrap();
        let access = scopes.access(&ScopeId::launch()).unwrap();
        let root = roots.root("alpha");
        fs::rename(&root, roots.root("original-alpha")).unwrap();
        fs::rename(roots.root("beta"), &root).unwrap();
        let mut browser = Browser::new(Executor::new().unwrap());
        browser
            .request(1, &access, Request::Read("src/lib.rs".into()))
            .unwrap();
        match reply(&mut browser).result {
            Ok(Content::File { text, .. }) => assert_eq!(text, "Alpha 雪"),
            Err(_) => (), // Existing platform identity checks may fail closed.
            other => panic!("unexpected root replacement response: {other:?}"),
        }
    }

    #[test]
    fn only_explicit_grants_reconnect_saved_scopes_and_bounds_preserve_existing_access() {
        let roots = Roots::new();
        let mut scopes = Scopes::new(roots.root("alpha")).unwrap();
        let launch = scopes.access(&ScopeId::launch()).unwrap();
        let saved = ScopeId("saved-external".into());
        assert!(scopes.access(&saved).is_none());
        scopes
            .rebind(&saved, Grant::open(&roots.root("beta")).unwrap())
            .unwrap();
        assert!(scopes.access(&saved).is_some());
        for _ in 2..MAX_SCOPES {
            scopes
                .install(Grant::open(&roots.root("alpha")).unwrap())
                .unwrap();
        }
        assert!(matches!(
            scopes.install(Grant::open(&roots.root("beta")).unwrap()),
            Err(Error::Limit(_))
        ));
        assert!(scopes
            .rebind(
                &ScopeId::launch(),
                Grant::open(&roots.root("beta")).unwrap()
            )
            .is_err());
        assert!(!scopes.close(&ScopeId::launch()));
        assert!(launch.same_binding(&scopes.access(&ScopeId::launch()).unwrap()));
        scopes
            .rebind(
                &ScopeId::launch(),
                Grant::open(&roots.root("alpha")).unwrap(),
            )
            .unwrap();
        assert!(!launch.is_valid());
        assert_eq!(
            scopes.launch_root(),
            roots.root("alpha").canonicalize().unwrap()
        );
        let previous = scopes.access(&saved).unwrap();
        scopes.next_epoch = u64::MAX;
        assert!(scopes
            .rebind(&saved, Grant::open(&roots.root("alpha")).unwrap())
            .is_err());
        assert!(previous.same_binding(&scopes.access(&saved).unwrap()));
    }
}
