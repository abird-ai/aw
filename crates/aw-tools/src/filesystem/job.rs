//! Canonical Job(Graph[Node]) lifecycle state projection.
//!
//! The host's original authority remains the ONLY effect authority. This module
//! does not launch tools, own a second workflow store, create permissions or
//! expose a model-facing JobManager. Node transitions are host-confirmed CAS
//! events inside one live Job session; ambiguous confirmation never triggers a
//! repeated effect.
use super::{
    batch::{Graph, Status},
    status::{self, Primary},
};
use sha2::{Digest as _, Sha256};

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum NodeState {
    Pending,
    Claimed { sequence: u64 },
    Running { sequence: u64 },
    Terminal { sequence: u64, status: Status },
}
impl NodeState {
    pub fn result(self) -> Option<Status> {
        match self {
            Self::Terminal { status, .. } => Some(status),
            _ => None,
        }
    }
    pub fn in_flight(self) -> bool {
        matches!(self, Self::Claimed { .. } | Self::Running { .. })
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Lifecycle {
    Running,
    Cancelling,
    Revoking,
    Terminal,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum StopReason {
    Cancelled,
    Revoked,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JournalError {
    RejectedBeforeEffect,
    OutcomeUnknown,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    InvalidIdentity,
    InvalidSequence,
    InvalidWidth,
    NotPermitted,
    NotReady,
    InvalidState,
    StaleGeneration,
    SourceRevoked,
    ExceedsParallelism,
    JournalRejected,
    JournalUnknown,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Action {
    Claim,
    Start,
    Settle(Status),
    /// Original host proved a claimed runner never launched.
    UnlaunchedSkip,
    Skip,
    Cancel,
    Revoke,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Transition {
    /// Exact original journal sequence checked by the host's atomic CAS.
    pub previous_sequence: u64,
    pub next_sequence: u64,
    pub run_binding: [u8; 32],
    pub graph_hash: [u8; 32],
    pub node: Option<usize>,
    pub before: Option<NodeState>,
    pub action: Action,
}
/// An owning session fingerprint is supplied by the original host; no client
/// ID is itself a capability. Job identity is scoped to the live runtime.
#[derive(Debug)]
pub struct Job {
    graph: Graph,
    binding: [u8; 32],
    graph_hash: [u8; 32],
    sequence: u64,
    admission_sequence: u64,
    state: Lifecycle,
    stop_reason: Option<StopReason>,
    nodes: Vec<NodeState>,
    width: usize,
    ambiguous_journal: bool,
}
impl Job {
    /// Call only after the original host durably admitted one immutable graph
    /// and assigned the exact runtime-session Job identity.
    fn after_journal(
        graph: Graph,
        runtime_session: &str,
        job_id: &str,
        epoch: u64,
        sequence: u64,
        width: usize,
    ) -> Result<Self, Error> {
        if runtime_session.is_empty()
            || job_id.is_empty()
            || runtime_session.len() > 256
            || job_id.len() > 256
            || runtime_session.chars().any(char::is_control)
            || job_id.chars().any(char::is_control)
            || epoch == 0
            || sequence == 0
        {
            return Err(Error::InvalidIdentity);
        }
        if !(1..=16).contains(&width) {
            return Err(Error::InvalidWidth);
        }
        let graph_hash = graph.fingerprint();
        let mut h = Sha256::new();
        h.update(b"aw-tools-core-job-session-v1");
        for value in [runtime_session, job_id] {
            h.update((value.len() as u64).to_le_bytes());
            h.update(value.as_bytes());
        }
        h.update(epoch.to_le_bytes());
        h.update(graph_hash);
        let binding = h.finalize().into();
        let nodes = vec![NodeState::Pending; graph.steps().len()];
        Ok(Self {
            graph,
            binding,
            graph_hash,
            sequence,
            admission_sequence: sequence,
            state: Lifecycle::Running,
            stop_reason: None,
            nodes,
            width,
            ambiguous_journal: false,
        })
    }
    /// Host-neutral runtime-session binding for dotlink/other consumers. The
    /// host must separately capture/revalidate exact node authority in NodePlan.
    pub fn after_runtime_intent(
        graph: Graph,
        runtime_session: &str,
        job_id: &str,
        epoch: u64,
        sequence: u64,
        width: usize,
    ) -> Result<Self, Error> {
        Self::after_journal(graph, runtime_session, job_id, epoch, sequence, width)
    }

    /// Exact actor/source/site/session/turn binding for an AW host intent.
    /// No client-visible job ID can be used to widen the original grant.
    pub fn after_task_intent(
        graph: Graph,
        task: &crate::task_fence::TaskIdentity,
        job_id: &str,
        sequence: u64,
        width: usize,
    ) -> Result<Self, Error> {
        task.validate().map_err(|_| Error::InvalidIdentity)?;
        let mut projection = Self::after_journal(
            graph,
            &task.session,
            job_id,
            task.source_epoch,
            sequence,
            width,
        )?;
        let mut hash = Sha256::new();
        hash.update(b"aw-tools-core-job-task-binding-v2");
        hash.update(task.fingerprint());
        hash.update((job_id.len() as u64).to_le_bytes());
        hash.update(job_id.as_bytes());
        hash.update(projection.graph_hash);
        projection.binding = hash.finalize().into();
        Ok(projection)
    }
    pub fn from_task_journal(
        graph: Graph,
        task: &crate::task_fence::TaskIdentity,
        job_id: &str,
        sequence: u64,
        width: usize,
        records: impl IntoIterator<Item = Transition>,
    ) -> Result<Self, Error> {
        let mut projection = Self::after_task_intent(graph, task, job_id, sequence, width)?;
        for record in records {
            projection.apply_committed(record)?;
        }
        Ok(projection)
    }
    pub fn binding(&self) -> [u8; 32] {
        self.binding
    }
    pub fn graph_hash(&self) -> [u8; 32] {
        self.graph_hash
    }
    pub fn sequence(&self) -> u64 {
        self.sequence
    }
    pub fn admission_sequence(&self) -> u64 {
        self.admission_sequence
    }
    pub fn lifecycle(&self) -> Lifecycle {
        self.state
    }
    pub fn stop_reason(&self) -> Option<StopReason> {
        self.stop_reason
    }
    pub fn state(&self, coordinate: usize) -> Option<NodeState> {
        coordinate
            .checked_sub(1)
            .and_then(|i| self.nodes.get(i).copied())
    }
    pub fn graph(&self) -> &Graph {
        &self.graph
    }
    pub fn total(&self) -> usize {
        self.nodes.len()
    }
    pub fn finished(&self) -> usize {
        self.nodes.iter().filter(|n| n.result().is_some()).count()
    }
    pub fn active(&self) -> usize {
        self.nodes.iter().filter(|n| n.in_flight()).count()
    }
    pub fn journal_ambiguous(&self) -> bool {
        self.ambiguous_journal
    }

    /// Condition state for a pending node. None means referenced predecessors
    /// are not terminal yet; false means the node is structurally skippable.
    pub fn pending_condition(&self, coordinate: usize) -> Result<Option<bool>, Error> {
        self.ready(coordinate)
    }

    pub fn summary(&self) -> Option<(Primary, bool, bool)> {
        if self.finished() != self.nodes.len() {
            return None;
        }
        let statuses = self.nodes.iter().filter_map(|n| n.result());
        let mut more = false;
        let mut incomplete = false;
        let mut outcomes = Vec::with_capacity(self.nodes.len());
        for result in statuses {
            outcomes.push(result.primary);
            more |= result.more;
            incomplete |= result.incomplete;
        }
        Some((status::aggregate(outcomes), more, incomplete))
    }
    /// An executable node ID binds Job, Graph and original 1-based coordinate
    /// and cannot accidentally attach to a sibling node or Job generation.
    pub fn node_key(&self, coordinate: usize) -> Result<[u8; 32], Error> {
        if !(1..=self.nodes.len()).contains(&coordinate) {
            return Err(Error::InvalidState);
        }
        let mut h = Sha256::new();
        h.update(b"aw-tools-core-graph-node-v1");
        h.update(self.binding);
        h.update((coordinate as u64).to_le_bytes());
        Ok(h.finalize().into())
    }
    fn states(&self) -> Vec<Option<Status>> {
        self.nodes.iter().map(|n| n.result()).collect()
    }
    fn ready(&self, coordinate: usize) -> Result<Option<bool>, Error> {
        if !(1..=self.nodes.len()).contains(&coordinate) {
            return Err(Error::InvalidState);
        }
        if self.nodes[coordinate - 1] != NodeState::Pending {
            return Err(Error::InvalidState);
        }
        Ok(self.graph.steps()[coordinate - 1]
            .when
            .evaluate(&self.states()))
    }
    /// Returns only runnable nodes whose dependencies are fully settled.
    /// The original host still must acquire a process-wide runner permit
    /// before calling claim; this is not permission to execute.
    pub fn runnable(&self) -> Vec<usize> {
        if self.state != Lifecycle::Running || self.ambiguous_journal {
            return Vec::new();
        }
        let free = self.width.saturating_sub(self.active());
        if free == 0 {
            return Vec::new();
        }
        let states = self.states();
        self.nodes
            .iter()
            .enumerate()
            .filter(|(_, state)| **state == NodeState::Pending)
            .filter(|(idx, _)| self.graph.steps()[*idx].when.evaluate(&states) == Some(true))
            .take(free)
            .map(|(idx, _)| idx + 1)
            .collect()
    }
    pub fn skippable(&self) -> Vec<usize> {
        if self.ambiguous_journal {
            return Vec::new();
        }
        if self.state != Lifecycle::Running {
            return self
                .nodes
                .iter()
                .enumerate()
                .filter(|(_, s)| **s == NodeState::Pending)
                .map(|(idx, _)| idx + 1)
                .collect();
        }
        let states = self.states();
        self.nodes
            .iter()
            .enumerate()
            .filter(|(_, state)| **state == NodeState::Pending)
            .filter(|(idx, _)| self.graph.steps()[*idx].when.evaluate(&states) == Some(false))
            .map(|(idx, _)| idx + 1)
            .collect()
    }
    /// Admission verification is pure and allocation-free. The original
    /// host's journal CAS is invoked only after this check succeeds.
    fn validate_record(&self, record: Transition) -> Result<(), Error> {
        if self.ambiguous_journal {
            return Err(Error::JournalUnknown);
        }
        if record.run_binding != self.binding || record.graph_hash != self.graph_hash {
            return Err(Error::InvalidIdentity);
        }
        if record.previous_sequence != self.sequence
            || record.next_sequence <= record.previous_sequence
        {
            return Err(Error::InvalidSequence);
        }
        let current = match record.node {
            Some(coordinate) => Some(self.state(coordinate).ok_or(Error::InvalidState)?),
            None => None,
        };
        if record.before != current {
            return Err(Error::StaleGeneration);
        }
        let legal = match (record.node, record.action) {
            (Some(coordinate), Action::Claim) => {
                self.state == Lifecycle::Running
                    && current == Some(NodeState::Pending)
                    && self.active() < self.width
                    && self.ready(coordinate)? == Some(true)
            }
            (Some(_), Action::Start) => {
                self.state == Lifecycle::Running
                    && matches!(current, Some(NodeState::Claimed { .. }))
            }
            (Some(_), Action::Settle(result)) => match current {
                Some(NodeState::Claimed { .. }) => result.primary == Primary::Unknown,
                Some(NodeState::Running { .. }) => result.primary != Primary::Skip,
                _ => false,
            },
            (Some(_), Action::UnlaunchedSkip) => matches!(current, Some(NodeState::Claimed { .. })),
            (Some(coordinate), Action::Skip) => {
                current == Some(NodeState::Pending)
                    && (self.state != Lifecycle::Running || self.ready(coordinate)? == Some(false))
            }
            (None, Action::Cancel) => self.state == Lifecycle::Running,
            (None, Action::Revoke) => {
                matches!(self.state, Lifecycle::Running | Lifecycle::Cancelling)
            }
            _ => false,
        };
        if !legal {
            return Err(Error::InvalidState);
        }
        Ok(())
    }
    /// Apply only an authenticated, already-durable host journal record. This
    /// never starts or replays filesystem or process effects.
    pub fn apply_committed(&mut self, record: Transition) -> Result<(), Error> {
        self.validate_record(record)?;
        match (record.node, record.action) {
            (Some(coordinate), Action::Claim) => {
                self.nodes[coordinate - 1] = NodeState::Claimed {
                    sequence: record.next_sequence,
                }
            }
            (Some(coordinate), Action::Start) => {
                self.nodes[coordinate - 1] = NodeState::Running {
                    sequence: record.next_sequence,
                }
            }
            (Some(coordinate), Action::Settle(result)) => {
                self.nodes[coordinate - 1] = NodeState::Terminal {
                    sequence: record.next_sequence,
                    status: result,
                }
            }
            (Some(coordinate), Action::UnlaunchedSkip) => {
                self.nodes[coordinate - 1] = NodeState::Terminal {
                    sequence: record.next_sequence,
                    status: Status {
                        primary: Primary::Skip,
                        more: false,
                        incomplete: false,
                    },
                }
            }
            (Some(coordinate), Action::Skip) => {
                self.nodes[coordinate - 1] = NodeState::Terminal {
                    sequence: record.next_sequence,
                    status: Status {
                        primary: Primary::Skip,
                        more: false,
                        incomplete: false,
                    },
                }
            }
            (None, Action::Cancel) => {
                self.state = Lifecycle::Cancelling;
                self.stop_reason = Some(StopReason::Cancelled);
            }
            (None, Action::Revoke) => {
                self.state = Lifecycle::Revoking;
                self.stop_reason = Some(StopReason::Revoked);
            }
            _ => unreachable!("validated transition"),
        }
        self.sequence = record.next_sequence;
        if self.finished() == self.nodes.len() {
            self.state = Lifecycle::Terminal;
        }
        Ok(())
    }
    /// Rebuild only a projection from trusted original-host journal entries.
    /// Claimed/Running nodes retain their in-flight state and require exact
    /// runner reconciliation; replay NEVER starts or repeats a node effect.
    pub fn from_runtime_journal(
        graph: Graph,
        runtime_session: &str,
        job_id: &str,
        epoch: u64,
        admission_sequence: u64,
        width: usize,
        records: impl IntoIterator<Item = Transition>,
    ) -> Result<Self, Error> {
        let mut projection = Self::after_journal(
            graph,
            runtime_session,
            job_id,
            epoch,
            admission_sequence,
            width,
        )?;
        for record in records {
            projection.apply_committed(record)?;
        }
        Ok(projection)
    }
    pub fn pending_reattach(&self) -> Vec<(usize, NodeState)> {
        self.nodes
            .iter()
            .copied()
            .enumerate()
            .filter(|(_, s)| s.in_flight())
            .map(|(i, s)| (i + 1, s))
            .collect()
    }
    /// Build and validate one exact transition without invoking host code.
    /// A real manager uses this under its lock, marks the transition in-flight,
    /// releases the lock, then asks the host journal to commit it.
    pub fn propose(
        &self,
        coordinate: Option<usize>,
        action: Action,
        next_sequence: u64,
    ) -> Result<Transition, Error> {
        if self.ambiguous_journal {
            return Err(Error::JournalUnknown);
        }
        if next_sequence <= self.sequence {
            return Err(Error::InvalidSequence);
        }
        let proposal = Transition {
            previous_sequence: self.sequence,
            next_sequence,
            run_binding: self.binding,
            graph_hash: self.graph_hash,
            node: coordinate,
            before: coordinate.and_then(|index| self.state(index)),
            action,
        };
        self.validate_record(proposal)?;
        Ok(proposal)
    }

    /// Persistence outcome is ambiguous: no further transition may be proposed
    /// until the original host reconstructs this Job from its authoritative log.
    pub fn fence_journal_unknown(&mut self) {
        self.ambiguous_journal = true;
    }

    fn persist(
        &mut self,
        coordinate: Option<usize>,
        action: Action,
        next_sequence: u64,
        persist: impl FnOnce(Transition) -> Result<(), JournalError>,
    ) -> Result<(), Error> {
        let proposal = self.propose(coordinate, action, next_sequence)?;
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| persist(proposal))) {
            Ok(Ok(())) => {
                // No other mutation of this exclusive &mut projection can
                // occur between the precheck and durable host CAS.
                if self.apply_committed(proposal).is_err() {
                    self.ambiguous_journal = true;
                    return Err(Error::JournalUnknown);
                }
                Ok(())
            }
            Ok(Err(JournalError::RejectedBeforeEffect)) => Err(Error::JournalRejected),
            Ok(Err(JournalError::OutcomeUnknown)) | Err(_) => {
                // The host may already have committed this transition.
                // No successor may start until the original journal owner
                // reconstructs an authoritative projection.
                self.ambiguous_journal = true;
                Err(Error::JournalUnknown)
            }
        }
    }
    /// Durable claim MUST precede all filesystem/shell effects. The host
    /// verifies exact source/site/effect grants before issuing a permit.
    pub fn claim(
        &mut self,
        coordinate: usize,
        source_live: bool,
        global_permit_acquired: bool,
        sequence: u64,
        persist: impl FnOnce(Transition) -> Result<(), JournalError>,
    ) -> Result<[u8; 32], Error> {
        if self.state != Lifecycle::Running {
            return Err(Error::NotPermitted);
        }
        if !source_live {
            return Err(Error::SourceRevoked);
        }
        if !global_permit_acquired {
            return Err(Error::NotPermitted);
        }
        if self.active() >= self.width {
            return Err(Error::ExceedsParallelism);
        }
        if self.ready(coordinate)? != Some(true) {
            return Err(Error::NotReady);
        }
        self.persist(Some(coordinate), Action::Claim, sequence, persist)?;
        self.node_key(coordinate)
    }
    /// The original host associates a claimed node with its exact runner.
    /// Unknown launch results do not turn into a new attempt.
    pub fn running(
        &mut self,
        coordinate: usize,
        claimed_sequence: u64,
        source_live: bool,
        launch_authorized: bool,
        sequence: u64,
        persist: impl FnOnce(Transition) -> Result<(), JournalError>,
    ) -> Result<(), Error> {
        if !source_live {
            return Err(Error::SourceRevoked);
        }
        if !launch_authorized {
            return Err(Error::NotPermitted);
        }
        if self.state != Lifecycle::Running {
            // A claim is not license to launch after revocation/cancellation.
            return Err(Error::NotPermitted);
        }
        if self.state(coordinate)
            != Some(NodeState::Claimed {
                sequence: claimed_sequence,
            })
        {
            return Err(Error::StaleGeneration);
        }
        self.persist(Some(coordinate), Action::Start, sequence, persist)
    }
    /// Terminal checkpoint is durably appended before any dependent can run.
    pub fn settle(
        &mut self,
        coordinate: usize,
        expected: NodeState,
        result: Status,
        sequence: u64,
        persist: impl FnOnce(Transition) -> Result<(), JournalError>,
    ) -> Result<(), Error> {
        if self.state(coordinate) != Some(expected) || !expected.in_flight() {
            return Err(Error::StaleGeneration);
        }
        match expected {
            NodeState::Claimed { .. }
                if !matches!(result.primary, Primary::Skip | Primary::Unknown) =>
            {
                // A mere claim isn't evidence that a child ran or completed.
                // Only no-launch proof (Skip) or an uncertain result is
                // admissible until a Running checkpoint has been persisted.
                return Err(Error::InvalidState);
            }
            NodeState::Running { .. } if result.primary == Primary::Skip => {
                return Err(Error::InvalidState);
            }
            _ => {}
        }
        self.persist(Some(coordinate), Action::Settle(result), sequence, persist)
    }
    /// A claimed node can be skipped only after the original Process
    /// host positively verifies no effect or runner was launched.
    pub fn settle_unlaunched(
        &mut self,
        coordinate: usize,
        claimed_sequence: u64,
        sequence: u64,
        prove_no_launch: impl FnOnce([u8; 32], u64) -> Result<(), Error>,
        persist: impl FnOnce(Transition) -> Result<(), JournalError>,
    ) -> Result<(), Error> {
        if self.state(coordinate)
            != Some(NodeState::Claimed {
                sequence: claimed_sequence,
            })
        {
            return Err(Error::StaleGeneration);
        }
        let key = self.node_key(coordinate)?;
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            prove_no_launch(key, claimed_sequence)
        }))
        .map_err(|_| Error::NotPermitted)??;
        self.persist(Some(coordinate), Action::UnlaunchedSkip, sequence, persist)
    }
    /// False branches, cancellation and revocation are journaled skips,
    /// never executable nodes.
    pub fn skip(
        &mut self,
        coordinate: usize,
        sequence: u64,
        persist: impl FnOnce(Transition) -> Result<(), JournalError>,
    ) -> Result<(), Error> {
        if self.ready(coordinate)?.is_none_or(|allowed| allowed) && self.state == Lifecycle::Running
        {
            return Err(Error::NotReady);
        }
        self.persist(Some(coordinate), Action::Skip, sequence, persist)
    }
    /// Host records cancellation/revocation intent before stopping new
    /// claims, signaling children and safely settling active native writes.
    pub fn stop(
        &mut self,
        revoke: bool,
        sequence: u64,
        persist: impl FnOnce(Transition) -> Result<(), JournalError>,
    ) -> Result<(), Error> {
        match (self.state, revoke) {
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
        self.persist(None, action, sequence, persist)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn graph() -> Graph {
        Graph::compile(
            &json!({"ops":[
                ["read",{"path":"a"}],
                ["read",{"path":"b"},"*"],
                ["write",{"path":"x","content":"y"},"1&2"],
                ["read",{"path":"fallback"},"3.fail|3.mixed"]
            ]}),
            |_, _| true,
        )
        .unwrap()
    }
    fn status(primary: Primary, more: bool) -> Status {
        Status {
            primary,
            more,
            incomplete: false,
        }
    }
    fn job() -> Job {
        Job::after_journal(graph(), "session", "job-1", 1, 5, 2).unwrap()
    }
    #[test]
    fn claim_then_terminal_checkpoint_then_dependent_start() {
        let mut j = job();
        assert_eq!(j.runnable(), [1, 2]);
        let first = j
            .claim(1, true, true, 6, |p| {
                assert_eq!(p.previous_sequence, 5);
                Ok(())
            })
            .unwrap();
        assert_ne!(first, j.node_key(2).unwrap());
        assert_eq!(j.runnable(), [2]);
        assert_eq!(
            j.claim(3, true, true, 7, |_| panic!("not ready")),
            Err(Error::NotReady)
        );
        j.running(1, 6, true, true, 7, |_| Ok(())).unwrap();
        j.settle(
            1,
            NodeState::Running { sequence: 7 },
            status(Primary::Ok, false),
            8,
            |_| Ok(()),
        )
        .unwrap();
        j.claim(2, true, true, 9, |_| Ok(())).unwrap();
        assert_eq!(
            j.settle(
                2,
                NodeState::Claimed { sequence: 9 },
                status(Primary::Unchanged, false),
                10,
                |_| Ok(())
            ),
            Err(Error::InvalidState),
            "known success requires a durable runner-start record"
        );
        j.running(2, 9, true, true, 10, |_| Ok(())).unwrap();
        j.settle(
            2,
            NodeState::Running { sequence: 10 },
            status(Primary::Unchanged, false),
            11,
            |_| Ok(()),
        )
        .unwrap();
        assert_eq!(j.runnable(), [3]);
        j.claim(3, true, true, 12, |_| Ok(())).unwrap();
        j.running(3, 12, true, true, 13, |_| Ok(())).unwrap();
        j.settle(
            3,
            NodeState::Running { sequence: 13 },
            status(Primary::Fail, false),
            14,
            |_| Ok(()),
        )
        .unwrap();
        assert_eq!(j.runnable(), [4]);
        j.claim(4, true, true, 15, |_| Ok(())).unwrap();
        j.running(4, 15, true, true, 16, |_| Ok(())).unwrap();
        j.settle(
            4,
            NodeState::Running { sequence: 16 },
            status(Primary::Ok, false),
            17,
            |_| Ok(()),
        )
        .unwrap();
        assert_eq!(j.lifecycle(), Lifecycle::Terminal);
        assert_eq!(j.summary(), Some((Primary::Mixed, false, false)));
    }
    #[test]
    fn paged_query_does_not_satisfy_bare_complete_dependency() {
        let mut j = job();
        j.claim(1, true, true, 6, |_| Ok(())).unwrap();
        j.running(1, 6, true, true, 7, |_| Ok(())).unwrap();
        j.settle(
            1,
            NodeState::Running { sequence: 7 },
            status(Primary::Ok, true),
            8,
            |_| Ok(()),
        )
        .unwrap();
        j.claim(2, true, true, 9, |_| Ok(())).unwrap();
        j.running(2, 9, true, true, 10, |_| Ok(())).unwrap();
        j.settle(
            2,
            NodeState::Running { sequence: 10 },
            status(Primary::Ok, false),
            11,
            |_| Ok(()),
        )
        .unwrap();
        assert!(j.runnable().is_empty());
        assert_eq!(j.skippable(), [3]);
        j.skip(3, 12, |_| Ok(())).unwrap();
        assert_eq!(j.skippable(), [4]);
        j.skip(4, 13, |_| Ok(())).unwrap();
        assert_eq!(j.summary(), Some((Primary::Ok, true, false)));
    }
    #[test]
    fn journal_unknown_cannot_replay_or_dispatch() {
        let mut j = job();
        assert_eq!(
            j.claim(1, true, true, 6, |_| Err(JournalError::OutcomeUnknown)),
            Err(Error::JournalUnknown)
        );
        assert!(j.runnable().is_empty());
        assert_eq!(j.state(1), Some(NodeState::Pending));
        assert_eq!(
            j.claim(1, true, true, 7, |_| Ok(())),
            Err(Error::JournalUnknown)
        );
        let mut j = job();
        assert_eq!(
            j.claim(1, true, true, 6, |_| Err(
                JournalError::RejectedBeforeEffect
            )),
            Err(Error::JournalRejected)
        );
        assert_eq!(j.runnable(), [1, 2]);
    }
    #[test]
    fn terminal_job_retains_cancel_reason_even_when_started_node_finishes_ok() {
        let graph = Graph::compile(&json!({"ops":[["read",{"path":"a"}]]}), |_, _| true).unwrap();
        let mut j = Job::after_journal(graph, "session", "job-1", 1, 5, 1).unwrap();
        j.claim(1, true, true, 6, |_| Ok(())).unwrap();
        j.running(1, 6, true, true, 7, |_| Ok(())).unwrap();
        j.stop(false, 8, |_| Ok(())).unwrap();
        assert_eq!(j.lifecycle(), Lifecycle::Cancelling);
        assert_eq!(j.stop_reason(), Some(StopReason::Cancelled));
        j.settle(
            1,
            NodeState::Running { sequence: 7 },
            Status {
                primary: Primary::Ok,
                more: false,
                incomplete: false,
            },
            9,
            |_| Ok(()),
        )
        .unwrap();
        assert_eq!(j.lifecycle(), Lifecycle::Terminal);
        assert_eq!(j.stop_reason(), Some(StopReason::Cancelled));
        assert_eq!(j.summary().unwrap().0, Primary::Ok);
    }

    #[test]
    fn cancellation_stops_new_nodes_but_not_safe_native_settlement() {
        let mut j = job();
        let id = j.node_key(1).unwrap();
        assert_ne!(
            id,
            Job::after_journal(graph(), "session", "job-2", 1, 5, 2)
                .unwrap()
                .node_key(1)
                .unwrap()
        );
        j.claim(1, true, true, 6, |_| Ok(())).unwrap();
        j.stop(false, 7, |_| Ok(())).unwrap();
        assert!(j.runnable().is_empty());
        assert_eq!(
            j.claim(2, true, true, 8, |_| Ok(())),
            Err(Error::NotPermitted)
        );
        j.skip(2, 8, |_| Ok(())).unwrap();
        j.settle(
            1,
            NodeState::Claimed { sequence: 6 },
            status(Primary::Unknown, false),
            9,
            |_| Ok(()),
        )
        .unwrap();
        j.skip(3, 10, |_| Ok(())).unwrap();
        j.skip(4, 11, |_| Ok(())).unwrap();
        assert_eq!(j.summary(), Some((Primary::Unknown, false, false)));
    }
    #[test]
    fn claimed_but_not_launched_does_not_run_after_revocation() {
        let mut j = job();
        j.claim(1, true, true, 6, |_| Ok(())).unwrap();
        j.stop(true, 7, |_| Ok(())).unwrap();
        assert_eq!(
            j.running(1, 6, true, true, 8, |_| panic!("must not start runner")),
            Err(Error::NotPermitted)
        );
        j.settle_unlaunched(
            1,
            6,
            8,
            |_, claim| {
                assert_eq!(claim, 6);
                Ok(())
            },
            |_| Ok(()),
        )
        .unwrap();
        j.skip(2, 9, |_| Ok(())).unwrap();
        j.skip(3, 10, |_| Ok(())).unwrap();
        j.skip(4, 11, |_| Ok(())).unwrap();
        assert_eq!(j.summary(), Some((Primary::Skip, false, false)));
        let mut running = job();
        running.claim(1, true, true, 6, |_| Ok(())).unwrap();
        running.running(1, 6, true, true, 7, |_| Ok(())).unwrap();
        assert_eq!(
            running.settle(
                1,
                NodeState::Running { sequence: 7 },
                status(Primary::Skip, false),
                8,
                |_| Ok(())
            ),
            Err(Error::InvalidState)
        );
    }
    #[test]
    fn cancelling_escalates_to_revocation_and_is_durably_recorded() {
        let mut j = job();
        j.stop(false, 6, |p| {
            assert_eq!(p.action, Action::Cancel);
            Ok(())
        })
        .unwrap();
        assert_eq!(j.lifecycle(), Lifecycle::Cancelling);
        j.stop(true, 7, |p| {
            assert_eq!(p.action, Action::Revoke);
            assert_eq!(p.previous_sequence, 6);
            Ok(())
        })
        .unwrap();
        assert_eq!(j.lifecycle(), Lifecycle::Revoking);
        assert_eq!(j.sequence(), 7);
        j.stop(true, 8, |_| panic!("already revoked; no journal duplicate"))
            .unwrap();
        assert_eq!(j.sequence(), 7);
    }
    #[test]
    fn runner_launch_rechecks_source_and_no_launch_skip_needs_proof() {
        let mut j = job();
        j.claim(1, true, true, 6, |_| Ok(())).unwrap();
        assert_eq!(
            j.running(1, 6, false, true, 7, |_| panic!(
                "must not launch revoked source"
            )),
            Err(Error::SourceRevoked)
        );
        assert_eq!(
            j.running(1, 6, true, false, 7, |_| panic!(
                "must not run without launch permit"
            )),
            Err(Error::NotPermitted)
        );
        assert_eq!(
            j.settle(
                1,
                NodeState::Claimed { sequence: 6 },
                status(Primary::Skip, false),
                7,
                |_| Ok(())
            ),
            Err(Error::InvalidState)
        );
        assert_eq!(
            j.settle_unlaunched(
                1,
                6,
                7,
                |_, _| Err(Error::NotPermitted),
                |_| panic!("proof denied")
            ),
            Err(Error::NotPermitted)
        );
        assert_eq!(j.state(1), Some(NodeState::Claimed { sequence: 6 }));
        assert_eq!(j.sequence(), 6);
        j.settle_unlaunched(1, 6, 7, |_, _| Ok(()), |_| Ok(()))
            .unwrap();
        assert_eq!(
            j.state(1),
            Some(NodeState::Terminal {
                sequence: 7,
                status: status(Primary::Skip, false)
            })
        );
    }
    #[test]
    fn no_effect_rejection_and_panic_are_distinguished() {
        let mut j = job();
        assert_eq!(
            j.claim(1, true, true, 6, |_| Err(
                JournalError::RejectedBeforeEffect
            )),
            Err(Error::JournalRejected)
        );
        assert_eq!(j.sequence(), 5);
        assert_eq!(
            j.claim(1, true, true, 6, |_| panic!("journal may have committed")),
            Err(Error::JournalUnknown)
        );
        assert_eq!(j.sequence(), 5);
        assert_eq!(
            j.claim(1, true, true, 7, |_| Ok(())),
            Err(Error::JournalUnknown)
        );
    }
    #[test]
    fn job_scope_identity_includes_actor_source_and_call() {
        use crate::{task_fence::TaskIdentity, ToolEffect};
        let original = TaskIdentity {
            actor: "owner".into(),
            source_kind: "host".into(),
            source_id: "host".into(),
            source_epoch: 7,
            execution_site: "local".into(),
            session: "session".into(),
            branch: "main".into(),
            turn_id: 42,
            call_id: "call".into(),
            effect: ToolEffect::Write,
        };
        let mut job = Job::after_task_intent(graph(), &original, "job-1", 5, 2).unwrap();
        let mut records = Vec::new();
        job.claim(1, true, true, 6, |record| {
            records.push(record);
            Ok(())
        })
        .unwrap();
        let rebuilt =
            Job::from_task_journal(graph(), &original, "job-1", 5, 2, records.clone()).unwrap();
        assert_eq!(rebuilt.binding(), job.binding());
        assert_eq!(rebuilt.state(1), Some(NodeState::Claimed { sequence: 6 }));
        let mut foreign = original.clone();
        foreign.source_epoch += 1;
        assert!(matches!(
            Job::from_task_journal(graph(), &foreign, "job-1", 5, 2, records.clone()),
            Err(Error::InvalidIdentity)
        ));
        foreign = original.clone();
        foreign.actor = "another-owner".into();
        assert!(matches!(
            Job::from_task_journal(graph(), &foreign, "job-1", 5, 2, records.clone()),
            Err(Error::InvalidIdentity)
        ));
        foreign = original.clone();
        foreign.call_id = "other-call".into();
        assert!(matches!(
            Job::from_task_journal(graph(), &foreign, "job-1", 5, 2, records),
            Err(Error::InvalidIdentity)
        ));
    }
    #[test]
    fn trusted_journal_reconstructs_without_effect_reexecution() {
        let mut primary = job();
        let mut records = Vec::new();
        primary
            .claim(1, true, true, 6, |record| {
                records.push(record);
                Ok(())
            })
            .unwrap();
        primary
            .running(1, 6, true, true, 7, |record| {
                records.push(record);
                Ok(())
            })
            .unwrap();
        primary
            .settle(
                1,
                NodeState::Running { sequence: 7 },
                status(Primary::Ok, false),
                8,
                |record| {
                    records.push(record);
                    Ok(())
                },
            )
            .unwrap();
        primary
            .claim(2, true, true, 9, |record| {
                records.push(record);
                Ok(())
            })
            .unwrap();
        // In flight at snapshot time, must be reattached or inspected by
        // original Process host; it is never automatically re-dispatched.
        let wire = serde_json::to_vec(&records).unwrap();
        let loaded: Vec<Transition> = serde_json::from_slice(&wire).unwrap();
        let recovered =
            Job::from_runtime_journal(graph(), "session", "job-1", 1, 5, 2, loaded).unwrap();
        assert_eq!(recovered.sequence(), 9);
        assert_eq!(recovered.state(1), primary.state(1));
        assert_eq!(
            recovered.pending_reattach(),
            vec![(2, NodeState::Claimed { sequence: 9 })]
        );
        assert_eq!(recovered.node_key(2), primary.node_key(2));
        assert!(
            recovered.runnable().is_empty(),
            "claimed node cannot be launched again by resume"
        );
    }
    #[test]
    fn tampered_or_reordered_records_fail_closed_before_resumption() {
        let mut source = job();
        let mut records = Vec::new();
        source
            .claim(1, true, true, 6, |record| {
                records.push(record);
                Ok(())
            })
            .unwrap();
        source
            .running(1, 6, true, true, 7, |record| {
                records.push(record);
                Ok(())
            })
            .unwrap();
        let from = |items: Vec<Transition>| {
            Job::from_runtime_journal(graph(), "session", "job-1", 1, 5, 2, items)
        };
        assert_eq!(from(records.clone()).unwrap().sequence(), 7);
        let mut wrong = records.clone();
        wrong[1].run_binding = [0; 32];
        assert!(matches!(from(wrong), Err(Error::InvalidIdentity)));
        let mut wrong = records.clone();
        wrong[0].node = Some(99);
        assert!(matches!(from(wrong), Err(Error::InvalidState)));
        let mut wrong = records.clone();
        wrong[1].before = Some(NodeState::Pending);
        assert!(matches!(from(wrong), Err(Error::StaleGeneration)));
        let mut wrong = records;
        wrong.swap(0, 1);
        assert!(matches!(from(wrong), Err(Error::InvalidSequence)));
    }
    #[test]
    fn stale_runner_generation_cannot_publish_terminal() {
        let mut j = job();
        j.claim(1, true, true, 6, |_| Ok(())).unwrap();
        assert_eq!(
            j.running(1, 5, true, true, 7, |_| Ok(())),
            Err(Error::StaleGeneration)
        );
        assert_eq!(
            j.settle(
                1,
                NodeState::Claimed { sequence: 5 },
                status(Primary::Ok, false),
                7,
                |_| Ok(())
            ),
            Err(Error::StaleGeneration)
        );
        assert_eq!(j.state(1), Some(NodeState::Claimed { sequence: 6 }));
    }
}
