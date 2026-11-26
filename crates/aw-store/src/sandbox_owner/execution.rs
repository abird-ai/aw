//! Exact, live native source execution admission. This is an owner-local
//! handle to an independent policy, not a persisted grant or another
//! execution owner. The ordinary source-unaware Session paths stay closed.
use super::{agent_project_and_layout, snapshot_with_sources, SandboxEffectAuthority};
use crate::{
    agent_owner::{self, AgentKey},
    db, principal_is_session_owner, source_barrier,
    store_scope::StoreScope,
    workspace_writer, Error, Result, SessionStore, SourceMutationSet, StoreMode,
};
use aw_agent_core::{
    tool::{ToolBindingAuthority, ToolBindingSnapshot, WeakToolBindingAuthority},
    AgentExecutionBinding, PrincipalId, ToolEffect, ToolSource, ToolSourceEvidence,
};
use aw_tools::filesystem::{access::Action, schema::Tool};
use rusqlite::{params, Connection};
use std::{path::PathBuf, sync::Arc};

type HeldPolicy = Box<dyn SandboxEffectAuthority + Send>;

/// Host-selected direct native route for one exact existing Actor/Session.
/// The factory must acquire the actual independent policy guard on every
/// call and hold it until dropped. It may not reconstruct a durable grant.
/// Tool authority is the original registry's weak handle; never create a
/// second registry or let the journal keep a retired one alive.
#[derive(Clone)]
pub struct SandboxExecution {
    store: SessionStore,
    scope: StoreScope,
    actor: PrincipalId,
    agent: AgentExecutionBinding,
    writer_epoch: u64,
    base: PathBuf,
    tools: WeakToolBindingAuthority,
    bindings: Arc<[ToolBindingSnapshot]>,
    acquire: Arc<dyn Fn() -> Result<HeldPolicy> + Send + Sync>,
}

impl SandboxExecution {
    pub fn from_trusted_host<A, F>(
        store: &SessionStore,
        actor: PrincipalId,
        agent: AgentExecutionBinding,
        base: PathBuf,
        tools: &ToolBindingAuthority,
        acquire: F,
    ) -> Result<Self>
    where
        A: SandboxEffectAuthority + Send + 'static,
        F: Fn() -> Result<A> + Send + Sync + 'static,
    {
        if store.inner.mode != StoreMode::OwnerLock {
            return Err(Error::Denied(
                "source execution requires the exclusive Store owner",
            ));
        }
        let bindings = tools.permitted_bindings(&actor)?;
        if bindings.is_empty()
            || bindings.iter().any(|binding| {
                let native = Tool::from_name(&binding.name).is_some_and(|tool| {
                    tool != Tool::Remove && binding.effect == native_effect(tool)
                });
                // One fixed foreground Process constructor, not general Graph execution.
                let process = bindings.len() == 1
                    && matches!(binding.name.as_str(), "bash" | "pwsh" | "powershell")
                    && binding.effect == ToolEffect::Process
                    && binding.claim_route
                        == Some(aw_agent_core::tool::ToolClaimRoute::ProcessSourceV1);
                !(native || process)
                    || binding.source != ToolSource::trusted_host()
                    || binding.execution_site != bindings[0].execution_site
            })
        {
            return Err(Error::Denied(
                "source execution requires an exact fixed source tool set",
            ));
        }
        let c = store.connection()?;
        let writer =
            workspace_writer::require_workspace_writer_for_session(&c, agent.session_id())?;
        let scope = StoreScope::load(&c)?;
        drop(c);
        let result = Self {
            store: store.clone(),
            scope,
            actor,
            agent,
            writer_epoch: writer.writer_epoch,
            base,
            tools: tools.downgrade(),
            bindings: bindings.into(),
            acquire: Arc::new(move || Ok(Box::new(acquire()?) as HeldPolicy)),
        };
        // Preflight through the same exact current guard, without opening
        // admission or minting an input.
        let guard = result.read(store)?;
        let c = store.connection()?;
        guard.require_in(
            &c,
            &result.actor,
            result.agent.session_id(),
            result.agent.branch_id(),
            false,
        )?;
        drop(c);
        drop(guard);
        Ok(result)
    }

    pub fn agent(&self) -> &AgentExecutionBinding {
        &self.agent
    }
    pub fn actor(&self) -> &PrincipalId {
        &self.actor
    }
    pub fn execution_site(&self) -> &str {
        &self.bindings[0].execution_site
    }
    pub fn writer_epoch(&self) -> u64 {
        self.writer_epoch
    }
    pub fn matches_tools(&self, tools: &ToolBindingAuthority) -> bool {
        self.tools.belongs_to(tools)
    }

    pub(crate) fn read<'a>(&'a self, store: &SessionStore) -> Result<SandboxExecutionGuard<'a>> {
        if !self.store.shares_owner(store) {
            return Err(Error::Denied(
                "source execution belongs to another Store owner",
            ));
        }
        let guard = (self.acquire)()?;
        if guard.tool_binding().is_some()
            || guard.policy().agent_ref != self.agent.canonical_ref()
            || guard.policy().actor != self.actor
        {
            return Err(Error::Denied(
                "source execution policy differs from its exact Actor",
            ));
        }
        // No Store lock while consulting registry authority. Actual native
        // effects also retain the original exact ToolBinding/Envelope
        // publication fence through their own source-effect commit.
        if self
            .tools
            .upgrade()?
            .permitted_bindings(&self.actor)?
            .as_slice()
            != self.bindings.as_ref()
        {
            return Err(Error::Denied("source execution tool authority changed"));
        }
        Ok(SandboxExecutionGuard {
            execution: self,
            guard,
        })
    }
}

