//! Durable checkpoint owner. The final transaction owns the immutable
//! HistoryBase and complete SourceSet; no materialization is performed here.

pub(crate) mod capture;
pub(crate) mod restore;
mod retirement;
use crate::{
    agent_owner::{self, AgentKey, AgentPermission},
    db, history_owner, id, label, now,
    source_compatibility::{compatibility, SourceCompatibility},
    user_for_principal, Error, Result, SessionStore,
};
use aw_agent_core::{
    validate_source_set_hard_limit, CheckpointPolicy, DriverKey, PrincipalId, SourceAccess,
    SourceAnchor, SourceCoverage, SourceDriverRegistryReader, SourceName, SourceSet,
    SourceSetLimit, HARD_MAX_SOURCES_PER_SANDBOX, MAX_SOURCE_ANCHOR_FIELDS_BYTES,
};
pub use capture::{
    CheckpointCaptureRequest, CheckpointCaptureSelection, InspectedCheckpointCapture,
    PreparedCheckpointCapture,
};
pub use restore::{
    CheckpointRestoreLayout, CheckpointRestoreRequest, CheckpointRestoreResult,
    InspectedCheckpointRestore, PreparedCheckpointRestore,
};
pub use retirement::CheckpointDeleteOutcome;
pub(crate) use retirement::RETENTION_CLEANUP_PREFIX;
use rusqlite::{params, Connection, OptionalExtension, Row, Transaction, TransactionBehavior};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckpointLifecycle {
    Live,
    Retired,
}

impl CheckpointLifecycle {
    fn parse(value: &str) -> Result<Self> {
        match value {
            "live" => Ok(Self::Live),
            "retired" => Ok(Self::Retired),
            _ => Err(Error::Storage("invalid checkpoint lifecycle")),
        }
    }
}

/// Source metadata only. Driver-owned source identities, retained versions
/// and locators require independent current source disclosure authority.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckpointSourceSummary {
    pub name: SourceName,
    pub driver: DriverKey,
    pub anchor_schema: u32,
    pub access: SourceAccess,
    pub checkpoint_policy: CheckpointPolicy,
    pub compatibility: SourceCompatibility,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckpointSnapshot {
    pub id: String,
    pub source_agent_ref: String,
    pub history_base_id: String,
    pub history_head: String,
    pub source_coverage: SourceCoverage,
    pub lifecycle: CheckpointLifecycle,
    pub note: Option<String>,
    pub created_by: String,
    pub created_at: i64,
    pub sources: Vec<CheckpointSourceSummary>,
}

fn coverage_name(coverage: SourceCoverage) -> &'static str {
    match coverage {
        SourceCoverage::AutoAllRequired => "auto_all_required",
        SourceCoverage::Selected => "selected",
        SourceCoverage::None => "none",
    }
}

fn parse_coverage(value: &str) -> Result<SourceCoverage> {
    match value {
        "auto_all_required" => Ok(SourceCoverage::AutoAllRequired),
        "selected" => Ok(SourceCoverage::Selected),
        "none" => Ok(SourceCoverage::None),
        _ => Err(Error::Storage("invalid checkpoint source coverage")),
    }
}

fn access_name(access: SourceAccess) -> &'static str {
    match access {
        SourceAccess::ReadOnly => "ro",
        SourceAccess::ReadWrite => "rw",
    }
}

fn policy_name(policy: CheckpointPolicy) -> &'static str {
    match policy {
        CheckpointPolicy::Required => "required",
        CheckpointPolicy::Ephemeral => "ephemeral",
    }
}

struct StoredSource {
    anchor: SourceAnchor,
    compatibility: SourceCompatibility,
}

impl StoredSource {
    fn decode(row: &Row<'_>) -> Result<Self> {
        let fields = db(row.get_ref(8))?;
        let fields = fields
            .as_str()
            .map_err(|_| Error::Storage("invalid checkpoint anchor fields"))?;
        if fields.len() > MAX_SOURCE_ANCHOR_FIELDS_BYTES {
            return Err(Error::Limit(
                "checkpoint anchor fields exceed durable bound",
            ));
        }
        let anchor = SourceAnchor {
            name: SourceName::new(db(row.get::<_, String>(0))?)?,
            driver: crate::decode(&db(row.get::<_, String>(1))?)?,
            anchor_schema: db(row.get(2))?,
            source_ref: db(row.get(3))?,
            access: match db(row.get::<_, String>(4))?.as_str() {
                "ro" => SourceAccess::ReadOnly,
                "rw" => SourceAccess::ReadWrite,
                _ => return Err(Error::Storage("invalid checkpoint source access")),
            },
            checkpoint_policy: match db(row.get::<_, String>(5))?.as_str() {
                "required" => CheckpointPolicy::Required,
                "ephemeral" => CheckpointPolicy::Ephemeral,
                _ => return Err(Error::Storage("invalid captured checkpoint policy")),
            },
            identity: db(row.get(6))?,
            retention: db(row.get(7))?,
            fields: crate::decode(fields)?,
        };
        Ok(Self {
            anchor,
            compatibility: SourceCompatibility::parse(&db(row.get::<_, String>(9))?)?,
        })
    }

    fn summary(&self) -> CheckpointSourceSummary {
        CheckpointSourceSummary {
            name: self.anchor.name.clone(),
            driver: self.anchor.driver.clone(),
            anchor_schema: self.anchor.anchor_schema,
            access: self.anchor.access,
            checkpoint_policy: self.anchor.checkpoint_policy,
            compatibility: self.compatibility,
        }
    }
}

fn sources_in(
    connection: &Connection,
    checkpoint: &str,
    coverage: SourceCoverage,
) -> Result<(SourceSet, Vec<SourceCompatibility>)> {
    let mut query = db(connection.prepare(
        "SELECT source_name,driver_key,anchor_schema,source_ref,
                captured_access,captured_checkpoint_policy,anchor_identity,retention,
                anchor_fields_json,compatibility
         FROM checkpoint_sources WHERE checkpoint_id=?1
         ORDER BY source_name LIMIT ?2",
    ))?;
    let mut rows = db(query.query(params![checkpoint, HARD_MAX_SOURCES_PER_SANDBOX + 1]))?;
    let mut anchors = Vec::new();
    let mut compatibilities = Vec::new();
    while let Some(row) = db(rows.next())? {
        validate_source_set_hard_limit(anchors.len() + 1)?;
        let stored = StoredSource::decode(row)?;
        anchors.push(stored.anchor);
        compatibilities.push(stored.compatibility);
    }
    let set = SourceSet { coverage, anchors };
    set.validate()?;
    Ok((set, compatibilities))
}

