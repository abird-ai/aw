//! Blocking human-workspace services for a trusted single-principal owner.
//! No HTTP server, renderer, model dispatch, arbitrary URL fetch, or implicit model context.
//! Call on a bounded host worker. Source candidate: compile/tests required.
mod contract;
mod lease;
mod screen;
mod views;
use aw_git::{Branch, Change, Repository, Review, Worktree};
use aw_terminal::{Status, Terminal};
pub use contract::{input_bytes, Command, GitMode, Site, TerminalInput, ViewRequest};
use lease::Lease;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, VecDeque},
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Instant,
};

pub type Result<T> = std::result::Result<T, Error>;
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("surface owner source changed; refresh capabilities explicitly")]
    SourceChanged,
    #[error("surface scope binding changed: {0}")]
    ScopeChanged(&'static str),
    #[error("surface action denied: {0}")]
    Denied(&'static str),
    #[error("surface input invalid: {0}")]
    Invalid(&'static str),
    #[error("surface state changed: {0}")]
    Conflict(&'static str),
    #[error("surface limit: {0}")]
    Limit(&'static str),
    #[error("surface unavailable: {0}")]
    Unavailable(&'static str),
}
/// A grant constructed by the local host, not deserialized from a browser.
#[derive(Clone)]
pub struct Options {
    pub root: PathBuf,
    pub git: PathBuf,
    pub bash: PathBuf,
    pub actor: String,
    pub allow_terminals: bool,
    pub sites: Vec<Site>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Context {
    pub id: String,
    pub label: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Capabilities {
    pub schema: u32,
    pub root: Context,
    pub source_id: String,
    pub view_root: Context,
    pub files: bool,
    pub git_read: bool,
    pub git_write: bool,
    pub terminals: bool,
    pub sites: Vec<Site>,
    pub max_terminals: usize,
    pub terminal_lease_seconds: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Item {
    pub id: String,
    pub key: String,
    pub label: String,
    pub kind: String,
    pub group: Option<String>,
    pub path: Option<String>,
    pub status: Option<String>,
}
#[derive(Clone)]
enum Selected {
    Change(Repository, Change),
    Worktree(Repository, Worktree, Arc<aw_platform::DirectoryIdentity>),
    Branch(Repository, Branch),
}
#[derive(Clone)]
struct Inventory {
    scope: String,
    items: Vec<Item>,
    targets: BTreeMap<String, Selected>,
}
struct HeldReview {
    scope: String,
    repo: Repository,
    review: Review,
}
struct StartReceipt {
    digest: [u8; 32],
    terminal: Option<String>,
}
struct Inner {
    contexts: BTreeMap<String, PathBuf>,
    repos: BTreeMap<String, Repository>,
    inventories: BTreeMap<String, Inventory>,
    inventory_order: VecDeque<String>,
    reviews: BTreeMap<(String, String), HeldReview>,
    review_order: VecDeque<(String, String)>,
    terminals: BTreeMap<String, Arc<Terminal>>,
    leases: BTreeMap<String, Lease>,
    starts: BTreeMap<String, StartReceipt>,
    closed: bool,
}
/// Context, views, terminal leases and launch receipts belong to this owner.
/// Registry objects do not prove OS isolation or authenticate an actor string.
pub struct SurfaceHost {
    options: Options,
    source_id: String,
    root_scope: String,
    files: aw_tools::browse::WorkspaceFiles,
    root_identity: aw_platform::DirectoryIdentity,
    inner: Mutex<Inner>,
}
fn fresh(prefix: &str) -> Result<String> {
    let mut b = [0u8; 16];
    getrandom::fill(&mut b).map_err(|_| Error::Unavailable("OS randomness"))?;
    Ok(format!(
        "{prefix}_{}",
        b.iter().map(|b| format!("{b:02x}")).collect::<String>()
    ))
}
fn digest(v: &impl Serialize) -> Result<[u8; 32]> {
    Ok(Sha256::digest(serde_json::to_vec(v).map_err(|_| Error::Invalid("serialization"))?).into())
}
fn busy_terminal(t: &Terminal) -> bool {
    matches!(t.status(), Status::Running | Status::Stopping)
}
fn terminal_size(rows: u16, cols: u16) -> Result<()> {
    if !(2..=60).contains(&rows) || !(4..=160).contains(&cols) {
        Err(Error::Invalid(
            "browser terminal size is 4–160 columns by 2–60 rows",
        ))
    } else {
        Ok(())
    }
}
impl SurfaceHost {
    pub fn new(mut options: Options) -> Result<Self> {
        if options.actor.is_empty() || options.actor.len() > 128 || options.sites.len() > 8 {
            return Err(Error::Invalid("actor/sites"));
        }
        options.root = options
            .root
            .canonicalize()
            .map_err(|_| Error::Invalid("workspace root"))?;
        if !options.root.is_dir() || !options.git.is_absolute() || !options.bash.is_absolute() {
            return Err(Error::Invalid(
                "explicit root and absolute programs required",
            ));
        }
        let mut ids = std::collections::BTreeSet::new();
        for site in &options.sites {
            site.validate()?;
            if !ids.insert(&site.id) {
                return Err(Error::Invalid("duplicate site ID"));
            }
        }
        let root_identity = aw_platform::DirectoryIdentity::open(&options.root)
            .map_err(|_| Error::Unavailable("workspace root unavailable"))?;
        let files = aw_tools::browse::WorkspaceFiles::open(&options.root)
            .map_err(|_| Error::Unavailable("file root unavailable"))?;
        if !root_identity.is_current(&options.root).unwrap_or(false) {
            return Err(Error::Denied("workspace root changed during authorization"));
        }
        let source_id = fresh("surface")?;
        let root_scope = fresh("scope")?;
        let contexts = BTreeMap::from([(root_scope.clone(), options.root.clone())]);
        Ok(Self {
            options,
            source_id,
            root_scope,
            files,
            root_identity,
            inner: Mutex::new(Inner {
                contexts,
                repos: BTreeMap::new(),
                inventories: BTreeMap::new(),
                inventory_order: VecDeque::new(),
                reviews: BTreeMap::new(),
                review_order: VecDeque::new(),
                terminals: BTreeMap::new(),
                leases: BTreeMap::new(),
                starts: BTreeMap::new(),
                closed: false,
            }),
        })
    }
    pub fn capabilities(&self) -> Capabilities {
        Capabilities {
            schema: 1,
            root: Context {
                id: "root".into(),
                label: view_label(&self.options.root.display().to_string()),
            },
            source_id: self.source_id.clone(),
            view_root: Context {
                id: self.root_scope.clone(),
                label: view_label(&self.options.root.display().to_string()),
            },
            files: true,
            git_read: true,
            git_write: false,
            terminals: self.options.allow_terminals,
            sites: self.options.sites.clone(),
            max_terminals: 4,
            terminal_lease_seconds: lease::TTL.as_secs(),
        }
    }
    pub fn sites(&self) -> &[Site] {
        &self.options.sites
    }
    fn context_key<'a>(&'a self, context: &'a str) -> &'a str {
        if context == "root" {
            &self.root_scope
        } else {
            context
        }
    }
    fn repo(&self, inner: &mut Inner, context: &str) -> Result<Repository> {
        let context = self.context_key(context);
        if context == self.root_scope {
            self.validate_root()?;
        }
        if let Some(repo) = inner.repos.get(context) {
            repo.validate_binding().map_err(|_| {
                Error::ScopeChanged("repository binding changed; reopen explicitly")
            })?;
            return Ok(repo.clone());
        }
        let root = inner
            .contexts
            .get(context)
            .ok_or(Error::ScopeChanged("unknown context"))?;
        let repo = Repository::discover(&self.options.git, root)
            .map_err(|_| Error::Unavailable("Git repository unavailable or unsupported"))?;
        if context == self.root_scope && repo.root() != self.options.root {
            return Err(Error::Denied(
                "start with the Git checkout root as the explicit workspace root",
            ));
        }
        if context == self.root_scope {
            self.validate_root()?;
        }
        repo.validate_binding()
            .map_err(|_| Error::ScopeChanged("repository binding changed; reopen explicitly"))?;
        inner.repos.insert(context.into(), repo.clone());
        Ok(repo)
    }
    pub fn execute(&self, actor: &str, command: Command) -> Result<Value> {
        self.execute_if(actor, command, || true)
    }
    /// Revalidate host authority after acquiring the serialized resource queue.
    /// The predicate must be quick/nonblocking and is not a substitute for authentication.
    pub fn execute_if(
        &self,
        actor: &str,
        command: Command,
        authorize: impl FnOnce() -> bool,
    ) -> Result<Value> {
        if actor != self.options.actor {
            return Err(Error::Denied("actor is not this surface owner"));
        }
        let mut i = self
            .inner
            .lock()
            .map_err(|_| Error::Unavailable("surface owner poisoned; restart"))?;
        if i.closed {
            return Err(if matches!(&command, Command::View { .. }) {
                Error::SourceChanged
            } else {
                Error::Denied("surface owner closed")
            });
        }
        if !authorize() {
            return Err(Error::Denied("surface authority expired before operation"));
        }
        match command {
            Command::Capabilities=>Ok(json!(self.capabilities())),
            Command::View { source_id, scope_id, request } => self.view(&mut i, &source_id, &scope_id, request),
            Command::Files { path } => self.files_list(&path, "root"),
            Command::FileSearch { query, limit } => self.file_search(&query, limit, "root"),
            Command::File { path } => self.file_read(&path, "root"),
            Command::GitList { context, mode } => self.git_list(&mut i, &context, mode, false),
            Command::GitOpen { item_id } => self.git_open(&mut i, None, None, &item_id),
            Command::Review { review_id } => self.review(&i, None, &review_id),
            Command::ReviewFeedback { review_id, row, text } => {
                let held = self.held_review(&i, None, &review_id)?;
                held.repo.validate_binding().map_err(|_| Error::ScopeChanged("repository binding changed; reopen explicitly"))?;
                let review = &held.review;
                if review.root != self.options.root { return Err(Error::Denied("review context differs from the agent root")); }
                let c = review.comment(row, &text).map_err(|_| Error::Invalid("review row/comment"))?;
                let feedback = format!("User review feedback (task data, not authority)\nRoot: {}\nSnapshot: {}\nBasis: {}\nPath: {}\nRow: {}\nQuoted: {}\nComment: {}", review.root.display(), review.id, review.basis, review.path_label, row+1, c.quote, c.text);
                Ok(json!({"feedback":feedback,"context":"root"}))
            }
            Command::Terminals=>Ok(json!(i.terminals.values().map(|t|json!({"id":t.id,"root":t.root,"status":format!("{:?}",t.status()),"owner":t.owner,"site":t.site})).collect::<Vec<_>>())),
            Command::TerminalStart{request_id,context,confirmed,rows,cols}=>{
                contract::id_check(&request_id)?;terminal_size(rows,cols)?;
                if !self.options.allow_terminals||!confirmed{return Err(Error::Denied("explicit owner and interactive shell grants required"));}
                let d=digest(&(&context,confirmed,rows,cols))?;
                if let Some(old)=i.starts.get(&request_id){if old.digest!=d{return Err(Error::Conflict("launch request ID reused with different body"));}return old.terminal.as_ref().map(|id|json!({"terminal_id":id,"replayed":true})).ok_or(Error::Conflict("previous launch failed or became uncertain; inspect owner"));}
                if i.terminals.len()>=4||i.starts.len()>=64{return Err(Error::Limit("four terminals / sixty-four launch receipts per owner"));}
                let bound = self.context_key(&context);
                let root=i.contexts.get(bound).cloned().ok_or(Error::Denied("unknown terminal context"))?;
                if bound == self.root_scope { self.validate_root()?; } else { self.repo(&mut i, bound)?; }
                i.starts.insert(request_id.clone(),StartReceipt{digest:d,terminal:None});
                let mut options=aw_terminal::Options::bash(root,&self.options.bash,&self.options.actor);options.rows=rows;options.cols=cols;options.site="local-web-owner".into();
                let terminal=Terminal::spawn(options,||{}).map_err(|_|Error::Unavailable("terminal launch failed; no automatic retry"))?;
                let id=terminal.id.clone();i.terminals.insert(id.clone(),Arc::new(terminal));i.starts.get_mut(&request_id).expect("inserted").terminal=Some(id.clone());
                Ok(json!({"terminal_id":id,"replayed":false}))
            }
            Command::TerminalClaim{terminal_id,client_id}=>{
                contract::id_check(&client_id)?;let t=get_terminal(&i,&terminal_id)?;if !busy_terminal(&t){return Err(Error::Conflict("terminal has exited"));}
                let now=Instant::now();
                if let Some(l)=i.leases.get_mut(&terminal_id){if !l.expired(now){if l.client!=client_id{return Err(Error::Conflict("another client controls this terminal; release or wait for expiry"));}l.renew(now);return Ok(l.reply());}}
                let l=Lease::new(client_id,fresh("lease")?,now);let reply=l.reply();i.leases.insert(terminal_id,l);Ok(reply)
            }
            Command::TerminalRelease{terminal_id,client_id,lease}=>{let l=i.leases.get(&terminal_id).ok_or(Error::Conflict("no terminal control lease"))?;l.check(&client_id,&lease,Instant::now())?;i.leases.remove(&terminal_id);Ok(json!({"released":true}))}
            Command::TerminalScreen{terminal_id,scrollback,client_id,lease}=>{
                if scrollback>aw_terminal::SCROLLBACK{return Err(Error::Invalid("scrollback outside bound"));}
                if let (Some(c),Some(token))=(&client_id,&lease){let l=i.leases.get_mut(&terminal_id).ok_or(Error::Conflict("control expired"))?;l.check(c,token,Instant::now())?;l.renew(Instant::now());}
                else if client_id.is_some()||lease.is_some(){return Err(Error::Invalid("client and lease must be supplied together"));}
                let t=get_terminal(&i,&terminal_id)?;Ok(json!(screen::Screen::from(t.snapshot(scrollback))))
            }
            Command::TerminalInput{terminal_id,client_id,lease,sequence,input}=>{
                let t=get_terminal(&i,&terminal_id)?;let l=i.leases.get_mut(&terminal_id).ok_or(Error::Conflict("claim terminal control first"))?;
                let now=Instant::now();l.check(&client_id,&lease,now)?;let d=digest(&input)?;
                if l.duplicate(sequence,d)?{return Ok(json!({"accepted_sequence":sequence,"replayed":true}));}
                let (a,b)=t.input_modes();let bytes=input_bytes(&input,a,b)?;
                t.input(bytes).map_err(|_|Error::Conflict("input not queued; do not skip sequence"))?;
                l.accept(sequence,d,now)?;Ok(json!({"accepted_sequence":sequence,"replayed":false}))
            }
            Command::TerminalResize{terminal_id,client_id,lease,rows,cols}=>{
                terminal_size(rows,cols)?;let t=get_terminal(&i,&terminal_id)?;let l=i.leases.get_mut(&terminal_id).ok_or(Error::Conflict("claim terminal control first"))?;
                l.check(&client_id,&lease,Instant::now())?;t.resize(rows,cols).map_err(|_|Error::Conflict("resize not queued"))?;l.renew(Instant::now());Ok(json!({"queued":true}))
            }
            Command::TerminalStop{terminal_id,confirmed}=>{if !confirmed{return Err(Error::Denied("explicit termination confirmation required"));}get_terminal(&i,&terminal_id)?.terminate();i.leases.remove(&terminal_id);Ok(json!({"termination_requested":true}))}
            Command::TerminalForget{terminal_id}=>{let t=get_terminal(&i,&terminal_id)?;if busy_terminal(&t){return Err(Error::Conflict("stop terminal before forgetting"));}i.terminals.remove(&terminal_id);i.leases.remove(&terminal_id);Ok(json!({"forgotten":true}))}
        }
    }
    /// Permanently revoke this host and terminate all retained terminals. Views
    /// have no way to reopen the revoked host or resurrect old process IDs.
    pub fn shutdown(&self) {
        if let Ok(mut i) = self.inner.lock() {
            i.closed = true;
            for t in i.terminals.values() {
                t.terminate();
            }
            i.leases.clear();
        }
    }
}
impl Drop for SurfaceHost {
    fn drop(&mut self) {
        self.shutdown();
    }
}
fn get_terminal(i: &Inner, id: &str) -> Result<Arc<Terminal>> {
    i.terminals
        .get(id)
        .cloned()
        .ok_or(Error::Denied("unknown terminal resource"))
}
// Human labels have no authority and must fit the browser's metadata budget.
fn view_label(value: &str) -> String {
    let mut result = String::new();
    for character in value.chars() {
        let character = if character.is_control() {
            '\u{fffd}'
        } else {
            character
        };
        if result.len() + character.len_utf8() > 1021 {
            result.push('…');
            break;
        }
        result.push(character);
    }
    result
}
fn review_value(r: &Review, root: &std::path::Path) -> Value {
    json!({"kind":"review","id":r.id,"root":view_label(&r.root.display().to_string()),"path":r.path_label,"basis":r.basis,"lines":r.lines,"binary":r.binary,"truncated":r.truncated_display,"same_agent_root":r.root==root,"read_only":true})
}

#[cfg(test)]
mod tests {
    use super::*;
    fn options() -> Options {
        Options {
            root: std::env::temp_dir(),
            git: PathBuf::from("/usr/bin/git"),
            bash: PathBuf::from("/bin/bash"),
            actor: "test-user".into(),
            allow_terminals: false,
            sites: vec![],
        }
    }
    #[test]
    fn creation_launches_nothing() {
        let h = SurfaceHost::new(options()).unwrap();
        assert_eq!(
            h.execute("test-user", Command::Terminals).unwrap(),
            json!([])
        );
        assert!(!h.capabilities().terminals);
    }
    #[test]
    fn wrong_actor_denied() {
        let h = SurfaceHost::new(options()).unwrap();
        assert!(h.execute("other", Command::Capabilities).is_err());
    }
    #[test]
    fn shell_requires_explicit_grant() {
        let h = SurfaceHost::new(options()).unwrap();
        assert!(h
            .execute(
                "test-user",
                Command::TerminalStart {
                    request_id: "x".into(),
                    context: "root".into(),
                    confirmed: true,
                    rows: 24,
                    cols: 80
                }
            )
            .is_err());
    }
    #[test]
    fn closed_host_does_not_reactivate() {
        let h = SurfaceHost::new(options()).unwrap();
        h.shutdown();
        assert!(h.execute("test-user", Command::Capabilities).is_err());
    }
    #[test]
    fn forged_inventory_item_fails() {
        let h = SurfaceHost::new(options()).unwrap();
        assert!(h
            .execute(
                "test-user",
                Command::GitOpen {
                    item_id: "forged".into()
                }
            )
            .is_err());
    }
    #[test]
    fn forged_review_fails() {
        let h = SurfaceHost::new(options()).unwrap();
        assert!(h
            .execute(
                "test-user",
                Command::Review {
                    review_id: "forged".into()
                }
            )
            .is_err());
    }
    #[test]
    fn browser_dimensions_bounded() {
        assert!(terminal_size(0, 80).is_err());
        assert!(terminal_size(61, 80).is_err());
        assert!(terminal_size(24, 161).is_err());
        assert!(terminal_size(24, 80).is_ok());
    }
    #[test]
    fn capabilities_do_not_include_programs_or_keys() {
        let h = SurfaceHost::new(options()).unwrap();
        let text = serde_json::to_string(&h.capabilities()).unwrap();
        assert!(!text.contains("/bin/bash"));
        assert!(!text.contains("api_key"));
    }
    #[test]
    fn file_traversal_denied() {
        let h = SurfaceHost::new(options()).unwrap();
        assert!(h
            .execute(
                "test-user",
                Command::File {
                    path: "../etc/passwd".into()
                }
            )
            .is_err());
    }
    #[test]
    fn revocation_gate_runs_after_lock_and_stops_action() {
        let h = SurfaceHost::new(options()).unwrap();
        assert!(h
            .execute_if("test-user", Command::Capabilities, || false)
            .is_err());
        assert!(h.execute("test-user", Command::Capabilities).is_ok());
    }
}
