//! Shared live-session JobManager. Hosts own authority, process launch and
//! transport projection; this manager owns canonical Job/Graph lifecycle,
//! deterministic admission, retained output and one exposed ID namespace.
//! Host callbacks may checkpoint transitions or replay claims when their live
//! transport requires it, but this module is not a persistent workflow database.
use super::{
    batch::{Graph, NodeTool, Status, Step},
    footprint::Footprint,
    job::{
        Action, Error as JobError, Job, JournalError, Lifecycle, NodeState, StopReason, Transition,
    },
    runner::ProcessTerminal,
    status::Primary,
};
use crate::{
    task::{
        SpoolWriters, TaskDisposition, DEFAULT_SESSION_SPOOL, MAX_HIDDEN_REQUEST_REPLAYS,
        MAX_SESSION_SPOOL,
    },
    task_fence::TaskIdentity,
    task_ops::{TaskInfo, TaskTerminalDetail},
    task_spool::{BoundedFileSpool, BoundedMemorySpool, SessionQuota, MAX_SPOOL_PAGE_BYTES},
};
use sha2::{Digest as _, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    fs::File,
    panic::AssertUnwindSafe,
    sync::{Arc, Mutex},
};
#[cfg(feature = "async")]
use tokio::sync::watch;

pub const DEFAULT_MAX_RETAINED_JOBS: usize = 256;
pub const DEFAULT_MAX_EXPOSED_JOBS: usize = DEFAULT_MAX_RETAINED_JOBS;
pub const DEFAULT_MAX_HIDDEN_JOBS: usize = DEFAULT_MAX_RETAINED_JOBS;
pub const MAX_RETAINED_JOBS: usize = 8_192;
pub const DEFAULT_MAX_ACTIVE_NODES: usize = 32;
pub const MAX_JOB_LIST_PAGE: usize = 256;
/// Guaranteed upper bound for the complete compact `job_get`/`job_cancel`
/// status grammar for one valid Graph (MAX_OPS nodes), including per-node
/// process terminal metadata. Hosts exposing non-paged status controls must
/// reserve at least this many result bytes.
pub const MAX_COMPACT_JOB_STATUS_BYTES: usize = 8 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SemanticBudget {
    pub work_bytes: u64,
    pub result_bytes: usize,
}

#[derive(Clone, Debug)]
pub struct NodePolicy {
    pub budget: SemanticBudget,
    pub footprint: Footprint,
    /// Registry/tool eligibility generation captured at preparation time.
    pub registry_generation: u64,
    /// Host-authored generation/fingerprint of the authority used to prepare
    /// this node. It is evidence for later revalidation, never a capability.
    pub authority_binding: [u8; 32],
    /// Prepared shell/interpreter/runtime identity where applicable. Native
    /// tools may use a stable host runtime identity for the current process.
    pub runtime_binding: [u8; 32],
    /// Whether this node's ordinary result is eligible for exact Task projection.
    pub task_result_eligible: bool,
}

#[derive(Clone, Debug)]
pub struct NodePlan {
    coordinate: usize,
    tool: NodeTool,
    arguments: serde_json::Value,
    budget: SemanticBudget,
    footprint: Footprint,
    registry_generation: u64,
    authority_binding: [u8; 32],
    runtime_binding: [u8; 32],
    task_result_eligible: bool,
    replay_identity: [u8; 32],
}
impl NodePlan {
    pub fn coordinate(&self) -> usize {
        self.coordinate
    }
    pub fn tool(&self) -> NodeTool {
        self.tool
    }
    pub fn arguments(&self) -> &serde_json::Value {
        &self.arguments
    }
    pub fn budget(&self) -> SemanticBudget {
        self.budget
    }
    pub fn footprint(&self) -> &Footprint {
        &self.footprint
    }
    pub fn registry_generation(&self) -> u64 {
        self.registry_generation
    }
    pub fn authority_binding(&self) -> [u8; 32] {
        self.authority_binding
    }
    pub fn runtime_binding(&self) -> [u8; 32] {
        self.runtime_binding
    }
    pub fn task_result_eligible(&self) -> bool {
        self.task_result_eligible
    }
    pub fn replay_identity(&self) -> [u8; 32] {
        self.replay_identity
    }
}

#[derive(Debug)]
pub enum PlanError<E> {
    InvalidBudget,
    UnboundAuthority,
    Host(E),
}

/// Capture all semantic policy before a Job exists. Outer transport/result
/// budgets are deliberately absent from this API.
pub fn plan_graph<E>(
    graph: &Graph,
    mut prepare: impl FnMut(usize, &Step) -> Result<NodePolicy, E>,
) -> Result<Vec<NodePlan>, PlanError<E>> {
    let graph_hash = graph.fingerprint();
    let mut plans = Vec::with_capacity(graph.steps().len());
    for (index, step) in graph.steps().iter().enumerate() {
        let coordinate = index + 1;
        let policy = prepare(coordinate, step).map_err(PlanError::Host)?;
        if policy.budget.work_bytes == 0 || policy.budget.result_bytes == 0 {
            return Err(PlanError::InvalidBudget);
        }
        if policy.registry_generation == 0
            || policy.authority_binding == [0; 32]
            || policy.runtime_binding == [0; 32]
        {
            return Err(PlanError::UnboundAuthority);
        }
        let mut hash = Sha256::new();
        hash.update(b"aw-tools-node-plan-v1\0");
        hash.update(graph_hash);
        hash.update((coordinate as u64).to_le_bytes());
        hash.update(step.tool.name().as_bytes());
        let args = serde_json::to_vec(&step.arguments).expect("validated finite JSON");
        hash.update((args.len() as u64).to_le_bytes());
        hash.update(&args);
        hash.update(policy.budget.work_bytes.to_le_bytes());
        hash.update((policy.budget.result_bytes as u64).to_le_bytes());
        hash.update(policy.registry_generation.to_le_bytes());
        hash.update(policy.authority_binding);
        hash.update(policy.runtime_binding);
        hash.update([u8::from(policy.task_result_eligible)]);
        hash.update(policy.footprint.fingerprint());
        plans.push(NodePlan {
            coordinate,
            tool: step.tool,
            arguments: step.arguments.clone(),
            budget: policy.budget,
            footprint: policy.footprint,
            registry_generation: policy.registry_generation,
            authority_binding: policy.authority_binding,
            runtime_binding: policy.runtime_binding,
            task_result_eligible: policy.task_result_eligible,
            replay_identity: hash.finalize().into(),
        });
    }
    Ok(plans)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// Aggregate live/retained Job metadata bound, including exposed and hidden Jobs.
    pub max_retained_jobs: usize,
    /// User-visible Jobs addressable through `list`/`get` after exposure.
    pub max_exposed_jobs: usize,
    /// Unexposed Jobs owned by optimistic calls or a host transport delivery ledger.
    /// This is independent of replay-identity tombstones.
    pub max_hidden_jobs: usize,
    pub max_active_nodes: usize,
    pub max_hidden_replays: usize,
    pub max_spool_bytes: u64,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            max_retained_jobs: DEFAULT_MAX_RETAINED_JOBS,
            max_exposed_jobs: DEFAULT_MAX_EXPOSED_JOBS,
            max_hidden_jobs: DEFAULT_MAX_HIDDEN_JOBS,
            max_active_nodes: DEFAULT_MAX_ACTIVE_NODES,
            max_hidden_replays: MAX_HIDDEN_REQUEST_REPLAYS,
            max_spool_bytes: DEFAULT_SESSION_SPOOL,
        }
    }
}
impl Limits {
    pub fn validate(self) -> Result<Self, Error> {
        if self.max_retained_jobs == 0
            || self.max_retained_jobs > MAX_RETAINED_JOBS
            || self.max_exposed_jobs == 0
            || self.max_exposed_jobs > self.max_retained_jobs
            || self.max_hidden_jobs == 0
            || self.max_hidden_jobs > self.max_retained_jobs
            || self.max_active_nodes == 0
            || self.max_active_nodes > 1024
            || self.max_hidden_replays == 0
            || self.max_hidden_replays > 65_536
            || self.max_spool_bytes == 0
            || self.max_spool_bytes > MAX_SESSION_SPOOL
        {
            return Err(Error::InvalidLimits);
        }
        Ok(self)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct RuntimeAdmission<'a> {
    pub id: &'a str,
    pub runtime_session: &'a str,
    pub epoch: u64,
    pub admission_sequence: u64,
    pub width: usize,
    pub exposed: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct ReservedExposedRuntimeAdmission<'a> {
    pub id: &'a str,
    pub runtime_session: &'a str,
    pub epoch: u64,
    pub admission_sequence: u64,
    pub width: usize,
}

#[derive(Clone, Copy, Debug)]
pub struct TaskAdmission<'a> {
    pub id: &'a str,
    pub task: &'a TaskIdentity,
    pub admission_sequence: u64,
    pub width: usize,
    pub exposed: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct ReservedExposedTaskAdmission<'a> {
    pub id: &'a str,
    pub task: &'a TaskIdentity,
    pub admission_sequence: u64,
    pub width: usize,
}

#[derive(Clone, Copy, Debug)]
pub struct StartRequest {
    pub coordinate: usize,
    pub claimed_sequence: u64,
    pub source_live: bool,
    pub launch_authorized: bool,
    pub sequence: u64,
}