struct CheckpointRow {
    snapshot: CheckpointSnapshot,
    session: String,
    branch: String,
}

fn row_in(connection: &Connection, checkpoint: &str) -> Result<CheckpointRow> {
    let record: Option<(
        String,
        String,
        String,
        String,
        String,
        String,
        String,
        String,
        Option<String>,
        String,
        i64,
    )> = db(connection
        .query_row(
            "SELECT checkpoint_id,source_agent_ref,source_session_id,source_branch_id,
                history_base_id,history_head,source_coverage,lifecycle,note,created_by,created_at
         FROM checkpoints WHERE checkpoint_id=?1",
            [checkpoint],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                    row.get(7)?,
                    row.get(8)?,
                    row.get(9)?,
                    row.get(10)?,
                ))
            },
        )
        .optional())?;
    let (
        id,
        source_agent_ref,
        session,
        branch,
        history_base_id,
        history_head,
        coverage,
        lifecycle,
        note,
        created_by,
        created_at,
    ) = record.ok_or(Error::Denied("checkpoint unavailable or not authorized"))?;
    Ok(CheckpointRow {
        snapshot: CheckpointSnapshot {
            id,
            source_agent_ref,
            history_base_id,
            history_head,
            source_coverage: parse_coverage(&coverage)?,
            lifecycle: CheckpointLifecycle::parse(&lifecycle)?,
            note,
            created_by,
            created_at,
            sources: Vec::new(),
        },
        session,
        branch,
    })
}

fn history_head_in(connection: &Connection, base: &str) -> Result<String> {
    db(connection.query_row(
        "SELECT s.exact_head_node FROM history_bases b
         JOIN history_spans s ON s.span_id=b.local_span_id WHERE b.base_id=?1",
        [base],
        |row| row.get(0),
    ))
}

fn snapshot_in(
    connection: &Connection,
    actor: &PrincipalId,
    checkpoint: &str,
) -> Result<CheckpointSnapshot> {
    Ok(snapshot_projected_in(connection, actor, checkpoint, None)?.snapshot)
}

fn snapshot_projected_in(
    connection: &Connection,
    actor: &PrincipalId,
    checkpoint: &str,
    approval: Option<&crate::SourceMetadataApproval>,
) -> Result<crate::SourceMetadataView<CheckpointSnapshot>> {
    let mut row = row_in(connection, checkpoint)?;
    let mut disclosed = Vec::new();
    let key = AgentKey::new(&row.session, &row.branch);
    agent_owner::authorize_permission(connection, actor, key, AgentPermission::View)?;
    if agent_owner::canonical_ref(connection, key)? != row.snapshot.source_agent_ref {
        return Err(Error::Storage("checkpoint source Agent identity changed"));
    }
    // A retired checkpoint is a replay tombstone. Retention metadata may have
    // been cleaned; never require a released history/source object to read it.
    if row.snapshot.lifecycle == CheckpointLifecycle::Live {
        if history_head_in(connection, &row.snapshot.history_base_id)? != row.snapshot.history_head
        {
            return Err(Error::Storage(
                "checkpoint history head disagrees with its base",
            ));
        }
        let (sources, compatibility) =
            sources_in(connection, checkpoint, row.snapshot.source_coverage)?;
        row.snapshot.sources = sources
            .anchors
            .into_iter()
            .zip(compatibility)
            .map(|(anchor, compatibility)| {
                if let Some(source) =
                    approval.and_then(|approval| approval.anchor(&row.snapshot, &anchor))
                {
                    disclosed.push(source);
                }
                StoredSource {
                    anchor,
                    compatibility,
                }
                .summary()
            })
            .collect();
    }
    Ok(crate::SourceMetadataView {
        snapshot: row.snapshot,
        disclosed,
    })
}

/// All fields belong to the one publication transaction. Source-bearing
/// callers use the shared Prepared publication boundary and hold the live
/// mutation/cleanup fences.
struct Publication<'a> {
    actor: &'a PrincipalId,
    checkpoint: &'a str,
    source: AgentKey<'a>,
    history_base_id: &'a str,
    sources: &'a SourceSet,
    note: Option<&'a str>,
    creator: &'a str,
    request_id: &'a str,
    request_fingerprint: &'a str,
}

fn publish_in(
    tx: &Transaction<'_>,
    registry: &SourceDriverRegistryReader,
    limit: SourceSetLimit,
    publication: Publication<'_>,
) -> Result<()> {
    publication.sources.admit(limit)?;
    // Complete admission precedes any publication. This only resolves
    // schemas, not actual provider authority or retention.
    for anchor in &publication.sources.anchors {
        registry.resolve(&anchor.pin()?)?;
    }
    let reference = agent_owner::canonical_ref(tx, publication.source)?;
    let history_head = history_head_in(tx, publication.history_base_id)?;
    db(tx.execute(
        "INSERT INTO checkpoints(checkpoint_id,source_agent_ref,source_session_id,source_branch_id,
             history_base_id,history_head,source_coverage,lifecycle,note,created_by,created_at,
             request_id,request_fingerprint)
         VALUES(?1,?2,?3,?4,?5,?6,?7,'live',?8,?9,?10,?11,?12)",
        params![
            publication.checkpoint,
            reference,
            publication.source.session,
            publication.source.branch,
            publication.history_base_id,
            history_head,
            coverage_name(publication.sources.coverage),
            publication.note,
            publication.creator,
            now(),
            publication.request_id,
            publication.request_fingerprint
        ],
    ))?;
    // Local owner provenance only. Public Agent changed Events and their
    // disclosure are separate from the checkpoint collection membership.
    crate::durability::commit_event(
        tx,
        &crate::durability::tx_id()?,
        "checkpoint.created",
        "checkpoint",
        publication.checkpoint,
        Some(publication.actor),
        serde_json::json!({"sync_scope":"local","checkpoint_id":publication.checkpoint}),
    )?;
    if publication.sources.anchors.is_empty() {
        return Ok(());
    }
    let mut insert = db(tx.prepare(
        "INSERT INTO checkpoint_sources(checkpoint_id,source_name,driver_key,anchor_schema,
             source_ref,captured_access,captured_checkpoint_policy,anchor_identity,retention,
             anchor_fields_json,compatibility)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,'compatible')",
    ))?;
    for anchor in &publication.sources.anchors {
        let fields = crate::json(&anchor.fields)?;
        if fields.len() > MAX_SOURCE_ANCHOR_FIELDS_BYTES {
            return Err(Error::Limit(
                "checkpoint anchor fields exceed durable bound",
            ));
        }
        db(insert.execute(params![
            publication.checkpoint,
            anchor.name.as_str(),
            crate::json(&anchor.driver)?,
            anchor.anchor_schema,
            anchor.source_ref,
            access_name(anchor.access),
            policy_name(anchor.checkpoint_policy),
            anchor.identity,
            anchor.retention,
            fields,
        ]))?;
    }
    Ok(())
}

