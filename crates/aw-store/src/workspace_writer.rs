use super::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceWriterInfo {
    pub workspace_id: String,
    pub domain_id: String,
    pub writer_node_id: String,
    pub writer_epoch: u64,
    pub authority_term: u64,
    pub state: String,
    pub revision: u64,
    pub updated_at: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceWriterEventInfo {
    pub seq: u64,
    pub event_id: String,
    pub workspace_id: String,
    pub domain_id: String,
    pub old_writer_node_id: Option<String>,
    pub old_writer_epoch: Option<u64>,
    pub new_writer_node_id: String,
    pub new_writer_epoch: u64,
    pub authority_term: u64,
    pub actor: String,
    pub reason: String,
    pub created_at: i64,
}

pub(crate) fn writer_row(c: &Connection, workspace_id: &str) -> Result<WorkspaceWriterInfo> {
    db(c.query_row(
        "SELECT workspace_id,domain_id,writer_node_id,writer_epoch,authority_term,state,revision,updated_at FROM workspace_writer_state WHERE workspace_id=?1",
        [workspace_id],
        |r| Ok(WorkspaceWriterInfo {
            workspace_id:r.get(0)?, domain_id:r.get(1)?, writer_node_id:r.get(2)?,
            writer_epoch:r.get(3)?, authority_term:r.get(4)?, state:r.get(5)?,
            revision:r.get(6)?, updated_at:r.get(7)?,
        }),
    ).optional())?.ok_or(Error::Storage("Workspace writer state missing"))
}

pub(crate) fn workspace_writer_for_session(
    c: &Connection,
    session_id: &str,
) -> Result<WorkspaceWriterInfo> {
    let workspace_id: String = db(c.query_row(
        "SELECT workspace_id FROM sessions WHERE id=?1",
        [session_id],
        |r| r.get(0),
    ))?;
    writer_row(c, &workspace_id)
}

pub(crate) fn require_workspace_writer(
    c: &Connection,
    workspace_id: &str,
) -> Result<WorkspaceWriterInfo> {
    let writer = writer_row(c, workspace_id)?;
    require_local_writer(c, &writer)?;
    Ok(writer)
}

pub(crate) fn require_workspace_writer_for_session(
    c: &Connection,
    session_id: &str,
) -> Result<WorkspaceWriterInfo> {
    let writer = workspace_writer_for_session(c, session_id)?;
    require_local_writer(c, &writer)?;
    Ok(writer)
}

pub(crate) fn replace_local_writer_node(
    c: &Connection,
    old_node: &str,
    new_node: &str,
) -> Result<()> {
    if old_node == new_node {
        return Ok(());
    }
    db(c.execute(
        "UPDATE workspace_writer_state SET writer_node_id=?1,revision=revision+1,updated_at=?2 WHERE writer_node_id=?3",
        params![new_node,now(),old_node],
    ))?;
    Ok(())
}

pub(crate) fn require_local_writer(c: &Connection, writer: &WorkspaceWriterInfo) -> Result<()> {
    if writer.state != "active" {
        return Err(Error::Denied("Workspace mutable refs are read-only"));
    }
    let local_node: String = db(c.query_row(
        "SELECT value FROM store_meta WHERE key='local_node_id'",
        [],
        |r| r.get(0),
    ))?;
    if writer.writer_node_id != local_node {
        return Err(Error::Denied(
            "this AwNode does not own the current Workspace writer epoch",
        ));
    }
    Ok(())
}

pub(crate) fn verify_writer_epoch_for_session(
    c: &Connection,
    session_id: &str,
    expected_epoch: u64,
) -> Result<WorkspaceWriterInfo> {
    let writer = require_workspace_writer_for_session(c, session_id)?;
    if writer.writer_epoch != expected_epoch {
        return Err(Error::Conflict(
            "Workspace writer epoch changed; preserve the result and reload instead of rebasing",
        ));
    }
    Ok(writer)
}

impl SessionStore {
    pub fn workspace_writer(
        &self,
        actor: &PrincipalId,
        domain_selector: Option<&str>,
        workspace_selector: &str,
    ) -> Result<WorkspaceWriterInfo> {
        let workspace = self.workspace(actor, domain_selector, workspace_selector)?;
        let c = self.connection()?;
        writer_row(&c, &workspace.id)
    }

    pub fn workspace_writer_events(
        &self,
        actor: &PrincipalId,
        domain_selector: Option<&str>,
        workspace_selector: &str,
    ) -> Result<Vec<WorkspaceWriterEventInfo>> {
        let workspace = self.workspace(actor, domain_selector, workspace_selector)?;
        let c = self.connection()?;
        let mut q = db(c.prepare(
            "SELECT seq,event_id,workspace_id,domain_id,old_writer_node_id,old_writer_epoch,new_writer_node_id,new_writer_epoch,authority_term,actor,reason,created_at FROM workspace_writer_events WHERE workspace_id=?1 ORDER BY seq LIMIT 512",
        ))?;
        let rows = db(q.query_map([&workspace.id], |r| {
            Ok(WorkspaceWriterEventInfo {
                seq: r.get(0)?,
                event_id: r.get(1)?,
                workspace_id: r.get(2)?,
                domain_id: r.get(3)?,
                old_writer_node_id: r.get(4)?,
                old_writer_epoch: r.get(5)?,
                new_writer_node_id: r.get(6)?,
                new_writer_epoch: r.get(7)?,
                authority_term: r.get(8)?,
                actor: r.get(9)?,
                reason: r.get(10)?,
                created_at: r.get(11)?,
            })
        }))?;
        db(rows.collect::<rusqlite::Result<Vec<_>>>())
    }

    pub fn failover_workspace_writer(
        &self,
        actor: &PrincipalId,
        domain_selector: Option<&str>,
        workspace_selector: &str,
        expected_epoch: u64,
        target_node_id: &str,
        reason: &str,
    ) -> Result<WorkspaceWriterInfo> {
        label(reason, 64)?;
        let workspace = self.workspace(actor, domain_selector, workspace_selector)?;
        let mut c = self.connection()?;
        let tx = db(c.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let user = ensure_user_for_principal(&tx, actor)?;
        let role: Option<String> = db(tx.query_row(
            "SELECT role FROM domain_memberships WHERE domain_id=?1 AND user_id=?2 AND status='active'",
            params![workspace.domain_id,user],
            |r| r.get(0),
        ).optional())?;
        if !matches!(role.as_deref(), Some("owner" | "admin")) {
            return Err(Error::Denied(
                "Workspace writer failover requires Domain owner/admin authority",
            ));
        }
        authority::require_domain_authority_write(&tx, &workspace.domain_id)?;
        let current = writer_row(&tx, &workspace.id)?;
        if current.writer_epoch != expected_epoch {
            return Err(Error::Conflict(
                "Workspace writer epoch changed; reload before failover",
            ));
        }
        if current.writer_node_id == target_node_id && current.state == "active" {
            return Ok(current);
        }
        let local_node: String = db(tx.query_row(
            "SELECT value FROM store_meta WHERE key='local_node_id'",
            [],
            |r| r.get(0),
        ))?;
        let target_valid = if target_node_id == local_node {
            true
        } else {
            db(tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM machines m JOIN awnet_peers p ON p.node_id=m.node_id WHERE m.node_id=?1 AND m.owning_domain_id=?2 AND m.kind='remote' AND m.status='active' AND p.trust_state='trusted')",
                params![target_node_id,workspace.domain_id],
                |r| r.get(0),
            ))?
        };
        if !target_valid {
            return Err(Error::Denied(
                "Workspace writer target must be this AwNode or a trusted active remote Machine in the Domain",
            ));
        }
        let authority_term: u64 = db(tx.query_row(
            "SELECT current_term FROM domain_authority_sets WHERE domain_id=?1",
            [&workspace.domain_id],
            |r| r.get(0),
        ))?;
        let next_epoch = current
            .writer_epoch
            .checked_add(1)
            .ok_or(Error::Limit("Workspace writer epoch exhausted"))?;
        let when = now();
        let changed = db(tx.execute(
            "UPDATE workspace_writer_state SET writer_node_id=?1,writer_epoch=?2,authority_term=?3,state='active',revision=revision+1,updated_at=?4 WHERE workspace_id=?5 AND writer_epoch=?6",
            params![target_node_id,next_epoch,authority_term,when,workspace.id,current.writer_epoch],
        ))?;
        if changed != 1 {
            return Err(Error::Conflict(
                "Workspace writer epoch changed during failover",
            ));
        }
        let event_id = id("wwe")?;
        db(tx.execute(
            "INSERT INTO workspace_writer_events(event_id,workspace_id,domain_id,old_writer_node_id,old_writer_epoch,new_writer_node_id,new_writer_epoch,authority_term,actor,reason,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
            params![event_id,workspace.id,workspace.domain_id,current.writer_node_id,current.writer_epoch,target_node_id,next_epoch,authority_term,actor.as_str(),reason,when],
        ))?;
        db(tx.execute(
            "UPDATE workspaces SET revision=revision+1,updated_at=?1 WHERE workspace_id=?2",
            params![when, workspace.id],
        ))?;
        let txid = durability::tx_id()?;
        durability::commit_event(
            &tx,
            &txid,
            "workspace.writer_failed_over",
            "workspace",
            &workspace.id,
            Some(actor),
            serde_json::json!({
                "domain_id":workspace.domain_id,
                "workspace_id":workspace.id,
                "old_writer_node_id":current.writer_node_id,
                "old_writer_epoch":current.writer_epoch,
                "new_writer_node_id":target_node_id,
                "new_writer_epoch":next_epoch,
                "authority_term":authority_term,
                "reason":reason
            }),
        )?;
        db(tx.commit())?;
        writer_row(&c, &workspace.id)
    }
}