#[derive(Clone, Copy, Debug)]
pub struct ProcessSettlement {
    pub coordinate: usize,
    pub expected: NodeState,
    pub terminal: ProcessTerminal,
    pub status: Status,
    pub sequence: u64,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    InvalidLimits,
    InvalidPlans,
    DuplicateId,
    NotFound,
    NotExposed,
    Capacity,
    Busy,
    Poisoned,
    InvalidOutput,
    OutputMissing,
    OutputReleased,
    OutputFailure,
    ReplayMismatch,
    ReplayUnknown,
    ReplayRejected,
    RestartNotTerminal,
    Job(JobError),
}
impl From<JobError> for Error {
    fn from(error: JobError) -> Self {
        Self::Job(error)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReplayDisposition {
    Claimed,
    Ready,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReplayRecord {
    pub key: [u8; 32],
    pub semantic_hash: [u8; 32],
    pub job_id: String,
    pub disposition: ReplayDisposition,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReplayPersist {
    Created,
    Existing(ReplayRecord),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReplayDecision {
    New { key: [u8; 32], job_id: String },
    Existing { job_id: String },
    Unknown,
}

#[derive(Clone, Debug)]
struct ReplayEntry {
    record: ReplayRecord,
}

pub fn replay_key(namespace: &str, request_id: &str) -> Result<[u8; 32], Error> {
    if namespace.is_empty()
        || request_id.is_empty()
        || namespace.len() > 256
        || request_id.len() > 256
        || namespace.chars().any(char::is_control)
        || request_id.chars().any(char::is_control)
    {
        return Err(Error::InvalidPlans);
    }
    let mut hash = Sha256::new();
    hash.update(b"aw-tools-replay-key-v1\0");
    for value in [namespace, request_id] {
        hash.update((value.len() as u64).to_le_bytes());
        hash.update(value.as_bytes());
    }
    Ok(hash.finalize().into())
}

pub fn semantic_hash(graph: &Graph, plans: &[NodePlan]) -> Result<[u8; 32], Error> {
    if graph.steps().len() != plans.len()
        || plans.iter().enumerate().any(|(index, plan)| {
            plan.coordinate != index + 1 || plan.tool != graph.steps()[index].tool
        })
    {
        return Err(Error::InvalidPlans);
    }
    let mut hash = Sha256::new();
    hash.update(b"aw-tools-job-semantic-v1\0");
    hash.update(graph.fingerprint());
    for plan in plans {
        hash.update(plan.replay_identity());
    }
    Ok(hash.finalize().into())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum OutputStream {
    Result,
    Stdout,
    Stderr,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct OutputOwner(pub [u8; 32]);
impl OutputOwner {
    pub fn new(value: [u8; 32]) -> Result<Self, Error> {
        if value == [0; 32] {
            return Err(Error::InvalidOutput);
        }
        Ok(Self(value))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutputPage {
    pub offset: u64,
    pub bytes: Vec<u8>,
    pub next_offset: Option<u64>,
    pub writer_finished: bool,
    /// Retention truncation is observability only; it never mutates Graph
    /// completion/incomplete semantics.
    pub truncated: bool,
    pub total_bytes: u64,
    pub retained_bytes: u64,
}

/// Single raw stream addressing model shared by get/read and hosts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutputPosition {
    Absolute(u64),
    Tail(u64),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OutputAck {
    pub changed: bool,
    pub released: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OutputOwnerState {
    pub through: u64,
    pub acknowledged: bool,
    pub released: bool,
}

enum OutputStore {
    File(BoundedFileSpool),
    Memory(BoundedMemorySpool),
}
impl OutputStore {
    fn discard_if_full(&mut self, seen: u64) -> Result<bool, Error> {
        match self {
            Self::File(spool) => spool.discard_if_full(seen),
            Self::Memory(spool) => spool.discard_if_full(seen),
        }
        .map_err(|_| Error::OutputFailure)
    }
    fn observe_after_failure(&mut self, seen: u64) -> Result<(), Error> {
        match self {
            Self::File(spool) => spool.observe_after_failure(seen),
            Self::Memory(spool) => spool.observe_after_failure(seen),
        }
        .map_err(|_| Error::OutputFailure)
    }
    fn append(&mut self, bytes: &[u8]) -> Result<usize, Error> {
        match self {
            Self::File(spool) => spool.append(bytes),
            Self::Memory(spool) => spool.append(bytes),
        }
        .map_err(|_| Error::OutputFailure)
    }
    fn mark_writer_failed(&mut self) {
        match self {
            Self::File(spool) => spool.mark_writer_failed(),
            Self::Memory(spool) => spool.mark_writer_failed(),
        }
    }
    fn finish_writer(&mut self) -> Result<(), Error> {
        match self {
            Self::File(spool) => spool.finish_writer(),
            Self::Memory(spool) => spool.finish_writer(),
        }
        .map_err(|_| Error::OutputFailure)
    }
    fn summary(&self) -> crate::task::StreamSummary {
        match self {
            Self::File(spool) => spool.summary(),
            Self::Memory(spool) => spool.summary(),
        }
    }
    fn retained_bytes(&self) -> u64 {
        match self {
            Self::File(spool) => spool.retained_bytes(),
            Self::Memory(spool) => spool.retained_bytes(),
        }
    }
    fn read_retained_page(&mut self, offset: u64, limit: usize) -> Result<Vec<u8>, Error> {
        match self {
            Self::File(spool) => spool.read_retained_page(offset, limit),
            Self::Memory(spool) => spool.read_retained_page(offset, limit),
        }
        .map_err(|_| Error::OutputFailure)
    }
    fn compact_retained(&mut self, keep: u64) -> Result<u64, Error> {
        match self {
            Self::File(spool) => spool.compact_retained(keep),
            Self::Memory(spool) => spool.compact_retained(keep),
        }
        .map_err(|_| Error::OutputFailure)
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct OwnerAck {
    through: u64,
    acknowledged: bool,
    /// Only the continuously delivered [0, delivered_through) prefix is ACKable.
    delivered_through: u64,
    delivered_once: bool,
}

struct OwnedOutput {
    spool: OutputStore,
    gate: Arc<SpoolWriters>,
    owners: BTreeMap<OutputOwner, OwnerAck>,
    writer_finished: bool,
    released: bool,
    #[cfg(feature = "async")]
    change: Option<watch::Sender<u64>>,
}
impl OwnedOutput {
    #[cfg(feature = "async")]
    fn signal(&self) {
        if let Some(change) = &self.change {
            change.send_modify(|revision| *revision = revision.wrapping_add(1));
        }
    }

    fn owners(
        owners: impl IntoIterator<Item = OutputOwner>,
    ) -> Result<BTreeMap<OutputOwner, OwnerAck>, Error> {
        let mut owner_set = BTreeMap::new();
        for owner in owners {
            if owner.0 == [0; 32] || owner_set.insert(owner, OwnerAck::default()).is_some() {
                return Err(Error::InvalidOutput);
            }
        }
        if owner_set.is_empty() || owner_set.len() > 16 {
            return Err(Error::InvalidOutput);
        }
        Ok(owner_set)
    }

    fn new_with_limit(
        file: File,
        quota: Arc<Mutex<SessionQuota>>,
        owners: impl IntoIterator<Item = OutputOwner>,
        limit: u64,
    ) -> Result<Self, Error> {
        let owner_set = Self::owners(owners)?;
        let gate = Arc::new(SpoolWriters::default());
        let spool = BoundedFileSpool::new_with_limit(file, quota, gate.clone(), limit)
            .map(OutputStore::File)
            .map_err(|_| Error::OutputFailure)?;
        Ok(Self {
            spool,
            gate,
            owners: owner_set,
            writer_finished: false,
            released: false,
            #[cfg(feature = "async")]
            change: None,
        })
    }

    fn new_memory(
        quota: Arc<Mutex<SessionQuota>>,
        owners: impl IntoIterator<Item = OutputOwner>,
        limit: u64,
    ) -> Result<Self, Error> {
        let owner_set = Self::owners(owners)?;
        let gate = Arc::new(SpoolWriters::default());
        let spool = BoundedMemorySpool::new_with_limit(quota, gate.clone(), limit)
            .map(OutputStore::Memory)
            .map_err(|_| Error::OutputFailure)?;
        Ok(Self {
            spool,
            gate,
            owners: owner_set,
            writer_finished: false,
            released: false,
            #[cfg(feature = "async")]
            change: None,
        })
    }

    fn new_reserved(
        quota: Arc<Mutex<SessionQuota>>,
        owners: impl IntoIterator<Item = OutputOwner>,
        capacity: u64,
    ) -> Result<Self, Error> {
        let owner_set = Self::owners(owners)?;
        let gate = Arc::new(SpoolWriters::default());
        let spool = BoundedMemorySpool::new_reserved(quota, gate.clone(), capacity)
            .map(OutputStore::Memory)
            .map_err(|_| Error::Capacity)?;
        Ok(Self {
            spool,
            gate,
            owners: owner_set,
            writer_finished: false,
            released: false,
            #[cfg(feature = "async")]
            change: None,
        })
    }

    fn finish_writer(&mut self) -> Result<(), Error> {
        if self.writer_finished {
            return Ok(());
        }
        // Closing the underlying writer lease is a lifetime transition even
        // when the write itself failed. Publish quiescence independently from
        // output success so retention/recovery never strands a terminal Job.
        let spool_result = self.spool.finish_writer();
        self.gate.seal();
        if !self.gate.is_idle() {
            return Err(Error::OutputFailure);
        }
        self.writer_finished = true;
        let release_result = self.maybe_release();
        #[cfg(feature = "async")]
        self.signal();
        spool_result?;
        let _ = release_result?;
        Ok(())
    }

    /// A node proven never launched cannot produce further bytes. Output
    /// payload failure is therefore not a reason to keep its execution permit
    /// live: once the writer lifetime is closed, terminal Skip may proceed and
    /// the failed payload remains independently unreadable.
    fn finish_unlaunched(&mut self) -> Result<(), Error> {
        match self.finish_writer() {
            Ok(()) => Ok(()),
            Err(Error::OutputFailure) if self.writer_finished => Ok(()),
            Err(error) => Err(error),
        }
    }

    fn read_for_owner(
        &mut self,
        owner: OutputOwner,
        position: OutputPosition,
        limit: usize,
    ) -> Result<OutputPage, Error> {
        if !self.owners.contains_key(&owner) {
            return Err(Error::InvalidOutput);
        }
        self.read_page(position, limit)
    }

    /// Trusted host attestation *after* bounded final projection succeeds.
    /// The host must mark only the raw prefix represented by its response.
    /// Reading or peeking never creates ACKable delivery evidence.
    fn mark_delivered_for_owner(
        &mut self,
        owner: OutputOwner,
        start: u64,
        count: usize,
    ) -> Result<(), Error> {
        if self.released {
            return Err(Error::OutputReleased);
        }
        let end = start
            .checked_add(count as u64)
            .ok_or(Error::InvalidOutput)?;
        if end > self.spool.retained_bytes() {
            return Err(Error::InvalidOutput);
        }
        let lease = self.owners.get_mut(&owner).ok_or(Error::InvalidOutput)?;
        if start <= lease.delivered_through {
            lease.delivered_through = lease.delivered_through.max(end);
            lease.delivered_once = true;
        }
        Ok(())
    }

    fn read_page(&mut self, position: OutputPosition, limit: usize) -> Result<OutputPage, Error> {
        if limit == 0 || limit > MAX_SPOOL_PAGE_BYTES {
            return Err(Error::InvalidOutput);
        }
        if self.released {
            return Err(Error::OutputReleased);
        }
        let summary = self.spool.summary();
        let offset = match position {
            OutputPosition::Absolute(offset) => offset,
            // If the requested tail exceeds a page, retain the newest bytes.
            OutputPosition::Tail(count) => summary
                .retained_bytes
                .saturating_sub(count.min(limit as u64)),
        };
        if offset > summary.retained_bytes || offset.checked_add(limit as u64).is_none() {
            return Err(Error::InvalidOutput);
        }
        let bytes = self.spool.read_retained_page(offset, limit)?;
        let next = offset + bytes.len() as u64;
        Ok(OutputPage {
            offset,
            bytes,
            next_offset: (next < summary.retained_bytes).then_some(next),
            writer_finished: self.writer_finished,
            truncated: summary.truncated,
            total_bytes: summary.total_bytes,
            retained_bytes: summary.retained_bytes,
        })
    }

    /// Released output retains logical ACK history, not an open grant.
    /// A replay may repeat only this exact owner's previously ACKed prefix.
    fn released_ack(&self, owner: OutputOwner, through: u64) -> Result<Option<OutputAck>, Error> {
        if !self.released {
            return Ok(None);
        }
        let ack = self.owners.get(&owner).ok_or(Error::InvalidOutput)?;
        if !ack.acknowledged || through > ack.through {
            return Err(Error::InvalidOutput);
        }
        Ok(Some(OutputAck {
            changed: false,
            released: true,
        }))
    }

    fn maybe_release(&mut self) -> Result<bool, Error> {
        if self.released || !self.writer_finished {
            return Ok(false);
        }
        let retained = self.spool.retained_bytes();
        if !self
            .owners
            .values()
            .all(|ack| ack.acknowledged && ack.through >= retained)
        {
            return Ok(false);
        }
        self.spool.compact_retained(0)?;
        self.released = true;
        #[cfg(feature = "async")]
        self.signal();
        Ok(true)
    }
}

/// Host-side writer for one already-admitted Job output stream. It has no
/// paging/ACK authority and cannot create streams; the JobManager remains the
/// sole output owner while an adapter-specific drain worker feeds bytes.
pub struct ShellOutputFiles {
    pub stdout: File,
    pub stderr: File,
    pub stream_capacity: u64,
}

#[derive(Clone)]
pub struct OutputWriter {
    output: Arc<Mutex<OwnedOutput>>,
}
impl OutputWriter {
    pub fn discard_if_full(&self, seen: u64) -> Result<bool, Error> {
        let mut output = self.output.lock().map_err(|_| Error::Poisoned)?;
        #[cfg(feature = "async")]
        let was_truncated = output.spool.summary().truncated;
        let discarded = output.spool.discard_if_full(seen)?;
        #[cfg(feature = "async")]
        if !was_truncated && output.spool.summary().truncated {
            // Only the first truncation changes read readiness. Later
            // discarded chunks update totals without waking every waiter.
            output.signal();
        }
        Ok(discarded)
    }

    pub fn append(&self, bytes: &[u8]) -> Result<usize, Error> {
        let mut output = self.output.lock().map_err(|_| Error::Poisoned)?;
        if output.writer_finished || output.released {
            return Err(Error::InvalidOutput);
        }
        #[cfg(feature = "async")]
        let was_truncated = output.spool.summary().truncated;
        let result = output.spool.append(bytes);
        #[cfg(feature = "async")]
        if result.is_err()
            || result.as_ref().is_ok_and(|written| *written != 0)
            || (!was_truncated && output.spool.summary().truncated)
        {
            // A failed write may publish a new failure state without bytes.
            // Later dropped chunks update totals without waking every reader.
            output.signal();
        }
        result
    }

    pub fn observe_after_failure(&self, seen: u64) -> Result<(), Error> {
        let mut output = self.output.lock().map_err(|_| Error::Poisoned)?;
        #[cfg(feature = "async")]
        let was_truncated = output.spool.summary().truncated;
        output.spool.observe_after_failure(seen)?;
        #[cfg(feature = "async")]
        if !was_truncated && output.spool.summary().truncated {
            output.signal();
        }
        Ok(())
    }

    pub fn mark_failed(&self) -> Result<(), Error> {
        let mut output = self.output.lock().map_err(|_| Error::Poisoned)?;
        output.spool.mark_writer_failed();
        #[cfg(feature = "async")]
        output.signal();
        Ok(())
    }

    pub fn finish(&self) -> Result<(), Error> {
        self.output
            .lock()
            .map_err(|_| Error::Poisoned)?
            .finish_writer()
    }
}

#[derive(Clone, Debug)]
pub struct TaskProjection {
    pub info: TaskInfo,
    pub result_owner: OutputOwner,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FiniteJobKind {
    Process,
    Graph,
}

#[derive(Clone, Debug)]
pub struct Snapshot {
    pub id: String,
    pub label: String,
    pub lifecycle: Lifecycle,
    pub stop_reason: Option<StopReason>,
    /// An unreconciled host-journal transition makes the Job outcome unknown
    /// without claiming the underlying Process or Graph has settled.
    pub journal_unknown: bool,
    pub sequence: u64,
    pub finished: usize,
    pub total: usize,
    pub summary: Option<(Primary, bool, bool)>,
    pub detail: Option<String>,
}

impl Snapshot {
    pub fn is_known_terminal(&self) -> bool {
        self.lifecycle == Lifecycle::Terminal && !self.journal_unknown
    }

    /// Canonical user-facing Job status. Terminal uncertainty dominates a
    /// cancellation/revocation request because intent is not proof that an
    /// already-launched external effect stopped.
    pub fn status_label(&self) -> &'static str {
        if self.journal_unknown {
            return "unknown";
        }
        match self.lifecycle {
            Lifecycle::Running => "running",
            Lifecycle::Cancelling => "cancelling",
            Lifecycle::Revoking => "revoking",
            Lifecycle::Terminal => {
                let Some((primary, _, _)) = self.summary else {
                    return "unknown";
                };
                if primary == Primary::Unknown {
                    return "unknown";
                }
                match self.stop_reason {
                    Some(StopReason::Cancelled) => "cancelled",
                    Some(StopReason::Revoked) => "revoked",
                    None => primary.as_str(),
                }
            }
        }
    }

    pub fn list_row(&self) -> String {
        let mut row = format!("{} {} {}", self.id, self.label, self.status_label());
        if self.total > 1 && self.lifecycle != Lifecycle::Terminal {
            row.push_str(&format!(" {}/{}", self.finished, self.total));
        }
        if let Some(detail) = &self.detail {
            row.push(' ');
            row.push_str(detail);
        }
        row
    }

    pub fn header(&self) -> String {
        let mut header = format!("{} {}", self.status_label(), self.label);
        if self.total > 1 {
            header.push_str(&format!(" {}/{}", self.finished, self.total));
        }
        if let Some(detail) = &self.detail {
            header.push(' ');
            header.push_str(detail);
        }
        header
    }
}

#[derive(Clone, Debug)]
pub struct ListPage {
    pub jobs: Vec<Snapshot>,
    pub next_offset: Option<usize>,
}

/// Stable continuation position in the immutable exposure ordering. The
/// adapter authenticates/encodes this value for callers; it is not authority.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListCursor {
    created_order: u64,
    id: String,
}

impl ListCursor {
    pub fn from_trusted_parts(created_order: u64, id: String) -> Result<Self, Error> {
        if created_order == 0 || id.is_empty() || id.len() > 256 || id.chars().any(char::is_control)
        {
            return Err(Error::InvalidOutput);
        }
        Ok(Self { created_order, id })
    }

    pub fn created_order(&self) -> u64 {
        self.created_order
    }

    pub fn id(&self) -> &str {
        &self.id
    }
}

#[derive(Clone, Debug)]
pub struct CursorListPage {
    pub jobs: Vec<Snapshot>,
    pub next_cursor: Option<ListCursor>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RunnerLoad {
    pub job_active: usize,
    pub global_active: usize,
    pub global_limit: usize,
}
impl RunnerLoad {
    pub fn global_free(self) -> usize {
        self.global_limit.saturating_sub(self.global_active)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Visibility {
    Hidden,
    ReservedExposed,
    Exposed,
}
impl Visibility {
    fn reserves_exposed(self) -> bool {
        !matches!(self, Self::Hidden)
    }
    fn is_exposed(self) -> bool {
        matches!(self, Self::Exposed)
    }
}

struct ManagedJob {
    job: Job,
    plans: Vec<NodePlan>,
    projection_kind: Option<FiniteJobKind>,
    /// Public compare-and-act revision. Distinct from Job's internal journal
    /// sequence because a control effect (for example stdin input) can mutate
    /// live execution without a lifecycle transition.
    revision: u64,
    visibility: Visibility,
    created_order: u64,
    transition_inflight: Option<Transition>,
    outputs: BTreeMap<(usize, OutputStream), Arc<Mutex<OwnedOutput>>>,
    process_terminals: BTreeMap<usize, ProcessTerminal>,
    task_binding: Option<TaskIdentity>,
    #[cfg(feature = "async")]
    change: watch::Sender<u64>,
}

impl ManagedJob {
    #[cfg(feature = "async")]
    fn signal(&self) {
        self.change
            .send_modify(|revision| *revision = revision.wrapping_add(1));
    }

    fn writers_quiesced(&self) -> bool {
        self.outputs
            .values()
            .all(|output| output.lock().is_ok_and(|output| output.writer_finished))
    }

    fn retention_releasable(&self) -> bool {
        if !self.visibility.is_exposed()
            || self.job.lifecycle() != Lifecycle::Terminal
            || self.job.journal_ambiguous()
            || self.transition_inflight.is_some()
            || !self.writers_quiesced()
        {
            return false;
        }
        self.outputs.values().all(|output| {
            output
                .lock()
                .is_ok_and(|output| output.released || output.spool.retained_bytes() == 0)
        })
    }
}

struct State {
    jobs: HashMap<String, ManagedJob>,
    /// Capacity-only pre-admission reservations. They own aggregate and class
    /// quota but intentionally contain no Graph/lifecycle state.
    pre_admissions: HashMap<String, Visibility>,
    next_order: u64,
    active_nodes: usize,
    replays: HashMap<[u8; 32], ReplayEntry>,
    replay_inflight: BTreeSet<[u8; 32]>,
}

pub struct JobManager {
    limits: Limits,
    state: Mutex<State>,
    spool_quota: Arc<Mutex<SessionQuota>>,
}

impl JobManager {
    pub fn new(limits: Limits) -> Result<Self, Error> {
        let limits = limits.validate()?;
        Ok(Self {
            limits,
            state: Mutex::new(State {
                jobs: HashMap::new(),
                pre_admissions: HashMap::new(),
                next_order: 1,
                active_nodes: 0,
                replays: HashMap::new(),
                replay_inflight: BTreeSet::new(),
            }),
            spool_quota: Arc::new(Mutex::new(SessionQuota::new(limits.max_spool_bytes))),
        })
    }

    fn validate_plans(job: &Job, plans: &[NodePlan]) -> Result<(), Error> {
        if plans.len() != job.total()
            || plans.iter().enumerate().any(|(index, plan)| {
                plan.coordinate != index + 1 || job.graph().steps()[index].tool != plan.tool
            })
        {
            return Err(Error::InvalidPlans);
        }
        Ok(())
    }

    fn exposed_jobs(state: &State) -> usize {
        state
            .jobs
            .values()
            .filter(|managed| managed.visibility.reserves_exposed())
            .count()
            + state
                .pre_admissions
                .values()
                .filter(|visibility| visibility.reserves_exposed())
                .count()
    }

    fn hidden_jobs(state: &State) -> usize {
        state
            .jobs
            .values()
            .filter(|managed| managed.visibility == Visibility::Hidden)
            .count()
            + state
                .pre_admissions
                .values()
                .filter(|visibility| **visibility == Visibility::Hidden)
                .count()
    }

    fn retained_slots(state: &State) -> usize {
        state.jobs.len().saturating_add(state.pre_admissions.len())
    }

    fn ensure_exposed_capacity(
        &self,
        state: &mut State,
        protected: Option<&str>,
        additional: usize,
    ) -> bool {
        if additional == 0 {
            return true;
        }
        loop {
            let exposed_ok = Self::exposed_jobs(state)
                .checked_add(additional)
                .is_some_and(|count| count <= self.limits.max_exposed_jobs);
            let aggregate_ok = Self::retained_slots(state)
                .checked_add(additional)
                .is_some_and(|count| count <= self.limits.max_retained_jobs);
            if exposed_ok && aggregate_ok {
                return true;
            }
            if !Self::prune_one_released_terminal(state, protected) {
                return false;
            }
        }
    }

    fn prune_one_released_terminal(state: &mut State, protected: Option<&str>) -> bool {
        let candidate = state
            .jobs
            .iter()
            .filter(|(id, managed)| {
                protected.is_none_or(|protected| id.as_str() != protected)
                    && managed.retention_releasable()
                    && !state
                        .replays
                        .values()
                        .any(|entry| entry.record.job_id == id.as_str())
            })
            .min_by(|(id_a, a), (id_b, b)| {
                a.created_order
                    .cmp(&b.created_order)
                    .then_with(|| id_a.cmp(id_b))
            })
            .map(|(id, _)| id.clone());
        if let Some(id) = candidate {
            state.jobs.remove(&id);
            true
        } else {
            false
        }
    }

    fn insert(
        &self,
        id: &str,
        job: Job,
        plans: Vec<NodePlan>,
        visibility: Visibility,
        task_binding: Option<TaskIdentity>,
        protected: Option<&str>,
    ) -> Result<(), Error> {
        Self::validate_plans(&job, &plans)?;
        let mut state = self.state.lock().map_err(|_| Error::Poisoned)?;
        if let Some(source_id) = protected {
            let source = state.jobs.get(source_id).ok_or(Error::NotFound)?;
            if source.job.journal_ambiguous() {
                return Err(Error::ReplayUnknown);
            }
            if source.job.lifecycle() != Lifecycle::Terminal || source.transition_inflight.is_some()
            {
                return Err(Error::RestartNotTerminal);
            }
        }
        if state.jobs.contains_key(id) {
            return Err(Error::DuplicateId);
        }
        if visibility.reserves_exposed() {
            if !self.ensure_exposed_capacity(&mut state, protected, 1) {
                return Err(Error::Capacity);
            }
        } else if Self::hidden_jobs(&state) >= self.limits.max_hidden_jobs
            || Self::retained_slots(&state) >= self.limits.max_retained_jobs
        {
            // Hidden host/transport work must never reclaim exposed history to admit
            // itself. The host may explicitly expire/release state and retry.
            return Err(Error::Capacity);
        }
        let restored_active = job.active();
        if state.active_nodes.saturating_add(restored_active) > self.limits.max_active_nodes {
            return Err(Error::Capacity);
        }
        let created_order = state.next_order;
        state.next_order = state.next_order.checked_add(1).ok_or(Error::Capacity)?;
        state.jobs.insert(
            id.to_owned(),
            ManagedJob {
                job,
                plans,
                projection_kind: None,
                revision: 1,
                visibility,
                created_order,
                transition_inflight: None,
                outputs: BTreeMap::new(),
                process_terminals: BTreeMap::new(),
                task_binding,
                #[cfg(feature = "async")]
                change: watch::channel(0u64).0,
            },
        );
        state.active_nodes += restored_active;
        Ok(())
    }

    /// Reserve aggregate + hidden/exposed-class metadata capacity before a
    /// host runs any paid Graph admission policy. This creates no Job and owns
    /// no lifecycle/output state.
    pub fn reserve_task_admission(&self, id: &str, exposed: bool) -> Result<(), Error> {
        if id.is_empty() || id.len() > 256 || id.chars().any(char::is_control) {
            return Err(Error::InvalidPlans);
        }
        let visibility = if exposed {
            Visibility::ReservedExposed
        } else {
            Visibility::Hidden
        };
        let mut state = self.state.lock().map_err(|_| Error::Poisoned)?;
        if state.jobs.contains_key(id) || state.pre_admissions.contains_key(id) {
            return Err(Error::DuplicateId);
        }
        if visibility.reserves_exposed() {
            if !self.ensure_exposed_capacity(&mut state, None, 1) {
                return Err(Error::Capacity);
            }
        } else if Self::hidden_jobs(&state) >= self.limits.max_hidden_jobs
            || Self::retained_slots(&state) >= self.limits.max_retained_jobs
        {
            return Err(Error::Capacity);
        }
        state.pre_admissions.insert(id.to_owned(), visibility);
        Ok(())
    }

    pub fn release_task_admission(&self, id: &str) -> Result<(), Error> {
        let mut state = self.state.lock().map_err(|_| Error::Poisoned)?;
        state
            .pre_admissions
            .remove(id)
            .map(|_| ())
            .ok_or(Error::NotFound)
    }

    /// Finalize one capacity-only reservation with the now-sealed Graph.
    /// Capacity is not checked a second time; all semantic/plan validation still
    /// happens here before the reservation is consumed.
    pub fn admit_pre_reserved_task(
        &self,
        graph: Graph,
        plans: Vec<NodePlan>,
        admission: TaskAdmission<'_>,
    ) -> Result<(), Error> {
        let job = Job::after_task_intent(
            graph,
            admission.task,
            admission.id,
            admission.admission_sequence,
            admission.width,
        )?;
        Self::validate_plans(&job, &plans)?;
        let expected = if admission.exposed {
            Visibility::ReservedExposed
        } else {
            Visibility::Hidden
        };
        let mut state = self.state.lock().map_err(|_| Error::Poisoned)?;
        if state.jobs.contains_key(admission.id) {
            return Err(Error::DuplicateId);
        }
        if state.pre_admissions.get(admission.id).copied() != Some(expected) {
            return Err(Error::InvalidPlans);
        }
        let restored_active = job.active();
        if state.active_nodes.saturating_add(restored_active) > self.limits.max_active_nodes {
            return Err(Error::Capacity);
        }
        let created_order = state.next_order;
        let next_order = state.next_order.checked_add(1).ok_or(Error::Capacity)?;
        state.pre_admissions.remove(admission.id);
        state.next_order = next_order;
        state.jobs.insert(
            admission.id.to_owned(),
            ManagedJob {
                job,
                plans,
                projection_kind: None,
                revision: 1,
                visibility: expected,
                created_order,
                transition_inflight: None,
                outputs: BTreeMap::new(),
                process_terminals: BTreeMap::new(),
                task_binding: Some(admission.task.clone()),
                #[cfg(feature = "async")]
                change: watch::channel(0u64).0,
            },
        );
        state.active_nodes += restored_active;
        Ok(())
    }

    pub fn admit_runtime(
        &self,
        graph: Graph,
        plans: Vec<NodePlan>,
        admission: RuntimeAdmission<'_>,
    ) -> Result<(), Error> {
        let job = Job::after_runtime_intent(
            graph,
            admission.runtime_session,
            admission.id,
            admission.epoch,
            admission.admission_sequence,
            admission.width,
        )?;
        self.insert(
            admission.id,
            job,
            plans,
            if admission.exposed {
                Visibility::Exposed
            } else {
                Visibility::Hidden
            },
            None,
            None,
        )
    }

    /// Runtime-scoped counterpart to `admit_reserved_exposed_task`. The
    /// provisional Job owns exposed-class capacity but remains absent from
    /// `get`/`list` until the host commits its external admission/replay fence.
    pub fn admit_reserved_exposed_runtime(
        &self,
        graph: Graph,
        plans: Vec<NodePlan>,
        admission: ReservedExposedRuntimeAdmission<'_>,
    ) -> Result<(), Error> {
        let job = Job::after_runtime_intent(
            graph,
            admission.runtime_session,
            admission.id,
            admission.epoch,
            admission.admission_sequence,
            admission.width,
        )?;
        self.insert(
            admission.id,
            job,
            plans,
            Visibility::ReservedExposed,
            None,
            None,
        )
    }

    pub fn admit_task(
        &self,
        graph: Graph,
        plans: Vec<NodePlan>,
        admission: TaskAdmission<'_>,
    ) -> Result<(), Error> {
        let job = Job::after_task_intent(
            graph,
            admission.task,
            admission.id,
            admission.admission_sequence,
            admission.width,
        )?;
        self.insert(
            admission.id,
            job,
            plans,
            if admission.exposed {
                Visibility::Exposed
            } else {
                Visibility::Hidden
            },
            Some(admission.task.clone()),
            None,
        )
    }

    /// Reserve an exposed-class slot while keeping the provisional Job out of
    /// `get`/`list` until a host replay/admission fence is committed. Publishing
    /// through `expose` is then infallible with respect to class capacity.
    pub fn admit_reserved_exposed_task(
        &self,
        graph: Graph,
        plans: Vec<NodePlan>,
        admission: ReservedExposedTaskAdmission<'_>,
    ) -> Result<(), Error> {
        let job = Job::after_task_intent(
            graph,
            admission.task,
            admission.id,
            admission.admission_sequence,
            admission.width,
        )?;
        self.insert(
            admission.id,
            job,
            plans,
            Visibility::ReservedExposed,
            Some(admission.task.clone()),
            None,
        )
    }

    /// Reconstruct from an authoritative transition sequence supplied by the
    /// host for the same live execution session. This does not define or imply
    /// cross-process Job resurrection.
    pub fn restore_runtime(
        &self,
        graph: Graph,
        records: impl IntoIterator<Item = Transition>,
        plans: Vec<NodePlan>,
        admission: RuntimeAdmission<'_>,
    ) -> Result<(), Error> {
        let job = Job::from_runtime_journal(
            graph,
            admission.runtime_session,
            admission.id,
            admission.epoch,
            admission.admission_sequence,
            admission.width,
            records,
        )?;
        self.insert(
            admission.id,
            job,
            plans,
            if admission.exposed {
                Visibility::Exposed
            } else {
                Visibility::Hidden
            },
            None,
            None,
        )
    }

    /// Task-scoped counterpart to `restore_runtime`; valid only under the
    /// host's authoritative same-session recovery/reattachment contract.
    pub fn restore_task(
        &self,
        graph: Graph,
        records: impl IntoIterator<Item = Transition>,
        plans: Vec<NodePlan>,
        admission: TaskAdmission<'_>,
    ) -> Result<(), Error> {
        let job = Job::from_task_journal(
            graph,
            admission.task,
            admission.id,
            admission.admission_sequence,
            admission.width,
            records,
        )?;
        self.insert(
            admission.id,
            job,
            plans,
            if admission.exposed {
                Visibility::Exposed
            } else {
                Visibility::Hidden
            },
            Some(admission.task.clone()),
            None,
        )
    }

    /// Atomically move an already-retained hidden Job into the reserved
    /// exposed class without making it visible. This supports optimistic hosts
    /// that begin hidden work and only later commit to a durable user-visible
    /// representation. The Job already counts against aggregate retention, so
    /// this transition reserves only exposed-class capacity.
    pub fn reserve_exposed(&self, id: &str) -> Result<(), Error> {
        let mut state = self.state.lock().map_err(|_| Error::Poisoned)?;
        let visibility = state.jobs.get(id).ok_or(Error::NotFound)?.visibility;
        match visibility {
            Visibility::Exposed | Visibility::ReservedExposed => return Ok(()),
            Visibility::Hidden => {}
        }
        while Self::exposed_jobs(&state) >= self.limits.max_exposed_jobs {
            if !Self::prune_one_released_terminal(&mut state, Some(id)) {
                return Err(Error::Capacity);
            }
        }
        state.jobs.get_mut(id).ok_or(Error::NotFound)?.visibility = Visibility::ReservedExposed;
        Ok(())
    }

    pub fn expose(&self, id: &str) -> Result<(), Error> {
        let mut state = self.state.lock().map_err(|_| Error::Poisoned)?;
        let visibility = state.jobs.get(id).ok_or(Error::NotFound)?.visibility;
        match visibility {
            Visibility::Exposed => return Ok(()),
            Visibility::ReservedExposed => {
                state.jobs.get_mut(id).ok_or(Error::NotFound)?.visibility = Visibility::Exposed;
                return Ok(());
            }
            Visibility::Hidden => {}
        }
        while Self::exposed_jobs(&state) >= self.limits.max_exposed_jobs {
            if !Self::prune_one_released_terminal(&mut state, Some(id)) {
                return Err(Error::Capacity);
            }
        }
        state.jobs.get_mut(id).ok_or(Error::NotFound)?.visibility = Visibility::Exposed;
        Ok(())
    }

    /// Set immutable user-facing finite Job semantics. This is projection
    /// metadata on the existing Job, never a second execution owner.
    pub fn set_finite_kind(&self, id: &str, kind: FiniteJobKind) -> Result<(), Error> {
        let mut state = self.state.lock().map_err(|_| Error::Poisoned)?;
        let managed = state.jobs.get_mut(id).ok_or(Error::NotFound)?;
        match managed.projection_kind {
            None => {
                managed.projection_kind = Some(kind);
                Ok(())
            }
            Some(existing) if existing == kind => Ok(()),
            Some(_) => Err(Error::ReplayMismatch),
        }
    }

    pub fn finite_kind(&self, id: &str) -> Result<FiniteJobKind, Error> {
        let state = self.state.lock().map_err(|_| Error::Poisoned)?;
        state
            .jobs
            .get(id)
            .ok_or(Error::NotFound)?
            .projection_kind
            .ok_or(Error::InvalidPlans)
    }

    /// Atomically compare and reserve the next public revision before an
    /// effectful control leaves the JobManager. The caller must establish its
    /// replay identity before invoking this method.
    pub fn compare_and_bump_revision(&self, id: &str, expected: Option<u64>) -> Result<u64, Error> {
        let mut state = self.state.lock().map_err(|_| Error::Poisoned)?;
        let managed = state.jobs.get_mut(id).ok_or(Error::NotFound)?;
        if expected.is_some_and(|expected| expected != managed.revision) {
            return Err(Error::Job(JobError::StaleGeneration));
        }
        managed.revision = managed.revision.checked_add(1).ok_or(Error::Capacity)?;
        Ok(managed.revision)
    }

    fn snapshot(id: &str, managed: &ManagedJob) -> Snapshot {
        let label = if managed.job.total() == 1 {
            managed.job.graph().steps()[0].tool.name().to_owned()
        } else {
            "graph".into()
        };
        let detail = (managed.job.total() == 1)
            .then(|| managed.process_terminals.get(&1).copied())
            .flatten()
            .map(ProcessTerminal::status_detail);
        Snapshot {
            id: id.to_owned(),
            label,
            lifecycle: managed.job.lifecycle(),
            stop_reason: managed.job.stop_reason(),
            journal_unknown: managed.job.journal_ambiguous(),
            sequence: managed.revision,
            finished: managed.job.finished(),
            total: managed.job.total(),
            summary: managed.job.summary(),
            detail,
        }
    }

    pub fn pending_reattach(&self, id: &str) -> Result<Vec<(usize, NodeState)>, Error> {
        let state = self.state.lock().map_err(|_| Error::Poisoned)?;
        Ok(state
            .jobs
            .get(id)
            .ok_or(Error::NotFound)?
            .job
            .pending_reattach())
    }

    pub fn task_binding(&self, id: &str) -> Result<TaskIdentity, Error> {
        let state = self.state.lock().map_err(|_| Error::Poisoned)?;
        state
            .jobs
            .get(id)
            .ok_or(Error::NotFound)?
            .task_binding
            .clone()
            .ok_or(Error::InvalidPlans)
    }

    pub fn task_projection(&self, id: &str, source_live: bool) -> Result<TaskProjection, Error> {
        let state = self.state.lock().map_err(|_| Error::Poisoned)?;
        let managed = state.jobs.get(id).ok_or(Error::NotFound)?;
        let binding = managed.task_binding.clone().ok_or(Error::InvalidOutput)?;
        if managed.job.total() != 1 || !managed.plans[0].task_result_eligible {
            return Err(Error::InvalidOutput);
        }
        let disposition = if managed.job.journal_ambiguous() {
            TaskDisposition::Uncertain
        } else {
            match managed.job.lifecycle() {
                Lifecycle::Running | Lifecycle::Cancelling | Lifecycle::Revoking => {
                    match managed.job.state(1) {
                        Some(NodeState::Pending) => TaskDisposition::Queued,
                        Some(NodeState::Claimed { .. } | NodeState::Running { .. }) => {
                            TaskDisposition::Running
                        }
                        Some(NodeState::Terminal { status, .. }) => match status.primary {
                            Primary::Ok | Primary::Unchanged => TaskDisposition::Completed,
                            Primary::Fail | Primary::Mixed => TaskDisposition::Failed,
                            Primary::Unknown => TaskDisposition::Uncertain,
                            Primary::Skip => TaskDisposition::Cancelled,
                        },
                        None => return Err(Error::InvalidOutput),
                    }
                }
                Lifecycle::Terminal => {
                    let Some((primary, _, _)) = managed.job.summary() else {
                        return Err(Error::InvalidOutput);
                    };
                    if primary == Primary::Unknown {
                        TaskDisposition::Uncertain
                    } else if managed.job.stop_reason().is_some() {
                        TaskDisposition::Cancelled
                    } else {
                        match primary {
                            Primary::Ok | Primary::Unchanged => TaskDisposition::Completed,
                            Primary::Fail | Primary::Mixed => TaskDisposition::Failed,
                            Primary::Unknown => unreachable!("handled above"),
                            Primary::Skip => TaskDisposition::Cancelled,
                        }
                    }
                }
            }
        };
        let mut hash = Sha256::new();
        hash.update(b"aw-tools-task-result-owner-v1\0");
        hash.update(managed.job.binding());
        hash.update(1u64.to_le_bytes());
        let result_owner = OutputOwner::new(hash.finalize().into())?;
        let terminal = managed
            .process_terminals
            .get(&1)
            .copied()
            .map(TaskTerminalDetail::from);
        Ok(TaskProjection {
            info: TaskInfo {
                binding,
                generation: managed.job.sequence(),
                journal_sequence: managed.job.admission_sequence(),
                state: disposition,
                terminal,
                source_live,
            },
            result_owner,
        })
    }

    pub fn current_sequence(&self, id: &str) -> Result<u64, Error> {
        let state = self.state.lock().map_err(|_| Error::Poisoned)?;
        Ok(state.jobs.get(id).ok_or(Error::NotFound)?.job.sequence())
    }

    pub fn node_state(&self, id: &str, coordinate: usize) -> Result<NodeState, Error> {
        let state = self.state.lock().map_err(|_| Error::Poisoned)?;
        state
            .jobs
            .get(id)
            .ok_or(Error::NotFound)?
            .job
            .state(coordinate)
            .ok_or(Error::InvalidPlans)
    }

    /// Typed terminal process facts retained for one node. This is a
    /// read-only host projection; lifecycle/status classification remains owned
    /// by the shared manager. `None` means the valid node has no process
    /// terminal record (for example a native filesystem node or work not yet
    /// settled as a launched process).
    pub fn process_terminal(
        &self,
        id: &str,
        coordinate: usize,
    ) -> Result<Option<ProcessTerminal>, Error> {
        let state = self.state.lock().map_err(|_| Error::Poisoned)?;
        let managed = state.jobs.get(id).ok_or(Error::NotFound)?;
        if !(1..=managed.job.total()).contains(&coordinate) {
            return Err(Error::InvalidPlans);
        }
        Ok(managed.process_terminals.get(&coordinate).copied())
    }

    fn node_status_label(managed: &ManagedJob, coordinate: usize, state: NodeState) -> String {
        match state {
            NodeState::Pending => "pending".into(),
            NodeState::Claimed { .. } | NodeState::Running { .. } => "run".into(),
            NodeState::Terminal { status, .. } => {
                let mut label = status.primary.as_str().to_owned();
                if status.more {
                    label.push_str(" more");
                }
                if status.incomplete {
                    label.push_str(" incomplete");
                }
                if let Some(terminal) = managed.process_terminals.get(&coordinate).copied() {
                    label.push(' ');
                    label.push_str(&terminal.status_detail());
                }
                label
            }
        }
    }

    pub fn render_job_status(&self, id: &str) -> Result<String, Error> {
        let state = self.state.lock().map_err(|_| Error::Poisoned)?;
        let managed = state.jobs.get(id).ok_or(Error::NotFound)?;
        if !managed.visibility.is_exposed() {
            return Err(Error::NotExposed);
        }
        let snapshot = Self::snapshot(id, managed);
        let mut rendered = snapshot.header();
        if managed.job.total() == 1 {
            return Ok(rendered);
        }
        let mut start = 1usize;
        while start <= managed.job.total() {
            let current = managed.job.state(start).ok_or(Error::InvalidPlans)?;
            let label = Self::node_status_label(managed, start, current);
            let mut end = start;
            while end < managed.job.total() {
                let next = managed.job.state(end + 1).ok_or(Error::InvalidPlans)?;
                if Self::node_status_label(managed, end + 1, next) != label {
                    break;
                }
                end += 1;
            }
            rendered.push('\n');
            if start == end {
                rendered.push_str(&format!("{start} {label}"));
            } else {
                rendered.push_str(&format!("{start}-{end} {label}"));
            }
            start = end + 1;
        }
        Ok(rendered)
    }

    /// One bounded notification stream per Job, shared by state transitions,
    /// retained output arrival/EOF and late stream attachment. Subscribe before
    /// reading state; the host then rechecks authoritative Job/output facts
    /// after every notification. Works across stdout/stderr and all Graph nodes.
    #[cfg(feature = "async")]
    pub fn subscribe_changes(&self, id: &str) -> Result<watch::Receiver<u64>, Error> {
        let state = self.state.lock().map_err(|_| Error::Poisoned)?;
        let managed = state.jobs.get(id).ok_or(Error::NotFound)?;
        if !managed.visibility.is_exposed() {
            return Err(Error::NotExposed);
        }
        Ok(managed.change.subscribe())
    }

    pub fn get(&self, id: &str) -> Result<Snapshot, Error> {
        let state = self.state.lock().map_err(|_| Error::Poisoned)?;
        let managed = state.jobs.get(id).ok_or(Error::NotFound)?;
        if !managed.visibility.is_exposed() {
            return Err(Error::NotExposed);
        }
        Ok(Self::snapshot(id, managed))
    }

    /// Host-runtime status for the same live Job, including optimistic hidden
    /// Jobs that have not yet been handed off to the model-visible namespace.
    pub fn runtime_snapshot(&self, id: &str) -> Result<Snapshot, Error> {
        let state = self.state.lock().map_err(|_| Error::Poisoned)?;
        let managed = state.jobs.get(id).ok_or(Error::NotFound)?;
        Ok(Self::snapshot(id, managed))
    }

    pub fn has_nonterminal_jobs(&self) -> Result<bool, Error> {
        let state = self.state.lock().map_err(|_| Error::Poisoned)?;
        Ok(state.jobs.values().any(|managed| {
            managed.job.lifecycle() != Lifecycle::Terminal
                || managed.job.journal_ambiguous()
                || managed.transition_inflight.is_some()
                || !managed.writers_quiesced()
        }))
    }

    /// Host-authorized retention expiry. This is intentionally clock-free:
    /// products own wall-clock policy and call this only after their retention
    /// deadline. Expiry may discard unacknowledged terminal payload bytes, but
    /// never live/in-flight work and never an execution still referenced by the
    /// shared replay ledger.
    pub fn expire_terminal(&self, id: &str) -> Result<(), Error> {
        let mut state = self.state.lock().map_err(|_| Error::Poisoned)?;
        let managed = state.jobs.get(id).ok_or(Error::NotFound)?;
        if managed.job.lifecycle() != Lifecycle::Terminal
            || managed.job.journal_ambiguous()
            || managed.transition_inflight.is_some()
            || !managed.writers_quiesced()
            || state
                .replays
                .values()
                .any(|entry| entry.record.job_id == id)
        {
            return Err(Error::Busy);
        }
        state.jobs.remove(id);
        Ok(())
    }

    /// Remove a provisional Job only while it is provably pre-effect: hidden,
    /// all nodes Pending, no transition/output/process state, and no Ready
    /// replay identity. Claimed/Unknown replay tombstones may outlive the local
    /// provisional Job because they intentionally forbid a duplicate execution.
    pub fn discard_unstarted(&self, id: &str) -> Result<(), Error> {
        let mut state = self.state.lock().map_err(|_| Error::Poisoned)?;
        let removable = {
            let managed = state.jobs.get(id).ok_or(Error::NotFound)?;
            !managed.visibility.is_exposed()
                && managed.job.lifecycle() == Lifecycle::Running
                && !managed.job.journal_ambiguous()
                && managed.job.active() == 0
                && managed.job.finished() == 0
                && managed.transition_inflight.is_none()
                && managed.outputs.is_empty()
                && managed.process_terminals.is_empty()
                && (1..=managed.job.total())
                    .all(|coordinate| managed.job.state(coordinate) == Some(NodeState::Pending))
        };
        let ready_replay = state.replays.values().any(|entry| {
            entry.record.job_id == id && entry.record.disposition == ReplayDisposition::Ready
        });
        if !removable || ready_replay {
            return Err(Error::Busy);
        }
        state.jobs.remove(id);
        Ok(())
    }

    /// Remove only a terminal Job that was never exposed. Dropping its output
    /// owners releases any retained spool capacity. Historical exposed Jobs are
    /// never removed through this optimistic-completion path.
    pub fn discard_unexposed_terminal(&self, id: &str) -> Result<(), Error> {
        let mut state = self.state.lock().map_err(|_| Error::Poisoned)?;
        let managed = state.jobs.get(id).ok_or(Error::NotFound)?;
        if managed.visibility != Visibility::Hidden
            || managed.job.lifecycle() != Lifecycle::Terminal
            || managed.job.journal_ambiguous()
            || managed.transition_inflight.is_some()
            || !managed.writers_quiesced()
        {
            return Err(Error::Busy);
        }
        state.jobs.remove(id);
        Ok(())
    }

    pub fn graph(&self, id: &str) -> Result<Graph, Error> {
        let state = self.state.lock().map_err(|_| Error::Poisoned)?;
        Ok(state
            .jobs
            .get(id)
            .ok_or(Error::NotFound)?
            .job
            .graph()
            .clone())
    }

    /// Newest exposed Job first. This is a live-query offset, not a snapshot
    /// token; exposure/expiry between calls may shift later offsets.
    pub fn list(&self, offset: usize, limit: usize) -> Result<ListPage, Error> {
        if limit == 0
            || limit > self.limits.max_exposed_jobs.min(MAX_JOB_LIST_PAGE)
            || offset.checked_add(limit).is_none()
        {
            return Err(Error::InvalidLimits);
        }
        let state = self.state.lock().map_err(|_| Error::Poisoned)?;
        let mut jobs = state
            .jobs
            .iter()
            .filter(|(_, managed)| managed.visibility.is_exposed())
            .collect::<Vec<_>>();
        jobs.sort_by(|(id_a, a), (id_b, b)| {
            b.created_order
                .cmp(&a.created_order)
                .then_with(|| id_a.cmp(id_b))
        });
        let start = offset.min(jobs.len());
        let end = start.saturating_add(limit).min(jobs.len());
        let page = jobs[start..end]
            .iter()
            .map(|(id, managed)| Self::snapshot(id, managed))
            .collect();
        Ok(ListPage {
            jobs: page,
            next_offset: (end < jobs.len()).then_some(end),
        })
    }

    /// Newest exposed Job first with an immutable logical continuation.
    /// Jobs exposed after the first page never move the continuation boundary.
    pub fn list_cursor(
        &self,
        cursor: Option<&ListCursor>,
        limit: usize,
    ) -> Result<CursorListPage, Error> {
        if limit == 0 || limit > self.limits.max_exposed_jobs.min(MAX_JOB_LIST_PAGE) {
            return Err(Error::InvalidLimits);
        }
        let state = self.state.lock().map_err(|_| Error::Poisoned)?;
        let mut jobs = state
            .jobs
            .iter()
            .filter(|(_, managed)| managed.visibility.is_exposed())
            .filter(|(id, managed)| {
                cursor.is_none_or(|cursor| {
                    managed.created_order < cursor.created_order
                        || (managed.created_order == cursor.created_order
                            && id.as_str() > cursor.id.as_str())
                })
            })
            .collect::<Vec<_>>();
        jobs.sort_by(|(id_a, a), (id_b, b)| {
            b.created_order
                .cmp(&a.created_order)
                .then_with(|| id_a.cmp(id_b))
        });
        let has_more = jobs.len() > limit;
        jobs.truncate(limit);
        let next_cursor = has_more.then(|| {
            let (id, managed) = jobs
                .last()
                .expect("has_more implies at least one returned Job");
            ListCursor {
                created_order: managed.created_order,
                id: (*id).clone(),
            }
        });
        Ok(CursorListPage {
            jobs: jobs
                .into_iter()
                .map(|(id, managed)| Self::snapshot(id, managed))
                .collect(),
            next_cursor,
        })
    }

    fn lower_conflict(managed: &ManagedJob, coordinate: usize) -> bool {
        let Some(current) = managed.plans.get(coordinate.saturating_sub(1)) else {
            return true;
        };
        for lower in 1..coordinate {
            let Some(plan) = managed.plans.get(lower - 1) else {
                return true;
            };
            if !plan.footprint.conflicts(&current.footprint) {
                continue;
            }
            match managed.job.state(lower) {
                Some(NodeState::Terminal { .. }) => {}
                Some(NodeState::Pending) => {
                    if managed.job.pending_condition(lower).ok() != Some(Some(false)) {
                        return true;
                    }
                }
                Some(NodeState::Claimed { .. } | NodeState::Running { .. }) | None => return true,
            }
        }
        false
    }

    pub fn runnable(&self, id: &str) -> Result<Vec<usize>, Error> {
        let state = self.state.lock().map_err(|_| Error::Poisoned)?;
        let managed = state.jobs.get(id).ok_or(Error::NotFound)?;
        if managed.transition_inflight.is_some() {
            return Ok(Vec::new());
        }
        let free_global = self
            .limits
            .max_active_nodes
            .saturating_sub(state.active_nodes);
        if free_global == 0 {
            return Ok(Vec::new());
        }
        Ok(managed
            .job
            .runnable()
            .into_iter()
            .filter(|coordinate| !Self::lower_conflict(managed, *coordinate))
            .take(free_global)
            .collect())
    }

    pub fn skippable(&self, id: &str) -> Result<Vec<usize>, Error> {
        let state = self.state.lock().map_err(|_| Error::Poisoned)?;
        let managed = state.jobs.get(id).ok_or(Error::NotFound)?;
        if managed.transition_inflight.is_some() {
            return Ok(Vec::new());
        }
        Ok(managed.job.skippable())
    }

    pub fn runner_load(&self, id: &str) -> Result<RunnerLoad, Error> {
        let state = self.state.lock().map_err(|_| Error::Poisoned)?;
        let managed = state.jobs.get(id).ok_or(Error::NotFound)?;
        Ok(RunnerLoad {
            job_active: managed.job.active(),
            global_active: state.active_nodes,
            global_limit: self.limits.max_active_nodes,
        })
    }

    pub fn plans(&self, id: &str) -> Result<Vec<NodePlan>, Error> {
        let state = self.state.lock().map_err(|_| Error::Poisoned)?;
        Ok(state.jobs.get(id).ok_or(Error::NotFound)?.plans.clone())
    }

    fn commit_transition(
        &self,
        id: &str,
        proposal: Transition,
        reserved_permit: bool,
        release_permit_on_commit: bool,
        persist: impl FnOnce(&str, Transition) -> Result<(), JournalError>,
    ) -> Result<(), Error> {
        // Never invoke host code while holding the manager mutex.
        let outcome = std::panic::catch_unwind(AssertUnwindSafe(|| persist(id, proposal)));
        let mut state = self.state.lock().map_err(|_| Error::Poisoned)?;
        let matches = state
            .jobs
            .get(id)
            .is_some_and(|managed| managed.transition_inflight == Some(proposal));
        if !matches {
            if let Some(managed) = state.jobs.get_mut(id) {
                managed.job.fence_journal_unknown();
                managed.transition_inflight = None;
                #[cfg(feature = "async")]
                managed.signal();
            }
            return Err(Error::Job(JobError::JournalUnknown));
        }
        match outcome {
            Ok(Ok(())) => {
                let applied = {
                    let managed = state.jobs.get_mut(id).expect("checked manager entry");
                    let result = managed.job.apply_committed(proposal);
                    managed.transition_inflight = None;
                    if result.is_ok() {
                        managed.revision = managed
                            .revision
                            .checked_add(1)
                            .ok_or(JobError::InvalidSequence)?;
                    }
                    result
                };
                if applied.is_err() {
                    let managed = state.jobs.get_mut(id).expect("checked manager entry");
                    managed.job.fence_journal_unknown();
                    #[cfg(feature = "async")]
                    managed.signal();
                    return Err(Error::Job(JobError::JournalUnknown));
                }
                #[cfg(feature = "async")]
                state.jobs.get(id).expect("checked manager entry").signal();
                if release_permit_on_commit {
                    state.active_nodes = state.active_nodes.saturating_sub(1);
                }
                Ok(())
            }
            Ok(Err(JournalError::RejectedBeforeEffect)) => {
                state
                    .jobs
                    .get_mut(id)
                    .expect("checked manager entry")
                    .transition_inflight = None;
                if reserved_permit {
                    state.active_nodes = state.active_nodes.saturating_sub(1);
                }
                Err(Error::Job(JobError::JournalRejected))
            }
            Ok(Err(JournalError::OutcomeUnknown)) | Err(_) => {
                let managed = state.jobs.get_mut(id).expect("checked manager entry");
                managed.transition_inflight = None;
                managed.job.fence_journal_unknown();
                #[cfg(feature = "async")]
                managed.signal();
                // A possibly committed Claim continues consuming its reserved
                // global slot; a possibly committed terminal does not release a
                // slot until authoritative reconstruction proves it terminal.
                Err(Error::Job(JobError::JournalUnknown))
            }
        }
    }

    pub fn job_output_owner(&self, id: &str) -> Result<OutputOwner, Error> {
        let state = self.state.lock().map_err(|_| Error::Poisoned)?;
        let managed = state.jobs.get(id).ok_or(Error::NotFound)?;
        let mut hash = Sha256::new();
        hash.update(b"aw-tools-job-output-owner-v1\0");
        hash.update(managed.job.binding());
        OutputOwner::new(hash.finalize().into())
    }

    /// Host-scoped model/SDK consumer lease. Hosts must hash an authenticated,
    /// stable consumer identity into `scope` (not a per-call/turn ID or a model
    /// argument), and independently authorize every read/ACK. The original
    /// Job owner and Task result owners remain separate domains.
    ///
    /// Derivation alone grants nothing: enroll_output_consumer must first
    /// register this owner against each currently retained stream.
    pub fn consumer_output_owner(&self, id: &str, scope: [u8; 32]) -> Result<OutputOwner, Error> {
        if scope == [0; 32] {
            return Err(Error::InvalidOutput);
        }
        let state = self.state.lock().map_err(|_| Error::Poisoned)?;
        let managed = state.jobs.get(id).ok_or(Error::NotFound)?;
        let mut hash = Sha256::new();
        hash.update(b"aw-tools-consumer-output-owner-v1\0");
        hash.update(managed.job.binding());
        hash.update(scope);
        OutputOwner::new(hash.finalize().into())
    }

    pub fn task_result_owner(&self, id: &str, coordinate: usize) -> Result<OutputOwner, Error> {
        let state = self.state.lock().map_err(|_| Error::Poisoned)?;
        let managed = state.jobs.get(id).ok_or(Error::NotFound)?;
        let plan = managed
            .plans
            .get(coordinate.checked_sub(1).ok_or(Error::InvalidOutput)?)
            .ok_or(Error::InvalidOutput)?;
        if !plan.task_result_eligible {
            return Err(Error::InvalidOutput);
        }
        let mut hash = Sha256::new();
        hash.update(b"aw-tools-task-result-owner-v1\0");
        hash.update(managed.job.binding());
        hash.update((coordinate as u64).to_le_bytes());
        OutputOwner::new(hash.finalize().into())
    }

    fn replay_decision(
        record: &ReplayRecord,
        semantic_hash: [u8; 32],
    ) -> Result<ReplayDecision, Error> {
        if record.semantic_hash != semantic_hash {
            return Err(Error::ReplayMismatch);
        }
        Ok(match record.disposition {
            ReplayDisposition::Ready => ReplayDecision::Existing {
                job_id: record.job_id.clone(),
            },
            ReplayDisposition::Claimed | ReplayDisposition::Unknown => ReplayDecision::Unknown,
        })
    }

    /// Read-only fast path for a replay identity that is already known to this
    /// live manager. `None` means the caller may proceed to bounded admission
    /// before attempting the exclusive replay claim. The subsequent claim is
    /// still authoritative because another caller can race this lookup.
    pub fn lookup_replay(
        &self,
        namespace: &str,
        request_id: &str,
        semantic_hash: [u8; 32],
    ) -> Result<Option<ReplayDecision>, Error> {
        if semantic_hash == [0; 32] {
            return Err(Error::InvalidPlans);
        }
        let key = replay_key(namespace, request_id)?;
        let state = self.state.lock().map_err(|_| Error::Poisoned)?;
        if let Some(entry) = state.replays.get(&key) {
            return Self::replay_decision(&entry.record, semantic_hash).map(Some);
        }
        if state.replay_inflight.contains(&key) {
            return Err(Error::Busy);
        }
        Ok(None)
    }

    /// Replay claim precedes root Job execution. A host that requires
    /// cross-process replay protection supplies an exclusive create-if-absent
    /// callback; a host whose outer session journal already forbids redispatch
    /// may commit this only in the live manager. Mismatched reuse fails closed.
    pub fn claim_replay(
        &self,
        namespace: &str,
        request_id: &str,
        semantic_hash: [u8; 32],
        proposed_job_id: &str,
        persist: impl FnOnce(&ReplayRecord) -> Result<ReplayPersist, JournalError>,
    ) -> Result<ReplayDecision, Error> {
        if semantic_hash == [0; 32]
            || proposed_job_id.is_empty()
            || proposed_job_id.len() > 256
            || proposed_job_id.chars().any(char::is_control)
        {
            return Err(Error::InvalidPlans);
        }
        let key = replay_key(namespace, request_id)?;
        {
            let mut state = self.state.lock().map_err(|_| Error::Poisoned)?;
            if let Some(existing) = state.replays.get(&key) {
                return Self::replay_decision(&existing.record, semantic_hash);
            }
            if state.replays.len() >= self.limits.max_hidden_replays {
                return Err(Error::Capacity);
            }
            if !state.replay_inflight.insert(key) {
                return Err(Error::Busy);
            }
        }
        let proposed = ReplayRecord {
            key,
            semantic_hash,
            job_id: proposed_job_id.to_owned(),
            disposition: ReplayDisposition::Claimed,
        };
        let outcome = std::panic::catch_unwind(AssertUnwindSafe(|| persist(&proposed)));
        let mut state = self.state.lock().map_err(|_| Error::Poisoned)?;
        state.replay_inflight.remove(&key);
        match outcome {
            Ok(Ok(ReplayPersist::Created)) => {
                state.replays.insert(
                    key,
                    ReplayEntry {
                        record: proposed.clone(),
                    },
                );
                Ok(ReplayDecision::New {
                    key,
                    job_id: proposed.job_id,
                })
            }
            Ok(Ok(ReplayPersist::Existing(existing))) => {
                if existing.key != key || existing.semantic_hash != semantic_hash {
                    return Err(Error::ReplayMismatch);
                }
                state.replays.insert(
                    key,
                    ReplayEntry {
                        record: existing.clone(),
                    },
                );
                Self::replay_decision(&existing, semantic_hash)
            }
            Ok(Err(JournalError::RejectedBeforeEffect)) => Err(Error::ReplayRejected),
            Ok(Err(JournalError::OutcomeUnknown)) | Err(_) => {
                let unknown = ReplayRecord {
                    disposition: ReplayDisposition::Unknown,
                    ..proposed
                };
                state.replays.insert(key, ReplayEntry { record: unknown });
                Err(Error::ReplayUnknown)
            }
        }
    }

    /// Mark the live replay claim ready only after the Job is admitted.
    /// An ambiguous host checkpoint leaves the identity unknown rather than
    /// making the request executable a second time.
    pub fn mark_replay_ready(
        &self,
        key: [u8; 32],
        job_id: &str,
        persist: impl FnOnce(&ReplayRecord) -> Result<(), JournalError>,
    ) -> Result<(), Error> {
        let ready = {
            let mut state = self.state.lock().map_err(|_| Error::Poisoned)?;
            if state.replay_inflight.contains(&key) {
                return Err(Error::Busy);
            }
            let current = state
                .replays
                .get(&key)
                .ok_or(Error::ReplayUnknown)?
                .record
                .clone();
            if current.job_id != job_id
                || current.disposition != ReplayDisposition::Claimed
                || !state.jobs.contains_key(job_id)
            {
                return Err(Error::ReplayUnknown);
            }
            if !state.replay_inflight.insert(key) {
                return Err(Error::Busy);
            }
            ReplayRecord {
                disposition: ReplayDisposition::Ready,
                ..current
            }
        };
        let outcome = std::panic::catch_unwind(AssertUnwindSafe(|| persist(&ready)));
        let mut state = self.state.lock().map_err(|_| Error::Poisoned)?;
        state.replay_inflight.remove(&key);
        match outcome {
            Ok(Ok(())) => {
                state.replays.insert(key, ReplayEntry { record: ready });
                Ok(())
            }
            Ok(Err(JournalError::RejectedBeforeEffect)) => Err(Error::ReplayRejected),
            Ok(Err(JournalError::OutcomeUnknown)) | Err(_) => {
                let unknown = ReplayRecord {
                    disposition: ReplayDisposition::Unknown,
                    ..ready
                };
                state.replays.insert(key, ReplayEntry { record: unknown });
                Err(Error::ReplayUnknown)
            }
        }
    }

    pub fn replay_record(&self, key: [u8; 32]) -> Result<ReplayRecord, Error> {
        let state = self.state.lock().map_err(|_| Error::Poisoned)?;
        Ok(state
            .replays
            .get(&key)
            .ok_or(Error::ReplayUnknown)?
            .record
            .clone())
    }

    /// Restart never reuses the source NodePlans. The caller must first compile
    /// and prepare a new Graph/plan set from current registry/authority/runtime
    /// policy, then pass those new immutable plans here.
    pub fn restart_runtime(
        &self,
        source_id: &str,
        graph: Graph,
        plans: Vec<NodePlan>,
        admission: RuntimeAdmission<'_>,
    ) -> Result<(), Error> {
        let job = Job::after_runtime_intent(
            graph,
            admission.runtime_session,
            admission.id,
            admission.epoch,
            admission.admission_sequence,
            admission.width,
        )?;
        self.insert(
            admission.id,
            job,
            plans,
            if admission.exposed {
                Visibility::Exposed
            } else {
                Visibility::Hidden
            },
            None,
            Some(source_id),
        )
    }

    pub fn restart_reserved_exposed_runtime(
        &self,
        source_id: &str,
        graph: Graph,
        plans: Vec<NodePlan>,
        admission: ReservedExposedRuntimeAdmission<'_>,
    ) -> Result<(), Error> {
        let job = Job::after_runtime_intent(
            graph,
            admission.runtime_session,
            admission.id,
            admission.epoch,
            admission.admission_sequence,
            admission.width,
        )?;
        self.insert(
            admission.id,
            job,
            plans,
            Visibility::ReservedExposed,
            None,
            Some(source_id),
        )
    }

    pub fn restart_task(
        &self,
        source_id: &str,
        graph: Graph,
        plans: Vec<NodePlan>,
        admission: TaskAdmission<'_>,
    ) -> Result<(), Error> {
        let job = Job::after_task_intent(
            graph,
            admission.task,
            admission.id,
            admission.admission_sequence,
            admission.width,
        )?;
        self.insert(
            admission.id,
            job,
            plans,
            if admission.exposed {
                Visibility::Exposed
            } else {
                Visibility::Hidden
            },
            Some(admission.task.clone()),
            Some(source_id),
        )
    }

    pub fn restart_reserved_exposed_task(
        &self,
        source_id: &str,
        graph: Graph,
        plans: Vec<NodePlan>,
        admission: ReservedExposedTaskAdmission<'_>,
    ) -> Result<(), Error> {
        let job = Job::after_task_intent(
            graph,
            admission.task,
            admission.id,
            admission.admission_sequence,
            admission.width,
        )?;
        self.insert(
            admission.id,
            job,
            plans,
            Visibility::ReservedExposed,
            Some(admission.task.clone()),
            Some(source_id),
        )
    }

    fn output_handle(
        &self,
        id: &str,
        coordinate: usize,
        stream: OutputStream,
    ) -> Result<Arc<Mutex<OwnedOutput>>, Error> {
        let state = self.state.lock().map_err(|_| Error::Poisoned)?;
        let managed = state.jobs.get(id).ok_or(Error::NotFound)?;
        if !(1..=managed.job.total()).contains(&coordinate) {
            return Err(Error::InvalidOutput);
        }
        managed
            .outputs
            .get(&(coordinate, stream))
            .cloned()
            .ok_or(Error::OutputMissing)
    }

    /// Journal fencing and destructive output changes share the same lock.
    /// Retained output readers/writers never take the manager lock while
    /// holding an output lock; preserve manager -> output lock order.
    fn with_known_output_mutation<T>(
        &self,
        id: &str,
        coordinate: usize,
        stream: OutputStream,
        commit: impl FnOnce(&mut OwnedOutput) -> Result<T, Error>,
    ) -> Result<T, Error> {
        let state = self.state.lock().map_err(|_| Error::Poisoned)?;
        let managed = state.jobs.get(id).ok_or(Error::NotFound)?;
        if managed.job.journal_ambiguous() {
            return Err(Error::ReplayUnknown);
        }
        if managed.transition_inflight.is_some() {
            return Err(Error::Busy);
        }
        if !(1..=managed.job.total()).contains(&coordinate) {
            return Err(Error::InvalidOutput);
        }
        let handle = managed
            .outputs
            .get(&(coordinate, stream))
            .ok_or(Error::OutputMissing)?;
        let mut output = handle.lock().map_err(|_| Error::Poisoned)?;
        commit(&mut output)
    }

    fn insert_outputs<const N: usize>(
        &self,
        id: &str,
        coordinate: usize,
        outputs: [(OutputStream, Arc<Mutex<OwnedOutput>>); N],
    ) -> Result<(), Error> {
        if N == 0 {
            return Err(Error::InvalidOutput);
        }
        let mut requested = BTreeSet::new();
        if outputs.iter().any(|(stream, _)| !requested.insert(*stream)) {
            return Err(Error::InvalidOutput);
        }
        let mut state = self.state.lock().map_err(|_| Error::Poisoned)?;
        let managed = state.jobs.get_mut(id).ok_or(Error::NotFound)?;
        if !(1..=managed.job.total()).contains(&coordinate)
            || managed
                .transition_inflight
                .is_some_and(|transition| transition.node == Some(coordinate))
            || !managed
                .job
                .state(coordinate)
                .is_some_and(NodeState::in_flight)
            || requested
                .iter()
                .any(|stream| managed.outputs.contains_key(&(coordinate, *stream)))
        {
            return Err(Error::InvalidOutput);
        }
        for (stream, output) in outputs {
            #[cfg(feature = "async")]
            {
                output.lock().map_err(|_| Error::Poisoned)?.change = Some(managed.change.clone());
            }
            managed.outputs.insert((coordinate, stream), output);
        }
        #[cfg(feature = "async")]
        managed.signal();
        Ok(())
    }

    fn insert_output(
        &self,
        id: &str,
        coordinate: usize,
        stream: OutputStream,
        output: Arc<Mutex<OwnedOutput>>,
    ) -> Result<(), Error> {
        self.insert_outputs(id, coordinate, [(stream, output)])
    }

    /// Host supplies an empty secure spool file after node claim. The manager
    /// owns its byte accounting and all retained owners from this point onward.
    pub fn attach_output(
        &self,
        id: &str,
        coordinate: usize,
        stream: OutputStream,
        file: File,
        owners: impl IntoIterator<Item = OutputOwner>,
    ) -> Result<(), Error> {
        self.attach_output_with_limit(
            id,
            coordinate,
            stream,
            file,
            owners,
            crate::task::MAX_SPOOL_PER_STREAM,
        )
    }

    /// File-backed streaming output with a host-selected retained-prefix cap.
    /// The cap is independent of the shared Session quota and may only narrow
    /// the shared per-stream safety maximum.
    pub fn attach_output_with_limit(
        &self,
        id: &str,
        coordinate: usize,
        stream: OutputStream,
        file: File,
        owners: impl IntoIterator<Item = OutputOwner>,
        limit: u64,
    ) -> Result<(), Error> {
        if !(1..=crate::task::MAX_SPOOL_PER_STREAM).contains(&limit) {
            return Err(Error::InvalidOutput);
        }
        let output = Arc::new(Mutex::new(OwnedOutput::new_with_limit(
            file,
            self.spool_quota.clone(),
            owners,
            limit,
        )?));
        self.insert_output(id, coordinate, stream, output)
    }

    /// Atomically attach all three ShellNode retained streams while the node
    /// is in-flight but before Start. The compact result has exact prepaid
    /// capacity; stdout/stderr admit incrementally and may truncate under the
    /// shared session quota while their pipes continue draining.
    pub fn attach_shell_outputs(
        &self,
        id: &str,
        coordinate: usize,
        owner: OutputOwner,
        result_capacity: u64,
        stream_capacity: u64,
    ) -> Result<(OutputWriter, OutputWriter), Error> {
        // Reserve exact result capacity first so a failure happens before any
        // dynamic stream object is published into the Job ledger.
        let result = Arc::new(Mutex::new(OwnedOutput::new_reserved(
            self.spool_quota.clone(),
            [owner],
            result_capacity,
        )?));
        let stdout = Arc::new(Mutex::new(OwnedOutput::new_memory(
            self.spool_quota.clone(),
            [owner],
            stream_capacity,
        )?));
        let stderr = Arc::new(Mutex::new(OwnedOutput::new_memory(
            self.spool_quota.clone(),
            [owner],
            stream_capacity,
        )?));
        self.insert_outputs(
            id,
            coordinate,
            [
                (OutputStream::Result, result),
                (OutputStream::Stdout, stdout.clone()),
                (OutputStream::Stderr, stderr.clone()),
            ],
        )?;
        Ok((
            OutputWriter { output: stdout },
            OutputWriter { output: stderr },
        ))
    }

    /// Atomically attach file-backed process stdout/stderr before launch.
    /// Multiple host projections may retain the same immutable stream without
    /// adapter-side partial attachment or rollback. Result payload ownership is
    /// intentionally separate because not every process host exposes one.
    pub fn attach_process_output_files(
        &self,
        id: &str,
        coordinate: usize,
        owners: impl IntoIterator<Item = OutputOwner>,
        files: ShellOutputFiles,
    ) -> Result<(OutputWriter, OutputWriter), Error> {
        if !(1..=crate::task::MAX_SPOOL_PER_STREAM).contains(&files.stream_capacity) {
            return Err(Error::InvalidOutput);
        }
        let owners = owners.into_iter().collect::<Vec<_>>();
        // Validate once before constructing either spool so malformed owner sets
        // cannot reserve quota or publish one side of the process output.
        OwnedOutput::owners(owners.iter().copied())?;
        let stdout = Arc::new(Mutex::new(OwnedOutput::new_with_limit(
            files.stdout,
            self.spool_quota.clone(),
            owners.iter().copied(),
            files.stream_capacity,
        )?));
        let stderr = Arc::new(Mutex::new(OwnedOutput::new_with_limit(
            files.stderr,
            self.spool_quota.clone(),
            owners.iter().copied(),
            files.stream_capacity,
        )?));
        self.insert_outputs(
            id,
            coordinate,
            [
                (OutputStream::Stdout, stdout.clone()),
                (OutputStream::Stderr, stderr.clone()),
            ],
        )?;
        Ok((
            OutputWriter { output: stdout },
            OutputWriter { output: stderr },
        ))
    }

    /// Host-owned file-spool variant of [`Self::attach_shell_outputs`].
    /// The compact result remains exact/prepaid in shared memory; stdout/stderr
    /// use already-opened empty secure files supplied by the host. All three
    /// streams become visible atomically or none do.
    pub fn attach_shell_output_files(
        &self,
        id: &str,
        coordinate: usize,
        owner: OutputOwner,
        result_capacity: u64,
        stdout: File,
        stderr: File,
    ) -> Result<(OutputWriter, OutputWriter), Error> {
        self.attach_shell_output_files_with_limit(
            id,
            coordinate,
            owner,
            result_capacity,
            ShellOutputFiles {
                stdout,
                stderr,
                stream_capacity: crate::task::MAX_SPOOL_PER_STREAM,
            },
        )
    }

    /// File-backed ShellNode outputs with an explicit retained-prefix ceiling
    /// shared by stdout/stderr. The compact result remains exact/prepaid.
    pub fn attach_shell_output_files_with_limit(
        &self,
        id: &str,
        coordinate: usize,
        owner: OutputOwner,
        result_capacity: u64,
        files: ShellOutputFiles,
    ) -> Result<(OutputWriter, OutputWriter), Error> {
        if !(1..=crate::task::MAX_SPOOL_PER_STREAM).contains(&files.stream_capacity) {
            return Err(Error::InvalidOutput);
        }
        let result = Arc::new(Mutex::new(OwnedOutput::new_reserved(
            self.spool_quota.clone(),
            [owner],
            result_capacity,
        )?));
        let stdout = Arc::new(Mutex::new(OwnedOutput::new_with_limit(
            files.stdout,
            self.spool_quota.clone(),
            [owner],
            files.stream_capacity,
        )?));
        let stderr = Arc::new(Mutex::new(OwnedOutput::new_with_limit(
            files.stderr,
            self.spool_quota.clone(),
            [owner],
            files.stream_capacity,
        )?));
        self.insert_outputs(
            id,
            coordinate,
            [
                (OutputStream::Result, result),
                (OutputStream::Stdout, stdout.clone()),
                (OutputStream::Stderr, stderr.clone()),
            ],
        )?;
        Ok((
            OutputWriter { output: stdout },
            OutputWriter { output: stderr },
        ))
    }

    /// Job-owned streaming memory output for ShellNode stdout/stderr.
    /// Capacity is admitted incrementally under the shared session quota and
    /// overflow becomes immutable truncation while the process pipe keeps draining.
    pub fn attach_output_memory(
        &self,
        id: &str,
        coordinate: usize,
        stream: OutputStream,
        owners: impl IntoIterator<Item = OutputOwner>,
    ) -> Result<(), Error> {
        let output = Arc::new(Mutex::new(OwnedOutput::new_memory(
            self.spool_quota.clone(),
            owners,
            crate::task::MAX_SPOOL_PER_STREAM,
        )?));
        self.insert_output(id, coordinate, stream, output)
    }

    /// Exact-result variant: reserves the full retained capacity before
    /// execution starts. If the shared spool quota cannot satisfy it, this
    /// fails before the caller releases the node into an effect.
    pub fn attach_output_reserved(
        &self,
        id: &str,
        coordinate: usize,
        stream: OutputStream,
        owners: impl IntoIterator<Item = OutputOwner>,
        capacity: u64,
    ) -> Result<(), Error> {
        let output = Arc::new(Mutex::new(OwnedOutput::new_reserved(
            self.spool_quota.clone(),
            owners,
            capacity,
        )?));
        self.insert_output(id, coordinate, stream, output)
    }

    pub fn output_writer(
        &self,
        id: &str,
        coordinate: usize,
        stream: OutputStream,
    ) -> Result<OutputWriter, Error> {
        let output = self.output_handle(id, coordinate, stream)?;
        {
            let guard = output.lock().map_err(|_| Error::Poisoned)?;
            if guard.writer_finished || guard.released {
                return Err(Error::InvalidOutput);
            }
        }
        Ok(OutputWriter { output })
    }

    pub fn add_output_owner(
        &self,
        id: &str,
        coordinate: usize,
        stream: OutputStream,
        owner: OutputOwner,
    ) -> Result<(), Error> {
        if owner.0 == [0; 32] {
            return Err(Error::InvalidOutput);
        }
        let output = self.output_handle(id, coordinate, stream)?;
        let mut output = output.lock().map_err(|_| Error::Poisoned)?;
        if output.released || output.owners.len() >= 16 || output.owners.contains_key(&owner) {
            return Err(Error::InvalidOutput);
        }
        output.owners.insert(owner, OwnerAck::default());
        Ok(())
    }

    /// Enroll an *already-authorized* consumer into all currently selected
    /// retained streams. No output disclosure/authority is conferred here.
    /// Multi-stream preflight is all-or-none; repeated enrollment is
    /// idempotent, and released output cannot be resurrected. Newly attached
    /// streams require a later explicit enrollment by the host.
    ///
    /// The caller must supply the stable consumer-derived OutputOwner, never
    /// a model-selected key. A maximum of 16 owners per stream prevents
    /// unbounded lease pinning.
    pub fn enroll_output_consumer(
        &self,
        id: &str,
        node: Option<usize>,
        view: Option<OutputStream>,
        owner: OutputOwner,
    ) -> Result<bool, Error> {
        if owner.0 == [0; 32] {
            return Err(Error::InvalidOutput);
        }
        let state = self.state.lock().map_err(|_| Error::Poisoned)?;
        let managed = state.jobs.get(id).ok_or(Error::NotFound)?;
        if !managed.visibility.is_exposed() {
            return Err(Error::NotExposed);
        }
        if node.is_some_and(|node| !(1..=managed.job.total()).contains(&node)) {
            return Err(Error::InvalidOutput);
        }
        let handles = managed
            .outputs
            .iter()
            .filter(|((coordinate, stream), _)| {
                node.is_none_or(|node| node == *coordinate)
                    && view.is_none_or(|view| view == *stream)
            })
            .map(|(_, output)| Arc::clone(output))
            .collect::<Vec<_>>();
        if handles.is_empty() {
            return Err(Error::OutputMissing);
        }
        // Lock order is Job state then stable node/stream-ordered outputs,
        // matching whole-Job Finish. No owner change until all checks pass.
        let mut outputs = Vec::with_capacity(handles.len());
        for handle in &handles {
            outputs.push(handle.lock().map_err(|_| Error::Poisoned)?);
        }
        for output in &outputs {
            if output.released {
                return Err(Error::OutputReleased);
            }
            if !output.owners.contains_key(&owner) && output.owners.len() >= 16 {
                return Err(Error::Capacity);
            }
        }
        let mut changed = false;
        for output in &mut outputs {
            if let std::collections::btree_map::Entry::Vacant(entry) = output.owners.entry(owner) {
                entry.insert(OwnerAck::default());
                changed = true;
            }
        }
        Ok(changed)
    }

    pub fn append_output(
        &self,
        id: &str,
        coordinate: usize,
        stream: OutputStream,
        bytes: &[u8],
    ) -> Result<usize, Error> {
        self.output_writer(id, coordinate, stream)?.append(bytes)
    }

    pub fn mark_output_failed(
        &self,
        id: &str,
        coordinate: usize,
        stream: OutputStream,
        observed_after_failure: u64,
    ) -> Result<(), Error> {
        let output = self.output_handle(id, coordinate, stream)?;
        let mut output = output.lock().map_err(|_| Error::Poisoned)?;
        output.spool.mark_writer_failed();
        if observed_after_failure != 0 {
            output.spool.observe_after_failure(observed_after_failure)?;
        }
        #[cfg(feature = "async")]
        output.signal();
        Ok(())
    }

    pub fn finish_output(
        &self,
        id: &str,
        coordinate: usize,
        stream: OutputStream,
    ) -> Result<(), Error> {
        let output = self.output_handle(id, coordinate, stream)?;
        let result = {
            let mut output = output.lock().map_err(|_| Error::Poisoned)?;
            output.finish_writer()
        };
        result
    }

    /// Pure raw-byte read. No ACK/delivery state is changed here.
    pub fn output_summary(
        &self,
        id: &str,
        coordinate: usize,
        stream: OutputStream,
    ) -> Result<(crate::task::StreamSummary, bool), Error> {
        let output = self.output_handle(id, coordinate, stream)?;
        let output = output.lock().map_err(|_| Error::Poisoned)?;
        if output.released {
            return Err(Error::OutputReleased);
        }
        Ok((output.spool.summary(), output.writer_finished))
    }

    pub fn output_page(
        &self,
        id: &str,
        coordinate: usize,
        stream: OutputStream,
        offset: u64,
        limit: usize,
    ) -> Result<OutputPage, Error> {
        let output = self.output_handle(id, coordinate, stream)?;
        let mut output = output.lock().map_err(|_| Error::Poisoned)?;
        output.read_page(OutputPosition::Absolute(offset), limit)
    }

    /// Shrink a finished retained stream to the exact prefix still required by
    /// one owner. Every other owner must already have acknowledged the complete
    /// current retained stream, so compaction can never discard bytes another
    /// owner still depends on. Ownership and ACK watermarks are unchanged.
    pub fn compact_output_for_owner(
        &self,
        id: &str,
        coordinate: usize,
        stream: OutputStream,
        owner: OutputOwner,
        keep: u64,
    ) -> Result<u64, Error> {
        self.with_known_output_mutation(id, coordinate, stream, |output| {
            if output.released {
                return Err(Error::OutputReleased);
            }
            if !output.writer_finished {
                return Err(Error::Busy);
            }
            let retained = output.spool.retained_bytes();
            if keep > retained || !output.owners.contains_key(&owner) {
                return Err(Error::InvalidOutput);
            }
            if output.owners.iter().any(|(candidate, ack)| {
                *candidate != owner && (!ack.acknowledged || ack.through < retained)
            }) {
                return Err(Error::Busy);
            }
            output.spool.compact_retained(keep)?;
            let _ = output.maybe_release()?;
            Ok(if output.released {
                0
            } else {
                output.spool.retained_bytes()
            })
        })
    }

    /// Pure owner-specific delivery/ACK projection. This is intentionally
    /// separate from the physical spool summary: multiple transport/task owners
    /// can have different logical watermarks over the same retained bytes.
    pub fn output_owner_state(
        &self,
        id: &str,
        coordinate: usize,
        stream: OutputStream,
        owner: OutputOwner,
    ) -> Result<OutputOwnerState, Error> {
        let output = self.output_handle(id, coordinate, stream)?;
        let output = output.lock().map_err(|_| Error::Poisoned)?;
        let ack = output.owners.get(&owner).ok_or(Error::InvalidOutput)?;
        Ok(OutputOwnerState {
            through: ack.through,
            acknowledged: ack.acknowledged,
            released: output.released,
        })
    }

    /// Exact owner-read: authorize and return bytes with NO delivery credit.
    /// The host projects and validates its final response before explicitly
    /// marking the represented prefix with record_delivered_output.
    pub fn output_page_for_owner_at(
        &self,
        id: &str,
        coordinate: usize,
        stream: OutputStream,
        owner: OutputOwner,
        position: OutputPosition,
        limit: usize,
    ) -> Result<OutputPage, Error> {
        let output = self.output_handle(id, coordinate, stream)?;
        let mut output = output.lock().map_err(|_| Error::Poisoned)?;
        output.read_for_owner(owner, position, limit)
    }

    pub fn output_page_for_owner(
        &self,
        id: &str,
        coordinate: usize,
        stream: OutputStream,
        owner: OutputOwner,
        offset: u64,
        limit: usize,
    ) -> Result<OutputPage, Error> {
        self.output_page_for_owner_at(
            id,
            coordinate,
            stream,
            owner,
            OutputPosition::Absolute(offset),
            limit,
        )
    }

    /// Trusted final-projection delivery proof for an exact output owner.
    /// A host must first authorize the view, read it and produce a complete
    /// bounded reply; `start/count` are raw bytes *included* in that reply,
    /// not bytes fetched into an intermediate buffer. This does not ACK.
    /// An out-of-order delivered range is not merged across unseen gaps.
    pub fn record_delivered_output(
        &self,
        id: &str,
        coordinate: usize,
        stream: OutputStream,
        owner: OutputOwner,
        start: u64,
        count: usize,
    ) -> Result<(), Error> {
        let output = self.output_handle(id, coordinate, stream)?;
        let mut output = output.lock().map_err(|_| Error::Poisoned)?;
        output.mark_delivered_for_owner(owner, start, count)
    }

    /// One combined byte budget for stdout and stderr of a Process node.
    /// Reserve half each; transfer any unused share to the other stream.
    /// Both stream snapshots use retained-byte positions, never guessed lines.
    pub fn process_tail_for_owner(
        &self,
        id: &str,
        coordinate: usize,
        owner: OutputOwner,
        limit: usize,
    ) -> Result<(Option<OutputPage>, Option<OutputPage>), Error> {
        if limit == 0 {
            return Ok((None, None));
        }
        if limit > MAX_SPOOL_PAGE_BYTES {
            return Err(Error::InvalidOutput);
        }
        let (stdout, stderr) = {
            let state = self.state.lock().map_err(|_| Error::Poisoned)?;
            let managed = state.jobs.get(id).ok_or(Error::NotFound)?;
            if !managed.visibility.is_exposed() {
                return Err(Error::NotExposed);
            }
            if !(1..=managed.job.total()).contains(&coordinate) {
                return Err(Error::InvalidOutput);
            }
            (
                managed
                    .outputs
                    .get(&(coordinate, OutputStream::Stdout))
                    .cloned(),
                managed
                    .outputs
                    .get(&(coordinate, OutputStream::Stderr))
                    .cloned(),
            )
        };
        let mut stdout = stdout
            .as_ref()
            .map(|output| output.lock().map_err(|_| Error::Poisoned))
            .transpose()?;
        let mut stderr = stderr
            .as_ref()
            .map(|output| output.lock().map_err(|_| Error::Poisoned))
            .transpose()?;
        if stdout.is_none() && stderr.is_none() {
            return Ok((None, None));
        }
        for stream in stdout.iter().chain(stderr.iter()) {
            if !stream.owners.contains_key(&owner) {
                return Err(Error::InvalidOutput);
            }
            if stream.released {
                return Err(Error::OutputReleased);
            }
        }
        let available_out = stdout.as_ref().map_or(0, |out| out.spool.retained_bytes());
        let available_err = stderr.as_ref().map_or(0, |err| err.spool.retained_bytes());
        let total = limit as u64;
        let mut out = available_out.min(total / 2 + total % 2);
        let mut err = available_err.min(total / 2);
        let unused = total - out - err;
        let extra_out = (available_out - out).min(unused);
        out += extra_out;
        err += (available_err - err).min(unused - extra_out);
        let out = if out != 0 {
            Some(
                stdout
                    .as_mut()
                    .ok_or(Error::OutputMissing)?
                    .read_for_owner(owner, OutputPosition::Tail(out), out as usize)?,
            )
        } else {
            None
        };
        let err = if err != 0 {
            Some(
                stderr
                    .as_mut()
                    .ok_or(Error::OutputMissing)?
                    .read_for_owner(owner, OutputPosition::Tail(err), err as usize)?,
            )
        } else {
            None
        };
        Ok((out, err))
    }

    /// Explicit release of unread retained output for one authorized lease.
    /// The caller must have independently authorized this Control operation.
    /// Root means every stream owned by this Job, not independent child Jobs.
    /// Validate ALL targets before updating ANY logical ACK.
    pub fn finish_output_for_owner(
        &self,
        id: &str,
        node: Option<usize>,
        view: Option<OutputStream>,
        owner: OutputOwner,
    ) -> Result<OutputAck, Error> {
        let state = self.state.lock().map_err(|_| Error::Poisoned)?;
        let managed = state.jobs.get(id).ok_or(Error::NotFound)?;
        if !managed.visibility.is_exposed() {
            return Err(Error::NotExposed);
        }
        if managed.job.journal_ambiguous() {
            return Err(Error::ReplayUnknown);
        }
        if let Some(node) = node {
            match managed.job.state(node) {
                Some(NodeState::Terminal { status, .. }) if status.primary != Primary::Unknown => {}
                Some(NodeState::Terminal { .. }) => return Err(Error::ReplayUnknown),
                Some(_) => return Err(Error::Busy),
                None => return Err(Error::InvalidOutput),
            }
        } else if managed.job.lifecycle() != Lifecycle::Terminal {
            return Err(Error::Busy);
        } else if (1..=managed.job.total()).any(|coordinate| {
            !matches!(
                managed.job.state(coordinate),
                Some(NodeState::Terminal { status, .. }) if status.primary != Primary::Unknown
            )
        }) {
            // A mixed/summary label is not proof that every effect is known.
            return Err(Error::ReplayUnknown);
        }
        let handles = managed
            .outputs
            .iter()
            .filter(|((coordinate, stream), _)| {
                node.is_none_or(|node| node == *coordinate)
                    && view.is_none_or(|view| view == *stream)
            })
            .map(|(_, output)| Arc::clone(output))
            .collect::<Vec<_>>();
        if handles.is_empty() {
            // A settled Skip (or wholly output-free Graph) has nothing to
            // release. Finish is idempotent for an unqualified output scope;
            // an explicitly requested nonexistent stream remains invalid.
            return if view.is_none() {
                Ok(OutputAck {
                    changed: false,
                    released: true,
                })
            } else {
                Err(Error::OutputMissing)
            };
        }
        // Output handles are in stable node/stream order. The Job state lock
        // prevents a concurrent restart from swapping the output namespace
        // before the logical owner commit.
        let mut outputs = Vec::with_capacity(handles.len());
        for handle in &handles {
            outputs.push(handle.lock().map_err(|_| Error::Poisoned)?);
        }
        for output in &outputs {
            if !output.writer_finished {
                return Err(Error::Busy);
            }
            if !output.owners.contains_key(&owner) {
                return Err(Error::InvalidOutput);
            }
        }
        let mut changed = false;
        for output in &mut outputs {
            if output.released {
                continue;
            }
            let through = output.spool.retained_bytes();
            let ack = output.owners.get_mut(&owner).ok_or(Error::InvalidOutput)?;
            if !ack.acknowledged || ack.through < through {
                ack.through = through;
                ack.acknowledged = true;
                changed = true;
            }
        }
        drop(state);
        // Physical cleanup may fail; logical owner ACKs remain committed and
        // retries may finish cleanup without resurrecting discarded bytes.
        let mut released = true;
        for output in &mut outputs {
            output.maybe_release()?;
            released &= output.released;
        }
        Ok(OutputAck { changed, released })
    }

    /// Strict public ACK: an owner cannot acknowledge bytes not credited by a
    /// trusted host's record_delivered_output after final response projection.
    /// Empty output still requires an explicit zero-byte delivery mark before
    /// offset zero is ACKable.
    pub fn acknowledge_delivered_output(
        &self,
        id: &str,
        coordinate: usize,
        stream: OutputStream,
        owner: OutputOwner,
        through: u64,
    ) -> Result<OutputAck, Error> {
        self.with_known_output_mutation(id, coordinate, stream, |output| {
            if let Some(ack) = output.released_ack(owner, through)? {
                return Ok(ack);
            }
            let current = output.owners.get(&owner).ok_or(Error::InvalidOutput)?;
            if !current.delivered_once || through > current.delivered_through {
                return Err(Error::InvalidOutput);
            }
            if current.acknowledged && through <= current.through {
                return Ok(OutputAck {
                    changed: false,
                    released: false,
                });
            }
            let retained = output.spool.retained_bytes();
            if through > retained {
                return Err(Error::InvalidOutput);
            }
            let current = output.owners.get_mut(&owner).ok_or(Error::InvalidOutput)?;
            current.through = current.through.max(through);
            current.acknowledged = true;
            let released = output.maybe_release()?;
            Ok(OutputAck {
                changed: true,
                released,
            })
        })
    }

    /// Effectful ACK is owner-specific and monotonic. Physical retained bytes
    /// are released only after the writer is finished and every exact owner has
    /// acknowledged through the complete retained prefix.
    pub fn acknowledge_output(
        &self,
        id: &str,
        coordinate: usize,
        stream: OutputStream,
        owner: OutputOwner,
        through: u64,
    ) -> Result<OutputAck, Error> {
        self.with_known_output_mutation(id, coordinate, stream, |output| {
            if let Some(ack) = output.released_ack(owner, through)? {
                return Ok(ack);
            }
            let retained = output.spool.retained_bytes();
            let current = output.owners.get_mut(&owner).ok_or(Error::InvalidOutput)?;
            // Physical prefix compaction never rewrites an owner's logical ACK
            // history. A lost response may therefore replay a watermark beyond the
            // now-retained prefix, but only up to that exact owner's already-recorded
            // high-water mark. New acknowledgements still cannot skip absent bytes.
            if current.acknowledged && through <= current.through {
                return Ok(OutputAck {
                    changed: false,
                    released: false,
                });
            }
            if through > retained {
                return Err(Error::InvalidOutput);
            }
            current.through = current.through.max(through);
            current.acknowledged = true;
            let released = output.maybe_release()?;
            Ok(OutputAck {
                changed: true,
                released,
            })
        })
    }

    fn retained_output_bytes(
        &self,
        id: &str,
        coordinate: usize,
        stream: OutputStream,
        max_bytes: usize,
    ) -> Result<Option<Vec<u8>>, Error> {
        if max_bytes == 0 {
            return Ok(None);
        }
        let first = match self.output_page(id, coordinate, stream, 0, 1) {
            Ok(page) => page,
            Err(Error::OutputMissing) => return Ok(Some(Vec::new())),
            Err(error) => return Err(error),
        };
        if first.retained_bytes > max_bytes as u64 {
            return Ok(None);
        }
        let total = first.retained_bytes as usize;
        if total == 0 {
            return Ok(Some(Vec::new()));
        }
        let mut bytes = Vec::with_capacity(total);
        let mut offset = 0u64;
        while bytes.len() < total {
            let limit = (total - bytes.len()).min(MAX_SPOOL_PAGE_BYTES);
            let page = self.output_page(id, coordinate, stream, offset, limit)?;
            if page.bytes.is_empty() && offset < page.retained_bytes {
                return Err(Error::OutputFailure);
            }
            offset = offset
                .checked_add(page.bytes.len() as u64)
                .ok_or(Error::InvalidOutput)?;
            bytes.extend_from_slice(&page.bytes);
        }
        Ok(Some(bytes))
    }

    fn graph_summary_line(managed: &ManagedJob) -> Result<String, Error> {
        let Some((primary, _, _)) = managed.job.summary() else {
            return Err(Error::Busy);
        };
        let mut success_complete = 0usize;
        let mut more = 0usize;
        let mut incomplete = 0usize;
        let mut fail = 0usize;
        let mut mixed = 0usize;
        let mut unknown = 0usize;
        let mut skip = 0usize;
        for coordinate in 1..=managed.job.total() {
            let Some(NodeState::Terminal { status, .. }) = managed.job.state(coordinate) else {
                return Err(Error::Busy);
            };
            if matches!(status.primary, Primary::Ok | Primary::Unchanged)
                && !status.more
                && !status.incomplete
            {
                success_complete += 1;
            }
            more += usize::from(status.more);
            incomplete += usize::from(status.incomplete);
            match status.primary {
                Primary::Fail => fail += 1,
                Primary::Mixed => mixed += 1,
                Primary::Unknown => unknown += 1,
                Primary::Skip => skip += 1,
                Primary::Ok | Primary::Unchanged => {}
            }
        }
        let mut line = primary.as_str().to_owned();
        let mut fields = Vec::new();
        if matches!(primary, Primary::Mixed | Primary::Unknown) && success_complete != 0 {
            fields.push(format!("ok={success_complete}"));
        }
        if more != 0 {
            fields.push(format!("more={more}"));
        }
        if incomplete != 0 {
            fields.push(format!("incomplete={incomplete}"));
        }
        if matches!(primary, Primary::Mixed | Primary::Unknown) {
            if fail != 0 {
                fields.push(format!("fail={fail}"));
            }
            if mixed != 0 {
                fields.push(format!("mixed={mixed}"));
            }
            if unknown != 0 {
                fields.push(format!("unknown={unknown}"));
            }
        }
        if skip != 0 {
            fields.push(format!("skip={skip}"));
        }
        if !fields.is_empty() {
            line.push(' ');
            line.push_str(&fields.join(" "));
        }
        Ok(line)
    }

    /// Render a completed Graph result in the sealed coordinate-keyed compact
    /// grammar. Returns `None` rather than truncating when the exact result
    /// cannot fit `max_bytes`; callers must expose/page the same Job instead.
    pub fn render_graph_result(&self, id: &str, max_bytes: usize) -> Result<Option<String>, Error> {
        if max_bytes == 0 {
            return Err(Error::InvalidOutput);
        }
        let (states, summary) = {
            let state = self.state.lock().map_err(|_| Error::Poisoned)?;
            let managed = state.jobs.get(id).ok_or(Error::NotFound)?;
            if managed.job.journal_ambiguous() {
                return Err(Error::ReplayUnknown);
            }
            if managed.job.lifecycle() != Lifecycle::Terminal {
                return Err(Error::Busy);
            }
            let states = (1..=managed.job.total())
                .map(|coordinate| managed.job.state(coordinate).ok_or(Error::InvalidPlans))
                .collect::<Result<Vec<_>, _>>()?;
            (states, Self::graph_summary_line(managed)?)
        };
        if summary.len() > max_bytes {
            return Ok(None);
        }
        let mut blocks = Vec::new();
        let mut used = summary.len();
        for (index, state) in states.into_iter().enumerate() {
            let coordinate = index + 1;
            let NodeState::Terminal { status, .. } = state else {
                return Err(Error::Busy);
            };
            if status.primary == Primary::Skip {
                let block = format!("{coordinate} skip condition");
                let extra = block.len() + 2;
                if used.saturating_add(extra) > max_bytes {
                    return Ok(None);
                }
                used += extra;
                blocks.push(block);
                continue;
            }
            let remaining = max_bytes.saturating_sub(used).saturating_sub(2);
            let Some(bytes) =
                self.retained_output_bytes(id, coordinate, OutputStream::Result, remaining)?
            else {
                return Ok(None);
            };
            let text = std::str::from_utf8(&bytes).map_err(|_| Error::OutputFailure)?;
            let trivial_ok = status.primary == Primary::Ok
                && !status.more
                && !status.incomplete
                && text.trim_end() == "ok";
            if trivial_ok {
                continue;
            }
            let block = if matches!(
                status.primary,
                Primary::Fail | Primary::Mixed | Primary::Unknown
            ) || (status.primary == Primary::Unchanged
                && text.trim_end() == "unchanged")
            {
                if text.is_empty() {
                    format!("{coordinate} {}", status.primary.as_str())
                } else {
                    format!("{coordinate} {text}")
                }
            } else if text.is_empty() {
                format!("{coordinate} {}", status.primary.as_str())
            } else {
                format!("{coordinate}\n{text}")
            };
            let extra = block.len() + 2;
            if used.saturating_add(extra) > max_bytes {
                return Ok(None);
            }
            used += extra;
            blocks.push(block);
        }
        let mut result = String::new();
        for block in blocks {
            result.push_str(&block);
            result.push_str("\n\n");
        }
        result.push_str(&summary);
        debug_assert!(result.len() <= max_bytes);
        Ok(Some(result))
    }

    pub fn spool_usage(&self) -> Result<(u64, u64), Error> {
        let quota = self.spool_quota.lock().map_err(|_| Error::Poisoned)?;
        Ok((quota.used(), quota.limit()))
    }

    pub fn claim(
        &self,
        id: &str,
        coordinate: usize,
        source_live: bool,
        sequence: u64,
        persist: impl FnOnce(&str, Transition) -> Result<(), JournalError>,
    ) -> Result<[u8; 32], Error> {
        if !source_live {
            return Err(Error::Job(JobError::SourceRevoked));
        }
        let proposal = {
            let mut state = self.state.lock().map_err(|_| Error::Poisoned)?;
            if state.active_nodes >= self.limits.max_active_nodes {
                return Err(Error::Capacity);
            }
            let managed = state.jobs.get_mut(id).ok_or(Error::NotFound)?;
            if managed.transition_inflight.is_some() {
                return Err(Error::Busy);
            }
            if Self::lower_conflict(managed, coordinate) {
                return Err(Error::Job(JobError::NotReady));
            }
            let proposal = managed
                .job
                .propose(Some(coordinate), Action::Claim, sequence)?;
            managed.transition_inflight = Some(proposal);
            state.active_nodes += 1;
            proposal
        };
        self.commit_transition(id, proposal, true, false, persist)?;
        let state = self.state.lock().map_err(|_| Error::Poisoned)?;
        state
            .jobs
            .get(id)
            .ok_or(Error::NotFound)?
            .job
            .node_key(coordinate)
            .map_err(Into::into)
    }

    pub fn start(
        &self,
        id: &str,
        request: StartRequest,
        persist: impl FnOnce(&str, Transition) -> Result<(), JournalError>,
    ) -> Result<(), Error> {
        if !request.source_live {
            return Err(Error::Job(JobError::SourceRevoked));
        }
        if !request.launch_authorized {
            return Err(Error::Job(JobError::NotPermitted));
        }
        let proposal = {
            let mut state = self.state.lock().map_err(|_| Error::Poisoned)?;
            let managed = state.jobs.get_mut(id).ok_or(Error::NotFound)?;
            if managed.transition_inflight.is_some() {
                return Err(Error::Busy);
            }
            if managed.job.state(request.coordinate)
                != Some(NodeState::Claimed {
                    sequence: request.claimed_sequence,
                })
            {
                return Err(Error::Job(JobError::StaleGeneration));
            }
            if managed.job.lifecycle() != Lifecycle::Running {
                return Err(Error::Job(JobError::NotPermitted));
            }
            let proposal =
                managed
                    .job
                    .propose(Some(request.coordinate), Action::Start, request.sequence)?;
            managed.transition_inflight = Some(proposal);
            proposal
        };
        self.commit_transition(id, proposal, false, false, persist)
    }

    pub fn settle(
        &self,
        id: &str,
        coordinate: usize,
        expected: NodeState,
        result: Status,
        sequence: u64,
        persist: impl FnOnce(&str, Transition) -> Result<(), JournalError>,
    ) -> Result<(), Error> {
        let proposal = {
            let mut state = self.state.lock().map_err(|_| Error::Poisoned)?;
            let managed = state.jobs.get_mut(id).ok_or(Error::NotFound)?;
            if managed.transition_inflight.is_some() {
                return Err(Error::Busy);
            }
            if managed.job.state(coordinate) != Some(expected) || !expected.in_flight() {
                return Err(Error::Job(JobError::StaleGeneration));
            }
            let proposal =
                managed
                    .job
                    .propose(Some(coordinate), Action::Settle(result), sequence)?;
            managed.transition_inflight = Some(proposal);
            proposal
        };
        self.commit_transition(id, proposal, false, true, persist)
    }

    /// Common post-launch process settlement. Adapter-specific runners
    /// produce `ProcessTerminal`; the shared manager owns Graph classification.
    pub fn settle_process(
        &self,
        id: &str,
        coordinate: usize,
        expected: NodeState,
        terminal: ProcessTerminal,
        sequence: u64,
        persist: impl FnOnce(&str, Transition) -> Result<(), JournalError>,
    ) -> Result<Status, Error> {
        let status = terminal.status();
        self.settle_process_with_status(
            id,
            ProcessSettlement {
                coordinate,
                expected,
                terminal,
                status,
                sequence,
            },
            persist,
        )
    }

    pub fn settle_process_with_status(
        &self,
        id: &str,
        settlement: ProcessSettlement,
        persist: impl FnOnce(&str, Transition) -> Result<(), JournalError>,
    ) -> Result<Status, Error> {
        self.settle(
            id,
            settlement.coordinate,
            settlement.expected,
            settlement.status,
            settlement.sequence,
            persist,
        )?;
        let mut state = self.state.lock().map_err(|_| Error::Poisoned)?;
        let managed = state.jobs.get_mut(id).ok_or(Error::NotFound)?;
        if !matches!(
            managed.job.state(settlement.coordinate),
            Some(NodeState::Terminal { .. })
        ) {
            return Err(Error::InvalidOutput);
        }
        managed
            .process_terminals
            .insert(settlement.coordinate, settlement.terminal);
        Ok(settlement.status)
    }

    pub fn settle_unlaunched(
        &self,
        id: &str,
        coordinate: usize,
        expected: NodeState,
        sequence: u64,
        prove_no_launch: impl FnOnce([u8; 32], u64) -> Result<(), JobError>,
        persist: impl FnOnce(&str, Transition) -> Result<(), JournalError>,
    ) -> Result<(), Error> {
        let expected_sequence = match expected {
            NodeState::Claimed { sequence } | NodeState::Running { sequence } => sequence,
            NodeState::Pending | NodeState::Terminal { .. } => {
                return Err(Error::Job(JobError::InvalidState));
            }
        };
        let (proposal, key) = {
            let mut state = self.state.lock().map_err(|_| Error::Poisoned)?;
            let managed = state.jobs.get_mut(id).ok_or(Error::NotFound)?;
            if managed.transition_inflight.is_some() {
                return Err(Error::Busy);
            }
            if managed.job.state(coordinate) != Some(expected) {
                return Err(Error::Job(JobError::StaleGeneration));
            }
            let proposal =
                managed
                    .job
                    .propose(Some(coordinate), Action::UnlaunchedSkip, sequence)?;
            let key = managed.job.node_key(coordinate)?;
            managed.transition_inflight = Some(proposal);
            (proposal, key)
        };
        let proof =
            std::panic::catch_unwind(AssertUnwindSafe(|| prove_no_launch(key, expected_sequence)));
        if !matches!(proof, Ok(Ok(()))) {
            let mut state = self.state.lock().map_err(|_| Error::Poisoned)?;
            if let Some(managed) = state.jobs.get_mut(id) {
                if managed.transition_inflight == Some(proposal) {
                    managed.transition_inflight = None;
                }
            }
            return match proof {
                Ok(Err(error)) => Err(Error::Job(error)),
                Err(_) => Err(Error::Job(JobError::NotPermitted)),
                Ok(Ok(())) => unreachable!(),
            };
        }

        // A proven-unlaunched node can no longer produce output. Seal every
        // stream attached during preflight before publishing the terminal Skip,
        // so adapters cannot strand terminal Jobs by forgetting one output
        // writer on a pre-Start failure.
        let seal = (|| -> Result<(), Error> {
            let outputs = {
                let state = self.state.lock().map_err(|_| Error::Poisoned)?;
                let managed = state.jobs.get(id).ok_or(Error::NotFound)?;
                if managed.transition_inflight != Some(proposal) {
                    return Err(Error::Job(JobError::JournalUnknown));
                }
                managed
                    .outputs
                    .iter()
                    .filter(|((node, _), _)| *node == coordinate)
                    .map(|(_, output)| output.clone())
                    .collect::<Vec<_>>()
            };
            for output in outputs {
                output
                    .lock()
                    .map_err(|_| Error::Poisoned)?
                    .finish_unlaunched()?;
            }
            Ok(())
        })();
        if let Err(error) = seal {
            let mut state = self.state.lock().map_err(|_| Error::Poisoned)?;
            if let Some(managed) = state.jobs.get_mut(id) {
                if managed.transition_inflight == Some(proposal) {
                    managed.transition_inflight = None;
                }
            }
            return Err(error);
        }

        self.commit_transition(id, proposal, false, true, persist)
    }

    pub fn skip(
        &self,
        id: &str,
        coordinate: usize,
        sequence: u64,
        persist: impl FnOnce(&str, Transition) -> Result<(), JournalError>,
    ) -> Result<(), Error> {
        let proposal = {
            let mut state = self.state.lock().map_err(|_| Error::Poisoned)?;
            let managed = state.jobs.get_mut(id).ok_or(Error::NotFound)?;
            if managed.transition_inflight.is_some() {
                return Err(Error::Busy);
            }
            let proposal = managed
                .job
                .propose(Some(coordinate), Action::Skip, sequence)?;
            managed.transition_inflight = Some(proposal);
            proposal
        };
        self.commit_transition(id, proposal, false, false, persist)
    }

    fn stop(
        &self,
        id: &str,
        revoke: bool,
        sequence: u64,
        persist: impl FnOnce(&str, Transition) -> Result<(), JournalError>,
    ) -> Result<(), Error> {
        let proposal = {
            let mut state = self.state.lock().map_err(|_| Error::Poisoned)?;
            let managed = state.jobs.get_mut(id).ok_or(Error::NotFound)?;
            if managed.transition_inflight.is_some() {
                return Err(Error::Busy);
            }
            match (managed.job.lifecycle(), revoke) {
                (Lifecycle::Terminal, _)
                | (Lifecycle::Revoking, _)
                | (Lifecycle::Cancelling, false) => return Ok(()),
                (Lifecycle::Running, _) | (Lifecycle::Cancelling, true) => {}
            }
            let action = if revoke {
                Action::Revoke
            } else {
                Action::Cancel
            };
            let proposal = managed.job.propose(None, action, sequence)?;
            managed.transition_inflight = Some(proposal);
            proposal
        };
        self.commit_transition(id, proposal, false, false, persist)
    }

    pub fn cancel(
        &self,
        id: &str,
        sequence: u64,
        persist: impl FnOnce(&str, Transition) -> Result<(), JournalError>,
    ) -> Result<(), Error> {
        self.stop(id, false, sequence, persist)
    }

    pub fn revoke(
        &self,
        id: &str,
        sequence: u64,
        persist: impl FnOnce(&str, Transition) -> Result<(), JournalError>,
    ) -> Result<(), Error> {
        self.stop(id, true, sequence, persist)
    }

    #[cfg(test)]
    fn active_nodes(&self) -> usize {
        self.state.lock().unwrap().active_nodes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn job_limits_use_shared_safety_maxima_not_one_hosts_defaults() {
        let limits = Limits {
            max_retained_jobs: DEFAULT_MAX_RETAINED_JOBS + 1,
            max_exposed_jobs: DEFAULT_MAX_RETAINED_JOBS + 1,
            max_hidden_jobs: DEFAULT_MAX_RETAINED_JOBS + 1,
            max_active_nodes: DEFAULT_MAX_ACTIVE_NODES + 1,
            max_hidden_replays: MAX_HIDDEN_REQUEST_REPLAYS + 1,
            max_spool_bytes: DEFAULT_SESSION_SPOOL + 1,
        };
        assert_eq!(limits.validate().unwrap(), limits);
        let too_wide = Limits {
            max_spool_bytes: MAX_SESSION_SPOOL + 1,
            ..Limits::default()
        };
        assert!(too_wide.validate().is_err());
        assert!(Limits {
            max_retained_jobs: 4,
            max_exposed_jobs: 5,
            ..Limits::default()
        }
        .validate()
        .is_err());
        assert!(Limits {
            max_retained_jobs: 4,
            max_hidden_jobs: 5,
            ..Limits::default()
        }
        .validate()
        .is_err());
    }

    use crate::{
        filesystem::{
            batch::Graph,
            footprint::{Access, Extent, Scope},
        },
        ToolEffect,
    };
    use serde_json::json;
    use std::{
        fs::OpenOptions,
        sync::atomic::{AtomicBool, AtomicU64, Ordering},
    };
    static NEXT_OUTPUT: AtomicU64 = AtomicU64::new(1);
    fn output_file() -> (File, std::path::PathBuf) {
        let path = std::env::temp_dir().join(format!(
            "aw-job-output-{}-{}",
            std::process::id(),
            NEXT_OUTPUT.fetch_add(1, Ordering::Relaxed)
        ));
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&path)
            .unwrap();
        (file, path)
    }

    fn claimed_output_job(id: &str, max_spool_bytes: u64) -> (JobManager, OutputOwner) {
        let graph = Graph::compile(&json!({"ops":[["read",{"path":"a"}]]}), |_, _| true).unwrap();
        let plans = plan_graph(&graph, |_, _| {
            Ok::<_, ()>(NodePolicy {
                budget: SemanticBudget {
                    work_bytes: 1,
                    result_bytes: 16,
                },
                footprint: Footprint::none(),
                registry_generation: 1,
                authority_binding: [1; 32],
                runtime_binding: [2; 32],
                task_result_eligible: false,
            })
        })
        .unwrap();
        let manager = JobManager::new(Limits {
            max_retained_jobs: 2,
            max_exposed_jobs: 2,
            max_hidden_jobs: 2,
            max_active_nodes: 2,
            max_hidden_replays: 2,
            max_spool_bytes,
        })
        .unwrap();
        manager
            .admit_runtime(
                graph,
                plans,
                RuntimeAdmission {
                    id,
                    runtime_session: "session",
                    epoch: 1,
                    admission_sequence: 1,
                    width: 1,
                    exposed: true,
                },
            )
            .unwrap();
        manager.claim(id, 1, true, 2, |_, _| Ok(())).unwrap();
        let owner = manager.job_output_owner(id).unwrap();
        (manager, owner)
    }

    #[test]
    fn settle_unlaunched_seals_preattached_output_before_terminal_expiry() {
        let (manager, owner) = claimed_output_job("unlaunched-output", 1024);
        manager
            .attach_output_reserved("unlaunched-output", 1, OutputStream::Result, [owner], 16)
            .unwrap();

        manager
            .settle_unlaunched(
                "unlaunched-output",
                1,
                NodeState::Claimed { sequence: 2 },
                3,
                |_, observed_claim| {
                    assert_eq!(observed_claim, 2);
                    Ok(())
                },
                |_, _| Ok(()),
            )
            .unwrap();

        assert!(matches!(
            manager.node_state("unlaunched-output", 1).unwrap(),
            NodeState::Terminal {
                status: Status {
                    primary: Primary::Skip,
                    ..
                },
                ..
            }
        ));
        assert!(
            manager
                .output_page("unlaunched-output", 1, OutputStream::Result, 0, 1)
                .unwrap()
                .writer_finished
        );
        manager.expire_terminal("unlaunched-output").unwrap();
        assert!(matches!(
            manager.runtime_snapshot("unlaunched-output"),
            Err(Error::NotFound)
        ));
    }

    #[test]
    fn output_attachment_is_fenced_while_unlaunched_settlement_is_inflight() {
        let (manager, owner) = claimed_output_job("unlaunched-attach-race", 1024);
        manager
            .settle_unlaunched(
                "unlaunched-attach-race",
                1,
                NodeState::Claimed { sequence: 2 },
                3,
                |_, observed_claim| {
                    assert_eq!(observed_claim, 2);
                    Ok(())
                },
                |_, _| {
                    assert!(matches!(
                        manager.attach_output_reserved(
                            "unlaunched-attach-race",
                            1,
                            OutputStream::Result,
                            [owner],
                            16,
                        ),
                        Err(Error::InvalidOutput)
                    ));
                    Ok(())
                },
            )
            .unwrap();
        assert!(matches!(
            manager.output_page("unlaunched-attach-race", 1, OutputStream::Result, 0, 1,),
            Err(Error::OutputMissing)
        ));
        manager.expire_terminal("unlaunched-attach-race").unwrap();
    }

    #[test]
    fn settle_unlaunched_does_not_strand_claim_when_preattached_output_failed() {
        let (manager, owner) = claimed_output_job("unlaunched-failed-output", 1024);
        manager
            .attach_output_reserved(
                "unlaunched-failed-output",
                1,
                OutputStream::Result,
                [owner],
                16,
            )
            .unwrap();
        manager
            .mark_output_failed("unlaunched-failed-output", 1, OutputStream::Result, 0)
            .unwrap();

        manager
            .settle_unlaunched(
                "unlaunched-failed-output",
                1,
                NodeState::Claimed { sequence: 2 },
                3,
                |_, observed_claim| {
                    assert_eq!(observed_claim, 2);
                    Ok(())
                },
                |_, _| Ok(()),
            )
            .unwrap();

        assert!(matches!(
            manager.node_state("unlaunched-failed-output", 1).unwrap(),
            NodeState::Terminal {
                status: Status {
                    primary: Primary::Skip,
                    ..
                },
                ..
            }
        ));
        assert_eq!(
            manager
                .runner_load("unlaunched-failed-output")
                .unwrap()
                .job_active,
            0
        );
        manager.expire_terminal("unlaunched-failed-output").unwrap();
    }

    fn graph() -> Graph {
        Graph::compile(
            &json!({"ops":[
                ["write",{"path":"src/a","content":"x"},"*"],
                ["read",{"path":"src/a"},"*"],
                ["read",{"path":"docs/a"},"*"]
            ]}),
            |_, _| true,
        )
        .unwrap()
    }

    fn footprint(path: &str, access: Access, extent: Extent) -> Footprint {
        Footprint::new(
            vec![Scope {
                path: path.into(),
                access,
                extent,
            }],
            false,
            false,
        )
        .unwrap()
    }

    fn plans(graph: &Graph) -> Vec<NodePlan> {
        plan_graph(graph, |coordinate, _| {
            let footprint = match coordinate {
                1 => footprint("src/a", Access::Write, Extent::Exact),
                2 => footprint("src/a", Access::Read, Extent::Exact),
                _ => footprint("docs/a", Access::Read, Extent::Exact),
            };
            Ok::<_, ()>(NodePolicy {
                budget: SemanticBudget {
                    work_bytes: 1024,
                    result_bytes: 4096,
                },
                footprint,
                registry_generation: 1,
                authority_binding: [coordinate as u8; 32],
                runtime_binding: [0xA5; 32],
                task_result_eligible: true,
            })
        })
        .unwrap()
    }

    fn manager(active: usize) -> JobManager {
        JobManager::new(Limits {
            max_retained_jobs: 256,
            max_exposed_jobs: 256,
            max_hidden_jobs: 256,
            max_active_nodes: active,
            max_hidden_replays: 64,
            max_spool_bytes: 1024 * 1024,
        })
        .unwrap()
    }

    #[test]
    fn worst_case_compact_job_status_fits_control_plane_bound() {
        use crate::filesystem::runner::{ProcessExit, ProcessOutputMeta};
        let ops = (0..crate::filesystem::batch::MAX_OPS)
            .map(|index| {
                serde_json::json!([
                    "bash",
                    {"command": format!("echo {index}")},
                    "*"
                ])
            })
            .collect::<Vec<_>>();
        let graph = Graph::compile_with(
            &serde_json::json!({"ops": ops}),
            |name| (name == "bash").then_some(NodeTool::Bash),
            |_, arguments| arguments.is_object(),
        )
        .unwrap();
        let node_plans = plan_graph(&graph, |coordinate, _| {
            Ok::<_, ()>(NodePolicy {
                budget: SemanticBudget {
                    work_bytes: 1,
                    result_bytes: 1,
                },
                footprint: Footprint::none(),
                registry_generation: 1,
                authority_binding: [coordinate as u8; 32],
                runtime_binding: [0x7B; 32],
                task_result_eligible: false,
            })
        })
        .unwrap();
        let manager = manager(1);
        manager
            .admit_runtime(
                graph,
                node_plans,
                RuntimeAdmission {
                    id: "max-status",
                    runtime_session: "session",
                    epoch: 1,
                    admission_sequence: 1,
                    width: 1,
                    exposed: true,
                },
            )
            .unwrap();
        for coordinate in 1..=crate::filesystem::batch::MAX_OPS {
            let claim_sequence = manager.current_sequence("max-status").unwrap() + 1;
            manager
                .claim(
                    "max-status",
                    coordinate,
                    true,
                    claim_sequence,
                    |_, _| Ok(()),
                )
                .unwrap();
            let start_sequence = manager.current_sequence("max-status").unwrap() + 1;
            manager
                .start(
                    "max-status",
                    StartRequest {
                        coordinate,
                        claimed_sequence: claim_sequence,
                        source_live: true,
                        launch_authorized: true,
                        sequence: start_sequence,
                    },
                    |_, _| Ok(()),
                )
                .unwrap();
            let settle_sequence = manager.current_sequence("max-status").unwrap() + 1;
            manager
                .settle_process_with_status(
                    "max-status",
                    ProcessSettlement {
                        coordinate,
                        expected: NodeState::Running {
                            sequence: start_sequence,
                        },
                        terminal: ProcessTerminal {
                            exit: ProcessExit::Code(i32::MIN + coordinate as i32),
                            stop: None,
                            occupancy: crate::filesystem::runner::Occupancy::Unknown,
                            output: ProcessOutputMeta {
                                stdout_bytes: u64::MAX,
                                stderr_bytes: u64::MAX,
                                stdout_truncated: true,
                                stderr_truncated: true,
                                stdout_complete: false,
                                stderr_complete: false,
                            },
                        },
                        status: Status {
                            primary: Primary::Unknown,
                            more: true,
                            incomplete: true,
                        },
                        sequence: settle_sequence,
                    },
                    |_, _| Ok(()),
                )
                .unwrap();
        }
        let rendered = manager.render_job_status("max-status").unwrap();
        assert!(
            rendered.len() <= MAX_COMPACT_JOB_STATUS_BYTES,
            "compact status grew to {} bytes: {rendered}",
            rendered.len()
        );
    }

    #[test]
    fn node_plan_captures_semantic_budget_and_changes_replay_identity() {
        let graph = graph();
        let base = plans(&graph);
        let changed = plan_graph(&graph, |coordinate, _| {
            Ok::<_, ()>(NodePolicy {
                budget: SemanticBudget {
                    work_bytes: if coordinate == 1 { 2048 } else { 1024 },
                    result_bytes: 4096,
                },
                footprint: base[coordinate - 1].footprint.clone(),
                registry_generation: 1,
                authority_binding: [coordinate as u8; 32],
                runtime_binding: [0xA5; 32],
                task_result_eligible: true,
            })
        })
        .unwrap();
        assert_eq!(base[0].budget().result_bytes, 4096);
        assert_ne!(base[0].replay_identity(), changed[0].replay_identity());
        assert_eq!(base[1].replay_identity(), changed[1].replay_identity());
    }

    #[test]
    fn lower_coordinate_conflict_gate_is_deterministic() {
        let graph = graph();
        let manager = manager(8);
        manager
            .admit_runtime(
                graph.clone(),
                plans(&graph),
                RuntimeAdmission {
                    id: "j",
                    runtime_session: "session",
                    epoch: 1,
                    admission_sequence: 1,
                    width: 8,
                    exposed: true,
                },
            )
            .unwrap();
        assert_eq!(manager.runnable("j").unwrap(), [1, 3]);
        manager.claim("j", 1, true, 2, |_, _| Ok(())).unwrap();
        manager
            .start(
                "j",
                StartRequest {
                    coordinate: 1,
                    claimed_sequence: 2,
                    source_live: true,
                    launch_authorized: true,
                    sequence: 3,
                },
                |_, _| Ok(()),
            )
            .unwrap();
        manager
            .settle(
                "j",
                1,
                NodeState::Running { sequence: 3 },
                Status {
                    primary: Primary::Ok,
                    more: false,
                    incomplete: false,
                },
                4,
                |_, _| Ok(()),
            )
            .unwrap();
        assert_eq!(manager.runnable("j").unwrap(), [2, 3]);
    }

    #[test]
    fn host_journal_callback_runs_without_manager_lock() {
        let graph = graph();
        let manager = manager(8);
        manager
            .admit_runtime(
                graph.clone(),
                plans(&graph),
                RuntimeAdmission {
                    id: "j",
                    runtime_session: "session",
                    epoch: 1,
                    admission_sequence: 1,
                    width: 8,
                    exposed: true,
                },
            )
            .unwrap();
        let reentered = AtomicBool::new(false);
        manager
            .claim("j", 1, true, 2, |_, _| {
                assert_eq!(manager.list(0, 1).unwrap().jobs.len(), 1);
                reentered.store(true, Ordering::SeqCst);
                Ok(())
            })
            .unwrap();
        assert!(reentered.load(Ordering::SeqCst));
    }

    #[cfg(feature = "async")]
    #[test]
    fn mismatched_journal_transition_wakes_observers_on_unknown_fence() {
        let graph = graph();
        let manager = manager(8);
        manager
            .admit_runtime(
                graph.clone(),
                plans(&graph),
                RuntimeAdmission {
                    id: "mismatched-watch",
                    runtime_session: "session",
                    epoch: 1,
                    admission_sequence: 1,
                    width: 8,
                    exposed: true,
                },
            )
            .unwrap();
        let mut watch = manager.subscribe_changes("mismatched-watch").unwrap();
        watch.borrow_and_update();
        assert_eq!(
            manager.claim("mismatched-watch", 1, true, 2, |_, proposed| {
                assert_eq!(proposed.action, Action::Claim);
                // Simulates the defensive mismatch path when the in-flight
                // proposal is no longer the one the journal callback returns.
                manager
                    .state
                    .lock()
                    .unwrap()
                    .jobs
                    .get_mut("mismatched-watch")
                    .unwrap()
                    .transition_inflight = None;
                Ok(())
            }),
            Err(Error::Job(JobError::JournalUnknown))
        );
        assert!(manager
            .state
            .lock()
            .unwrap()
            .jobs
            .get("mismatched-watch")
            .unwrap()
            .job
            .journal_ambiguous());
        assert!(
            watch.has_changed().unwrap(),
            "a newly unknown journal must notify existing finite observers"
        );
    }

    #[test]
    fn unknown_claim_fences_job_and_keeps_global_permit_reserved() {
        let graph = graph();
        let manager = manager(1);
        manager
            .admit_runtime(
                graph.clone(),
                plans(&graph),
                RuntimeAdmission {
                    id: "j",
                    runtime_session: "session",
                    epoch: 1,
                    admission_sequence: 1,
                    width: 8,
                    exposed: true,
                },
            )
            .unwrap();
        assert_eq!(
            manager.claim("j", 1, true, 2, |_, _| Err(JournalError::OutcomeUnknown)),
            Err(Error::Job(JobError::JournalUnknown))
        );
        assert_eq!(manager.active_nodes(), 1);
        assert!(manager.runnable("j").unwrap().is_empty());
        let snapshot = manager.get("j").unwrap();
        assert_eq!(
            snapshot.lifecycle,
            Lifecycle::Running,
            "journal ambiguity is not terminal"
        );
        assert_eq!(
            snapshot.status_label(),
            "unknown",
            "an ambiguous journal cannot be presented as an ordinary running Job"
        );
        assert!(
            manager
                .render_job_status("j")
                .unwrap()
                .starts_with("unknown graph"),
            "read-only status must disclose the unknown outcome"
        );
    }

    #[test]
    fn typed_process_terminal_projection_returns_only_settled_process_facts() {
        use crate::filesystem::runner::{ProcessExit, ProcessOutputMeta};
        let graph = Graph::compile_with(
            &json!({"ops":[["bash",{"command":"true"}]]}),
            |name| (name == "bash").then_some(NodeTool::Bash),
            |node, arguments| node == NodeTool::Bash && arguments.is_object(),
        )
        .unwrap();
        let node_plans = plans(&graph);
        let manager = manager(1);
        manager
            .admit_runtime(
                graph,
                node_plans,
                RuntimeAdmission {
                    id: "terminal-facts",
                    runtime_session: "s",
                    epoch: 1,
                    admission_sequence: 1,
                    width: 1,
                    exposed: true,
                },
            )
            .unwrap();
        assert_eq!(manager.process_terminal("terminal-facts", 1).unwrap(), None);
        assert_eq!(
            manager.process_terminal("terminal-facts", 2),
            Err(Error::InvalidPlans)
        );
        manager
            .claim("terminal-facts", 1, true, 2, |_, _| Ok(()))
            .unwrap();
        manager
            .start(
                "terminal-facts",
                StartRequest {
                    coordinate: 1,
                    claimed_sequence: 2,
                    source_live: true,
                    launch_authorized: true,
                    sequence: 3,
                },
                |_, _| Ok(()),
            )
            .unwrap();
        let terminal = ProcessTerminal {
            exit: ProcessExit::Code(7),
            stop: None,
            occupancy: crate::filesystem::runner::Occupancy::Empty,
            output: ProcessOutputMeta {
                stdout_bytes: 11,
                stderr_bytes: 3,
                stdout_truncated: false,
                stderr_truncated: true,
                stdout_complete: true,
                stderr_complete: true,
            },
        };
        manager
            .settle_process(
                "terminal-facts",
                1,
                NodeState::Running { sequence: 3 },
                terminal,
                4,
                |_, _| Ok(()),
            )
            .unwrap();
        assert_eq!(
            manager.process_terminal("terminal-facts", 1).unwrap(),
            Some(terminal)
        );
    }

    #[test]
    fn compact_job_status_keeps_cancelled_lifecycle_separate_from_node_outcome() {
        let graph = Graph::compile(
            &json!({"ops":[
                ["read",{"path":"a"}],
                ["read",{"path":"b"},"1.ok"]
            ]}),
            |_, _| true,
        )
        .unwrap();
        let plans = plan_graph(&graph, |_, _| {
            Ok::<_, ()>(NodePolicy {
                budget: SemanticBudget {
                    work_bytes: 1,
                    result_bytes: 1024,
                },
                footprint: Footprint::none(),
                registry_generation: 1,
                authority_binding: [1; 32],
                runtime_binding: [2; 32],
                task_result_eligible: false,
            })
        })
        .unwrap();
        let manager = manager(2);
        manager
            .admit_runtime(
                graph,
                plans,
                RuntimeAdmission {
                    id: "cancelled",
                    runtime_session: "session",
                    epoch: 1,
                    admission_sequence: 1,
                    width: 1,
                    exposed: true,
                },
            )
            .unwrap();
        manager
            .claim("cancelled", 1, true, 2, |_, _| Ok(()))
            .unwrap();
        manager
            .start(
                "cancelled",
                StartRequest {
                    coordinate: 1,
                    claimed_sequence: 2,
                    source_live: true,
                    launch_authorized: true,
                    sequence: 3,
                },
                |_, _| Ok(()),
            )
            .unwrap();
        manager.cancel("cancelled", 4, |_, _| Ok(())).unwrap();
        manager
            .settle(
                "cancelled",
                1,
                NodeState::Running { sequence: 3 },
                Status {
                    primary: Primary::Ok,
                    more: false,
                    incomplete: false,
                },
                5,
                |_, _| Ok(()),
            )
            .unwrap();
        manager.skip("cancelled", 2, 6, |_, _| Ok(())).unwrap();
        let snapshot = manager.get("cancelled").unwrap();
        assert_eq!(snapshot.stop_reason, Some(StopReason::Cancelled));
        assert_eq!(snapshot.list_row(), "cancelled graph cancelled");
        assert_eq!(
            manager.render_job_status("cancelled").unwrap(),
            "cancelled graph 2/2\n1 ok\n2 skip"
        );
    }

    fn admit_simple(manager: &JobManager, id: &str, exposed: bool) {
        let graph = Graph::compile(&json!({"ops":[["read",{"path":"a"}]]}), |_, _| true).unwrap();
        let plans = plan_graph(&graph, |_, _| {
            Ok::<_, ()>(NodePolicy {
                budget: SemanticBudget {
                    work_bytes: 1,
                    result_bytes: 64,
                },
                footprint: Footprint::none(),
                registry_generation: 1,
                authority_binding: [1; 32],
                runtime_binding: [2; 32],
                task_result_eligible: false,
            })
        })
        .unwrap();
        manager
            .admit_runtime(
                graph,
                plans,
                RuntimeAdmission {
                    id,
                    runtime_session: "session",
                    epoch: 1,
                    admission_sequence: 1,
                    width: 1,
                    exposed,
                },
            )
            .unwrap();
    }

    fn terminal_no_output_with_visibility(manager: &JobManager, id: &str, exposed: bool) {
        let graph = Graph::compile(&json!({"ops":[["read",{"path":"a"}]]}), |_, _| true).unwrap();
        let plans = plan_graph(&graph, |_, _| {
            Ok::<_, ()>(NodePolicy {
                budget: SemanticBudget {
                    work_bytes: 1,
                    result_bytes: 64,
                },
                footprint: Footprint::none(),
                registry_generation: 1,
                authority_binding: [1; 32],
                runtime_binding: [2; 32],
                task_result_eligible: false,
            })
        })
        .unwrap();
        manager
            .admit_runtime(
                graph,
                plans,
                RuntimeAdmission {
                    id,
                    runtime_session: "session",
                    epoch: 1,
                    admission_sequence: 1,
                    width: 1,
                    exposed,
                },
            )
            .unwrap();
        manager.claim(id, 1, true, 2, |_, _| Ok(())).unwrap();
        manager
            .start(
                id,
                StartRequest {
                    coordinate: 1,
                    claimed_sequence: 2,
                    source_live: true,
                    launch_authorized: true,
                    sequence: 3,
                },
                |_, _| Ok(()),
            )
            .unwrap();
        manager
            .settle(
                id,
                1,
                NodeState::Running { sequence: 3 },
                Status {
                    primary: Primary::Ok,
                    more: false,
                    incomplete: false,
                },
                4,
                |_, _| Ok(()),
            )
            .unwrap();
    }

    fn terminal_no_output(manager: &JobManager, id: &str) {
        terminal_no_output_with_visibility(manager, id, true);
    }

    #[test]
    fn host_retention_expiry_can_drop_unacknowledged_terminal_payload() {
        let graph = Graph::compile(&json!({"ops":[["read",{"path":"a"}]]}), |_, _| true).unwrap();
        let plans = plan_graph(&graph, |_, _| {
            Ok::<_, ()>(NodePolicy {
                budget: SemanticBudget {
                    work_bytes: 1,
                    result_bytes: 64,
                },
                footprint: Footprint::none(),
                registry_generation: 1,
                authority_binding: [1; 32],
                runtime_binding: [2; 32],
                task_result_eligible: false,
            })
        })
        .unwrap();
        let manager = manager(2);
        manager
            .admit_runtime(
                graph,
                plans,
                RuntimeAdmission {
                    id: "expire",
                    runtime_session: "session",
                    epoch: 1,
                    admission_sequence: 1,
                    width: 1,
                    exposed: true,
                },
            )
            .unwrap();
        manager.claim("expire", 1, true, 2, |_, _| Ok(())).unwrap();
        manager
            .start(
                "expire",
                StartRequest {
                    coordinate: 1,
                    claimed_sequence: 2,
                    source_live: true,
                    launch_authorized: true,
                    sequence: 3,
                },
                |_, _| Ok(()),
            )
            .unwrap();
        let owner = manager.job_output_owner("expire").unwrap();
        manager
            .attach_output_reserved("expire", 1, OutputStream::Result, [owner], 16)
            .unwrap();
        manager
            .append_output("expire", 1, OutputStream::Result, b"payload")
            .unwrap();
        manager
            .finish_output("expire", 1, OutputStream::Result)
            .unwrap();
        manager
            .settle(
                "expire",
                1,
                NodeState::Running { sequence: 3 },
                Status {
                    primary: Primary::Ok,
                    more: false,
                    incomplete: false,
                },
                4,
                |_, _| Ok(()),
            )
            .unwrap();
        assert_eq!(manager.spool_usage().unwrap().0, 7);
        manager.expire_terminal("expire").unwrap();
        assert!(matches!(manager.get("expire"), Err(Error::NotFound)));
        assert_eq!(manager.spool_usage().unwrap().0, 0);
    }

    #[test]
    fn host_retention_expiry_refuses_live_and_replay_owned_jobs() {
        let graph = Graph::compile(&json!({"ops":[["read",{"path":"a"}]]}), |_, _| true).unwrap();
        let plans = plan_graph(&graph, |_, _| {
            Ok::<_, ()>(NodePolicy {
                budget: SemanticBudget {
                    work_bytes: 1,
                    result_bytes: 64,
                },
                footprint: Footprint::none(),
                registry_generation: 1,
                authority_binding: [1; 32],
                runtime_binding: [2; 32],
                task_result_eligible: false,
            })
        })
        .unwrap();
        let manager = manager(2);
        manager
            .admit_runtime(
                graph,
                plans,
                RuntimeAdmission {
                    id: "live",
                    runtime_session: "session",
                    epoch: 1,
                    admission_sequence: 1,
                    width: 1,
                    exposed: true,
                },
            )
            .unwrap();
        assert_eq!(manager.expire_terminal("live"), Err(Error::Busy));
        manager.claim("live", 1, true, 2, |_, _| Ok(())).unwrap();
        manager
            .start(
                "live",
                StartRequest {
                    coordinate: 1,
                    claimed_sequence: 2,
                    source_live: true,
                    launch_authorized: true,
                    sequence: 3,
                },
                |_, _| Ok(()),
            )
            .unwrap();
        manager
            .settle(
                "live",
                1,
                NodeState::Running { sequence: 3 },
                Status {
                    primary: Primary::Ok,
                    more: false,
                    incomplete: false,
                },
                4,
                |_, _| Ok(()),
            )
            .unwrap();
        let decision = manager
            .claim_replay("ns", "request", [7; 32], "live", |_| {
                Ok(ReplayPersist::Created)
            })
            .unwrap();
        let ReplayDecision::New { key, .. } = decision else {
            panic!("expected replay claim");
        };
        manager.mark_replay_ready(key, "live", |_| Ok(())).unwrap();
        assert_eq!(manager.expire_terminal("live"), Err(Error::Busy));
        assert!(manager.get("live").is_ok());
    }

    #[test]
    fn provisional_unstarted_job_can_be_discarded_without_erasing_claim_tombstone() {
        let manager = JobManager::new(Limits {
            max_retained_jobs: 1,
            max_exposed_jobs: 1,
            max_hidden_jobs: 1,
            max_active_nodes: 1,
            max_hidden_replays: 2,
            max_spool_bytes: 1024,
        })
        .unwrap();
        let graph = Graph::compile(&json!({"ops":[["read",{"path":"a"}]]}), |_, _| true).unwrap();
        let node_plans = plan_graph(&graph, |_, _| {
            Ok::<_, ()>(NodePolicy {
                budget: SemanticBudget {
                    work_bytes: 1,
                    result_bytes: 64,
                },
                footprint: Footprint::none(),
                registry_generation: 1,
                authority_binding: [1; 32],
                runtime_binding: [2; 32],
                task_result_eligible: false,
            })
        })
        .unwrap();
        manager
            .admit_runtime(
                graph,
                node_plans,
                RuntimeAdmission {
                    id: "provisional",
                    runtime_session: "session",
                    epoch: 1,
                    admission_sequence: 1,
                    width: 1,
                    exposed: false,
                },
            )
            .unwrap();
        let decision = manager
            .claim_replay("ns", "request", [7; 32], "provisional", |_| {
                Ok(ReplayPersist::Created)
            })
            .unwrap();
        let ReplayDecision::New { key, .. } = decision else {
            panic!("expected a new replay claim");
        };
        manager.discard_unstarted("provisional").unwrap();
        assert!(matches!(
            manager.runtime_snapshot("provisional"),
            Err(Error::NotFound)
        ));
        assert_eq!(
            manager.replay_record(key).unwrap().disposition,
            ReplayDisposition::Claimed
        );
        assert_eq!(
            manager.lookup_replay("ns", "request", [7; 32]).unwrap(),
            Some(ReplayDecision::Unknown)
        );
    }

    #[test]
    fn ready_replay_prevents_discarding_its_provisional_job() {
        let manager = JobManager::new(Limits {
            max_retained_jobs: 1,
            max_exposed_jobs: 1,
            max_hidden_jobs: 1,
            max_active_nodes: 1,
            max_hidden_replays: 2,
            max_spool_bytes: 1024,
        })
        .unwrap();
        let graph = Graph::compile(&json!({"ops":[["read",{"path":"a"}]]}), |_, _| true).unwrap();
        let node_plans = plan_graph(&graph, |_, _| {
            Ok::<_, ()>(NodePolicy {
                budget: SemanticBudget {
                    work_bytes: 1,
                    result_bytes: 64,
                },
                footprint: Footprint::none(),
                registry_generation: 1,
                authority_binding: [1; 32],
                runtime_binding: [2; 32],
                task_result_eligible: false,
            })
        })
        .unwrap();
        manager
            .admit_runtime(
                graph,
                node_plans,
                RuntimeAdmission {
                    id: "ready",
                    runtime_session: "session",
                    epoch: 1,
                    admission_sequence: 1,
                    width: 1,
                    exposed: false,
                },
            )
            .unwrap();
        let decision = manager
            .claim_replay("ns", "ready-request", [8; 32], "ready", |_| {
                Ok(ReplayPersist::Created)
            })
            .unwrap();
        let ReplayDecision::New { key, .. } = decision else {
            panic!("expected a new replay claim");
        };
        manager.mark_replay_ready(key, "ready", |_| Ok(())).unwrap();
        assert_eq!(manager.discard_unstarted("ready"), Err(Error::Busy));
        assert!(manager.runtime_snapshot("ready").is_ok());
    }

    #[test]
    fn restart_never_prunes_source_job_to_make_capacity() {
        let manager = JobManager::new(Limits {
            max_retained_jobs: 1,
            max_exposed_jobs: 1,
            max_hidden_jobs: 1,
            max_active_nodes: 1,
            max_hidden_replays: 2,
            max_spool_bytes: 1024,
        })
        .unwrap();
        terminal_no_output(&manager, "source");
        let graph = Graph::compile(&json!({"ops":[["read",{"path":"b"}]]}), |_, _| true).unwrap();
        let node_plans = plan_graph(&graph, |_, _| {
            Ok::<_, ()>(NodePolicy {
                budget: SemanticBudget {
                    work_bytes: 1,
                    result_bytes: 64,
                },
                footprint: Footprint::none(),
                registry_generation: 1,
                authority_binding: [1; 32],
                runtime_binding: [2; 32],
                task_result_eligible: false,
            })
        })
        .unwrap();
        assert_eq!(
            manager.restart_runtime(
                "source",
                graph,
                node_plans,
                RuntimeAdmission {
                    id: "restart",
                    runtime_session: "session",
                    epoch: 1,
                    admission_sequence: 1,
                    width: 1,
                    exposed: false,
                },
            ),
            Err(Error::Capacity)
        );
        assert!(manager.get("source").is_ok());
        assert!(matches!(
            manager.runtime_snapshot("restart"),
            Err(Error::NotFound)
        ));
    }

    #[test]
    fn journal_unknown_terminal_cannot_expire_or_be_capacity_evicted() {
        let manager = JobManager::new(Limits {
            max_retained_jobs: 2,
            max_exposed_jobs: 2,
            max_hidden_jobs: 2,
            max_active_nodes: 2,
            max_hidden_replays: 2,
            max_spool_bytes: 1024,
        })
        .unwrap();
        terminal_no_output(&manager, "uncertain");
        terminal_no_output(&manager, "known");
        {
            let mut state = manager.state.lock().unwrap();
            state
                .jobs
                .get_mut("uncertain")
                .unwrap()
                .job
                .fence_journal_unknown();
        }
        let snapshot = manager.get("uncertain").unwrap();
        assert_eq!(snapshot.lifecycle, Lifecycle::Terminal);
        assert!(snapshot.journal_unknown);
        assert!(!snapshot.is_known_terminal());
        assert!(manager.get("known").unwrap().is_known_terminal());
        assert_eq!(
            manager.render_graph_result("uncertain", 1024),
            Err(Error::ReplayUnknown),
            "journal-unknown terminal state cannot render as a completed Graph result"
        );
        assert!(
            manager.has_nonterminal_jobs().unwrap(),
            "journal-unknown terminal work remains live until reconciliation"
        );
        assert_eq!(
            manager.expire_terminal("uncertain"),
            Err(Error::Busy),
            "host expiry cannot erase journal-unknown evidence"
        );
        let graph = manager.graph("uncertain").unwrap();
        let fresh = manager.plans("uncertain").unwrap();
        assert_eq!(
            manager.restart_runtime(
                "uncertain",
                graph,
                fresh,
                RuntimeAdmission {
                    id: "uncertain-restart",
                    runtime_session: "session",
                    epoch: 2,
                    admission_sequence: 1,
                    width: 1,
                    exposed: true,
                },
            ),
            Err(Error::ReplayUnknown),
            "an unresolved journal must never become a restart source"
        );
        assert!(matches!(
            manager.runtime_snapshot("uncertain-restart"),
            Err(Error::NotFound)
        ));
        terminal_no_output(&manager, "new");
        assert!(
            manager.get("uncertain").unwrap().journal_unknown,
            "capacity pressure may reclaim known terminal history, not uncertain history"
        );
        assert!(matches!(manager.get("known"), Err(Error::NotFound)));
        assert!(manager.get("new").is_ok());

        // With no other safe candidate, pressure must fail rather than
        // drop the last journal-unknown Job.
        let solo = JobManager::new(Limits {
            max_retained_jobs: 1,
            max_exposed_jobs: 1,
            max_hidden_jobs: 1,
            max_active_nodes: 1,
            max_hidden_replays: 1,
            max_spool_bytes: 1024,
        })
        .unwrap();
        terminal_no_output(&solo, "uncertain");
        {
            solo.state
                .lock()
                .unwrap()
                .jobs
                .get_mut("uncertain")
                .unwrap()
                .job
                .fence_journal_unknown();
        }
        assert_eq!(
            solo.reserve_task_admission("pending", true),
            Err(Error::Capacity)
        );
        assert!(solo.get("uncertain").unwrap().journal_unknown);

        let hidden = JobManager::new(Limits {
            max_retained_jobs: 2,
            max_exposed_jobs: 2,
            max_hidden_jobs: 2,
            max_active_nodes: 2,
            max_hidden_replays: 2,
            max_spool_bytes: 1024,
        })
        .unwrap();
        terminal_no_output_with_visibility(&hidden, "hidden-uncertain", false);
        {
            hidden
                .state
                .lock()
                .unwrap()
                .jobs
                .get_mut("hidden-uncertain")
                .unwrap()
                .job
                .fence_journal_unknown();
        }
        assert_eq!(
            hidden.discard_unexposed_terminal("hidden-uncertain"),
            Err(Error::Busy),
            "optimistic hidden cleanup cannot erase journal-unknown evidence"
        );
        assert!(
            hidden
                .runtime_snapshot("hidden-uncertain")
                .unwrap()
                .journal_unknown
        );
    }

    #[test]
    fn journal_unknown_claim_cannot_be_discarded_as_unstarted() {
        let manager = manager(2);
        admit_simple(&manager, "unknown-claim", false);
        assert_eq!(
            manager.claim("unknown-claim", 1, true, 2, |_, _| {
                Err(JournalError::OutcomeUnknown)
            }),
            Err(Error::Job(JobError::JournalUnknown))
        );
        let snapshot = manager.runtime_snapshot("unknown-claim").unwrap();
        assert_eq!(snapshot.lifecycle, Lifecycle::Running);
        assert!(snapshot.journal_unknown);
        assert_eq!(
            manager.runner_load("unknown-claim").unwrap().global_active,
            1
        );
        assert_eq!(
            manager.discard_unstarted("unknown-claim"),
            Err(Error::Busy),
            "a possibly committed Claim must retain its Job record"
        );
        assert!(
            manager
                .runtime_snapshot("unknown-claim")
                .unwrap()
                .journal_unknown
        );
        assert_eq!(
            manager.runner_load("unknown-claim").unwrap().global_active,
            1,
            "the reserved permit stays fenced until reconciliation"
        );
        assert!(manager.has_nonterminal_jobs().unwrap());
    }

    #[test]
    fn hidden_terminal_cannot_retire_while_output_writer_is_live() {
        let manager = manager(2);
        admit_simple(&manager, "hidden-live-writer", false);
        manager
            .claim("hidden-live-writer", 1, true, 2, |_, _| Ok(()))
            .unwrap();
        let owner = manager.job_output_owner("hidden-live-writer").unwrap();
        manager
            .attach_output_memory("hidden-live-writer", 1, OutputStream::Result, [owner])
            .unwrap();
        let writer = manager
            .output_writer("hidden-live-writer", 1, OutputStream::Result)
            .unwrap();
        manager
            .start(
                "hidden-live-writer",
                StartRequest {
                    coordinate: 1,
                    claimed_sequence: 2,
                    source_live: true,
                    launch_authorized: true,
                    sequence: 3,
                },
                |_, _| Ok(()),
            )
            .unwrap();
        manager
            .settle(
                "hidden-live-writer",
                1,
                NodeState::Running { sequence: 3 },
                Status {
                    primary: Primary::Ok,
                    more: false,
                    incomplete: false,
                },
                4,
                |_, _| Ok(()),
            )
            .unwrap();

        assert!(
            manager.has_nonterminal_jobs().unwrap(),
            "an open retained writer keeps the Job runtime live after local settlement"
        );
        assert_eq!(
            manager.discard_unexposed_terminal("hidden-live-writer"),
            Err(Error::Busy),
            "optimistic retirement must not erase a Job while an output writer is still live"
        );
        writer.finish().unwrap();
        assert!(!manager.has_nonterminal_jobs().unwrap());
        manager
            .discard_unexposed_terminal("hidden-live-writer")
            .unwrap();
    }

    #[test]
    fn capacity_prune_cannot_retire_terminal_job_with_live_output_writer() {
        let manager = JobManager::new(Limits {
            max_retained_jobs: 1,
            max_exposed_jobs: 1,
            max_hidden_jobs: 1,
            max_active_nodes: 1,
            max_hidden_replays: 1,
            max_spool_bytes: 1024,
        })
        .unwrap();
        admit_simple(&manager, "live-writer-capacity", true);
        manager
            .claim("live-writer-capacity", 1, true, 2, |_, _| Ok(()))
            .unwrap();
        let owner = manager.job_output_owner("live-writer-capacity").unwrap();
        manager
            .attach_output_memory("live-writer-capacity", 1, OutputStream::Result, [owner])
            .unwrap();
        let writer = manager
            .output_writer("live-writer-capacity", 1, OutputStream::Result)
            .unwrap();
        manager
            .start(
                "live-writer-capacity",
                StartRequest {
                    coordinate: 1,
                    claimed_sequence: 2,
                    source_live: true,
                    launch_authorized: true,
                    sequence: 3,
                },
                |_, _| Ok(()),
            )
            .unwrap();
        manager
            .settle(
                "live-writer-capacity",
                1,
                NodeState::Running { sequence: 3 },
                Status {
                    primary: Primary::Ok,
                    more: false,
                    incomplete: false,
                },
                4,
                |_, _| Ok(()),
            )
            .unwrap();

        assert_eq!(
            manager.reserve_task_admission("next", true),
            Err(Error::Capacity),
            "capacity pressure must not erase a terminal Job while an output writer is live"
        );
        assert!(manager.get("live-writer-capacity").is_ok());

        writer.finish().unwrap();
        manager.reserve_task_admission("next", true).unwrap();
        assert!(matches!(
            manager.get("live-writer-capacity"),
            Err(Error::NotFound)
        ));
    }

    #[test]
    fn retained_capacity_expires_only_oldest_released_terminal_without_replay_owner() {
        let manager = JobManager::new(Limits {
            max_retained_jobs: 2,
            max_exposed_jobs: 2,
            max_hidden_jobs: 2,
            max_active_nodes: 2,
            max_hidden_replays: 4,
            max_spool_bytes: 1024,
        })
        .unwrap();
        terminal_no_output(&manager, "old");
        terminal_no_output(&manager, "newer");
        // Third admission pressure may expire the oldest safe terminal Job.
        terminal_no_output(&manager, "latest");
        assert!(matches!(manager.get("old"), Err(Error::NotFound)));
        assert!(manager.get("newer").is_ok());
        assert!(manager.get("latest").is_ok());
    }

    #[test]
    fn hidden_job_can_reserve_exposed_capacity_without_becoming_visible() {
        let manager = JobManager::new(Limits {
            max_retained_jobs: 2,
            max_exposed_jobs: 1,
            max_hidden_jobs: 2,
            max_active_nodes: 2,
            max_hidden_replays: 2,
            max_spool_bytes: 1024,
        })
        .unwrap();
        admit_simple(&manager, "hidden", false);
        manager.reserve_exposed("hidden").unwrap();
        assert!(matches!(manager.get("hidden"), Err(Error::NotExposed)));
        assert!(manager.list(0, 1).unwrap().jobs.is_empty());

        let graph = Graph::compile(&json!({"ops":[["read",{"path":"b"}]]}), |_, _| true).unwrap();
        let node_plans = plans(&graph);
        assert_eq!(
            manager.admit_runtime(
                graph,
                node_plans,
                RuntimeAdmission {
                    id: "other-visible",
                    runtime_session: "s",
                    epoch: 1,
                    admission_sequence: 1,
                    width: 1,
                    exposed: true,
                },
            ),
            Err(Error::Capacity)
        );
        manager.expose("hidden").unwrap();
        assert_eq!(manager.get("hidden").unwrap().id, "hidden");
    }

    #[test]
    fn failed_hidden_to_reserved_transition_leaves_job_hidden_and_retriable() {
        let manager = JobManager::new(Limits {
            max_retained_jobs: 2,
            max_exposed_jobs: 1,
            max_hidden_jobs: 2,
            max_active_nodes: 2,
            max_hidden_replays: 2,
            max_spool_bytes: 1024,
        })
        .unwrap();
        admit_simple(&manager, "visible", true);
        admit_simple(&manager, "hidden", false);
        assert_eq!(manager.reserve_exposed("hidden"), Err(Error::Capacity));
        assert!(matches!(manager.get("hidden"), Err(Error::NotExposed)));
        assert_eq!(manager.get("visible").unwrap().id, "visible");
    }

    #[test]
    fn reserved_exposed_runtime_job_is_invisible_and_publishes_without_second_capacity_check() {
        let manager = JobManager::new(Limits {
            max_retained_jobs: 2,
            max_exposed_jobs: 1,
            max_hidden_jobs: 1,
            max_active_nodes: 2,
            max_hidden_replays: 2,
            max_spool_bytes: 1024,
        })
        .unwrap();
        let graph = Graph::compile(&json!({"ops":[["read",{"path":"a"}]]}), |_, _| true).unwrap();
        let node_plans = plans(&graph);
        manager
            .admit_reserved_exposed_runtime(
                graph.clone(),
                node_plans.clone(),
                ReservedExposedRuntimeAdmission {
                    id: "reserved-runtime",
                    runtime_session: "s",
                    epoch: 1,
                    admission_sequence: 1,
                    width: 1,
                },
            )
            .unwrap();
        assert!(matches!(
            manager.get("reserved-runtime"),
            Err(Error::NotExposed)
        ));
        assert!(manager.list(0, 1).unwrap().jobs.is_empty());
        assert_eq!(
            manager.admit_runtime(
                graph,
                node_plans,
                RuntimeAdmission {
                    id: "visible-2",
                    runtime_session: "s",
                    epoch: 1,
                    admission_sequence: 1,
                    width: 1,
                    exposed: true,
                },
            ),
            Err(Error::Capacity)
        );
        manager.expose("reserved-runtime").unwrap();
        assert_eq!(
            manager.get("reserved-runtime").unwrap().id,
            "reserved-runtime"
        );
    }

    #[test]
    fn reserved_exposed_runtime_restart_uses_fresh_plan_and_owns_visible_capacity() {
        let manager = JobManager::new(Limits {
            max_retained_jobs: 2,
            max_exposed_jobs: 2,
            max_hidden_jobs: 2,
            max_active_nodes: 2,
            max_hidden_replays: 2,
            max_spool_bytes: 1024,
        })
        .unwrap();
        terminal_no_output(&manager, "source");
        let graph = Graph::compile(&json!({"ops":[["read",{"path":"b"}]]}), |_, _| true).unwrap();
        let fresh = plan_graph(&graph, |_, _| {
            Ok::<_, ()>(NodePolicy {
                budget: SemanticBudget {
                    work_bytes: 2,
                    result_bytes: 128,
                },
                footprint: Footprint::none(),
                registry_generation: 9,
                authority_binding: [7; 32],
                runtime_binding: [8; 32],
                task_result_eligible: false,
            })
        })
        .unwrap();
        manager
            .restart_reserved_exposed_runtime(
                "source",
                graph,
                fresh.clone(),
                ReservedExposedRuntimeAdmission {
                    id: "restart-reserved",
                    runtime_session: "s",
                    epoch: 2,
                    admission_sequence: 1,
                    width: 1,
                },
            )
            .unwrap();
        assert!(matches!(
            manager.get("restart-reserved"),
            Err(Error::NotExposed)
        ));
        assert_eq!(
            manager.plans("restart-reserved").unwrap()[0].registry_generation(),
            9
        );
        manager.expose("restart-reserved").unwrap();
        assert_eq!(
            manager.get("restart-reserved").unwrap().id,
            "restart-reserved"
        );
    }

    #[test]
    fn preseal_capacity_reservation_is_invisible_and_consumed_without_second_quota_race() {
        let manager = JobManager::new(Limits {
            max_retained_jobs: 1,
            max_exposed_jobs: 1,
            max_hidden_jobs: 1,
            max_active_nodes: 1,
            max_hidden_replays: 2,
            max_spool_bytes: 1024,
        })
        .unwrap();
        manager.reserve_task_admission("planned", true).unwrap();
        assert!(matches!(manager.get("planned"), Err(Error::NotFound)));
        assert!(manager.list(0, 1).unwrap().jobs.is_empty());

        let graph = Graph::compile(&json!({"ops":[["read",{"path":"a"}]]}), |_, _| true).unwrap();
        let node_plans = plans(&graph);
        assert_eq!(
            manager.admit_runtime(
                graph.clone(),
                node_plans.clone(),
                RuntimeAdmission {
                    id: "other",
                    runtime_session: "s",
                    epoch: 1,
                    admission_sequence: 1,
                    width: 1,
                    exposed: true,
                },
            ),
            Err(Error::Capacity),
            "pre-seal reservation must own aggregate/exposed capacity before a Job exists"
        );

        let task = TaskIdentity {
            actor: "a".into(),
            source_kind: "host".into(),
            source_id: "source".into(),
            source_epoch: 1,
            execution_site: "local".into(),
            session: "s".into(),
            branch: "main".into(),
            turn_id: 1,
            call_id: "planned".into(),
            effect: ToolEffect::Control,
        };
        manager
            .admit_pre_reserved_task(
                graph,
                node_plans,
                TaskAdmission {
                    id: "planned",
                    task: &task,
                    admission_sequence: 1,
                    width: 1,
                    exposed: true,
                },
            )
            .unwrap();
        assert!(matches!(manager.get("planned"), Err(Error::NotExposed)));
        manager.expose("planned").unwrap();
        assert_eq!(manager.get("planned").unwrap().id, "planned");

        // A released pre-admission returns capacity without creating history.
        let manager = JobManager::new(Limits {
            max_retained_jobs: 1,
            max_exposed_jobs: 1,
            max_hidden_jobs: 1,
            max_active_nodes: 1,
            max_hidden_replays: 2,
            max_spool_bytes: 1024,
        })
        .unwrap();
        manager.reserve_task_admission("temporary", true).unwrap();
        manager.release_task_admission("temporary").unwrap();
        assert!(manager.list(0, 1).unwrap().jobs.is_empty());
        let graph = Graph::compile(&json!({"ops":[["read",{"path":"b"}]]}), |_, _| true).unwrap();
        manager
            .admit_runtime(
                graph.clone(),
                plans(&graph),
                RuntimeAdmission {
                    id: "visible",
                    runtime_session: "s",
                    epoch: 1,
                    admission_sequence: 1,
                    width: 1,
                    exposed: true,
                },
            )
            .unwrap();
    }

    #[test]
    fn reserved_exposed_job_is_invisible_but_owns_visible_capacity_until_publish() {
        let manager = JobManager::new(Limits {
            max_retained_jobs: 2,
            max_exposed_jobs: 1,
            max_hidden_jobs: 1,
            max_active_nodes: 2,
            max_hidden_replays: 2,
            max_spool_bytes: 1024,
        })
        .unwrap();
        let graph = Graph::compile(&json!({"ops":[["read",{"path":"a"}]]}), |_, _| true).unwrap();
        let node_plans = plans(&graph);
        let task = TaskIdentity {
            actor: "a".into(),
            source_kind: "host".into(),
            source_id: "source".into(),
            source_epoch: 1,
            execution_site: "local".into(),
            session: "s".into(),
            branch: "main".into(),
            turn_id: 1,
            call_id: "reserved".into(),
            effect: ToolEffect::Read,
        };
        manager
            .admit_reserved_exposed_task(
                graph.clone(),
                node_plans.clone(),
                ReservedExposedTaskAdmission {
                    id: "reserved",
                    task: &task,
                    admission_sequence: 1,
                    width: 1,
                },
            )
            .unwrap();
        assert!(matches!(manager.get("reserved"), Err(Error::NotExposed)));
        assert!(manager.list(0, 1).unwrap().jobs.is_empty());

        // The reservation owns the only exposed slot, but not the hidden slot.
        manager
            .admit_runtime(
                graph.clone(),
                node_plans.clone(),
                RuntimeAdmission {
                    id: "hidden",
                    runtime_session: "s",
                    epoch: 1,
                    admission_sequence: 1,
                    width: 1,
                    exposed: false,
                },
            )
            .unwrap();
        assert_eq!(
            manager.admit_runtime(
                graph,
                node_plans,
                RuntimeAdmission {
                    id: "visible-2",
                    runtime_session: "s",
                    epoch: 1,
                    admission_sequence: 1,
                    width: 1,
                    exposed: true,
                },
            ),
            Err(Error::Capacity)
        );

        // Publication needs no second capacity reservation.
        manager.expose("reserved").unwrap();
        assert_eq!(manager.get("reserved").unwrap().id, "reserved");
        assert_eq!(manager.list(0, 1).unwrap().jobs[0].id, "reserved");
    }

    #[test]
    fn reserved_exposed_job_can_be_discarded_while_still_pre_effect() {
        let manager = JobManager::new(Limits {
            max_retained_jobs: 1,
            max_exposed_jobs: 1,
            max_hidden_jobs: 1,
            max_active_nodes: 1,
            max_hidden_replays: 2,
            max_spool_bytes: 1024,
        })
        .unwrap();
        let graph = Graph::compile(&json!({"ops":[["read",{"path":"a"}]]}), |_, _| true).unwrap();
        let node_plans = plans(&graph);
        let task = TaskIdentity {
            actor: "a".into(),
            source_kind: "host".into(),
            source_id: "source".into(),
            source_epoch: 1,
            execution_site: "local".into(),
            session: "s".into(),
            branch: "main".into(),
            turn_id: 1,
            call_id: "reserved-discard".into(),
            effect: ToolEffect::Read,
        };
        manager
            .admit_reserved_exposed_task(
                graph.clone(),
                node_plans.clone(),
                ReservedExposedTaskAdmission {
                    id: "reserved",
                    task: &task,
                    admission_sequence: 1,
                    width: 1,
                },
            )
            .unwrap();
        manager.discard_unstarted("reserved").unwrap();
        assert!(matches!(
            manager.runtime_snapshot("reserved"),
            Err(Error::NotFound)
        ));
        manager
            .admit_runtime(
                graph,
                node_plans,
                RuntimeAdmission {
                    id: "visible",
                    runtime_session: "s",
                    epoch: 1,
                    admission_sequence: 1,
                    width: 1,
                    exposed: true,
                },
            )
            .unwrap();
    }

    #[test]
    fn hidden_and_exposed_jobs_have_independent_class_quotas_under_aggregate_bound() {
        let manager = JobManager::new(Limits {
            max_retained_jobs: 5,
            max_exposed_jobs: 1,
            max_hidden_jobs: 4,
            max_active_nodes: 8,
            max_hidden_replays: 1,
            max_spool_bytes: 1024,
        })
        .unwrap();
        admit_simple(&manager, "visible", true);
        for index in 0..4 {
            admit_simple(&manager, &format!("hidden-{index}"), false);
        }
        assert_eq!(manager.list(0, 1).unwrap().jobs[0].id, "visible");

        let graph = Graph::compile(&json!({"ops":[["read",{"path":"b"}]]}), |_, _| true).unwrap();
        let node_plans = plan_graph(&graph, |_, _| {
            Ok::<_, ()>(NodePolicy {
                budget: SemanticBudget {
                    work_bytes: 1,
                    result_bytes: 64,
                },
                footprint: Footprint::none(),
                registry_generation: 1,
                authority_binding: [3; 32],
                runtime_binding: [4; 32],
                task_result_eligible: false,
            })
        })
        .unwrap();
        assert_eq!(
            manager.admit_runtime(
                graph,
                node_plans,
                RuntimeAdmission {
                    id: "hidden-overflow",
                    runtime_session: "session",
                    epoch: 1,
                    admission_sequence: 1,
                    width: 1,
                    exposed: false,
                },
            ),
            Err(Error::Capacity)
        );
    }

    #[test]
    fn exposing_hidden_job_never_evicts_unreleased_visible_state() {
        let manager = JobManager::new(Limits {
            max_retained_jobs: 3,
            max_exposed_jobs: 1,
            max_hidden_jobs: 2,
            max_active_nodes: 3,
            max_hidden_replays: 2,
            max_spool_bytes: 1024,
        })
        .unwrap();
        admit_simple(&manager, "visible", true);
        admit_simple(&manager, "hidden", false);
        assert_eq!(manager.expose("hidden"), Err(Error::Capacity));
        assert_eq!(manager.list(0, 1).unwrap().jobs[0].id, "visible");
        assert!(manager.runtime_snapshot("hidden").is_ok());
    }

    #[test]
    fn exposing_hidden_job_may_reclaim_only_safe_released_visible_terminal() {
        let manager = JobManager::new(Limits {
            max_retained_jobs: 3,
            max_exposed_jobs: 1,
            max_hidden_jobs: 2,
            max_active_nodes: 3,
            max_hidden_replays: 2,
            max_spool_bytes: 1024,
        })
        .unwrap();
        terminal_no_output(&manager, "released");
        admit_simple(&manager, "hidden", false);
        manager.expose("hidden").unwrap();
        assert!(matches!(manager.get("released"), Err(Error::NotFound)));
        assert_eq!(manager.list(0, 1).unwrap().jobs[0].id, "hidden");
    }

    #[test]
    fn hidden_job_capacity_is_independent_from_replay_identity_capacity() {
        let manager = JobManager::new(Limits {
            max_retained_jobs: 3,
            max_exposed_jobs: 1,
            max_hidden_jobs: 2,
            max_active_nodes: 3,
            max_hidden_replays: 1,
            max_spool_bytes: 1024,
        })
        .unwrap();
        admit_simple(&manager, "hidden-a", false);
        admit_simple(&manager, "hidden-b", false);
        assert!(matches!(
            manager.claim_replay("ns", "one", [1; 32], "hidden-a", |_| {
                Ok(ReplayPersist::Created)
            }),
            Ok(ReplayDecision::New { .. })
        ));
        assert_eq!(
            manager.claim_replay("ns", "two", [2; 32], "hidden-b", |_| {
                Ok(ReplayPersist::Created)
            }),
            Err(Error::Capacity)
        );
        assert!(manager.runtime_snapshot("hidden-a").is_ok());
        assert!(manager.runtime_snapshot("hidden-b").is_ok());
    }

    #[test]
    fn replay_owned_terminal_is_not_pressure_evicted() {
        let manager = JobManager::new(Limits {
            max_retained_jobs: 1,
            max_exposed_jobs: 1,
            max_hidden_jobs: 1,
            max_active_nodes: 1,
            max_hidden_replays: 4,
            max_spool_bytes: 1024,
        })
        .unwrap();
        terminal_no_output(&manager, "owned");
        let semantic = [7; 32];
        let decision = manager
            .claim_replay("ns", "request", semantic, "owned", |_| {
                Ok(ReplayPersist::Created)
            })
            .unwrap();
        let ReplayDecision::New { key, .. } = decision else {
            panic!("expected new replay claim");
        };
        manager.mark_replay_ready(key, "owned", |_| Ok(())).unwrap();
        let graph = Graph::compile(&json!({"ops":[["read",{"path":"b"}]]}), |_, _| true).unwrap();
        let plans = plan_graph(&graph, |_, _| {
            Ok::<_, ()>(NodePolicy {
                budget: SemanticBudget {
                    work_bytes: 1,
                    result_bytes: 64,
                },
                footprint: Footprint::none(),
                registry_generation: 1,
                authority_binding: [1; 32],
                runtime_binding: [2; 32],
                task_result_eligible: false,
            })
        })
        .unwrap();
        assert_eq!(
            manager.admit_runtime(
                graph,
                plans,
                RuntimeAdmission {
                    id: "blocked",
                    runtime_session: "session",
                    epoch: 1,
                    admission_sequence: 1,
                    width: 1,
                    exposed: true,
                },
            ),
            Err(Error::Capacity)
        );
        assert!(manager.get("owned").is_ok());
    }

    #[test]
    fn job_list_is_real_offset_paging_newest_first() {
        let manager = manager(8);
        for number in 0..70 {
            let graph =
                Graph::compile(&json!({"ops":[["read",{"path":"a"}]]}), |_, _| true).unwrap();
            let plans = plan_graph(&graph, |_, _| {
                Ok::<_, ()>(NodePolicy {
                    budget: SemanticBudget {
                        work_bytes: 1,
                        result_bytes: 1024,
                    },
                    footprint: Footprint::none(),
                    registry_generation: 1,
                    authority_binding: [1; 32],
                    runtime_binding: [0xA5; 32],
                    task_result_eligible: true,
                })
            })
            .unwrap();
            manager
                .admit_runtime(
                    graph,
                    plans,
                    RuntimeAdmission {
                        id: &format!("j{number:02}"),
                        runtime_session: "session",
                        epoch: 1,
                        admission_sequence: 1,
                        width: 1,
                        exposed: true,
                    },
                )
                .unwrap();
        }
        let first = manager.list(0, 64).unwrap();
        assert_eq!(first.jobs.len(), 64);
        assert_eq!(first.jobs[0].id, "j69");
        assert_eq!(first.next_offset, Some(64));
        let second = manager.list(64, 64).unwrap();
        assert_eq!(second.jobs.len(), 6);
        assert_eq!(second.jobs[0].id, "j05");
        assert_eq!(second.next_offset, None);
    }

    #[test]
    fn task_owner_is_explicit_and_can_join_finished_unreleased_result() {
        let graph = Graph::compile(&json!({"ops":[["read",{"path":"a"}]]}), |_, _| true).unwrap();
        let plans = plan_graph(&graph, |_, _| {
            Ok::<_, ()>(NodePolicy {
                budget: SemanticBudget {
                    work_bytes: 1,
                    result_bytes: 64,
                },
                footprint: Footprint::none(),
                registry_generation: 1,
                authority_binding: [1; 32],
                runtime_binding: [2; 32],
                task_result_eligible: true,
            })
        })
        .unwrap();
        let task = TaskIdentity {
            actor: "a".into(),
            source_kind: "host".into(),
            source_id: "source".into(),
            source_epoch: 1,
            execution_site: "local".into(),
            session: "s".into(),
            branch: "main".into(),
            turn_id: 1,
            call_id: "call".into(),
            effect: ToolEffect::Read,
        };
        let manager = manager(2);
        manager
            .admit_task(
                graph,
                plans,
                TaskAdmission {
                    id: "task-owner",
                    task: &task,
                    admission_sequence: 1,
                    width: 1,
                    exposed: true,
                },
            )
            .unwrap();
        manager
            .claim("task-owner", 1, true, 2, |_, _| Ok(()))
            .unwrap();
        let job_owner = manager.job_output_owner("task-owner").unwrap();
        let task_owner = manager.task_result_owner("task-owner", 1).unwrap();
        let (file, path) = output_file();
        manager
            .attach_output("task-owner", 1, OutputStream::Result, file, [job_owner])
            .unwrap();
        manager
            .append_output("task-owner", 1, OutputStream::Result, b"abc")
            .unwrap();
        manager
            .finish_output("task-owner", 1, OutputStream::Result)
            .unwrap();
        manager
            .add_output_owner("task-owner", 1, OutputStream::Result, task_owner)
            .unwrap();
        assert!(
            !manager
                .acknowledge_output("task-owner", 1, OutputStream::Result, job_owner, 3)
                .unwrap()
                .released
        );
        assert!(
            manager
                .acknowledge_output("task-owner", 1, OutputStream::Result, task_owner, 3)
                .unwrap()
                .released
        );
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn empty_output_remains_readable_until_explicit_zero_ack() {
        let graph = Graph::compile(&json!({"ops":[["read",{"path":"a"}]]}), |_, _| true).unwrap();
        let plans = plan_graph(&graph, |_, _| {
            Ok::<_, ()>(NodePolicy {
                budget: SemanticBudget {
                    work_bytes: 1,
                    result_bytes: 64,
                },
                footprint: Footprint::none(),
                registry_generation: 1,
                authority_binding: [1; 32],
                runtime_binding: [2; 32],
                task_result_eligible: false,
            })
        })
        .unwrap();
        let manager = manager(2);
        manager
            .admit_runtime(
                graph,
                plans,
                RuntimeAdmission {
                    id: "empty-output",
                    runtime_session: "session",
                    epoch: 1,
                    admission_sequence: 1,
                    width: 1,
                    exposed: true,
                },
            )
            .unwrap();
        manager
            .claim("empty-output", 1, true, 2, |_, _| Ok(()))
            .unwrap();
        let owner = manager.job_output_owner("empty-output").unwrap();
        manager
            .attach_output_memory("empty-output", 1, OutputStream::Stderr, [owner])
            .unwrap();
        manager
            .finish_output("empty-output", 1, OutputStream::Stderr)
            .unwrap();
        let page = manager
            .output_page("empty-output", 1, OutputStream::Stderr, 0, 1)
            .unwrap();
        assert!(page.bytes.is_empty());
        assert!(page.writer_finished);
        assert!(
            manager
                .acknowledge_output("empty-output", 1, OutputStream::Stderr, owner, 0)
                .unwrap()
                .released
        );
        assert_eq!(
            manager.output_page("empty-output", 1, OutputStream::Stderr, 0, 1),
            Err(Error::OutputReleased)
        );
    }

    #[test]
    fn owner_aware_compaction_requires_every_other_owner_to_release_the_tail() {
        let graph = Graph::compile(&json!({"ops":[["read",{"path":"a"}]]}), |_, _| true).unwrap();
        let plans = plan_graph(&graph, |_, _| {
            Ok::<_, ()>(NodePolicy {
                budget: SemanticBudget {
                    work_bytes: 1,
                    result_bytes: 1024,
                },
                footprint: Footprint::none(),
                registry_generation: 1,
                authority_binding: [1; 32],
                runtime_binding: [2; 32],
                task_result_eligible: true,
            })
        })
        .unwrap();
        let manager = manager(8);
        manager
            .admit_runtime(
                graph,
                plans,
                RuntimeAdmission {
                    id: "compact-owner",
                    runtime_session: "session",
                    epoch: 1,
                    admission_sequence: 1,
                    width: 1,
                    exposed: true,
                },
            )
            .unwrap();
        manager
            .claim("compact-owner", 1, true, 2, |_, _| Ok(()))
            .unwrap();
        let job_owner = manager.job_output_owner("compact-owner").unwrap();
        let task_owner = manager.task_result_owner("compact-owner", 1).unwrap();
        manager
            .attach_output_memory(
                "compact-owner",
                1,
                OutputStream::Stdout,
                [job_owner, task_owner],
            )
            .unwrap();
        manager
            .append_output("compact-owner", 1, OutputStream::Stdout, b"abcdefgh")
            .unwrap();
        manager
            .finish_output("compact-owner", 1, OutputStream::Stdout)
            .unwrap();

        assert_eq!(
            manager.compact_output_for_owner(
                "compact-owner",
                1,
                OutputStream::Stdout,
                task_owner,
                4,
            ),
            Err(Error::Busy),
            "the Job owner still needs the historical tail"
        );
        manager
            .record_delivered_output("compact-owner", 1, OutputStream::Stdout, job_owner, 0, 8)
            .unwrap();
        manager
            .record_delivered_output("compact-owner", 1, OutputStream::Stdout, task_owner, 0, 8)
            .unwrap();
        assert!(
            !manager
                .acknowledge_delivered_output(
                    "compact-owner",
                    1,
                    OutputStream::Stdout,
                    job_owner,
                    8,
                )
                .unwrap()
                .released
        );
        assert_eq!(
            manager.add_output_owner("compact-owner", 1, OutputStream::Stdout, job_owner),
            Err(Error::InvalidOutput),
            "duplicate owner registration must be rejected without resetting its ACK"
        );
        assert_eq!(
            manager
                .compact_output_for_owner("compact-owner", 1, OutputStream::Stdout, task_owner, 4,)
                .unwrap(),
            4,
            "the original Job-owner ACK must survive the rejected duplicate add"
        );
        let page = manager
            .output_page("compact-owner", 1, OutputStream::Stdout, 0, 8)
            .unwrap();
        assert_eq!(page.bytes, b"abcd");
        assert_eq!(page.retained_bytes, 4);
        assert_eq!(page.total_bytes, 8);
        assert_eq!(manager.spool_usage().unwrap().0, 4);
        assert_eq!(
            manager
                .acknowledge_delivered_output(
                    "compact-owner",
                    1,
                    OutputStream::Stdout,
                    job_owner,
                    8,
                )
                .unwrap(),
            OutputAck {
                changed: false,
                released: false,
            },
            "lost ACK replay keeps its original logical high-water mark"
        );
        assert_eq!(
            manager.acknowledge_delivered_output(
                "compact-owner",
                1,
                OutputStream::Stdout,
                task_owner,
                5,
            ),
            Err(Error::InvalidOutput),
            "an owner cannot newly acknowledge delivered bytes removed by compaction"
        );
        assert!(
            manager
                .acknowledge_delivered_output(
                    "compact-owner",
                    1,
                    OutputStream::Stdout,
                    task_owner,
                    4,
                )
                .unwrap()
                .released
        );
        assert_eq!(manager.spool_usage().unwrap().0, 0);
    }

    #[test]
    fn retained_output_read_is_pure_and_multi_owner_ack_releases_once() {
        let graph = Graph::compile(&json!({"ops":[["read",{"path":"a"}]]}), |_, _| true).unwrap();
        let node_plans = plan_graph(&graph, |_, _| {
            Ok::<_, ()>(NodePolicy {
                budget: SemanticBudget {
                    work_bytes: 1,
                    result_bytes: 1024,
                },
                footprint: Footprint::none(),
                registry_generation: 1,
                authority_binding: [1; 32],
                runtime_binding: [0xA5; 32],
                task_result_eligible: true,
            })
        })
        .unwrap();
        let manager = manager(8);
        manager
            .admit_runtime(
                graph,
                node_plans,
                RuntimeAdmission {
                    id: "o",
                    runtime_session: "session",
                    epoch: 1,
                    admission_sequence: 1,
                    width: 1,
                    exposed: true,
                },
            )
            .unwrap();
        manager.claim("o", 1, true, 2, |_, _| Ok(())).unwrap();
        let job_owner = manager.job_output_owner("o").unwrap();
        let task_owner = OutputOwner::new([9; 32]).unwrap();
        let (file, path) = output_file();
        manager
            .attach_output("o", 1, OutputStream::Result, file, [job_owner, task_owner])
            .unwrap();
        manager
            .append_output("o", 1, OutputStream::Result, b"abcdef")
            .unwrap();
        let first = manager
            .output_page("o", 1, OutputStream::Result, 1, 3)
            .unwrap();
        assert_eq!(first.bytes, b"bcd");
        assert!(!first.writer_finished);
        assert_eq!(manager.spool_usage().unwrap().0, 6);
        manager.finish_output("o", 1, OutputStream::Result).unwrap();
        let one = manager
            .acknowledge_output("o", 1, OutputStream::Result, job_owner, 6)
            .unwrap();
        assert_eq!(
            one,
            OutputAck {
                changed: true,
                released: false
            }
        );
        assert_eq!(manager.spool_usage().unwrap().0, 6);
        let repeat = manager
            .acknowledge_output("o", 1, OutputStream::Result, job_owner, 6)
            .unwrap();
        assert_eq!(
            repeat,
            OutputAck {
                changed: false,
                released: false
            }
        );
        let two = manager
            .acknowledge_output("o", 1, OutputStream::Result, task_owner, 6)
            .unwrap();
        assert_eq!(
            two,
            OutputAck {
                changed: true,
                released: true
            }
        );
        assert_eq!(manager.spool_usage().unwrap().0, 0);
        assert_eq!(
            manager.output_page("o", 1, OutputStream::Result, 0, 1),
            Err(Error::OutputReleased)
        );
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn graph_result_renderer_is_coordinate_keyed_compact_and_all_or_nothing() {
        let graph = Graph::compile(
            &json!({"ops":[
                ["read",{"path":"a"}],
                ["write",{"path":"b","content":"x"},"1.ok"],
                ["read",{"path":"c"},"1.fail"]
            ]}),
            |_, _| true,
        )
        .unwrap();
        let node_plans = plan_graph(&graph, |_, _| {
            Ok::<_, ()>(NodePolicy {
                budget: SemanticBudget {
                    work_bytes: 1,
                    result_bytes: 1024,
                },
                footprint: Footprint::none(),
                registry_generation: 1,
                authority_binding: [1; 32],
                runtime_binding: [2; 32],
                task_result_eligible: false,
            })
        })
        .unwrap();
        let manager = manager(4);
        manager
            .admit_runtime(
                graph,
                node_plans,
                RuntimeAdmission {
                    id: "render",
                    runtime_session: "session",
                    epoch: 1,
                    admission_sequence: 1,
                    width: 2,
                    exposed: false,
                },
            )
            .unwrap();
        let owner = manager.job_output_owner("render").unwrap();
        for (coordinate, bytes) in [(1, b"alpha\n".as_slice()), (2, b"ok".as_slice())] {
            manager
                .claim(
                    "render",
                    coordinate,
                    true,
                    manager.current_sequence("render").unwrap() + 1,
                    |_, _| Ok(()),
                )
                .unwrap();
            let running = manager.current_sequence("render").unwrap();
            manager
                .start(
                    "render",
                    StartRequest {
                        coordinate,
                        claimed_sequence: running,
                        source_live: true,
                        launch_authorized: true,
                        sequence: running + 1,
                    },
                    |_, _| Ok(()),
                )
                .unwrap();
            let running = manager.current_sequence("render").unwrap();
            let (file, path) = output_file();
            manager
                .attach_output("render", coordinate, OutputStream::Result, file, [owner])
                .unwrap();
            manager
                .append_output("render", coordinate, OutputStream::Result, bytes)
                .unwrap();
            manager
                .finish_output("render", coordinate, OutputStream::Result)
                .unwrap();
            manager
                .settle(
                    "render",
                    coordinate,
                    NodeState::Running { sequence: running },
                    Status {
                        primary: Primary::Ok,
                        more: false,
                        incomplete: false,
                    },
                    running + 1,
                    |_, _| Ok(()),
                )
                .unwrap();
            let _ = std::fs::remove_file(path);
        }
        let sequence = manager.current_sequence("render").unwrap();
        manager
            .skip("render", 3, sequence + 1, |_, _| Ok(()))
            .unwrap();
        let rendered = manager
            .render_graph_result("render", 1024)
            .unwrap()
            .unwrap();
        assert_eq!(rendered, "1\nalpha\n\n\n3 skip condition\n\nok skip=1");
        assert!(manager
            .render_graph_result("render", rendered.len() - 1)
            .unwrap()
            .is_none());
    }

    #[test]
    fn exact_reserved_output_fails_before_effect_when_capacity_is_unavailable() {
        let graph = Graph::compile(&json!({"ops":[["read",{"path":"a"}]]}), |_, _| true).unwrap();
        let node_plans = plan_graph(&graph, |_, _| {
            Ok::<_, ()>(NodePolicy {
                budget: SemanticBudget {
                    work_bytes: 1,
                    result_bytes: 8,
                },
                footprint: Footprint::none(),
                registry_generation: 1,
                authority_binding: [1; 32],
                runtime_binding: [2; 32],
                task_result_eligible: true,
            })
        })
        .unwrap();
        let manager = JobManager::new(Limits {
            max_retained_jobs: 4,
            max_exposed_jobs: 4,
            max_hidden_jobs: 4,
            max_active_nodes: 2,
            max_hidden_replays: 4,
            max_spool_bytes: 4,
        })
        .unwrap();
        manager
            .admit_runtime(
                graph,
                node_plans,
                RuntimeAdmission {
                    id: "r",
                    runtime_session: "session",
                    epoch: 1,
                    admission_sequence: 1,
                    width: 1,
                    exposed: true,
                },
            )
            .unwrap();
        manager.claim("r", 1, true, 2, |_, _| Ok(())).unwrap();
        let owner = manager.job_output_owner("r").unwrap();
        assert_eq!(
            manager.attach_output_reserved("r", 1, OutputStream::Result, [owner], 8),
            Err(Error::Capacity)
        );
        assert_eq!(manager.spool_usage().unwrap().0, 0);
    }

    #[test]
    fn file_shell_output_attachment_is_atomic_and_uses_shared_quota() {
        let graph = Graph::compile(&json!({"ops":[["read",{"path":"a"}]]}), |_, _| true).unwrap();
        let plans = plan_graph(&graph, |_, _| {
            Ok::<_, ()>(NodePolicy {
                budget: SemanticBudget {
                    work_bytes: 1,
                    result_bytes: 16,
                },
                footprint: Footprint::none(),
                registry_generation: 1,
                authority_binding: [1; 32],
                runtime_binding: [2; 32],
                task_result_eligible: false,
            })
        })
        .unwrap();
        let manager = JobManager::new(Limits {
            max_retained_jobs: 2,
            max_exposed_jobs: 2,
            max_hidden_jobs: 2,
            max_active_nodes: 2,
            max_hidden_replays: 2,
            max_spool_bytes: 32,
        })
        .unwrap();
        manager
            .admit_runtime(
                graph,
                plans,
                RuntimeAdmission {
                    id: "file-shell-streams",
                    runtime_session: "session",
                    epoch: 1,
                    admission_sequence: 1,
                    width: 1,
                    exposed: true,
                },
            )
            .unwrap();
        manager
            .claim("file-shell-streams", 1, true, 2, |_, _| Ok(()))
            .unwrap();
        let owner = manager.job_output_owner("file-shell-streams").unwrap();
        let (stdout_file, stdout_path) = output_file();
        let (stderr_file, stderr_path) = output_file();
        let (stdout, stderr) = manager
            .attach_shell_output_files("file-shell-streams", 1, owner, 16, stdout_file, stderr_file)
            .unwrap();
        assert_eq!(manager.spool_usage().unwrap().0, 16);
        assert_eq!(stdout.append(b"abc").unwrap(), 3);
        assert_eq!(stderr.append(b"de").unwrap(), 2);
        assert_eq!(manager.spool_usage().unwrap().0, 21);
        stdout.finish().unwrap();
        stderr.finish().unwrap();
        assert_eq!(
            manager
                .output_page("file-shell-streams", 1, OutputStream::Stdout, 0, 3,)
                .unwrap()
                .bytes,
            b"abc"
        );
        let _ = std::fs::remove_file(stdout_path);
        let _ = std::fs::remove_file(stderr_path);
    }

    #[test]
    fn duplicate_single_stream_attachment_never_replaces_original_output() {
        // File-backed streaming output.
        let (manager, owner) = claimed_output_job("dup-file", 64);
        let (first_file, first_path) = output_file();
        manager
            .attach_output_with_limit("dup-file", 1, OutputStream::Stdout, first_file, [owner], 8)
            .unwrap();
        let writer = manager
            .output_writer("dup-file", 1, OutputStream::Stdout)
            .unwrap();
        assert_eq!(writer.append(b"abc").unwrap(), 3);
        let (replacement, replacement_path) = output_file();
        assert_eq!(
            manager.attach_output_with_limit(
                "dup-file",
                1,
                OutputStream::Stdout,
                replacement,
                [owner],
                8,
            ),
            Err(Error::InvalidOutput)
        );
        assert_eq!(manager.spool_usage().unwrap().0, 3);
        assert_eq!(writer.append(b"d").unwrap(), 1);
        writer.finish().unwrap();
        assert_eq!(
            manager
                .output_page("dup-file", 1, OutputStream::Stdout, 0, 8)
                .unwrap()
                .bytes,
            b"abcd"
        );
        let _ = std::fs::remove_file(first_path);
        let _ = std::fs::remove_file(replacement_path);

        // Incremental in-memory output.
        let (manager, owner) = claimed_output_job("dup-memory", 64);
        manager
            .attach_output_memory("dup-memory", 1, OutputStream::Stdout, [owner])
            .unwrap();
        let writer = manager
            .output_writer("dup-memory", 1, OutputStream::Stdout)
            .unwrap();
        assert_eq!(writer.append(b"abc").unwrap(), 3);
        assert_eq!(
            manager.attach_output_memory("dup-memory", 1, OutputStream::Stdout, [owner]),
            Err(Error::InvalidOutput)
        );
        assert_eq!(manager.spool_usage().unwrap().0, 3);
        assert_eq!(writer.append(b"d").unwrap(), 1);
        writer.finish().unwrap();
        assert_eq!(
            manager
                .output_page("dup-memory", 1, OutputStream::Stdout, 0, 8)
                .unwrap()
                .bytes,
            b"abcd"
        );

        // Exact/prepaid result output. A rejected replacement must release its
        // temporary reservation without disturbing the original reservation.
        let (manager, owner) = claimed_output_job("dup-reserved", 32);
        manager
            .attach_output_reserved("dup-reserved", 1, OutputStream::Result, [owner], 8)
            .unwrap();
        assert_eq!(manager.spool_usage().unwrap().0, 8);
        assert_eq!(
            manager.attach_output_reserved("dup-reserved", 1, OutputStream::Result, [owner], 8,),
            Err(Error::InvalidOutput)
        );
        assert_eq!(
            manager.spool_usage().unwrap().0,
            8,
            "rejected replacement reservation must be returned"
        );
        let writer = manager
            .output_writer("dup-reserved", 1, OutputStream::Result)
            .unwrap();
        assert_eq!(writer.append(b"abcd").unwrap(), 4);
        writer.finish().unwrap();
        assert_eq!(manager.spool_usage().unwrap().0, 4);
        assert_eq!(
            manager
                .output_page("dup-reserved", 1, OutputStream::Result, 0, 8)
                .unwrap()
                .bytes,
            b"abcd"
        );
    }

    #[test]
    fn invalid_file_stream_capacity_fails_before_publication_or_quota() {
        let graph = Graph::compile(&json!({"ops":[["read",{"path":"a"}]]}), |_, _| true).unwrap();
        let plans = plan_graph(&graph, |_, _| {
            Ok::<_, ()>(NodePolicy {
                budget: SemanticBudget {
                    work_bytes: 1,
                    result_bytes: 16,
                },
                footprint: Footprint::none(),
                registry_generation: 1,
                authority_binding: [1; 32],
                runtime_binding: [2; 32],
                task_result_eligible: false,
            })
        })
        .unwrap();
        let manager = JobManager::new(Limits {
            max_retained_jobs: 2,
            max_exposed_jobs: 2,
            max_hidden_jobs: 2,
            max_active_nodes: 2,
            max_hidden_replays: 2,
            max_spool_bytes: 64,
        })
        .unwrap();
        manager
            .admit_runtime(
                graph,
                plans,
                RuntimeAdmission {
                    id: "invalid-file-cap",
                    runtime_session: "session",
                    epoch: 1,
                    admission_sequence: 1,
                    width: 1,
                    exposed: true,
                },
            )
            .unwrap();
        manager
            .claim("invalid-file-cap", 1, true, 2, |_, _| Ok(()))
            .unwrap();
        let owner = manager.job_output_owner("invalid-file-cap").unwrap();
        let (file, path) = output_file();
        assert!(matches!(
            manager.attach_output_with_limit(
                "invalid-file-cap",
                1,
                OutputStream::Stdout,
                file,
                [owner],
                0,
            ),
            Err(Error::InvalidOutput)
        ));
        assert!(matches!(
            manager.output_summary("invalid-file-cap", 1, OutputStream::Stdout),
            Err(Error::OutputMissing)
        ));
        assert_eq!(manager.spool_usage().unwrap().0, 0);
        let _ = std::fs::remove_file(path);
    }

    fn two_node_terminal_output_job(
        id: &str,
        second_primary: Primary,
    ) -> (JobManager, OutputOwner) {
        let graph = Graph::compile(
            &json!({"ops":[
                ["read",{"path":"a"}],
                ["read",{"path":"b"}]
            ]}),
            |_, _| true,
        )
        .unwrap();
        let plans = plan_graph(&graph, |_, _| {
            Ok::<_, ()>(NodePolicy {
                budget: SemanticBudget {
                    work_bytes: 1,
                    result_bytes: 32,
                },
                footprint: Footprint::none(),
                registry_generation: 1,
                authority_binding: [1; 32],
                runtime_binding: [2; 32],
                task_result_eligible: false,
            })
        })
        .unwrap();
        let manager = manager(2);
        manager
            .admit_runtime(
                graph,
                plans,
                RuntimeAdmission {
                    id,
                    runtime_session: "session",
                    epoch: 1,
                    admission_sequence: 1,
                    width: 2,
                    exposed: true,
                },
            )
            .unwrap();
        let owner = manager.job_output_owner(id).unwrap();
        manager.claim(id, 1, true, 2, |_, _| Ok(())).unwrap();
        manager
            .attach_output_memory(id, 1, OutputStream::Result, [owner])
            .unwrap();
        let writer = manager.output_writer(id, 1, OutputStream::Result).unwrap();
        writer.append(b"done").unwrap();
        writer.finish().unwrap();
        manager
            .start(
                id,
                StartRequest {
                    coordinate: 1,
                    claimed_sequence: 2,
                    source_live: true,
                    launch_authorized: true,
                    sequence: 3,
                },
                |_, _| Ok(()),
            )
            .unwrap();
        manager
            .settle(
                id,
                1,
                NodeState::Running { sequence: 3 },
                Status {
                    primary: Primary::Ok,
                    more: false,
                    incomplete: false,
                },
                4,
                |_, _| Ok(()),
            )
            .unwrap();
        manager.claim(id, 2, true, 5, |_, _| Ok(())).unwrap();
        manager
            .start(
                id,
                StartRequest {
                    coordinate: 2,
                    claimed_sequence: 5,
                    source_live: true,
                    launch_authorized: true,
                    sequence: 6,
                },
                |_, _| Ok(()),
            )
            .unwrap();
        manager
            .settle(
                id,
                2,
                NodeState::Running { sequence: 6 },
                Status {
                    primary: second_primary,
                    more: false,
                    incomplete: false,
                },
                7,
                |_, _| Ok(()),
            )
            .unwrap();
        assert_eq!(manager.get(id).unwrap().lifecycle, Lifecycle::Terminal);
        (manager, owner)
    }

    #[test]
    fn terminal_graph_with_unknown_node_preserves_known_results_and_blocks_root_finish() {
        let (manager, owner) = two_node_terminal_output_job("unknown-graph", Primary::Unknown);
        let snapshot = manager.get("unknown-graph").unwrap();
        assert_eq!(snapshot.summary, Some((Primary::Unknown, false, false)));
        assert_eq!(snapshot.status_label(), "unknown");
        let summary = manager
            .render_graph_result("unknown-graph", 4096)
            .unwrap()
            .expect("compact Graph summary fits");
        assert!(summary.contains("unknown ok=1"), "{summary}");
        assert!(summary.contains("unknown=1"), "{summary}");
        let before = manager
            .output_owner_state("unknown-graph", 1, OutputStream::Result, owner)
            .unwrap();
        assert_eq!(before.through, 0);
        assert!(!before.acknowledged && !before.released);
        assert_eq!(
            manager.finish_output_for_owner("unknown-graph", None, None, owner),
            Err(Error::ReplayUnknown)
        );
        assert_eq!(
            manager.finish_output_for_owner("unknown-graph", Some(2), None, owner),
            Err(Error::ReplayUnknown)
        );
        assert_eq!(
            manager
                .output_owner_state("unknown-graph", 1, OutputStream::Result, owner)
                .unwrap(),
            before,
            "an unknown component must never partially release known retained data"
        );
        assert_eq!(manager.spool_usage().unwrap().0, 4);

        // A known node can be finished independently: this is an explicit
        // scoped Control action, not whole-Graph cleanup or an implicit ACK.
        let ack = manager
            .finish_output_for_owner("unknown-graph", Some(1), None, owner)
            .unwrap();
        assert!(ack.changed && ack.released);
    }

    #[test]
    fn terminal_skipped_node_without_stream_has_idempotent_noop_finish() {
        let (manager, owner) = claimed_output_job("skip-no-output", 1024);
        assert_eq!(
            manager.finish_output_for_owner("skip-no-output", None, None, owner),
            Err(Error::Busy),
            "an unsettled Job cannot be finished even with no output attached"
        );
        manager
            .settle_unlaunched(
                "skip-no-output",
                1,
                NodeState::Claimed { sequence: 2 },
                3,
                |_, observed_claim| {
                    assert_eq!(observed_claim, 2);
                    Ok(())
                },
                |_, _| Ok(()),
            )
            .unwrap();
        assert_eq!(
            manager.get("skip-no-output").unwrap().lifecycle,
            Lifecycle::Terminal
        );
        assert_eq!(manager.spool_usage().unwrap().0, 0);

        for target in [Some(1), None] {
            assert_eq!(
                manager.finish_output_for_owner("skip-no-output", target, None, owner),
                Ok(OutputAck {
                    changed: false,
                    released: true,
                }),
                "an empty terminal output scope has nothing left to release"
            );
        }
        assert_eq!(
            manager.finish_output_for_owner(
                "skip-no-output",
                Some(1),
                Some(OutputStream::Result),
                owner,
            ),
            Err(Error::OutputMissing),
            "an explicit nonexistent stream is still an invalid target"
        );
    }

    #[test]
    fn fully_settled_heterogeneous_graph_remains_mixed_and_allows_explicit_finish() {
        let (manager, owner) = two_node_terminal_output_job("known-mixed", Primary::Fail);
        let snapshot = manager.get("known-mixed").unwrap();
        assert_eq!(snapshot.summary, Some((Primary::Mixed, false, false)));
        let summary = manager
            .render_graph_result("known-mixed", 4096)
            .unwrap()
            .expect("mixed Graph summary fits");
        assert!(summary.contains("mixed ok=1 fail=1"), "{summary}");
        let ack = manager
            .finish_output_for_owner("known-mixed", None, None, owner)
            .unwrap();
        assert!(ack.changed && ack.released);
        assert_eq!(manager.spool_usage().unwrap().0, 0);
        let again = manager
            .finish_output_for_owner("known-mixed", None, None, owner)
            .unwrap();
        assert!(!again.changed && again.released);
        assert_eq!(
            manager.acknowledge_delivered_output("known-mixed", 1, OutputStream::Result, owner, 4),
            Ok(OutputAck {
                changed: false,
                released: true
            }),
            "a terminal Finish must preserve the legitimate owner's replay history"
        );
        assert_eq!(
            manager.acknowledge_delivered_output(
                "known-mixed",
                1,
                OutputStream::Result,
                OutputOwner::new([0xF1; 32]).unwrap(),
                0
            ),
            Err(Error::InvalidOutput),
            "terminal Finish must not authorize strangers after release"
        );
    }

    #[cfg(feature = "async")]
    #[test]
    fn discard_if_full_notifies_waiting_reader_of_truncation_without_eof_or_ack() {
        let (manager, owner) = claimed_output_job("discard-watch", 8);
        manager
            .attach_output_memory("discard-watch", 1, OutputStream::Stdout, [owner])
            .unwrap();
        let writer = manager
            .output_writer("discard-watch", 1, OutputStream::Stdout)
            .unwrap();
        assert_eq!(writer.append(b"12345678").unwrap(), 8);
        let mut watch = manager.subscribe_changes("discard-watch").unwrap();
        watch.borrow_and_update();

        assert!(writer.discard_if_full(3).unwrap(), "the quota is exhausted");
        let (summary, finished) = manager
            .output_summary("discard-watch", 1, OutputStream::Stdout)
            .unwrap();
        assert!(summary.truncated);
        assert_eq!(summary.total_bytes, 11);
        assert_eq!(summary.retained_bytes, 8);
        assert!(!finished, "the producer is still running");
        assert!(
            watch.has_changed().unwrap(),
            "truncation is a read-wait wake condition even without new retained bytes"
        );
        watch.borrow_and_update();
        assert!(writer.discard_if_full(2).unwrap());
        let (after, finished) = manager
            .output_summary("discard-watch", 1, OutputStream::Stdout)
            .unwrap();
        assert_eq!(after.total_bytes, 13);
        assert!(after.truncated && !finished);
        assert!(
            !watch.has_changed().unwrap(),
            "discarding further chunks must not repeatedly wake readers"
        );
        assert_eq!(
            manager.acknowledge_delivered_output(
                "discard-watch",
                1,
                OutputStream::Stdout,
                owner,
                8
            ),
            Err(Error::InvalidOutput),
            "a writer notification must not grant delivery proof"
        );
    }

    #[cfg(feature = "async")]
    #[test]
    fn append_after_truncation_counts_bytes_without_repeated_wakeups() {
        let (manager, owner) = claimed_output_job("append-truncated-watch", 8);
        manager
            .attach_output_memory("append-truncated-watch", 1, OutputStream::Stdout, [owner])
            .unwrap();
        let writer = manager
            .output_writer("append-truncated-watch", 1, OutputStream::Stdout)
            .unwrap();
        let mut watch = manager.subscribe_changes("append-truncated-watch").unwrap();
        watch.borrow_and_update();

        assert_eq!(writer.append(b"12345678").unwrap(), 8);
        assert!(
            watch.has_changed().unwrap(),
            "new retained bytes must wake readers"
        );
        watch.borrow_and_update();
        assert_eq!(writer.append(b"9").unwrap(), 0);
        assert!(
            watch.has_changed().unwrap(),
            "first truncation must wake readers"
        );
        watch.borrow_and_update();

        for _ in 0..16 {
            assert_eq!(writer.append(b"later").unwrap(), 0);
        }
        let (summary, finished) = manager
            .output_summary("append-truncated-watch", 1, OutputStream::Stdout)
            .unwrap();
        assert_eq!(summary.retained_bytes, 8);
        assert_eq!(summary.total_bytes, 89);
        assert!(summary.truncated && !finished);
        assert!(
            !watch.has_changed().unwrap(),
            "later dropped chunks must not repeatedly wake a known-truncated stream"
        );
        assert_eq!(
            manager.acknowledge_delivered_output(
                "append-truncated-watch",
                1,
                OutputStream::Stdout,
                owner,
                8,
            ),
            Err(Error::InvalidOutput),
            "append and watch events never authorize raw output ACK"
        );
    }

    #[cfg(feature = "async")]
    #[test]
    fn failed_writer_drain_accounts_bytes_without_repeated_change_wakes() {
        let (manager, owner) = claimed_output_job("writer-failure-watch", 64);
        manager
            .attach_output_memory("writer-failure-watch", 1, OutputStream::Stdout, [owner])
            .unwrap();
        let writer = manager
            .output_writer("writer-failure-watch", 1, OutputStream::Stdout)
            .unwrap();
        let mut watch = manager.subscribe_changes("writer-failure-watch").unwrap();
        watch.borrow_and_update();
        assert_eq!(writer.append(b"abc").unwrap(), 3);
        assert!(watch.has_changed().unwrap());
        watch.borrow_and_update();

        writer.mark_failed().unwrap();
        assert!(
            watch.has_changed().unwrap(),
            "first output failure must notify readers"
        );
        watch.borrow_and_update();
        writer.observe_after_failure(5).unwrap();
        assert!(
            watch.has_changed().unwrap(),
            "the first discarded bytes publish truncation"
        );
        watch.borrow_and_update();
        for _ in 0..15 {
            writer.observe_after_failure(5).unwrap();
        }
        let (summary, finished) = manager
            .output_summary("writer-failure-watch", 1, OutputStream::Stdout)
            .unwrap();
        assert_eq!((summary.retained_bytes, summary.total_bytes), (3, 83));
        assert!(!finished, "the writer is still draining");
        assert!(
            !watch.has_changed().unwrap(),
            "subsequent source accounting cannot change retained bytes or failure readiness"
        );
        assert_eq!(writer.finish(), Err(Error::OutputFailure));
        assert!(watch.has_changed().unwrap(), "final EOF must notify");
        assert_eq!(
            manager.acknowledge_delivered_output(
                "writer-failure-watch",
                1,
                OutputStream::Stdout,
                owner,
                3
            ),
            Err(Error::InvalidOutput),
            "failure updates cannot grant delivery credit"
        );
    }

    #[cfg(feature = "async")]
    #[test]
    fn failed_append_notifies_before_eof_without_granting_delivery() {
        let (manager, owner) = claimed_output_job("failed-append-watch", 64);
        manager
            .attach_output_memory("failed-append-watch", 1, OutputStream::Stdout, [owner])
            .unwrap();
        let writer = manager
            .output_writer("failed-append-watch", 1, OutputStream::Stdout)
            .unwrap();
        // Simulate a spool that failed internally before a producer notices.
        writer.output.lock().unwrap().spool.mark_writer_failed();
        let mut watch = manager.subscribe_changes("failed-append-watch").unwrap();
        watch.borrow_and_update();
        assert_eq!(writer.append(b"failed bytes"), Err(Error::OutputFailure));
        assert!(
            watch.has_changed().unwrap(),
            "a write error must wake output observers"
        );
        let (summary, finished) = manager
            .output_summary("failed-append-watch", 1, OutputStream::Stdout)
            .unwrap();
        assert_eq!((summary.retained_bytes, summary.total_bytes), (0, 0));
        assert!(!finished);
        assert_eq!(
            manager.acknowledge_delivered_output(
                "failed-append-watch",
                1,
                OutputStream::Stdout,
                owner,
                0
            ),
            Err(Error::InvalidOutput)
        );
    }

    #[cfg(feature = "async")]
    #[test]
    fn one_job_change_watch_covers_late_stream_attach_bytes_and_terminal() {
        let (manager, owner) = claimed_output_job("watch-events", 1024);
        let mut changes = manager.subscribe_changes("watch-events").unwrap();
        let initial = *changes.borrow_and_update();
        manager
            .attach_output_memory("watch-events", 1, OutputStream::Stdout, [owner])
            .unwrap();
        assert!(
            changes.has_changed().unwrap(),
            "late stream attachment must notify"
        );
        assert_ne!(*changes.borrow_and_update(), initial);
        let writer = manager
            .output_writer("watch-events", 1, OutputStream::Stdout)
            .unwrap();
        writer.append(b"hello").unwrap();
        assert!(
            changes.has_changed().unwrap(),
            "new retained bytes must notify"
        );
        changes.borrow_and_update();
        writer.finish().unwrap();
        assert!(
            changes.has_changed().unwrap(),
            "EOF must notify even without bytes"
        );
        changes.borrow_and_update();
        manager
            .start(
                "watch-events",
                StartRequest {
                    coordinate: 1,
                    claimed_sequence: 2,
                    source_live: true,
                    launch_authorized: true,
                    sequence: 3,
                },
                |_, _| Ok(()),
            )
            .unwrap();
        assert!(
            changes.has_changed().unwrap(),
            "start may notify, but is not settlement"
        );
        changes.borrow_and_update();
        assert_eq!(
            manager.get("watch-events").unwrap().lifecycle,
            Lifecycle::Running
        );
        manager
            .settle(
                "watch-events",
                1,
                NodeState::Running { sequence: 3 },
                Status {
                    primary: Primary::Ok,
                    more: false,
                    incomplete: false,
                },
                4,
                |_, _| Ok(()),
            )
            .unwrap();
        assert!(changes.has_changed().unwrap(), "settled Job must notify");
        assert_eq!(
            manager.get("watch-events").unwrap().lifecycle,
            Lifecycle::Terminal
        );
        assert_eq!(
            manager
                .output_page_for_owner("watch-events", 1, OutputStream::Stdout, owner, 0, 16)
                .unwrap()
                .bytes,
            b"hello"
        );
        // Notification is not an ACK. Subscriber observations cannot
        // authorize a prefix without a successful final output projection.
        assert_eq!(
            manager.acknowledge_delivered_output("watch-events", 1, OutputStream::Stdout, owner, 5),
            Err(Error::InvalidOutput)
        );
    }

    #[test]
    fn tail_peek_cannot_ack_unread_prefix_and_forward_paging_recovers() {
        let (manager, owner) = claimed_output_job("owner-gaps", 1024);
        let (file, path) = output_file();
        manager
            .attach_output("owner-gaps", 1, OutputStream::Stdout, file, [owner])
            .unwrap();
        let writer = manager
            .output_writer("owner-gaps", 1, OutputStream::Stdout)
            .unwrap();
        writer.append(b"abcdefghij").unwrap();
        writer.finish().unwrap();
        let tail = manager
            .output_page_for_owner_at(
                "owner-gaps",
                1,
                OutputStream::Stdout,
                owner,
                OutputPosition::Tail(3),
                3,
            )
            .unwrap();
        assert_eq!((tail.offset, tail.bytes.as_slice()), (7, b"hij".as_slice()));
        assert_eq!(
            manager.acknowledge_delivered_output("owner-gaps", 1, OutputStream::Stdout, owner, 10),
            Err(Error::InvalidOutput)
        );
        let first = manager
            .output_page_for_owner("owner-gaps", 1, OutputStream::Stdout, owner, 0, 4)
            .unwrap();
        assert_eq!(
            (first.offset, first.bytes.as_slice()),
            (0, b"abcd".as_slice())
        );
        // Fetching bytes creates no proof. Only the host can mark the exact
        // prefix it actually included in its bounded final response.
        assert_eq!(
            manager.acknowledge_delivered_output("owner-gaps", 1, OutputStream::Stdout, owner, 4),
            Err(Error::InvalidOutput)
        );
        assert_eq!(
            manager.record_delivered_output("owner-gaps", 1, OutputStream::Stdout, owner, 0, 11),
            Err(Error::InvalidOutput),
            "host cannot credit bytes beyond the retained spool"
        );
        manager
            .record_delivered_output("owner-gaps", 1, OutputStream::Stdout, owner, 0, 4)
            .unwrap();
        manager
            .acknowledge_delivered_output("owner-gaps", 1, OutputStream::Stdout, owner, 4)
            .unwrap();
        assert_eq!(
            manager.acknowledge_delivered_output("owner-gaps", 1, OutputStream::Stdout, owner, 10),
            Err(Error::InvalidOutput)
        );
        // A later out-of-order read, even when explicitly projected,
        // does not pre-authorize a gap.
        manager
            .output_page_for_owner("owner-gaps", 1, OutputStream::Stdout, owner, 8, 2)
            .unwrap();
        manager
            .record_delivered_output("owner-gaps", 1, OutputStream::Stdout, owner, 8, 2)
            .unwrap();
        assert_eq!(
            manager.acknowledge_delivered_output("owner-gaps", 1, OutputStream::Stdout, owner, 10),
            Err(Error::InvalidOutput)
        );
        manager
            .output_page_for_owner("owner-gaps", 1, OutputStream::Stdout, owner, 4, 6)
            .unwrap();
        manager
            .record_delivered_output("owner-gaps", 1, OutputStream::Stdout, owner, 4, 6)
            .unwrap();
        assert!(
            manager
                .acknowledge_delivered_output("owner-gaps", 1, OutputStream::Stdout, owner, 10)
                .unwrap()
                .released
        );
        assert_eq!(manager.spool_usage().unwrap().0, 0);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn process_tail_is_aggregate_and_quick_finish_waits_for_settlement_and_every_owner() {
        let (manager, owner) = claimed_output_job("owner-tail", 4096);
        let second = OutputOwner::new([0xA7; 32]).unwrap();
        let (stdout_file, stdout_path) = output_file();
        let (stderr_file, stderr_path) = output_file();
        let (stdout, stderr) = manager
            .attach_process_output_files(
                "owner-tail",
                1,
                [owner, second],
                ShellOutputFiles {
                    stdout: stdout_file,
                    stderr: stderr_file,
                    stream_capacity: 1024,
                },
            )
            .unwrap();
        stdout.append(&vec![b'a'; 900]).unwrap();
        stderr.append(&[b'b'; 100]).unwrap();
        let (out, err) = manager
            .process_tail_for_owner("owner-tail", 1, owner, 512)
            .unwrap();
        let (out, err) = (out.unwrap(), err.unwrap());
        assert_eq!((out.bytes.len(), err.bytes.len()), (412, 100));
        assert_eq!((out.offset, err.offset), (488, 0));
        assert_eq!(
            manager.acknowledge_delivered_output("owner-tail", 1, OutputStream::Stdout, owner, 900),
            Err(Error::InvalidOutput)
        );
        assert_eq!(
            manager.acknowledge_delivered_output("owner-tail", 1, OutputStream::Stderr, owner, 0),
            Err(Error::InvalidOutput),
            "jobs_get tail must not credit even a zero-byte prefix"
        );
        let (out, err) = manager
            .process_tail_for_owner("owner-tail", 1, owner, 1024)
            .unwrap();
        assert_eq!(
            (out.unwrap().bytes.len(), err.unwrap().bytes.len()),
            (900, 100)
        );
        stdout.finish().unwrap();
        stderr.finish().unwrap();
        assert_eq!(
            manager.finish_output_for_owner("owner-tail", None, None, owner),
            Err(Error::Busy)
        );
        manager
            .start(
                "owner-tail",
                StartRequest {
                    coordinate: 1,
                    claimed_sequence: 2,
                    source_live: true,
                    launch_authorized: true,
                    sequence: 3,
                },
                |_, _| Ok(()),
            )
            .unwrap();
        manager
            .settle(
                "owner-tail",
                1,
                NodeState::Running { sequence: 3 },
                Status {
                    primary: Primary::Ok,
                    more: false,
                    incomplete: false,
                },
                4,
                |_, _| Ok(()),
            )
            .unwrap();
        let first = manager
            .finish_output_for_owner("owner-tail", None, None, owner)
            .unwrap();
        assert!(first.changed && !first.released);
        assert_eq!(manager.spool_usage().unwrap().0, 1000);
        let second_ack = manager
            .finish_output_for_owner("owner-tail", None, None, second)
            .unwrap();
        assert!(second_ack.changed && second_ack.released);
        assert_eq!(manager.spool_usage().unwrap().0, 0);
        let repeat = manager
            .finish_output_for_owner("owner-tail", None, None, owner)
            .unwrap();
        assert!(!repeat.changed && repeat.released);
        let _ = std::fs::remove_file(stdout_path);
        let _ = std::fs::remove_file(stderr_path);
    }

    #[test]
    fn consumer_lease_scope_is_job_bound_and_all_streams_are_independent() {
        let (manager, producer) = claimed_output_job("consumer-scope", 1024);
        for stream in [OutputStream::Stdout, OutputStream::Stderr] {
            manager
                .attach_output_memory("consumer-scope", 1, stream, [producer])
                .unwrap();
            let writer = manager.output_writer("consumer-scope", 1, stream).unwrap();
            writer.append(b"abcdef").unwrap();
            writer.finish().unwrap();
        }
        let alpha = manager
            .consumer_output_owner("consumer-scope", [0xA1; 32])
            .unwrap();
        let beta = manager
            .consumer_output_owner("consumer-scope", [0xB2; 32])
            .unwrap();
        assert_ne!(alpha, beta);
        assert_ne!(alpha, producer);
        assert_ne!(beta, producer);
        assert_eq!(
            manager.consumer_output_owner("consumer-scope", [0; 32]),
            Err(Error::InvalidOutput)
        );

        // Identity alone is never a grant: the owner must be explicitly
        // enrolled in the exact retained streams by the authorized host.
        assert_eq!(
            manager.output_page_for_owner("consumer-scope", 1, OutputStream::Stdout, alpha, 0, 6),
            Err(Error::InvalidOutput)
        );
        assert!(manager
            .enroll_output_consumer("consumer-scope", Some(1), None, alpha)
            .unwrap());
        assert!(
            !manager
                .enroll_output_consumer("consumer-scope", Some(1), None, alpha)
                .unwrap(),
            "stable consumer enrollment is idempotent"
        );
        assert!(manager
            .enroll_output_consumer("consumer-scope", None, None, beta)
            .unwrap());

        manager
            .start(
                "consumer-scope",
                StartRequest {
                    coordinate: 1,
                    claimed_sequence: 2,
                    source_live: true,
                    launch_authorized: true,
                    sequence: 3,
                },
                |_, _| Ok(()),
            )
            .unwrap();
        manager
            .settle(
                "consumer-scope",
                1,
                NodeState::Running { sequence: 3 },
                Status {
                    primary: Primary::Ok,
                    more: false,
                    incomplete: false,
                },
                4,
                |_, _| Ok(()),
            )
            .unwrap();

        // Each consumer has a separate delivered/ACK watermark. Producer
        // Finish cannot discard the other two independently held leases.
        let producer_ack = manager
            .finish_output_for_owner("consumer-scope", None, None, producer)
            .unwrap();
        assert!(producer_ack.changed && !producer_ack.released);
        assert_eq!(manager.spool_usage().unwrap().0, 12);

        let a_page = manager
            .output_page_for_owner("consumer-scope", 1, OutputStream::Stdout, alpha, 0, 6)
            .unwrap();
        assert_eq!(a_page.bytes, b"abcdef");
        manager
            .record_delivered_output("consumer-scope", 1, OutputStream::Stdout, alpha, 0, 6)
            .unwrap();
        assert_eq!(
            manager.acknowledge_delivered_output(
                "consumer-scope",
                1,
                OutputStream::Stdout,
                beta,
                1
            ),
            Err(Error::InvalidOutput),
            "another consumer's delivery is not this consumer's proof"
        );
        manager
            .acknowledge_delivered_output("consumer-scope", 1, OutputStream::Stdout, alpha, 6)
            .unwrap();
        let a_ack = manager
            .finish_output_for_owner("consumer-scope", None, None, alpha)
            .unwrap();
        assert!(a_ack.changed && !a_ack.released);
        assert_eq!(manager.spool_usage().unwrap().0, 12);

        let b_page = manager
            .output_page_for_owner("consumer-scope", 1, OutputStream::Stderr, beta, 0, 3)
            .unwrap();
        assert_eq!(b_page.bytes, b"abc");
        manager
            .record_delivered_output("consumer-scope", 1, OutputStream::Stderr, beta, 0, 3)
            .unwrap();
        assert_eq!(
            manager.acknowledge_delivered_output(
                "consumer-scope",
                1,
                OutputStream::Stderr,
                beta,
                4
            ),
            Err(Error::InvalidOutput)
        );
        assert_eq!(manager.spool_usage().unwrap().0, 12);
        let released = manager
            .finish_output_for_owner("consumer-scope", None, None, beta)
            .unwrap();
        assert!(released.changed && released.released);
        assert_eq!(manager.spool_usage().unwrap().0, 0);
        assert_eq!(
            manager.enroll_output_consumer(
                "consumer-scope",
                None,
                None,
                manager
                    .consumer_output_owner("consumer-scope", [0xC3; 32])
                    .unwrap()
            ),
            Err(Error::OutputReleased),
            "released bytes are not resurrected by a later consumer"
        );

        let (other_manager, _) = claimed_output_job("other-job", 128);
        assert_ne!(
            other_manager
                .consumer_output_owner("other-job", [0xA1; 32])
                .unwrap(),
            alpha,
            "the same host consumer in another Job is a different lease"
        );
    }

    #[test]
    fn consumer_enrollment_preflights_all_streams_before_minting_any_lease() {
        let (manager, producer) = claimed_output_job("owner-capacity", 512);
        for stream in [OutputStream::Stdout, OutputStream::Stderr] {
            manager
                .attach_output_memory("owner-capacity", 1, stream, [producer])
                .unwrap();
        }
        // Saturate only stderr; stdout still has room. An aggregate enrolment
        // must not leave an orphaned stdout lease when stderr admission fails.
        for index in 1..16u8 {
            manager
                .add_output_owner(
                    "owner-capacity",
                    1,
                    OutputStream::Stderr,
                    OutputOwner::new([index; 32]).unwrap(),
                )
                .unwrap();
        }
        let extra = manager
            .consumer_output_owner("owner-capacity", [0xEE; 32])
            .unwrap();
        assert_eq!(
            manager.enroll_output_consumer("owner-capacity", None, None, extra),
            Err(Error::Capacity)
        );
        assert_eq!(
            manager.output_owner_state("owner-capacity", 1, OutputStream::Stdout, extra),
            Err(Error::InvalidOutput),
            "no partial enrollment despite earlier stream being available"
        );
        assert!(manager
            .enroll_output_consumer("owner-capacity", Some(1), Some(OutputStream::Stdout), extra)
            .unwrap());
        assert!(!manager
            .enroll_output_consumer("owner-capacity", Some(1), Some(OutputStream::Stdout), extra)
            .unwrap());
        assert_eq!(
            manager.enroll_output_consumer("owner-capacity", Some(7), None, extra),
            Err(Error::InvalidOutput)
        );
        assert_eq!(
            manager.enroll_output_consumer(
                "owner-capacity",
                Some(1),
                Some(OutputStream::Result),
                extra
            ),
            Err(Error::OutputMissing)
        );
    }

    #[test]
    fn released_stream_ack_requires_original_owner_and_known_offset() {
        let (manager, owner) = claimed_output_job("released-owner", 1024);
        let stranger = OutputOwner::new([0xEA; 32]).unwrap();
        let stream = OutputStream::Stdout;
        manager
            .attach_output_memory("released-owner", 1, stream, [owner])
            .unwrap();
        let writer = manager.output_writer("released-owner", 1, stream).unwrap();
        assert_eq!(writer.append(b"abcdef").unwrap(), 6);
        writer.finish().unwrap();
        manager
            .record_delivered_output("released-owner", 1, stream, owner, 0, 6)
            .unwrap();
        assert_eq!(
            manager.acknowledge_delivered_output("released-owner", 1, stream, owner, 6),
            Ok(OutputAck {
                changed: true,
                released: true
            }),
        );
        assert_eq!(manager.spool_usage().unwrap().0, 0);

        // Physical release is not a universal grant; owner and exact range
        // remain enforceable after compaction and even for zero-byte ACKs.
        for offset in [0, 6, 7] {
            assert_eq!(
                manager
                    .acknowledge_delivered_output("released-owner", 1, stream, stranger, offset,),
                Err(Error::InvalidOutput),
            );
            assert_eq!(
                manager.acknowledge_output("released-owner", 1, stream, stranger, offset),
                Err(Error::InvalidOutput),
            );
        }
        for offset in [7, u64::MAX] {
            assert_eq!(
                manager.acknowledge_delivered_output("released-owner", 1, stream, owner, offset),
                Err(Error::InvalidOutput),
            );
            assert_eq!(
                manager.acknowledge_output("released-owner", 1, stream, owner, offset),
                Err(Error::InvalidOutput),
            );
        }

        // Replaying a previously acknowledged range by the correct owner
        // remains a no-op, including after the physical spool is gone.
        for offset in [0, 6] {
            assert_eq!(
                manager.acknowledge_delivered_output("released-owner", 1, stream, owner, offset),
                Ok(OutputAck {
                    changed: false,
                    released: true
                }),
            );
            assert_eq!(
                manager.acknowledge_output("released-owner", 1, stream, owner, offset),
                Ok(OutputAck {
                    changed: false,
                    released: true
                }),
            );
        }
        assert_eq!(manager.spool_usage().unwrap().0, 0);
    }

    #[test]
    fn journal_unknown_fences_both_output_ack_paths_before_release() {
        let (manager, owner) = claimed_output_job("journal-unknown-ack", 1024);
        let stream = OutputStream::Stdout;
        manager
            .attach_output_memory("journal-unknown-ack", 1, stream, [owner])
            .unwrap();
        let writer = manager
            .output_writer("journal-unknown-ack", 1, stream)
            .unwrap();
        writer.append(b"secret").unwrap();
        writer.finish().unwrap();
        manager
            .record_delivered_output("journal-unknown-ack", 1, stream, owner, 0, 6)
            .unwrap();

        assert_eq!(
            manager.start(
                "journal-unknown-ack",
                StartRequest {
                    coordinate: 1,
                    claimed_sequence: 2,
                    source_live: true,
                    launch_authorized: true,
                    sequence: 3,
                },
                |_, _| {
                    // Persistence runs outside the manager mutex. No output
                    // mutation may race this unresolved host journal commit.
                    assert_eq!(
                        manager.acknowledge_delivered_output(
                            "journal-unknown-ack",
                            1,
                            stream,
                            owner,
                            6
                        ),
                        Err(Error::Busy)
                    );
                    assert_eq!(
                        manager.acknowledge_output("journal-unknown-ack", 1, stream, owner, 6),
                        Err(Error::Busy)
                    );
                    assert_eq!(
                        manager.compact_output_for_owner(
                            "journal-unknown-ack",
                            1,
                            stream,
                            owner,
                            0
                        ),
                        Err(Error::Busy)
                    );
                    assert_eq!(manager.spool_usage().unwrap().0, 6);
                    Err(JournalError::OutcomeUnknown)
                }
            ),
            Err(Error::Job(JobError::JournalUnknown)),
        );
        assert!(manager.get("journal-unknown-ack").unwrap().journal_unknown);
        for ack in [
            manager.acknowledge_delivered_output("journal-unknown-ack", 1, stream, owner, 6),
            manager.acknowledge_output("journal-unknown-ack", 1, stream, owner, 6),
        ] {
            assert_eq!(
                ack,
                Err(Error::ReplayUnknown),
                "journal-fenced output must be retained for reconciliation"
            );
        }
        assert_eq!(
            manager
                .output_owner_state("journal-unknown-ack", 1, stream, owner)
                .unwrap(),
            OutputOwnerState {
                through: 0,
                acknowledged: false,
                released: false,
            }
        );
        assert_eq!(manager.spool_usage().unwrap().0, 6);
        assert_eq!(
            manager.compact_output_for_owner("journal-unknown-ack", 1, stream, owner, 0),
            Err(Error::ReplayUnknown),
            "trusted compaction must not discard a journal-unknown spool either"
        );
        assert_eq!(manager.spool_usage().unwrap().0, 6);
        assert_eq!(
            manager.finish_output_for_owner("journal-unknown-ack", None, None, owner),
            Err(Error::ReplayUnknown)
        );
    }

    #[test]
    fn owner_state_is_exact_and_independent_for_shared_output() {
        let (manager, job_owner) = claimed_output_job("owner-state", 64);
        let task_owner = OutputOwner::new([0x6B; 32]).unwrap();
        let (stdout_file, stdout_path) = output_file();
        manager
            .attach_output_with_limit(
                "owner-state",
                1,
                OutputStream::Stdout,
                stdout_file,
                [job_owner, task_owner],
                8,
            )
            .unwrap();
        let writer = manager
            .output_writer("owner-state", 1, OutputStream::Stdout)
            .unwrap();
        assert_eq!(writer.append(b"abcdef").unwrap(), 6);
        writer.finish().unwrap();
        assert_eq!(
            manager
                .output_owner_state("owner-state", 1, OutputStream::Stdout, job_owner)
                .unwrap(),
            OutputOwnerState {
                through: 0,
                acknowledged: false,
                released: false,
            }
        );
        manager
            .acknowledge_output("owner-state", 1, OutputStream::Stdout, job_owner, 4)
            .unwrap();
        assert_eq!(
            manager
                .output_owner_state("owner-state", 1, OutputStream::Stdout, job_owner)
                .unwrap(),
            OutputOwnerState {
                through: 4,
                acknowledged: true,
                released: false,
            }
        );
        assert_eq!(
            manager
                .output_owner_state("owner-state", 1, OutputStream::Stdout, task_owner)
                .unwrap(),
            OutputOwnerState {
                through: 0,
                acknowledged: false,
                released: false,
            }
        );
        manager
            .acknowledge_output("owner-state", 1, OutputStream::Stdout, job_owner, 6)
            .unwrap();
        assert!(
            manager
                .acknowledge_output("owner-state", 1, OutputStream::Stdout, task_owner, 6)
                .unwrap()
                .released
        );
        assert_eq!(
            manager
                .output_owner_state("owner-state", 1, OutputStream::Stdout, job_owner)
                .unwrap(),
            OutputOwnerState {
                through: 6,
                acknowledged: true,
                released: true,
            }
        );
        let _ = std::fs::remove_file(stdout_path);
    }

    #[test]
    fn process_file_streams_publish_atomically_with_multiple_owners() {
        let (manager, job_owner) = claimed_output_job("process-multi-owner", 64);
        let task_owner = OutputOwner::new([0x7A; 32]).unwrap();
        let (stdout_file, stdout_path) = output_file();
        let (stderr_file, stderr_path) = output_file();
        let (stdout, stderr) = manager
            .attach_process_output_files(
                "process-multi-owner",
                1,
                [job_owner, task_owner],
                ShellOutputFiles {
                    stdout: stdout_file,
                    stderr: stderr_file,
                    stream_capacity: 8,
                },
            )
            .unwrap();
        assert_eq!(stdout.append(b"abcdef").unwrap(), 6);
        assert_eq!(stderr.append(b"uvwxyz").unwrap(), 6);
        stdout.finish().unwrap();
        stderr.finish().unwrap();

        for stream in [OutputStream::Stdout, OutputStream::Stderr] {
            let first = manager
                .acknowledge_output("process-multi-owner", 1, stream, job_owner, 6)
                .unwrap();
            assert!(first.changed && !first.released);
            assert_eq!(manager.spool_usage().unwrap().0, 12);
        }
        assert!(
            manager
                .acknowledge_output(
                    "process-multi-owner",
                    1,
                    OutputStream::Stdout,
                    task_owner,
                    6,
                )
                .unwrap()
                .released
        );
        assert_eq!(manager.spool_usage().unwrap().0, 6);
        assert!(
            manager
                .acknowledge_output(
                    "process-multi-owner",
                    1,
                    OutputStream::Stderr,
                    task_owner,
                    6,
                )
                .unwrap()
                .released
        );
        assert_eq!(manager.spool_usage().unwrap().0, 0);
        let _ = std::fs::remove_file(stdout_path);
        let _ = std::fs::remove_file(stderr_path);
    }

    #[test]
    fn process_file_stream_attachment_is_all_or_nothing() {
        let (manager, owner) = claimed_output_job("process-atomic", 64);
        let (existing_file, existing_path) = output_file();
        manager
            .attach_output_with_limit(
                "process-atomic",
                1,
                OutputStream::Stdout,
                existing_file,
                [owner],
                8,
            )
            .unwrap();
        let writer = manager
            .output_writer("process-atomic", 1, OutputStream::Stdout)
            .unwrap();
        assert_eq!(writer.append(b"old").unwrap(), 3);

        let (stdout_file, stdout_path) = output_file();
        let (stderr_file, stderr_path) = output_file();
        assert!(matches!(
            manager.attach_process_output_files(
                "process-atomic",
                1,
                [owner],
                ShellOutputFiles {
                    stdout: stdout_file,
                    stderr: stderr_file,
                    stream_capacity: 8,
                },
            ),
            Err(Error::InvalidOutput)
        ));
        assert!(matches!(
            manager.output_summary("process-atomic", 1, OutputStream::Stderr),
            Err(Error::OutputMissing)
        ));
        assert_eq!(writer.append(b"!").unwrap(), 1);
        writer.finish().unwrap();
        assert_eq!(
            manager
                .output_page("process-atomic", 1, OutputStream::Stdout, 0, 8)
                .unwrap()
                .bytes,
            b"old!"
        );
        let _ = std::fs::remove_file(existing_path);
        let _ = std::fs::remove_file(stdout_path);
        let _ = std::fs::remove_file(stderr_path);
    }

    #[test]
    fn file_shell_stream_capacity_is_a_real_retained_prefix_ceiling() {
        let graph = Graph::compile(&json!({"ops":[["read",{"path":"a"}]]}), |_, _| true).unwrap();
        let plans = plan_graph(&graph, |_, _| {
            Ok::<_, ()>(NodePolicy {
                budget: SemanticBudget {
                    work_bytes: 1,
                    result_bytes: 16,
                },
                footprint: Footprint::none(),
                registry_generation: 1,
                authority_binding: [1; 32],
                runtime_binding: [2; 32],
                task_result_eligible: false,
            })
        })
        .unwrap();
        let manager = JobManager::new(Limits {
            max_retained_jobs: 2,
            max_exposed_jobs: 2,
            max_hidden_jobs: 2,
            max_active_nodes: 2,
            max_hidden_replays: 2,
            max_spool_bytes: 64,
        })
        .unwrap();
        manager
            .admit_runtime(
                graph,
                plans,
                RuntimeAdmission {
                    id: "file-shell-cap",
                    runtime_session: "session",
                    epoch: 1,
                    admission_sequence: 1,
                    width: 1,
                    exposed: true,
                },
            )
            .unwrap();
        manager
            .claim("file-shell-cap", 1, true, 2, |_, _| Ok(()))
            .unwrap();
        let owner = manager.job_output_owner("file-shell-cap").unwrap();
        let (stdout_file, stdout_path) = output_file();
        let (stderr_file, stderr_path) = output_file();
        let (stdout, stderr) = manager
            .attach_shell_output_files_with_limit(
                "file-shell-cap",
                1,
                owner,
                8,
                ShellOutputFiles {
                    stdout: stdout_file,
                    stderr: stderr_file,
                    stream_capacity: 3,
                },
            )
            .unwrap();
        assert_eq!(stdout.append(b"abcdef").unwrap(), 3);
        assert_eq!(stderr.append(b"uvwxyz").unwrap(), 3);
        stdout.finish().unwrap();
        stderr.finish().unwrap();
        let out = manager
            .output_page("file-shell-cap", 1, OutputStream::Stdout, 0, 8)
            .unwrap();
        let err = manager
            .output_page("file-shell-cap", 1, OutputStream::Stderr, 0, 8)
            .unwrap();
        assert_eq!(out.bytes, b"abc");
        assert_eq!(err.bytes, b"uvw");
        assert_eq!(
            (out.total_bytes, out.retained_bytes, out.truncated),
            (6, 3, true)
        );
        assert_eq!(
            (err.total_bytes, err.retained_bytes, err.truncated),
            (6, 3, true)
        );
        let _ = std::fs::remove_file(stdout_path);
        let _ = std::fs::remove_file(stderr_path);
    }

    #[test]
    fn failed_file_shell_attachment_publishes_no_partial_streams() {
        use std::io::Write as _;
        let graph = Graph::compile(&json!({"ops":[["read",{"path":"a"}]]}), |_, _| true).unwrap();
        let plans = plan_graph(&graph, |_, _| {
            Ok::<_, ()>(NodePolicy {
                budget: SemanticBudget {
                    work_bytes: 1,
                    result_bytes: 16,
                },
                footprint: Footprint::none(),
                registry_generation: 1,
                authority_binding: [1; 32],
                runtime_binding: [2; 32],
                task_result_eligible: false,
            })
        })
        .unwrap();
        let manager = manager(2);
        manager
            .admit_runtime(
                graph,
                plans,
                RuntimeAdmission {
                    id: "file-shell-fail",
                    runtime_session: "session",
                    epoch: 1,
                    admission_sequence: 1,
                    width: 1,
                    exposed: true,
                },
            )
            .unwrap();
        manager
            .claim("file-shell-fail", 1, true, 2, |_, _| Ok(()))
            .unwrap();
        let owner = manager.job_output_owner("file-shell-fail").unwrap();
        let (stdout_file, stdout_path) = output_file();
        let (mut stderr_file, stderr_path) = output_file();
        stderr_file.write_all(b"not-empty").unwrap();
        assert!(manager
            .attach_shell_output_files("file-shell-fail", 1, owner, 16, stdout_file, stderr_file,)
            .is_err());
        for stream in [
            OutputStream::Result,
            OutputStream::Stdout,
            OutputStream::Stderr,
        ] {
            assert!(matches!(
                manager.output_page("file-shell-fail", 1, stream, 0, 1),
                Err(Error::OutputMissing)
            ));
        }
        assert_eq!(manager.spool_usage().unwrap().0, 0);
        let _ = std::fs::remove_file(stdout_path);
        let _ = std::fs::remove_file(stderr_path);
    }

    #[test]
    fn shell_output_attachment_is_atomic_and_result_capacity_is_prepaid() {
        let graph = Graph::compile(&json!({"ops":[["read",{"path":"a"}]]}), |_, _| true).unwrap();
        let plans = plan_graph(&graph, |_, _| {
            Ok::<_, ()>(NodePolicy {
                budget: SemanticBudget {
                    work_bytes: 1,
                    result_bytes: 16,
                },
                footprint: Footprint::none(),
                registry_generation: 1,
                authority_binding: [1; 32],
                runtime_binding: [2; 32],
                task_result_eligible: false,
            })
        })
        .unwrap();
        let manager = JobManager::new(Limits {
            max_retained_jobs: 2,
            max_exposed_jobs: 2,
            max_hidden_jobs: 2,
            max_active_nodes: 2,
            max_hidden_replays: 2,
            max_spool_bytes: 32,
        })
        .unwrap();
        manager
            .admit_runtime(
                graph,
                plans,
                RuntimeAdmission {
                    id: "shell-streams",
                    runtime_session: "session",
                    epoch: 1,
                    admission_sequence: 1,
                    width: 1,
                    exposed: true,
                },
            )
            .unwrap();
        manager
            .claim("shell-streams", 1, true, 2, |_, _| Ok(()))
            .unwrap();
        let owner = manager.job_output_owner("shell-streams").unwrap();
        let (stdout, stderr) = manager
            .attach_shell_outputs("shell-streams", 1, owner, 16, 8)
            .unwrap();
        assert_eq!(manager.spool_usage().unwrap().0, 16);
        assert_eq!(stdout.append(b"abc").unwrap(), 3);
        assert_eq!(stderr.append(b"de").unwrap(), 2);
        assert_eq!(manager.spool_usage().unwrap().0, 21);
        // A second attach cannot partially replace any of the three streams.
        assert!(manager
            .attach_shell_outputs("shell-streams", 1, owner, 8, 8)
            .is_err());
        assert_eq!(
            manager
                .output_page("shell-streams", 1, OutputStream::Stdout, 0, 3)
                .unwrap()
                .bytes,
            b"abc"
        );
    }

    #[test]
    fn memory_output_writer_uses_same_job_paging_ack_and_truncation_metadata() {
        let graph = Graph::compile(&json!({"ops":[["read",{"path":"a"}]]}), |_, _| true).unwrap();
        let node_plans = plan_graph(&graph, |_, _| {
            Ok::<_, ()>(NodePolicy {
                budget: SemanticBudget {
                    work_bytes: 1,
                    result_bytes: 1024,
                },
                footprint: Footprint::none(),
                registry_generation: 1,
                authority_binding: [1; 32],
                runtime_binding: [2; 32],
                task_result_eligible: false,
            })
        })
        .unwrap();
        let manager = JobManager::new(Limits {
            max_retained_jobs: 2,
            max_exposed_jobs: 2,
            max_hidden_jobs: 2,
            max_active_nodes: 2,
            max_hidden_replays: 2,
            max_spool_bytes: 5,
        })
        .unwrap();
        manager
            .admit_runtime(
                graph,
                node_plans,
                RuntimeAdmission {
                    id: "memory-writer",
                    runtime_session: "session",
                    epoch: 1,
                    admission_sequence: 1,
                    width: 1,
                    exposed: true,
                },
            )
            .unwrap();
        manager
            .claim("memory-writer", 1, true, 2, |_, _| Ok(()))
            .unwrap();
        let owner = manager.job_output_owner("memory-writer").unwrap();
        manager
            .attach_output_memory("memory-writer", 1, OutputStream::Stdout, [owner])
            .unwrap();
        let writer = manager
            .output_writer("memory-writer", 1, OutputStream::Stdout)
            .unwrap();
        assert_eq!(writer.append(b"abcdefgh").unwrap(), 5);
        writer.finish().unwrap();
        let page = manager
            .output_page("memory-writer", 1, OutputStream::Stdout, 0, 5)
            .unwrap();
        assert_eq!(page.bytes, b"abcde");
        assert!(page.truncated);
        assert_eq!(page.total_bytes, 8);
        assert_eq!(page.retained_bytes, 5);
        assert!(
            manager
                .acknowledge_output("memory-writer", 1, OutputStream::Stdout, owner, 5,)
                .unwrap()
                .released
        );
    }

    #[test]
    fn host_output_writer_feeds_same_manager_stream_and_finishes_once() {
        let graph = Graph::compile(&json!({"ops":[["read",{"path":"a"}]]}), |_, _| true).unwrap();
        let node_plans = plan_graph(&graph, |_, _| {
            Ok::<_, ()>(NodePolicy {
                budget: SemanticBudget {
                    work_bytes: 1,
                    result_bytes: 1024,
                },
                footprint: Footprint::none(),
                registry_generation: 1,
                authority_binding: [1; 32],
                runtime_binding: [2; 32],
                task_result_eligible: false,
            })
        })
        .unwrap();
        let manager = manager(2);
        manager
            .admit_runtime(
                graph,
                node_plans,
                RuntimeAdmission {
                    id: "writer",
                    runtime_session: "session",
                    epoch: 1,
                    admission_sequence: 1,
                    width: 1,
                    exposed: true,
                },
            )
            .unwrap();
        manager.claim("writer", 1, true, 2, |_, _| Ok(())).unwrap();
        let owner = manager.job_output_owner("writer").unwrap();
        let (file, path) = output_file();
        manager
            .attach_output("writer", 1, OutputStream::Stdout, file, [owner])
            .unwrap();
        let writer = manager
            .output_writer("writer", 1, OutputStream::Stdout)
            .unwrap();
        assert_eq!(writer.append(b"abc").unwrap(), 3);
        writer.finish().unwrap();
        writer.finish().unwrap();
        let page = manager
            .output_page("writer", 1, OutputStream::Stdout, 0, 3)
            .unwrap();
        assert_eq!(page.bytes, b"abc");
        assert!(page.writer_finished);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn failed_output_writer_still_publishes_quiescence_for_retention() {
        let graph = Graph::compile(&json!({"ops":[["read",{"path":"a"}]]}), |_, _| true).unwrap();
        let node_plans = plan_graph(&graph, |_, _| {
            Ok::<_, ()>(NodePolicy {
                budget: SemanticBudget {
                    work_bytes: 1,
                    result_bytes: 1024,
                },
                footprint: Footprint::none(),
                registry_generation: 1,
                authority_binding: [1; 32],
                runtime_binding: [2; 32],
                task_result_eligible: false,
            })
        })
        .unwrap();
        let manager = manager(2);
        manager
            .admit_runtime(
                graph,
                node_plans,
                RuntimeAdmission {
                    id: "failed-writer",
                    runtime_session: "session",
                    epoch: 1,
                    admission_sequence: 1,
                    width: 1,
                    exposed: true,
                },
            )
            .unwrap();
        manager
            .claim("failed-writer", 1, true, 2, |_, _| Ok(()))
            .unwrap();
        let owner = manager.job_output_owner("failed-writer").unwrap();
        manager
            .attach_output_memory("failed-writer", 1, OutputStream::Stdout, [owner])
            .unwrap();
        let writer = manager
            .output_writer("failed-writer", 1, OutputStream::Stdout)
            .unwrap();
        assert_eq!(writer.append(b"abc").unwrap(), 3);
        writer.mark_failed().unwrap();
        assert_eq!(writer.finish(), Err(Error::OutputFailure));
        let (summary, finished) = manager
            .output_summary("failed-writer", 1, OutputStream::Stdout)
            .unwrap();
        assert_eq!(summary.retained_bytes, 3);
        assert!(finished, "failed writer lifetime must still be quiescent");
        drop(writer);

        let sequence = manager.current_sequence("failed-writer").unwrap();
        manager
            .settle(
                "failed-writer",
                1,
                NodeState::Claimed { sequence: 2 },
                Status {
                    primary: Primary::Unknown,
                    more: false,
                    incomplete: false,
                },
                sequence + 1,
                |_, _| Ok(()),
            )
            .unwrap();
        manager.expire_terminal("failed-writer").unwrap();
        assert!(matches!(manager.get("failed-writer"), Err(Error::NotFound)));
        assert_eq!(manager.spool_usage().unwrap().0, 0);
    }

    #[test]
    fn output_truncation_is_observability_not_graph_incomplete() {
        let graph = Graph::compile(&json!({"ops":[["read",{"path":"a"}]]}), |_, _| true).unwrap();
        let node_plans = plan_graph(&graph, |_, _| {
            Ok::<_, ()>(NodePolicy {
                budget: SemanticBudget {
                    work_bytes: 1,
                    result_bytes: 1024,
                },
                footprint: Footprint::none(),
                registry_generation: 1,
                authority_binding: [1; 32],
                runtime_binding: [0xA5; 32],
                task_result_eligible: true,
            })
        })
        .unwrap();
        let manager = JobManager::new(Limits {
            max_retained_jobs: 4,
            max_exposed_jobs: 4,
            max_hidden_jobs: 4,
            max_active_nodes: 2,
            max_hidden_replays: 4,
            max_spool_bytes: 3,
        })
        .unwrap();
        manager
            .admit_runtime(
                graph,
                node_plans,
                RuntimeAdmission {
                    id: "t",
                    runtime_session: "session",
                    epoch: 1,
                    admission_sequence: 1,
                    width: 1,
                    exposed: true,
                },
            )
            .unwrap();
        manager.claim("t", 1, true, 2, |_, _| Ok(())).unwrap();
        manager
            .start(
                "t",
                StartRequest {
                    coordinate: 1,
                    claimed_sequence: 2,
                    source_live: true,
                    launch_authorized: true,
                    sequence: 3,
                },
                |_, _| Ok(()),
            )
            .unwrap();
        let owner = manager.job_output_owner("t").unwrap();
        let (file, path) = output_file();
        manager
            .attach_output("t", 1, OutputStream::Stdout, file, [owner])
            .unwrap();
        assert_eq!(
            manager
                .append_output("t", 1, OutputStream::Stdout, b"abcdef")
                .unwrap(),
            3
        );
        manager.finish_output("t", 1, OutputStream::Stdout).unwrap();
        manager
            .settle(
                "t",
                1,
                NodeState::Running { sequence: 3 },
                Status {
                    primary: Primary::Ok,
                    more: false,
                    incomplete: false,
                },
                4,
                |_, _| Ok(()),
            )
            .unwrap();
        let output = manager
            .output_page("t", 1, OutputStream::Stdout, 0, 3)
            .unwrap();
        assert!(output.truncated);
        assert_eq!(output.total_bytes, 6);
        assert_eq!(output.retained_bytes, 3);
        assert_eq!(
            manager.get("t").unwrap().summary,
            Some((Primary::Ok, false, false))
        );
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn restored_inflight_job_recharges_global_permit_and_requires_reattach() {
        let graph = Graph::compile(&json!({"ops":[["read",{"path":"a"}]]}), |_, _| true).unwrap();
        let node_plans = plan_graph(&graph, |_, _| {
            Ok::<_, ()>(NodePolicy {
                budget: SemanticBudget {
                    work_bytes: 1,
                    result_bytes: 1024,
                },
                footprint: Footprint::none(),
                registry_generation: 1,
                authority_binding: [1; 32],
                runtime_binding: [2; 32],
                task_result_eligible: true,
            })
        })
        .unwrap();
        let mut original = Job::after_runtime_intent(graph.clone(), "s", "old", 1, 1, 1).unwrap();
        let mut records = Vec::new();
        original
            .claim(1, true, true, 2, |record| {
                records.push(record);
                Ok(())
            })
            .unwrap();
        let restored = JobManager::new(Limits {
            max_retained_jobs: 4,
            max_exposed_jobs: 4,
            max_hidden_jobs: 4,
            max_active_nodes: 1,
            max_hidden_replays: 4,
            max_spool_bytes: 1024,
        })
        .unwrap();
        restored
            .restore_runtime(
                graph.clone(),
                records,
                node_plans.clone(),
                RuntimeAdmission {
                    id: "old",
                    runtime_session: "s",
                    epoch: 1,
                    admission_sequence: 1,
                    width: 1,
                    exposed: true,
                },
            )
            .unwrap();
        assert_eq!(
            restored.pending_reattach("old").unwrap(),
            [(1, NodeState::Claimed { sequence: 2 })]
        );
        assert_eq!(restored.active_nodes(), 1);

        let other_graph =
            Graph::compile(&json!({"ops":[["read",{"path":"b"}]]}), |_, _| true).unwrap();
        let other_plans = plan_graph(&other_graph, |_, _| {
            Ok::<_, ()>(NodePolicy {
                budget: SemanticBudget {
                    work_bytes: 1,
                    result_bytes: 1024,
                },
                footprint: Footprint::none(),
                registry_generation: 1,
                authority_binding: [3; 32],
                runtime_binding: [4; 32],
                task_result_eligible: true,
            })
        })
        .unwrap();
        restored
            .admit_runtime(
                other_graph,
                other_plans,
                RuntimeAdmission {
                    id: "new",
                    runtime_session: "s",
                    epoch: 1,
                    admission_sequence: 1,
                    width: 1,
                    exposed: true,
                },
            )
            .unwrap();
        assert_eq!(
            restored.claim("new", 1, true, 2, |_, _| Ok(())),
            Err(Error::Capacity)
        );
    }

    #[test]
    fn multi_node_task_keeps_original_binding_without_minting_task_result_projection() {
        let graph = graph();
        let plans = plans(&graph);
        let task = TaskIdentity {
            actor: "a".into(),
            source_kind: "host".into(),
            source_id: "source".into(),
            source_epoch: 2,
            execution_site: "local".into(),
            session: "s".into(),
            branch: "main".into(),
            turn_id: 1,
            call_id: "call".into(),
            effect: ToolEffect::Read,
        };
        let manager = manager(8);
        manager
            .admit_task(
                graph,
                plans,
                TaskAdmission {
                    id: "graph-task",
                    task: &task,
                    admission_sequence: 1,
                    width: 3,
                    exposed: true,
                },
            )
            .unwrap();
        assert_eq!(manager.task_binding("graph-task").unwrap(), task);
        assert!(matches!(
            manager.task_projection("graph-task", true),
            Err(Error::InvalidOutput)
        ));
    }

    #[test]
    fn cancelled_terminal_unknown_stays_unknown_and_task_uncertain() {
        let graph = Graph::compile(&json!({"ops":[["read",{"path":"a"}]]}), |_, _| true).unwrap();
        let node_plans = plan_graph(&graph, |_, _| {
            Ok::<_, ()>(NodePolicy {
                budget: SemanticBudget {
                    work_bytes: 1,
                    result_bytes: 1024,
                },
                footprint: Footprint::none(),
                registry_generation: 1,
                authority_binding: [1; 32],
                runtime_binding: [2; 32],
                task_result_eligible: true,
            })
        })
        .unwrap();
        let task = TaskIdentity {
            actor: "a".into(),
            source_kind: "host".into(),
            source_id: "source".into(),
            source_epoch: 2,
            execution_site: "local".into(),
            session: "s".into(),
            branch: "main".into(),
            turn_id: 1,
            call_id: "cancel-unknown".into(),
            effect: ToolEffect::Process,
        };
        let manager = manager(8);
        manager
            .admit_task(
                graph,
                node_plans,
                TaskAdmission {
                    id: "cancel-unknown",
                    task: &task,
                    admission_sequence: 1,
                    width: 1,
                    exposed: true,
                },
            )
            .unwrap();
        manager
            .claim("cancel-unknown", 1, true, 2, |_, _| Ok(()))
            .unwrap();
        manager
            .start(
                "cancel-unknown",
                StartRequest {
                    coordinate: 1,
                    claimed_sequence: 2,
                    source_live: true,
                    launch_authorized: true,
                    sequence: 3,
                },
                |_, _| Ok(()),
            )
            .unwrap();
        manager.cancel("cancel-unknown", 4, |_, _| Ok(())).unwrap();
        manager
            .settle(
                "cancel-unknown",
                1,
                NodeState::Running { sequence: 3 },
                Status {
                    primary: Primary::Unknown,
                    more: false,
                    incomplete: false,
                },
                5,
                |_, _| Ok(()),
            )
            .unwrap();

        let snapshot = manager.get("cancel-unknown").unwrap();
        assert_eq!(snapshot.stop_reason, Some(StopReason::Cancelled));
        assert_eq!(snapshot.summary, Some((Primary::Unknown, false, false)));
        assert_eq!(snapshot.status_label(), "unknown");
        assert_eq!(snapshot.list_row(), "cancel-unknown read unknown");
        assert_eq!(
            manager.render_job_status("cancel-unknown").unwrap(),
            "unknown read"
        );
        assert_eq!(
            manager
                .task_projection("cancel-unknown", true)
                .unwrap()
                .info
                .state,
            TaskDisposition::Uncertain
        );
    }

    #[test]
    fn single_node_task_projection_keeps_process_terminal_behind_later_failure() {
        use crate::{
            execution_stop::StopReason as ProcessStopReason,
            filesystem::runner::{ProcessExit, ProcessOutputMeta, ProcessTerminal},
        };
        let graph = Graph::compile_with(
            &json!({"ops":[["bash",{"command":"true"}]]}),
            |name| (name == "bash").then_some(NodeTool::Bash),
            |node, arguments| node == NodeTool::Bash && arguments.is_object(),
        )
        .unwrap();
        let node_plans = plan_graph(&graph, |_, _| {
            Ok::<_, ()>(NodePolicy {
                budget: SemanticBudget {
                    work_bytes: 1,
                    result_bytes: 1024,
                },
                footprint: Footprint::none(),
                registry_generation: 1,
                authority_binding: [1; 32],
                runtime_binding: [2; 32],
                task_result_eligible: true,
            })
        })
        .unwrap();
        let task = TaskIdentity {
            actor: "a".into(),
            source_kind: "host".into(),
            source_id: "source".into(),
            source_epoch: 2,
            execution_site: "local".into(),
            session: "s".into(),
            branch: "main".into(),
            turn_id: 1,
            call_id: "call".into(),
            effect: ToolEffect::Process,
        };
        let manager = manager(8);
        manager
            .admit_task(
                graph,
                node_plans,
                TaskAdmission {
                    id: "task",
                    task: &task,
                    admission_sequence: 10,
                    width: 1,
                    exposed: true,
                },
            )
            .unwrap();
        let queued = manager.task_projection("task", true).unwrap();
        assert_eq!(queued.info.state, TaskDisposition::Queued);
        assert_eq!(queued.info.journal_sequence, 10);
        assert_eq!(queued.info.binding, task);
        assert_eq!(queued.info.terminal, None);
        assert_eq!(
            queued.result_owner,
            manager.task_result_owner("task", 1).unwrap()
        );
        manager.claim("task", 1, true, 11, |_, _| Ok(())).unwrap();
        manager
            .start(
                "task",
                StartRequest {
                    coordinate: 1,
                    claimed_sequence: 11,
                    source_live: true,
                    launch_authorized: true,
                    sequence: 12,
                },
                |_, _| Ok(()),
            )
            .unwrap();
        assert_eq!(
            manager.task_projection("task", true).unwrap().info.state,
            TaskDisposition::Running
        );

        let terminal = ProcessTerminal {
            exit: ProcessExit::Signal(9),
            stop: Some(crate::execution_stop::StopRecord::stopped(
                ProcessStopReason::Timeout,
            )),
            occupancy: crate::filesystem::runner::Occupancy::Live,
            output: ProcessOutputMeta {
                stdout_bytes: 10,
                stderr_bytes: 2,
                stdout_truncated: false,
                stderr_truncated: false,
                stdout_complete: true,
                stderr_complete: true,
            },
        };
        manager
            .settle_process_with_status(
                "task",
                ProcessSettlement {
                    coordinate: 1,
                    expected: NodeState::Running { sequence: 12 },
                    terminal,
                    // Later result/spool/projection failure may make the Task
                    // uncertain without erasing observed process facts.
                    status: Status {
                        primary: Primary::Unknown,
                        more: false,
                        incomplete: false,
                    },
                    sequence: 13,
                },
                |_, _| Ok(()),
            )
            .unwrap();

        let done = manager.task_projection("task", true).unwrap();
        assert_eq!(done.info.state, TaskDisposition::Uncertain);
        assert_eq!(done.info.generation, 13);
        assert_eq!(done.info.terminal, Some(TaskTerminalDetail::from(terminal)));
        assert_eq!(
            done.info.terminal.and_then(|terminal| terminal.stop),
            Some(ProcessStopReason::Timeout)
        );
    }

    #[test]
    fn replay_claim_binds_full_semantics_and_never_duplicates_unknown() {
        let graph = Graph::compile(&json!({"ops":[["read",{"path":"a"}]]}), |_, _| true).unwrap();
        let node_plans = plan_graph(&graph, |_, _| {
            Ok::<_, ()>(NodePolicy {
                budget: SemanticBudget {
                    work_bytes: 1,
                    result_bytes: 1024,
                },
                footprint: Footprint::none(),
                registry_generation: 7,
                authority_binding: [1; 32],
                runtime_binding: [2; 32],
                task_result_eligible: true,
            })
        })
        .unwrap();
        let semantics = semantic_hash(&graph, &node_plans).unwrap();
        let manager = manager(8);
        let decision = manager
            .claim_replay("session", "request", semantics, "r1", |_| {
                Ok(ReplayPersist::Created)
            })
            .unwrap();
        let ReplayDecision::New { key, job_id } = decision else {
            panic!("expected fresh replay claim")
        };
        assert_eq!(job_id, "r1");
        manager
            .admit_runtime(
                graph.clone(),
                node_plans,
                RuntimeAdmission {
                    id: "r1",
                    runtime_session: "session",
                    epoch: 1,
                    admission_sequence: 1,
                    width: 1,
                    exposed: true,
                },
            )
            .unwrap();
        manager.mark_replay_ready(key, "r1", |_| Ok(())).unwrap();
        assert_eq!(
            manager
                .claim_replay("session", "request", semantics, "ignored", |_| {
                    panic!("existing replay must not call create callback")
                })
                .unwrap(),
            ReplayDecision::Existing {
                job_id: "r1".into()
            }
        );
        let mut changed = semantics;
        changed[0] ^= 1;
        assert_eq!(
            manager.claim_replay("session", "request", changed, "r2", |_| {
                Ok(ReplayPersist::Created)
            }),
            Err(Error::ReplayMismatch)
        );
    }

    #[test]
    fn ambiguous_replay_claim_is_permanent_unknown_in_live_namespace() {
        let manager = JobManager::new(Limits {
            max_retained_jobs: 2,
            max_exposed_jobs: 2,
            max_hidden_jobs: 2,
            max_active_nodes: 1,
            max_hidden_replays: 1,
            max_spool_bytes: 1024,
        })
        .unwrap();
        assert_eq!(
            manager.claim_replay("s", "x", [1; 32], "j", |_| {
                Err(JournalError::OutcomeUnknown)
            }),
            Err(Error::ReplayUnknown)
        );
        assert_eq!(
            manager
                .claim_replay("s", "x", [1; 32], "j2", |_| {
                    panic!("unknown tombstone must not retry persistence")
                })
                .unwrap(),
            ReplayDecision::Unknown
        );
        assert_eq!(
            manager.claim_replay("s", "y", [2; 32], "j3", |_| { Ok(ReplayPersist::Created) }),
            Err(Error::Capacity)
        );
    }

    #[test]
    fn restart_requires_terminal_source_and_uses_fresh_current_plan() {
        let graph = Graph::compile(&json!({"ops":[["read",{"path":"a"}]]}), |_, _| true).unwrap();
        let original = plan_graph(&graph, |_, _| {
            Ok::<_, ()>(NodePolicy {
                budget: SemanticBudget {
                    work_bytes: 1,
                    result_bytes: 1024,
                },
                footprint: Footprint::none(),
                registry_generation: 1,
                authority_binding: [1; 32],
                runtime_binding: [2; 32],
                task_result_eligible: true,
            })
        })
        .unwrap();
        let manager = manager(8);
        manager
            .admit_runtime(
                graph.clone(),
                original.clone(),
                RuntimeAdmission {
                    id: "old",
                    runtime_session: "s",
                    epoch: 1,
                    admission_sequence: 1,
                    width: 1,
                    exposed: true,
                },
            )
            .unwrap();
        let fresh = plan_graph(&graph, |_, _| {
            Ok::<_, ()>(NodePolicy {
                budget: SemanticBudget {
                    work_bytes: 2,
                    result_bytes: 2048,
                },
                footprint: Footprint::none(),
                registry_generation: 2,
                authority_binding: [3; 32],
                runtime_binding: [4; 32],
                task_result_eligible: true,
            })
        })
        .unwrap();
        assert_eq!(
            manager.restart_runtime(
                "old",
                graph.clone(),
                fresh.clone(),
                RuntimeAdmission {
                    id: "new",
                    runtime_session: "s",
                    epoch: 2,
                    admission_sequence: 1,
                    width: 1,
                    exposed: true,
                },
            ),
            Err(Error::RestartNotTerminal)
        );
        manager.claim("old", 1, true, 2, |_, _| Ok(())).unwrap();
        manager
            .start(
                "old",
                StartRequest {
                    coordinate: 1,
                    claimed_sequence: 2,
                    source_live: true,
                    launch_authorized: true,
                    sequence: 3,
                },
                |_, _| Ok(()),
            )
            .unwrap();
        manager
            .settle(
                "old",
                1,
                NodeState::Running { sequence: 3 },
                Status {
                    primary: Primary::Ok,
                    more: false,
                    incomplete: false,
                },
                4,
                |_, _| Ok(()),
            )
            .unwrap();
        manager
            .restart_runtime(
                "old",
                graph,
                fresh.clone(),
                RuntimeAdmission {
                    id: "new",
                    runtime_session: "s",
                    epoch: 2,
                    admission_sequence: 1,
                    width: 1,
                    exposed: true,
                },
            )
            .unwrap();
        let new_plans = manager.plans("new").unwrap();
        assert_eq!(new_plans[0].registry_generation(), 2);
        assert_eq!(new_plans[0].authority_binding(), [3; 32]);
        assert_eq!(new_plans[0].runtime_binding(), [4; 32]);
        assert_ne!(
            new_plans[0].replay_identity(),
            original[0].replay_identity()
        );
    }

    #[test]
    fn aw_task_identity_can_admit_same_shared_manager() {
        let graph = Graph::compile(&json!({"ops":[["read",{"path":"a"}]]}), |_, _| true).unwrap();
        let plans = plan_graph(&graph, |_, _| {
            Ok::<_, ()>(NodePolicy {
                budget: SemanticBudget {
                    work_bytes: 1,
                    result_bytes: 1024,
                },
                footprint: Footprint::none(),
                registry_generation: 1,
                authority_binding: [2; 32],
                runtime_binding: [0xA5; 32],
                task_result_eligible: true,
            })
        })
        .unwrap();
        let task = TaskIdentity {
            actor: "a".into(),
            source_kind: "host".into(),
            source_id: "source".into(),
            source_epoch: 2,
            execution_site: "local".into(),
            session: "s".into(),
            branch: "main".into(),
            turn_id: 1,
            call_id: "call".into(),
            effect: ToolEffect::Read,
        };
        let manager = manager(8);
        manager
            .admit_task(
                graph,
                plans,
                TaskAdmission {
                    id: "task-job",
                    task: &task,
                    admission_sequence: 1,
                    width: 1,
                    exposed: true,
                },
            )
            .unwrap();
        assert_eq!(manager.get("task-job").unwrap().label, "read");
    }
}
