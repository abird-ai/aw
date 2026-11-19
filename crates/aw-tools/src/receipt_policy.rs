//! Model/result receipt selection policy.
//!
//! This is pure, copyable configuration: no catalog, filesystem backend,
//! transport, clock or execution state.

use crate::{filesystem::schema::Tool, operation::OperationId};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReceiptCategory {
    Filesystem,
    Process,
    Batch,
    Jobs,
    Agent,
}

impl ReceiptCategory {
    const fn bit(self) -> u8 {
        1 << match self {
            Self::Filesystem => 0,
            Self::Process => 1,
            Self::Batch => 2,
            Self::Jobs => 3,
            Self::Agent => 4,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReceiptScope {
    categories: u8,
    include: u64,
    exclude: u64,
}

impl ReceiptScope {
    const ALL_CATEGORIES: u8 = (1 << 5) - 1;
    const FILESYSTEM_OPERATIONS: u64 = (1 << 13) - 1;
    const PROCESS_OPERATIONS: u64 = (1 << 13) | (1 << 14) | (1 << 15);
    const BATCH_OPERATIONS: u64 = 1 << 16;
    const AGENT_OPERATIONS: u64 = 1 << 17;
    const JOB_OPERATIONS: u64 = ((1 << 7) - 1) << 18;

    pub const fn none() -> Self {
        Self {
            categories: 0,
            include: 0,
            exclude: 0,
        }
    }

    pub const fn all() -> Self {
        Self {
            categories: Self::ALL_CATEGORIES,
            include: 0,
            exclude: 0,
        }
    }

    pub const fn with_category(mut self, category: ReceiptCategory) -> Self {
        self.categories |= category.bit();
        self
    }

    pub const fn include(mut self, operation: OperationId) -> Self {
        self.include |= operation_bit(operation);
        self
    }

    pub const fn exclude(mut self, operation: OperationId) -> Self {
        self.exclude |= operation_bit(operation);
        self
    }

    pub const fn contains(self, operation: OperationId) -> bool {
        let bit = operation_bit(operation);
        self.exclude & bit == 0
            && (self.include & bit != 0
                || self.categories & operation_category(operation).bit() != 0)
    }

    pub const fn is_empty(self) -> bool {
        self.effective_operations() == 0
    }

    const fn effective_operations(self) -> u64 {
        let mut selected = self.include;
        if self.categories & ReceiptCategory::Filesystem.bit() != 0 {
            selected |= Self::FILESYSTEM_OPERATIONS;
        }
        if self.categories & ReceiptCategory::Process.bit() != 0 {
            selected |= Self::PROCESS_OPERATIONS;
        }
        if self.categories & ReceiptCategory::Batch.bit() != 0 {
            selected |= Self::BATCH_OPERATIONS;
        }
        if self.categories & ReceiptCategory::Jobs.bit() != 0 {
            selected |= Self::JOB_OPERATIONS;
        }
        if self.categories & ReceiptCategory::Agent.bit() != 0 {
            selected |= Self::AGENT_OPERATIONS;
        }
        selected & !self.exclude
    }
}

impl Default for ReceiptScope {
    fn default() -> Self {
        Self::all()
    }
}

/// Optional model/result receipt metadata policy.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ReceiptProfile {
    pub unix_ts: bool,
    /// Host-selected advisory turn checkpoint; never a process/Job deadline.
    pub checkpoint_minutes: Option<u16>,
    pub scope: ReceiptScope,
}

impl ReceiptProfile {
    /// Whether this operation carries a host-supplied Unix timestamp receipt.
    pub const fn timestamps(self, operation: OperationId) -> bool {
        self.unix_ts && self.scope.contains(operation)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReceiptMetadata {
    pub ts: u64,
}

const fn operation_category(operation: OperationId) -> ReceiptCategory {
    match operation {
        OperationId::Fs(_) => ReceiptCategory::Filesystem,
        OperationId::Bash | OperationId::Pwsh | OperationId::Powershell => ReceiptCategory::Process,
        OperationId::Batch => ReceiptCategory::Batch,
        OperationId::Agent => ReceiptCategory::Agent,
        OperationId::JobsList
        | OperationId::JobsGet
        | OperationId::JobsRead
        | OperationId::JobsAck
        | OperationId::JobsSend
        | OperationId::JobsCancel
        | OperationId::JobsSubscribe => ReceiptCategory::Jobs,
    }
}

const fn operation_bit(operation: OperationId) -> u64 {
    let index = match operation {
        OperationId::Fs(Tool::Ls) => 0,
        OperationId::Fs(Tool::Find) => 1,
        OperationId::Fs(Tool::Grep) => 2,
        OperationId::Fs(Tool::Read) => 3,
        OperationId::Fs(Tool::Write) => 4,
        OperationId::Fs(Tool::Edit) => 5,
        OperationId::Fs(Tool::Patch) => 6,
        OperationId::Fs(Tool::Copy) => 7,
        OperationId::Fs(Tool::Move) => 8,
        OperationId::Fs(Tool::Remove) => 9,
        OperationId::Fs(Tool::Bread) => 10,
        OperationId::Fs(Tool::Bwrite) => 11,
        OperationId::Fs(Tool::Bedit) => 12,
        OperationId::Bash => 13,
        OperationId::Pwsh => 14,
        OperationId::Powershell => 15,
        OperationId::Batch => 16,
        OperationId::Agent => 17,
        OperationId::JobsList => 18,
        OperationId::JobsGet => 19,
        OperationId::JobsRead => 20,
        OperationId::JobsAck => 21,
        OperationId::JobsSend => 22,
        OperationId::JobsCancel => 23,
        OperationId::JobsSubscribe => 24,
    };
    1u64 << index
}

const _: () = assert!(OperationId::MODEL_SURFACE_MAX <= 64);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scope_is_category_first_then_include_then_exclude() {
        let scope = ReceiptScope::none()
            .with_category(ReceiptCategory::Jobs)
            .include(OperationId::Fs(Tool::Read))
            .exclude(OperationId::JobsRead);

        assert!(scope.contains(OperationId::JobsGet));
        assert!(scope.contains(OperationId::Fs(Tool::Read)));
        assert!(!scope.contains(OperationId::JobsRead));
        assert!(!scope.contains(OperationId::Bash));
    }

    #[test]
    fn excluding_every_operation_makes_scope_empty() {
        let no_jobs = ReceiptScope::none()
            .with_category(ReceiptCategory::Jobs)
            .exclude(OperationId::JobsList)
            .exclude(OperationId::JobsGet)
            .exclude(OperationId::JobsRead)
            .exclude(OperationId::JobsAck)
            .exclude(OperationId::JobsSend)
            .exclude(OperationId::JobsCancel)
            .exclude(OperationId::JobsSubscribe);
        assert!(no_jobs.is_empty());
    }
}