/// Stable replay across the history-only and source-bearing constructors.
fn replay_in(
    connection: &Connection,
    actor: &PrincipalId,
    creator: &str,
    request_id: &str,
    fingerprint: &str,
) -> Result<Option<CheckpointSnapshot>> {
    let replay: Option<(String, String)> = db(connection
        .query_row(
            "SELECT checkpoint_id,request_fingerprint FROM checkpoints
             WHERE created_by=?1 AND request_id=?2",
            params![creator, request_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional())?;
    if let Some((checkpoint, stored)) = replay {
        if stored != fingerprint {
            return Err(Error::Denied(
                "checkpoint request identity reused with different semantics",
            ));
        }
        return match snapshot_in(connection, actor, &checkpoint) {
            Ok(snapshot) => Ok(Some(snapshot)),
            Err(Error::Denied(_)) => Err(Error::Denied(
                "prior invocation is non-retryable; result unavailable",
            )),
            Err(error) => Err(error),
        };
    }
    if let Some(receipt) = crate::construction_receipt::lookup_in(
        connection,
        crate::construction_receipt::ConstructionOperation::CheckpointCreate,
        creator,
        request_id,
    )? {
        if receipt.fingerprint != fingerprint {
            return Err(Error::Denied(
                "checkpoint request identity reused with different semantics",
            ));
        }
        return Err(Error::Conflict(
            "checkpoint request is non-retryable; inspect its prepared receipt",
        ));
    }
    Ok(None)
}

fn fingerprint(reference: &str, note: Option<&str>) -> Result<String> {
    let request = crate::json(&(reference, SourceCoverage::None, note))?;
    let mut hash = Sha256::new();
    hash.update(b"aw-checkpoint-create-v1\0");
    hash.update(request.as_bytes());
    Ok(format!("{:x}", hash.finalize()))
}

/// Trusted host scope for a history checkpoint request. The original
/// requested ref is fingerprinted before resolving the current Agent alias.
/// This carries no capture or view authority by itself.
pub struct CheckpointCreateScope<'a> {
    pub current_agent_ref: &'a str,
    pub current_project_ref: &'a str,
}

impl CheckpointCreateScope<'_> {
    fn require_project(&self, reference: &str) -> Result<()> {
        if reference.rsplit_once("/a:").map(|(project, _)| project)
            != Some(self.current_project_ref)
        {
            return Err(Error::Denied("cross-Project checkpoint access unavailable"));
        }
        Ok(())
    }
}

impl SessionStore {
    /// Trusted host-only, history-only checkpoint construction. The caller
    /// must admit the checkpoint-create capability and supply a stable
    /// authenticated, namespace-scoped request identity from its envelope.
    /// No source capture, grants, external effects or public RPC are exposed.
    pub fn checkpoint_create_history_only(
        &self,
        actor: &PrincipalId,
        reference: &str,
        note: Option<&str>,
        request_id: &str,
    ) -> Result<CheckpointSnapshot> {
        self.checkpoint_create_without_sources(
            actor,
            reference,
            note,
            request_id,
            SourceCoverage::None,
            None,
        )
    }

    /// Trusted lower Auto capture when the effective Sandbox has no required
    /// sources. Preserve Auto coverage and the ordinary Auto replay identity.
    /// Never fall back to history-only when any required binding exists.
    pub fn checkpoint_create_empty_auto(
        &self,
        actor: &PrincipalId,
        reference: &str,
        note: Option<&str>,
        request_id: &str,
    ) -> Result<CheckpointSnapshot> {
        self.checkpoint_create_without_sources(
            actor,
            reference,
            note,
            request_id,
            SourceCoverage::AutoAllRequired,
            None,
        )
    }

    /// Host-admitted history-only or empty Auto checkpoint. Preserve the
    /// literal request fingerprint and replay before resolving `.`. The
    /// caller owns the authenticated request namespace and capability.
    pub fn checkpoint_create_history_host(
        &self,
        actor: &PrincipalId,
        scope: CheckpointCreateScope<'_>,
        reference: &str,
        note: Option<&str>,
        request_id: &str,
        coverage: SourceCoverage,
    ) -> Result<CheckpointSnapshot> {
        self.checkpoint_create_without_sources(
            actor,
            reference,
            note,
            request_id,
            coverage,
            Some(scope),
        )
    }

