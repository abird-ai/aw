//! Shared schema-pin compatibility check for the existing durable owners.
//! Diagnostics are not driver authority or proof of external materialization.
use crate::{db, Error, Result};
use aw_agent_core::{DriverSchemaPin, SourceDriverRegistry, SourceDriverRegistryReader};
use rusqlite::{Connection, TransactionBehavior};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceCompatibility {
    Compatible,
    RecoveryRequired,
}

impl SourceCompatibility {
    pub(crate) fn parse(value: &str) -> Result<Self> {
        match value {
            "compatible" => Ok(Self::Compatible),
            "recovery_required" => Ok(Self::RecoveryRequired),
            _ => Err(Error::Storage("invalid source compatibility")),
        }
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Compatible => "compatible",
            Self::RecoveryRequired => "recovery_required",
        }
    }
}

pub(crate) fn compatibility(
    registry: &SourceDriverRegistryReader,
    pin: &DriverSchemaPin,
) -> Result<SourceCompatibility> {
    match registry.resolve(pin) {
        Ok(_) => Ok(SourceCompatibility::Compatible),
        Err(Error::Denied(_)) => Ok(SourceCompatibility::RecoveryRequired),
        Err(error) => Err(error),
    }
}

pub(crate) fn revalidate(
    connection: &mut Connection,
    registry: &SourceDriverRegistry,
    exclusive_owner_recovery: bool,
) -> Result<()> {
    let tx = db(connection.transaction_with_behavior(TransactionBehavior::Immediate))?;
    let registry = registry.reader()?;
    crate::sandbox_owner::source_bindings::revalidate_in(&tx, &registry, exclusive_owner_recovery)?;
    crate::checkpoint_owner::revalidate_in(&tx, &registry)?;
    // Compatibility is derived on demand from retained descriptors; never
    // rewrite an immutable receipt or treat compatibility as live authority.
    crate::construction_receipt::visit_unresolved_sources(&tx, &registry, |_, _, _| Ok(()))?;
    db(tx.commit())
}