fn native_effect(tool: Tool) -> ToolEffect {
    if tool.action() == Action::Read {
        ToolEffect::Read
    } else {
        ToolEffect::Write
    }
}

pub(crate) struct SandboxExecutionGuard<'a> {
    execution: &'a SandboxExecution,
    guard: HeldPolicy,
}

impl SandboxExecutionGuard<'_> {
    /// Verify exact Store/actor/Agent/writer, immutable complete named
    /// layout and independent current policy in the ORIGINAL transaction.
    /// Held admission permits current factual settlement but not a new
    /// Input/Turn. The caller retains self through its transaction.
    pub(crate) fn require_in(
        &self,
        c: &Connection,
        actor: &PrincipalId,
        session: &str,
        branch: &str,
        future: bool,
    ) -> Result<()> {
        let e = self.execution;
        if actor != &e.actor
            || session != e.agent.session_id()
            || branch != e.agent.branch_id()
            || StoreScope::load(c)? != e.scope
            || !principal_is_session_owner(c, session, actor)?
        {
            return Err(Error::Denied("source execution scope changed"));
        }
        let key = AgentKey::new(session, branch);
        if agent_owner::canonical_ref(c, key)? != e.agent.canonical_ref() {
            return Err(Error::Denied("source execution Agent changed"));
        }
        workspace_writer::verify_writer_epoch_for_session(c, session, e.writer_epoch)?;
        let (lifecycle, _) = agent_owner::lifecycle(c, key)?;
        if lifecycle != "open" {
            return Err(Error::Denied("source execution Agent is closing"));
        }
        let (project, layout, sandbox, _, _) = agent_project_and_layout(c, session, branch)?;
        let sandbox = sandbox.ok_or(Error::Denied("source execution requires a bound Sandbox"))?;
        let snapshot = snapshot_with_sources(c, &sandbox)?;
        if snapshot.project_ref != project.canonical()
            || snapshot.layout_digest != layout.digest
            || snapshot.tool_root != e.base
            || snapshot.working_directory != e.base
            || snapshot.tool_root.to_str() != Some(layout.tool_root.as_str())
            || snapshot.working_directory.to_str() != Some(layout.working_directory.as_str())
            || snapshot.sources.is_empty()
        {
            return Err(Error::Denied(
                "source execution differs from the immutable named layout",
            ));
        }
        // The first native route is deliberately isolated to one bound
        // Agent/Session. No ambient alias, shared Sandbox or fork is admitted.
        let shared: bool = db(c.query_row(
            "SELECT EXISTS(SELECT 1 FROM agent_state_current a
              WHERE (a.sandbox_id=?1 OR a.session_id=?2)
                AND NOT (a.session_id=?2 AND a.branch_id=?3))
             OR EXISTS(SELECT 1 FROM sandboxes_current s
                WHERE s.sandbox_id<>?1 AND
                (s.tool_root IN (?4,?5) OR s.working_directory IN (?4,?5))
                AND s.lifecycle='open')",
            params![
                sandbox,
                session,
                branch,
                layout.tool_root,
                layout.working_directory
            ],
            |row| row.get(0),
        ))?;
        if shared {
            return Err(Error::Denied(
                "source execution requires an isolated exact Agent layout",
            ));
        }
        super::require_no_retained_layout_in(c, &layout.tool_root, &layout.working_directory)?;
        source_barrier::require_unleased_layout_in(
            c,
            &layout.tool_root,
            &layout.working_directory,
        )?;
        let observed = e.store.sandbox_grant_preflight_in(
            c,
            actor,
            e.agent.canonical_ref(),
            self.guard.policy(),
            self.guard.tool_ceiling(),
        )?;
        if observed.sandbox_ref != sandbox || observed.sources.len() != snapshot.sources.len() {
            return Err(Error::Denied(
                "source execution requires the complete approved binding set",
            ));
        }
        // All selected write tools need at least one writable source.
        if e.bindings
            .iter()
            .any(|binding| binding.effect == ToolEffect::Write)
            && !observed
                .sources
                .iter()
                .any(|source| source.access == aw_agent_core::SourceAccess::ReadWrite)
        {
            return Err(Error::Denied(
                "source execution has no writable approved source",
            ));
        }
        if future {
            SourceMutationSet::from_binding_refs(
                observed
                    .sources
                    .iter()
                    .map(|source| &source.binding.binding),
            )?
            .require_unleased_in(c)?;
        }
        Ok(())
    }

    pub(crate) fn require_tool(
        &self,
        name: &str,
        effect: &ToolEffect,
        site: &str,
        source: Option<&ToolSourceEvidence>,
    ) -> Result<Option<aw_agent_core::tool::ToolClaimRoute>> {
        self.execution
            .bindings
            .iter()
            .find(|binding| {
                binding.name == name
                    && &binding.effect == effect
                    && binding.execution_site == site
                    && source == Some(&ToolSourceEvidence::from_source(&binding.source))
            })
            .map(|binding| binding.claim_route)
            .ok_or(Error::Denied(
                "source execution tool differs from original native route",
            ))
    }
}

pub(crate) fn read<'a>(
    source: Option<&'a SandboxExecution>,
    store: &SessionStore,
) -> Result<Option<SandboxExecutionGuard<'a>>> {
    source.map(|source| source.read(store)).transpose()
}

pub(crate) fn require_future_in(
    c: &Connection,
    actor: &PrincipalId,
    key: AgentKey<'_>,
    source: Option<&SandboxExecutionGuard<'_>>,
) -> Result<()> {
    match source {
        Some(source) => {
            let (_, admission) = agent_owner::lifecycle(c, key)?;
            if admission != "open" {
                return Err(Error::Denied("Agent admission is held"));
            }
            source.require_in(c, actor, key.session, key.branch, true)
        }
        None => agent_owner::require_future_admission(c, key),
    }
}
