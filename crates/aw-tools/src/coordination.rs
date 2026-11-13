//! Small host-neutral coordination values shared by Graph/Job owners. This
//! module owns no policy, membership database, approval service or scheduler.

use crate::{
    execution_ref::{NodeCoordinate, ResolvedExecutionRef, Revision},
    filesystem::status::Primary,
};

#[derive(Clone, Copy, Debug)]
pub struct ActivationReceipt<'a> {
    pub coordinate: NodeCoordinate,
    pub outcome: Primary,
    /// Authorized public projection only; provider-private state never belongs here.
    pub public_result: Option<&'a str>,
}

/// Borrowed deterministic input for activation-time routing/guardian policy.
/// The Graph owner supplies only the target node's declared dependency closure.
#[derive(Clone, Copy, Debug)]
pub struct GraphActivationContext<'a> {
    pub graph_fingerprint: &'a [u8; 32],
    pub coordinate: NodeCoordinate,
    dependency_receipts: &'a [ActivationReceipt<'a>],
}

impl<'a> GraphActivationContext<'a> {
    pub fn new(
        graph_fingerprint: &'a [u8; 32],
        coordinate: NodeCoordinate,
        dependency_receipts: &'a [ActivationReceipt<'a>],
    ) -> Result<Self, CoordinationError> {
        let mut previous = None;
        for receipt in dependency_receipts {
            if receipt.coordinate.get() >= coordinate.get()
                || previous.is_some_and(|p| receipt.coordinate.get() <= p)
            {
                return Err(CoordinationError::InvalidActivationContext);
            }
            previous = Some(receipt.coordinate.get());
        }
        Ok(Self {
            graph_fingerprint,
            coordinate,
            dependency_receipts,
        })
    }

    pub fn dependency_receipts(&self) -> &'a [ActivationReceipt<'a>] {
        self.dependency_receipts
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ApprovalId(pub [u8; 32]);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ApprovalTarget {
    Direct,
    Node(NodeCoordinate),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ApprovalState {
    Pending,
    Approved,
    Denied,
    Expired,
}

impl ApprovalState {
    pub const fn terminal(self) -> bool {
        !matches!(self, Self::Pending)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PendingApproval {
    pub approval_id: ApprovalId,
    pub target: ApprovalTarget,
    pub policy_decision_revision: Revision,
    /// Opaque host-authenticated scope fingerprint; never a bearer grant.
    pub approval_scope: [u8; 32],
    /// Opaque authorized approver-set fingerprint; membership stays host-owned.
    pub authorized_approvers: [u8; 32],
    pub expiry_unix_ms: u64,
    pub state: ApprovalState,
}

impl PendingApproval {
    pub fn resolve(&mut self, state: ApprovalState) -> Result<ApprovalState, CoordinationError> {
        if state == ApprovalState::Pending {
            return Err(CoordinationError::InvalidApprovalTransition);
        }
        match self.state {
            ApprovalState::Pending => {
                self.state = state;
                Ok(state)
            }
            existing if existing == state => Ok(existing),
            _ => Err(CoordinationError::InvalidApprovalTransition),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PendingApprovalSet {
    pub graph_ref: ResolvedExecutionRef,
    pub admission_identity: [u8; 32],
    pub guardian_decision_revision: Revision,
    entries: Vec<PendingApproval>,
}

impl PendingApprovalSet {
    pub fn new(
        graph_ref: ResolvedExecutionRef,
        admission_identity: [u8; 32],
        guardian_decision_revision: Revision,
        mut entries: Vec<PendingApproval>,
        max_entries: usize,
    ) -> Result<Self, CoordinationError> {
        if entries.is_empty() || entries.len() > max_entries || max_entries == 0 {
            return Err(CoordinationError::ApprovalSetTooLarge);
        }
        entries.sort_by_key(|entry| match entry.target {
            ApprovalTarget::Direct => 0,
            ApprovalTarget::Node(node) => node.get(),
        });
        for pair in entries.windows(2) {
            if pair[0].target == pair[1].target || pair[0].approval_id == pair[1].approval_id {
                return Err(CoordinationError::DuplicateApproval);
            }
        }
        if entries
            .iter()
            .any(|entry| matches!(entry.target, ApprovalTarget::Direct))
            && entries.len() != 1
        {
            return Err(CoordinationError::DuplicateApproval);
        }
        Ok(Self {
            graph_ref,
            admission_identity,
            guardian_decision_revision,
            entries,
        })
    }

    pub fn entries(&self) -> &[PendingApproval] {
        &self.entries
    }

    pub fn entry_mut(&mut self, id: ApprovalId) -> Option<&mut PendingApproval> {
        self.entries
            .iter_mut()
            .find(|entry| entry.approval_id == id)
    }

    pub fn ready(&self) -> bool {
        self.entries
            .iter()
            .all(|entry| entry.state == ApprovalState::Approved)
    }

    pub fn failed(&self) -> bool {
        self.entries
            .iter()
            .any(|entry| matches!(entry.state, ApprovalState::Denied | ApprovalState::Expired))
    }
}

/// Borrowed canonical refs used to reject transparent self/ancestor controls.
/// The authoritative owner supplies current wait/ownership ancestry; path
/// ancestry by itself is deliberately insufficient.
#[derive(Clone, Copy, Debug)]
pub struct ControlAncestry<'a> {
    pub self_ref: &'a ResolvedExecutionRef,
    pub ownership_ancestors: &'a [ResolvedExecutionRef],
    pub wait_dependencies: &'a [ResolvedExecutionRef],
}

impl ControlAncestry<'_> {
    pub fn rejects_target(&self, target: &ResolvedExecutionRef) -> bool {
        target == self.self_ref
            || self.ownership_ancestors.iter().any(|item| item == target)
            || self.wait_dependencies.iter().any(|item| item == target)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CoordinationError {
    InvalidActivationContext,
    ApprovalSetTooLarge,
    DuplicateApproval,
    InvalidApprovalTransition,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::execution_ref::{AgentRef, Handle};

    fn node(value: u32) -> NodeCoordinate {
        NodeCoordinate::new(value).unwrap()
    }

    fn graph_ref() -> ResolvedExecutionRef {
        let agent = AgentRef::new(
            Handle::new("d").unwrap(),
            Handle::new("i").unwrap(),
            Handle::new("p").unwrap(),
            Handle::new("a").unwrap(),
        );
        crate::execution_ref::ExecutionRef::parse("t:1/j:g")
            .unwrap()
            .resolve(Some(&agent), None)
            .unwrap()
    }

    fn approval(value: u8, coordinate: u32) -> PendingApproval {
        PendingApproval {
            approval_id: ApprovalId([value; 32]),
            target: ApprovalTarget::Node(node(coordinate)),
            policy_decision_revision: Revision::new(1).unwrap(),
            approval_scope: [2; 32],
            authorized_approvers: [3; 32],
            expiry_unix_ms: 100,
            state: ApprovalState::Pending,
        }
    }

    #[test]
    fn activation_context_is_borrowed_sorted_dependency_only_input() {
        let fingerprint = [9; 32];
        let receipts = [
            ActivationReceipt {
                coordinate: node(1),
                outcome: Primary::Ok,
                public_result: Some("one"),
            },
            ActivationReceipt {
                coordinate: node(3),
                outcome: Primary::Unchanged,
                public_result: Some("three"),
            },
        ];
        let context = GraphActivationContext::new(&fingerprint, node(4), &receipts).unwrap();
        assert_eq!(context.dependency_receipts().len(), 2);

        let out_of_order = [receipts[1], receipts[0]];
        assert!(GraphActivationContext::new(&fingerprint, node(4), &out_of_order).is_err());
        assert!(GraphActivationContext::new(&fingerprint, node(3), &receipts).is_err());
    }

    #[test]
    fn pending_set_is_canonical_bounded_and_idempotent() {
        let mut set = PendingApprovalSet::new(
            graph_ref(),
            [7; 32],
            Revision::new(2).unwrap(),
            vec![approval(2, 4), approval(1, 2)],
            64,
        )
        .unwrap();
        assert_eq!(
            set.entries()
                .iter()
                .map(|entry| match entry.target {
                    ApprovalTarget::Node(node) => node.get(),
                    ApprovalTarget::Direct => 0,
                })
                .collect::<Vec<_>>(),
            vec![2, 4]
        );
        assert!(!set.ready());
        assert_eq!(
            set.entry_mut(ApprovalId([1; 32]))
                .unwrap()
                .resolve(ApprovalState::Approved)
                .unwrap(),
            ApprovalState::Approved
        );
        assert_eq!(
            set.entry_mut(ApprovalId([1; 32]))
                .unwrap()
                .resolve(ApprovalState::Approved)
                .unwrap(),
            ApprovalState::Approved
        );
        assert!(set
            .entry_mut(ApprovalId([1; 32]))
            .unwrap()
            .resolve(ApprovalState::Denied)
            .is_err());
    }

    #[test]
    fn ancestry_uses_owner_edges_not_path_prefixes() {
        let self_ref = graph_ref();
        let ancestor = crate::execution_ref::ExecutionRef::parse("a:parent")
            .unwrap()
            .resolve(None, None)
            .unwrap();
        let unrelated = crate::execution_ref::ExecutionRef::parse("a:other")
            .unwrap()
            .resolve(None, None)
            .unwrap();
        let ancestry = ControlAncestry {
            self_ref: &self_ref,
            ownership_ancestors: std::slice::from_ref(&ancestor),
            wait_dependencies: &[],
        };
        assert!(ancestry.rejects_target(&self_ref));
        assert!(ancestry.rejects_target(&ancestor));
        assert!(!ancestry.rejects_target(&unrelated));
    }
}