    fn checkpoint_create_without_sources(
        &self,
        actor: &PrincipalId,
        reference: &str,
        note: Option<&str>,
        request_id: &str,
        coverage: SourceCoverage,
        scope: Option<CheckpointCreateScope<'_>>,
    ) -> Result<CheckpointSnapshot> {
        label(request_id, 256)?;
        label(reference, 2048)?;
        if let Some(note) = note {
            label(note, 2048)?;
        }
        let fingerprint = match coverage {
            SourceCoverage::None => fingerprint(reference, note)?,
            SourceCoverage::AutoAllRequired => {
                capture::fingerprint(reference, &CheckpointCaptureSelection::Auto, note)?
            }
            SourceCoverage::Selected => {
                return Err(Error::Config("selected checkpoint cannot be empty".into()))
            }
        };
        let mut connection = self.connection()?;
        let tx = db(connection.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let creator = user_for_principal(&tx, actor)?
            .ok_or(Error::Denied("checkpoint caller is inactive or unknown"))?;
        // Replay lookup precedes target resolution and current admission.
        if let Some(snapshot) = replay_in(&tx, actor, &creator, request_id, &fingerprint)? {
            if let Some(scope) = &scope {
                scope.require_project(&snapshot.source_agent_ref)?;
            }
            return Ok(snapshot);
        }
        let reference = if let Some(scope) = &scope {
            let reference = if reference == "." {
                scope.current_agent_ref
            } else {
                reference
            };
            scope.require_project(reference)?;
            reference
        } else {
            reference
        };
        let (session, branch) = agent_owner::key_for_ref(&tx, reference)?;
        let source = AgentKey::new(&session, &branch);
        agent_owner::authorize_permission(&tx, actor, source, AgentPermission::View)?;
        if coverage == SourceCoverage::AutoAllRequired {
            capture::require_empty_auto_in(&tx, source)?;
        }
        let history = history_owner::seal_latest_committed_in(&tx, &session, &branch)?;
        let checkpoint = id("cp")?;
        let registry = self.inner.source_drivers.reader()?;
        publish_in(
            &tx,
            &registry,
            self.inner.source_limit,
            Publication {
                actor,
                checkpoint: &checkpoint,
                source,
                history_base_id: &history.id,
                sources: &SourceSet {
                    coverage,
                    anchors: Vec::new(),
                },
                note,
                creator: &creator,
                request_id,
                request_fingerprint: &fingerprint,
            },
        )?;
        let snapshot = snapshot_in(&tx, actor, &checkpoint)?;
        db(tx.commit())?;
        Ok(snapshot)
    }

    /// Current Agent view authority is required even for immutable
    /// checkpoint metadata. Source identities and materialization are withheld.
    pub fn checkpoint_get_metadata(
        &self,
        actor: &PrincipalId,
        checkpoint: &str,
    ) -> Result<CheckpointSnapshot> {
        label(checkpoint, 128)?;
        let mut connection = self.connection()?;
        let tx = db(connection.transaction())?;
        let snapshot = snapshot_in(&tx, actor, checkpoint)?;
        db(tx.commit())?;
        Ok(snapshot)
    }

    /// Current history view and independent exact anchor disclosure in the
    /// same SQLite read transaction. Never fetch or resolve backing data.
    pub fn checkpoint_get_with_source_metadata(
        &self,
        actor: &PrincipalId,
        checkpoint: &str,
        approval: &crate::SourceMetadataApproval,
    ) -> Result<crate::SourceMetadataView<CheckpointSnapshot>> {
        label(checkpoint, 128)?;
        let mut connection = self.connection()?;
        let tx = db(connection.transaction())?;
        approval.require_scope_in(&tx, actor)?;
        let view = snapshot_projected_in(&tx, actor, checkpoint, Some(approval))?;
        db(tx.commit())?;
        Ok(view)
    }
}

/// Include live and retired-but-retained checkpoint anchors, not only
/// those visible through the public live checkpoint list.
pub(crate) fn visit_driver_pins(
    connection: &Connection,
    driver: &aw_agent_core::DriverKey,
    mut visit: impl FnMut(aw_agent_core::DriverSchemaPin) -> Result<()>,
) -> Result<()> {
    let driver_key = crate::json(driver)?;
    let mut query = db(connection.prepare(
        "SELECT DISTINCT c.checkpoint_id,c.source_coverage FROM checkpoints c
         JOIN checkpoint_sources s ON s.checkpoint_id=c.checkpoint_id
         WHERE s.driver_key=?1 ORDER BY c.checkpoint_id",
    ))?;
    let mut rows = db(query.query([&driver_key]))?;
    while let Some(row) = db(rows.next())? {
        let checkpoint: String = db(row.get(0))?;
        let coverage = parse_coverage(&db(row.get::<_, String>(1))?)?;
        let (set, _) = sources_in(connection, &checkpoint, coverage)?;
        for anchor in set.anchors {
            if &anchor.driver == driver {
                visit(anchor.pin()?)?;
            }
        }
    }
    Ok(())
}

/// Resolve retained anchors against the same registry snapshot as bindings.
/// Retired rows still awaiting retention cleanup keep their driver dependency.
/// Missing/incompatible definitions are diagnostics, not source mutations.
pub(crate) fn revalidate_in(
    connection: &Connection,
    registry: &SourceDriverRegistryReader,
) -> Result<()> {
    let mut query = db(connection.prepare(
        "SELECT checkpoint_id,source_coverage FROM checkpoints c
         WHERE (lifecycle='live' AND source_coverage='selected') OR EXISTS(
             SELECT 1 FROM checkpoint_sources s WHERE s.checkpoint_id=c.checkpoint_id
         ) ORDER BY checkpoint_id",
    ))?;
    let mut rows = db(query.query([]))?;
    while let Some(row) = db(rows.next())? {
        let checkpoint: String = db(row.get(0))?;
        let coverage = parse_coverage(&db(row.get::<_, String>(1))?)?;
        let (set, compatibilities) = sources_in(connection, &checkpoint, coverage)?;
        for (anchor, stored) in set.anchors.into_iter().zip(compatibilities) {
            let next = compatibility(registry, &anchor.pin()?)?;
            if next != stored {
                db(connection.execute(
                    "UPDATE checkpoint_sources SET compatibility=?1
                     WHERE checkpoint_id=?2 AND source_name=?3",
                    params![next.as_str(), checkpoint, anchor.name.as_str()],
                ))?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use aw_agent_core::{
        AccountOwner, BillingOwner, CredentialCustody, InvocationIdentity, Outcome, SchemaSupport,
        SourceDriverRegistry, SourceDriverSchemas, SourceIdentityField, SourceIdentityFieldKind,
        Turn, TurnJournal, TurnStart,
    };
    use std::path::PathBuf;

    struct PrivateTemp(PathBuf);
    impl Drop for PrivateTemp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[tokio::test]
    async fn checkpoints_retain_exact_history_and_source_sets_across_restart() {
        let temp = PrivateTemp(std::env::temp_dir().join(id("aw-checkpoint-test").unwrap()));
        let actor = PrincipalId::new("internal:fixture:checkpoint-owner").unwrap();
        let viewer = PrincipalId::new("internal:fixture:checkpoint-viewer").unwrap();
        let store = SessionStore::open(&temp.0).unwrap();
        let (session, branch) = store
            .create(
                &actor,
                "checkpoint root",
                crate::SessionBinding {
                    working_directory: PathBuf::from("/tmp"),
                    tool_root: PathBuf::from("/tmp"),
                    system_prompt: "system".into(),
                    profile_hint: "fixture".into(),
                    model_hint: "model".into(),
                    destination_fingerprint: "0".repeat(64),
                },
            )
            .unwrap();
        let key = AgentKey::new(&session.id, &branch.id);
        let reference = agent_owner::canonical_ref(&store.connection().unwrap(), key).unwrap();
        let project = reference.rsplit_once("/a:").unwrap().0;
        drop(store);

        // A real v55 disk reopen, with an existing Agent and history.
        {
            let connection = Connection::open(temp.0.join("sessions.sqlite3")).unwrap();
            connection
                .execute_batch(
                    "DROP TABLE agent_vault_grants_current;
                 DROP TABLE agent_sandbox_grants_current;
                 DROP INDEX construction_legacy_actor;
                 DROP INDEX construction_source_prepare_id;
                 DROP INDEX construction_receipt_replay;
                 DROP TABLE checkpoint_sources;
                 DROP TABLE checkpoints;
                 UPDATE store_meta SET value='55' WHERE key='schema_epoch';
                 UPDATE schema_meta SET schema_epoch=55,activation_epoch=55 WHERE singleton=1;
                 UPDATE migration_lease SET current_epoch=55 WHERE singleton=1;
                 PRAGMA user_version=55;",
                )
                .unwrap();
        }
        let git = DriverKey::builtin("fixture-git").unwrap();
        let dataset = DriverKey::extension("fixture", "dataset").unwrap();
        let schemas = |from, current| SourceDriverSchemas {
            binding: SchemaSupport::new(1, 1).unwrap(),
            anchor: SchemaSupport::new(from, current).unwrap(),
            recovery: SchemaSupport::new(1, 1).unwrap(),
        };
        let registry = SourceDriverRegistry::new();
        registry.register(git.clone(), schemas(1, 2), ()).unwrap();
        registry
            .register(dataset.clone(), schemas(1, 1), ())
            .unwrap();
        let limit = SourceSetLimit::new(2).unwrap();
        let store = SessionStore::open_with_sources(&temp.0, registry.clone(), limit).unwrap();
        {
            let conn = store.connection().unwrap();
            assert_eq!(
                conn.pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
                    .unwrap(),
                crate::LATEST_SCHEMA_VERSION
            );
            assert_eq!(
                conn.query_row("SELECT count(*) FROM checkpoints", [], |row| row
                    .get::<_, u64>(0))
                    .unwrap(),
                0
            );
        }

        let domain = store.default_domain(&actor).unwrap();
        let viewer_ref = {
            let connection = store.connection().unwrap();
            let user = crate::ensure_user_for_principal(&connection, &viewer).unwrap();
            format!("/d:{}/i:{user}", domain.id)
        };
        let rev = store
            .agent_public_revision(&actor, &session.id, &branch.id)
            .unwrap();
        store
            .agent_access_set(
                &actor,
                &session.id,
                &branch.id,
                &viewer_ref,
                Some(crate::AgentPermissions {
                    view: true,
                    ..Default::default()
                }),
                rev,
            )
            .unwrap();

        let (_, handle) = store.activate(&actor, &session.id, &branch.id).unwrap();
        let identity = InvocationIdentity {
            actor: actor.clone(),
            account_id: "fixture".into(),
            account_owner: AccountOwner::User(actor.clone()),
            payer: BillingOwner::User(actor.clone()),
            credential_custody: CredentialCustody::UserDevice("fixture".into()),
            credential_version: None,
            inference_executor: "local".into(),
            model: "fixture".into(),
            grant_revision: 1,
        };
        let turn = |id: u64, prompt: &str| Turn {
            id,
            prompt: prompt.into(),
            answer: format!("answer {id}"),
            model: identity.model.clone(),
            profile: identity.account_id.clone(),
            outcome: Outcome::Completed,
            meta: None,
            error: None,
            actor: Some(actor.clone()),
            tool_records: Vec::new(),
            invocations: Vec::new(),
            context_messages: Vec::new(),
            needs_reconciliation: false,
        };
        handle
            .begin(TurnStart {
                turn_id: 1,
                prompt: "settled".into(),
                identity: identity.clone(),
            })
            .await
            .unwrap();
        handle.finish(turn(1, "settled")).await.unwrap();
        let settled = store
            .checkpoint_create_history_only(&actor, &reference, None, "host:settled")
            .unwrap();
        handle
            .begin(TurnStart {
                turn_id: 2,
                prompt: "running".into(),
                identity: identity.clone(),
            })
            .await
            .unwrap();
        let rev = store
            .agent_public_revision(&actor, &session.id, &branch.id)
            .unwrap();
        let during = store
            .checkpoint_create_history_only(&viewer, &reference, Some("during turn"), "host:during")
            .unwrap();
        assert_eq!(during.history_base_id, settled.history_base_id);
        assert_eq!(during.history_head, settled.history_head);
        assert_eq!(during.source_coverage, SourceCoverage::None);
        assert!(store.checkpoint_delete(&viewer, &settled.id).is_err());
        assert!(store
            .checkpoint_delete_host(&viewer, project, &settled.id, "host:denied")
            .is_err());
        assert_eq!(
            store
                .checkpoint_list_metadata(&viewer, &reference, None, 1)
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            store
                .agent_public_revision(&actor, &session.id, &branch.id)
                .unwrap(),
            rev
        );
        handle.finish(turn(2, "running")).await.unwrap();
        assert_eq!(
            store
                .checkpoint_create_history_only(
                    &viewer,
                    &reference,
                    Some("during turn"),
                    "host:during"
                )
                .unwrap(),
            during
        );
        assert!(store
            .checkpoint_create_history_only(&viewer, &reference, None, "host:during")
            .is_err());
        let later = store
            .checkpoint_create_history_only(&actor, &reference, None, "host:later")
            .unwrap();
        assert_ne!(later.history_head, settled.history_head);
        let rev = store
            .agent_public_revision(&actor, &session.id, &branch.id)
            .unwrap();
        store
            .agent_access_set(
                &actor,
                &session.id,
                &branch.id,
                &viewer_ref,
                Some(crate::AgentPermissions {
                    view: true,
                    manage: true,
                    ..Default::default()
                }),
                rev,
            )
            .unwrap();
        assert_eq!(
            store
                .checkpoint_delete_host(&viewer, project, &during.id, "host:managed-delete")
                .unwrap(),
            CheckpointDeleteOutcome::Retired
        );
        let rev = store
            .agent_public_revision(&actor, &session.id, &branch.id)
            .unwrap();
        store
            .agent_access_set(&actor, &session.id, &branch.id, &viewer_ref, None, rev)
            .unwrap();
        assert!(store.checkpoint_get_metadata(&viewer, &during.id).is_err());
        assert_eq!(
            store
                .checkpoint_delete_host(&viewer, project, &during.id, "host:managed-delete")
                .unwrap(),
            CheckpointDeleteOutcome::PriorInvocation
        );
        assert!(store
            .checkpoint_delete_host(&viewer, project, &settled.id, "host:managed-delete")
            .is_err());
        assert!(store
            .checkpoint_delete_host(&viewer, project, &during.id, "host:new-denied")
            .is_err());
        assert!(store
            .checkpoint_create_history_only(&viewer, &reference, Some("during turn"), "host:during")
            .is_err());

        // This is the lower storage boundary only. Driver-issued fixture
        // anchors exercise exact layout/schema roundtrip without external
        // effects or bypassing production Prepared admission.
        let set = SourceSet {
            coverage: SourceCoverage::Selected,
            anchors: vec![
                SourceAnchor {
                    name: SourceName::new("code").unwrap(),
                    driver: git.clone(),
                    anchor_schema: 2,
                    source_ref: "protected-repo-identity".into(),
                    access: SourceAccess::ReadWrite,
                    checkpoint_policy: CheckpointPolicy::Required,
                    identity: "protected-version".into(),
                    retention: "protected-retention".into(),
                    fields: vec![SourceIdentityField::new(
                        "base",
                        SourceIdentityFieldKind::Version,
                        "protected-base",
                    )
                    .unwrap()],
                },
                SourceAnchor {
                    name: SourceName::new("dataset").unwrap(),
                    driver: dataset.clone(),
                    anchor_schema: 1,
                    source_ref: "protected-dataset-identity".into(),
                    access: SourceAccess::ReadOnly,
                    checkpoint_policy: CheckpointPolicy::Ephemeral,
                    identity: "protected-dataset-version".into(),
                    retention: "protected-dataset-retention".into(),
                    fields: Vec::new(),
                },
            ],
        };
        let checkpoint = id("cp").unwrap();
        let creator = user_for_principal(&store.connection().unwrap(), &actor)
            .unwrap()
            .unwrap();
        let insert = |store: &SessionStore, id: &str, set: &SourceSet, limit: SourceSetLimit| {
            let mut connection = store.connection()?;
            let tx = db(connection.transaction_with_behavior(TransactionBehavior::Immediate))?;
            let history = history_owner::seal_in(&tx, &session.id, &branch.id)?;
            publish_in(
                &tx,
                &store.inner.source_drivers.reader()?,
                limit,
                Publication {
                    actor: &actor,
                    checkpoint: id,
                    source: key,
                    history_base_id: &history.id,
                    sources: set,
                    note: None,
                    creator: &creator,
                    request_id: id,
                    request_fingerprint: &"0".repeat(64),
                },
            )?;
            db(tx.commit())
        };
        assert!(matches!(
            insert(
                &store,
                "cp_over_limit",
                &set,
                SourceSetLimit::new(1).unwrap()
            ),
            Err(Error::Limit(_))
        ));
        let mut invalid = set.clone();
        invalid.coverage = SourceCoverage::AutoAllRequired;
        assert!(insert(&store, "cp_bad_coverage", &invalid, limit).is_err());
        invalid = set.clone();
        invalid.anchors[1].name = invalid.anchors[0].name.clone();
        assert!(insert(&store, "cp_duplicate", &invalid, limit).is_err());
        invalid = set.clone();
        invalid.anchors[1].anchor_schema = 2;
        assert!(insert(&store, "cp_unknown_schema", &invalid, limit).is_err());
        insert(&store, &checkpoint, &set, limit).unwrap();
        let expected = store.checkpoint_get_metadata(&actor, &checkpoint).unwrap();
        assert_eq!(expected.sources.len(), 2);
        assert_eq!(expected.history_base_id, later.history_base_id);
        assert!(expected
            .sources
            .iter()
            .all(|source| source.compatibility == SourceCompatibility::Compatible));
        assert!(!crate::json(&expected).unwrap().contains("protected-"));
        {
            let conn = store.connection().unwrap();
            assert_eq!(sources_in(&conn, &checkpoint, set.coverage).unwrap().0, set);
            assert_eq!(
                conn.query_row("SELECT count(*) FROM checkpoints", [], |r| r
                    .get::<_, u64>(0))
                    .unwrap(),
                4
            );
            assert!(conn.execute("UPDATE checkpoint_sources SET anchor_identity='changed' WHERE checkpoint_id=?1", [&checkpoint]).is_err());
            assert!(conn
                .execute(
                    "DELETE FROM checkpoint_sources WHERE checkpoint_id=?1",
                    [&checkpoint]
                )
                .is_err());
            assert!(conn
                .execute(
                    "UPDATE checkpoints SET history_head='changed' WHERE checkpoint_id=?1",
                    [&checkpoint]
                )
                .is_err());
            assert!(conn
                .execute(
                    "DELETE FROM checkpoints WHERE checkpoint_id=?1",
                    [&checkpoint]
                )
                .is_err());
            // Actual retirement changes lifecycle only; replay survives and
            // the historical ID is no longer a live history retention root.
            drop(conn);
            let before = store.agent_get(&actor, &reference).unwrap();
            store.checkpoint_delete(&actor, &settled.id).unwrap();
            store.checkpoint_delete(&actor, &settled.id).unwrap();
            assert_eq!(store.agent_get(&actor, &reference).unwrap(), before);
            assert!(!store
                .checkpoint_list_metadata(&actor, &reference, None, 256)
                .unwrap()
                .iter()
                .any(|snapshot| snapshot.id == settled.id));
            assert!(store
                .checkpoint_list_metadata(&actor, &reference, Some(&settled.id), 1)
                .is_ok());
            let conn = store.connection().unwrap();
            let retained: Option<String> = conn
                .query_row(
                    "SELECT live_history_base_id FROM checkpoints WHERE checkpoint_id=?1",
                    [&settled.id],
                    |row| row.get(0),
                )
                .unwrap();
            assert!(retained.is_none());
        }
        assert_eq!(
            store
                .checkpoint_create_history_only(&actor, &reference, None, "host:settled")
                .unwrap()
                .lifecycle,
            CheckpointLifecycle::Retired
        );
        drop(handle);
        drop(store);

        // Missing/incompatible exact artifact schemas do not rewrite anchors.
        let incompatible = SourceDriverRegistry::new();
        incompatible
            .register(git.clone(), schemas(1, 1), ())
            .unwrap();
        let store = SessionStore::open_with_sources(&temp.0, incompatible, limit).unwrap();
        for driver in [&git, &dataset] {
            let dependencies = store.source_driver_dependencies(driver).unwrap();
            assert!(dependencies.iter().any(|dependency| dependency.kind
                == crate::SourceDependencyKind::CheckpointAnchor
                && dependency.compatibility == SourceCompatibility::RecoveryRequired
                && dependency.count == 1));
        }
        let mut degraded = expected.clone();
        for source in &mut degraded.sources {
            source.compatibility = SourceCompatibility::RecoveryRequired;
        }
        assert_eq!(
            store.checkpoint_get_metadata(&actor, &checkpoint).unwrap(),
            degraded
        );
        assert_eq!(
            sources_in(&store.connection().unwrap(), &checkpoint, set.coverage)
                .unwrap()
                .0,
            set
        );
        let backup = temp.0.join("checkpoint-backup.sqlite3");
        store.backup(&backup).unwrap();
        drop(store);

        let reinstalled = SourceDriverRegistry::new();
        let dataset_handle = reinstalled
            .register(dataset.clone(), schemas(1, 1), ())
            .unwrap();
        let git_handle = reinstalled
            .register(git.clone(), schemas(1, 2), ())
            .unwrap();
        let store = SessionStore::open_with_sources(
            &temp.0,
            reinstalled.clone(),
            SourceSetLimit::new(1).unwrap(),
        )
        .unwrap();
        assert_eq!(
            store.checkpoint_get_metadata(&actor, &checkpoint).unwrap(),
            expected
        );
        assert!(matches!(
            insert(&store, "cp_lowered_limit", &set, store.inner.source_limit),
            Err(Error::Limit(_))
        ));
        store.restore_backup(&backup).unwrap();
        assert_eq!(
            store.checkpoint_get_metadata(&actor, &checkpoint).unwrap(),
            expected
        );

        // Direct mode installs no definitions, but protected metadata stays readable.
        #[cfg(target_os = "linux")]
        let store = {
            drop(store);
            let direct =
                SessionStore::open_trusted_local_direct(&temp.0, Default::default()).unwrap();
            assert_eq!(
                direct.checkpoint_get_metadata(&actor, &checkpoint).unwrap(),
                degraded
            );
            drop(direct);
            SessionStore::open_with_sources(&temp.0, reinstalled.clone(), limit).unwrap()
        };
        // Use the existing lower anchor fixture to exercise complete-set
        // retirement/cleanup custody and shared retention. The real Git
        // conditional ref release is qualified by its repository test.
        let mut cleanup_set = set.clone();
        for anchor in &mut cleanup_set.anchors {
            anchor.retention.push_str("-cleanup");
        }
        let first = "cp_cleanup_a";
        let second = "cp_cleanup_b";
        insert(&store, first, &cleanup_set, limit).unwrap();
        insert(&store, second, &cleanup_set, limit).unwrap();
        assert!(store
            .checkpoint_reconcile_retired_sources(&actor, first, |_| panic!(
                "live checkpoint cannot release retention"
            ))
            .is_err());
        let before = store.agent_get(&actor, &reference).unwrap();
        assert_eq!(
            store
                .checkpoint_delete_host(&actor, project, first, "host:retire-sources")
                .unwrap(),
            CheckpointDeleteOutcome::Retired
        );
        assert_eq!(
            store
                .checkpoint_delete_host(&actor, project, first, "host:retire-sources")
                .unwrap(),
            CheckpointDeleteOutcome::Retired
        );
        {
            let conn = store.connection().unwrap();
            assert_eq!(
                sources_in(&conn, first, cleanup_set.coverage).unwrap().0,
                cleanup_set
            );
            assert_eq!(conn.query_row("SELECT count(*) FROM commit_log WHERE kind='checkpoint.retired' AND entity_type='checkpoint' AND entity_id=?1", [first], |r| r.get::<_, u64>(0)).unwrap(), 1);
        }
        assert!(store
            .checkpoint_reconcile_retired_sources(&actor, first, |_| panic!(
                "shared live retention must remain pinned"
            ))
            .is_err());
        store.checkpoint_delete(&actor, second).unwrap();
        assert_eq!(store.agent_get(&actor, &reference).unwrap(), before);
        let mutations = crate::SourceMutationSet::from_anchors(&cleanup_set.anchors).unwrap();
        // This isolated Store fixture controls its Workspace writer.
        store
            .connection()
            .unwrap()
            .execute("UPDATE workspace_writer_state SET state='read_only'", [])
            .unwrap();
        assert!(matches!(
            store.checkpoint_reconcile_retired_sources_fenced::<()>(
                &actor,
                first,
                |_| panic!("read-only writer reached physical approval"),
                |_, _| panic!("read-only writer reached release"),
            ),
            Err(Error::Denied(_))
        ));
        store
            .connection()
            .unwrap()
            .execute("UPDATE workspace_writer_state SET state='active'", [])
            .unwrap();
        assert!(matches!(store.checkpoint_reconcile_retired_sources_fenced(
            &actor, first,
            |_| {
                // Race a new writer epoch against the read-only approval
                // phase, before the actual source custody claim.
                store.connection()?.execute(
                    "UPDATE workspace_writer_state SET writer_epoch=writer_epoch+1,revision=revision+1",
                    [],
                ).map_err(|_| Error::Storage("fixture writer change failed"))?;
                Ok(())
            },
            |_, _| panic!("changed Workspace writer reached physical release"),
        ), Err(Error::Conflict(_))));
        assert_eq!(
            sources_in(&store.connection().unwrap(), first, cleanup_set.coverage)
                .unwrap()
                .0,
            cleanup_set
        );
        // Neither refusal left an Unknown cleanup or mutated retained rows.
        let held = store
            .acquire_source_mutation_barrier(&mutations, "other-retention")
            .unwrap();
        assert!(store
            .checkpoint_reconcile_retired_sources(&actor, first, |_| panic!(
                "busy source cannot release retention"
            ))
            .is_err());
        held.settle(crate::SourceMutationOutcome::Settled).unwrap();
        let paths: Vec<_> = cleanup_set
            .anchors
            .iter()
            .map(|anchor| temp.0.join(anchor.name.as_str()))
            .collect();
        for path in &paths {
            std::fs::write(path, b"retained").unwrap();
        }
        assert!(store
            .checkpoint_reconcile_retired_sources(&actor, first, |original| {
                assert_eq!(original, &cleanup_set);
                assert!(store
                    .acquire_source_mutation_barrier(&mutations, "during-cleanup")
                    .is_err());
                std::fs::remove_file(&paths[0])?;
                Err(Error::Conflict("fixture interruption after first release"))
            })
            .is_err());
        assert!(!paths[0].exists() && paths[1].exists());
        assert_eq!(
            sources_in(&store.connection().unwrap(), first, cleanup_set.coverage)
                .unwrap()
                .0,
            cleanup_set
        );
        drop(store);

        let missing = SessionStore::open(&temp.0).unwrap();
        assert_eq!(
            missing
                .checkpoint_delete_host(&actor, project, first, "host:retire-sources")
                .unwrap(),
            CheckpointDeleteOutcome::Retired
        );
        assert!(missing
            .checkpoint_delete_host(&actor, project, second, "host:retire-sources")
            .is_err());
        assert!(missing
            .checkpoint_reconcile_retired_sources(&actor, first, |_| panic!(
                "missing driver cannot inspect retained source"
            ))
            .is_err());
        assert!(missing
            .acquire_source_mutation_barrier(&mutations, "restart-cleanup")
            .is_err());
        drop(missing);
        let store = SessionStore::open_with_sources(
            &temp.0,
            reinstalled.clone(),
            SourceSetLimit::new(1).unwrap(),
        )
        .unwrap();
        store
            .checkpoint_reconcile_retired_sources(&actor, first, |original| {
                assert_eq!(original, &cleanup_set);
                for path in &paths {
                    match std::fs::remove_file(path) {
                        Ok(()) => {}
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                        Err(error) => return Err(error.into()),
                    }
                }
                Ok(())
            })
            .unwrap();
        store
            .checkpoint_reconcile_retired_sources(&actor, first, |_| {
                panic!("released retention must not replay inspection")
            })
            .unwrap();
        assert_eq!(
            store
                .source_driver_dependencies(&git)
                .unwrap()
                .iter()
                .filter(|dep| dep.kind == crate::SourceDependencyKind::CheckpointAnchor)
                .map(|dep| dep.count)
                .sum::<u64>(),
            2,
            "retired-but-retained anchor remains a dependency"
        );
        // Shared retired anchor can now observe absence; a lowered source
        // limit does not make old cleanup dependencies impossible to release.
        store
            .checkpoint_reconcile_retired_sources(&actor, second, |original| {
                assert_eq!(original, &cleanup_set);
                assert!(paths.iter().all(|path| !path.exists()));
                Ok(())
            })
            .unwrap();
        assert_eq!(
            store
                .checkpoint_get_metadata(&actor, first)
                .unwrap()
                .lifecycle,
            CheckpointLifecycle::Retired
        );
        assert_eq!(
            store.checkpoint_get_metadata(&actor, &checkpoint).unwrap(),
            expected
        );
        assert_eq!(store.agent_get(&actor, &reference).unwrap(), before);
        // Reopen with sole registry/Store ownership: the original live
        // anchor, not an outstanding clone, must block retirement.
        drop(store);
        let mut store = SessionStore::open_with_sources(&temp.0, reinstalled, limit).unwrap();
        for driver in [&git, &dataset] {
            let deps = store.source_driver_dependencies(driver).unwrap();
            assert_eq!(
                deps.iter()
                    .filter(|dep| dep.kind == crate::SourceDependencyKind::CheckpointAnchor)
                    .map(|dep| dep.count)
                    .sum::<u64>(),
                1
            );
            assert!(deps
                .iter()
                .all(|dep| dep.compatibility == SourceCompatibility::Compatible));
        }
        assert!(matches!(
            store.remove_source_driver(&dataset_handle, None),
            Err(Error::Conflict(
                "SourceDriver has retained durable dependencies"
            ))
        ));
        assert!(matches!(
            store.replace_source_driver(&git_handle, schemas(1, 2), (), None),
            Err(Error::Conflict(
                "SourceDriver has retained durable dependencies"
            ))
        ));
        {
            let conn = store.connection().unwrap();
            assert_eq!(
                conn.query_row(
                    "SELECT count(*) FROM checkpoint_sources WHERE checkpoint_id IN (?1,?2)",
                    params![first, second],
                    |row| row.get::<_, usize>(0),
                )
                .unwrap(),
                0
            );
            conn.execute(
                "WITH RECURSIVE n(x) AS (SELECT 1 UNION ALL SELECT x+1 FROM n WHERE x<1023)
                 INSERT INTO checkpoint_sources
                 SELECT checkpoint_id,'extra-'||x,driver_key,anchor_schema,source_ref,
                        captured_access,captured_checkpoint_policy,anchor_identity,retention,
                        anchor_fields_json,compatibility
                 FROM checkpoint_sources,n WHERE checkpoint_id=?1 AND source_name='code'",
                [&checkpoint],
            )
            .unwrap();
            assert!(matches!(
                snapshot_in(&conn, &actor, &checkpoint),
                Err(Error::Limit(_))
            ));
        }
        drop(store);
        assert!(matches!(
            SessionStore::open_with_sources(&temp.0, SourceDriverRegistry::new(), limit),
            Err(Error::Limit(_))
        ));
    }
}
