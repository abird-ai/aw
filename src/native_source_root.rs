//! Explicit, local stdio startup of one existing native Git-backed Actor.
//! The original Store, Git host and Actor own all effects; no ambient tool
//! configuration and no implicit source grant or Agent opening.
use crate::git_maintenance::{canonical_private_directory, overlaps};
use aw::{
    config::Settings,
    sessions::{self, PersistentSession},
    Agent, Error, PrincipalId, Result,
};
use aw_agent_core::{DriverKey, SandboxDriverRegistry, SourceAccess};
use aw_git::source_driver::GitSourceDriver;
use aw_host::{
    source_filesystem::NativeFilesystemDriver,
    source_git::{GitNativeSourceHost, GitNativeSourceResolver},
};
use aw_store::{AgentAdmission, AgentLifecycle, SessionBinding, SessionInfo, SessionStore};
use aw_store_api::SessionBackend;
use aw_tools::filesystem::{access::Action, schema::Tool};
use aw_tools_adapter::{LocalToolOptions, ToolMode};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    sync::Arc,
};

#[derive(Debug)]
pub(crate) struct Selection {
    pub root: PathBuf,
    pub grant_rev: u64,
    pub open_rev: Option<u64>,
}

pub(crate) struct Activation<'a> {
    pub manager: &'a PrincipalId,
    pub session: &'a SessionInfo,
    pub branch: &'a str,
    pub binding: SessionBinding,
    pub settings: &'a Settings,
    pub tools: &'a LocalToolOptions,
    pub account: Option<&'a str>,
    pub pre_dispatch_retries: u8,
}

pub(crate) struct Activated {
    pub persistent: PersistentSession,
    pub agent: Agent,
    pub owner: aw::actor_owner::Service,
    pub withdrawal: Arc<Withdrawal>,
}

/// The same live approval used by the native route and journal. Stdio
/// authenticates the bound local host; it cannot name another authority.
pub(crate) struct Withdrawal(GitNativeSourceHost);

impl aw::headless::LiveSourceWithdrawalPort for Withdrawal {
    fn request_withdrawal(&self) -> bool {
        self.0.request_withdrawal()
    }
}

pub(crate) fn require_native_tools(mode: ToolMode, names: &[String]) -> Result<()> {
    if !matches!(mode, ToolMode::ReadOnly | ToolMode::Workspace)
        || names.is_empty()
        || names.iter().collect::<BTreeSet<_>>().len() != names.len()
        || names.iter().any(|name| {
            Tool::from_name(name).is_none_or(|tool| {
                tool == Tool::Remove
                    || (mode == ToolMode::ReadOnly && tool.action() != Action::Read)
            })
        })
    {
        return Err(Error::Denied(
            "native source RPC requires an explicit non-Remove native filesystem tool set",
        ));
    }
    Ok(())
}

fn require_disjoint(a: &Path, b: &Path) -> Result<()> {
    if overlaps(a, b)
        || aw_platform::DirectoryIdentity::open(a)?.identity()?
            == aw_platform::DirectoryIdentity::open(b)?.identity()?
    {
        return Err(Error::Denied(
            "native source execution overlaps private host state",
        ));
    }
    Ok(())
}

impl Selection {
    /// The two CAS decisions are explicit operator inputs. Reopening a
    /// Store creates a new independent live policy, never restores a grant.
    pub(crate) fn activate(&self, store: SessionStore, input: Activation<'_>) -> Result<Activated> {
        let Activation {
            manager,
            session,
            branch,
            binding,
            settings,
            tools,
            account,
            pre_dispatch_retries,
        } = input;
        if self.grant_rev > i64::MAX as u64
            || self
                .open_rev
                .is_some_and(|rev| rev == 0 || rev > i64::MAX as u64)
        {
            return Err(Error::Denied(
                "native source startup requires valid exact revisions",
            ));
        }
        require_native_tools(tools.mode, &tools.selected_tools)?;
        sessions::check_binding(&binding, &session.binding, false)?;
        if tools.root != binding.tool_root
            || tools.execution_site != "local"
            || tools.process_input
            || tools.allow_agent_graph
            || tools.allow_wake
            || tools.operation_router.is_installed()?
        {
            return Err(Error::Denied(
                "native source startup cannot use another execution route",
            ));
        }
        let state = store
            .directory()
            .ok_or(Error::Denied("native source startup needs a private Store"))?;
        canonical_private_directory(state)?;
        canonical_private_directory(&self.root)?;
        require_disjoint(state, &self.root)?;
        if !matches!(
            store.session_target(manager, &session.id)?,
            aw_store::SessionExecutionTarget::Local { .. }
        ) {
            return Err(Error::Denied(
                "native source startup requires the existing local Session",
            ));
        }
        let info = store.agent_binding(manager, &session.id, branch)?;
        if info.session_id != session.id {
            return Err(Error::Denied(
                "native source startup requires the exact Session",
            ));
        }
        let reference = format!(
            "/d:{}/i:{}/p:{}/a:{}",
            info.domain_handle, info.identity_handle, info.project_handle, info.agent_handle
        );
        let execution = store.agent_execution_binding(manager, &reference)?;
        let writer = store.workspace_writer(manager, None, &session.workspace_id)?;
        store.actor_input_writer_fence(manager, &session.id, writer.writer_epoch)?;
        if writer.state != "active" {
            return Err(Error::Denied(
                "native source startup needs its active original writer",
            ));
        }
        let snapshot = store.agent_get(manager, &reference)?;
        if snapshot.lifecycle != AgentLifecycle::Open
            || self.open_rev.is_some() && !snapshot.caller_permissions.manage
            || self.open_rev.is_some_and(|rev| rev != snapshot.rev)
            || (snapshot.admission == AgentAdmission::Held && self.open_rev.is_none())
        {
            return Err(Error::Denied(
                "native source startup needs open admission or its explicit current open revision",
            ));
        }
        if store.sandbox_grant_status(manager, &reference)?.revision != self.grant_rev {
            return Err(Error::Conflict("stale Sandbox grant revision"));
        }
        let view = store.read_agent_branch_view(manager, &session.id, execution.branch_id())?;
        if view.branch.active_run.is_some() || view.history.needs_reconciliation {
            return Err(Error::Denied(
                "native source startup cannot bypass unresolved original execution",
            ));
        }

        let git = store.source_driver_handle::<GitSourceDriver>(&DriverKey::builtin("git")?)?;
        let resolver = GitNativeSourceResolver::new(store.clone(), git.clone())?;
        let user = store.current_user(manager)?.user_id;
        let (approval, actor) = resolver.provision_configured_approval_with(
            manager,
            &self.root,
            |sandbox, spec, sources| {
                if spec.agent_ref != reference
                    || snapshot.sandbox_ref.as_deref() != Some(sandbox.canonical_ref.as_str())
                    || sandbox.tool_root != binding.tool_root
                    || sandbox.working_directory != binding.working_directory
                    || spec.base != binding.tool_root
                    || sources.len() != sandbox.sources.len()
                    || (spec.actor != *manager && spec.actor != session.owner)
                    || store.current_user(&spec.actor)?.user_id != user
                {
                    return Err(Error::Denied(
                        "native approval differs from its exact local Actor and complete layout",
                    ));
                }
                store.actor_input_writer_fence(&spec.actor, &session.id, writer.writer_epoch)?;
                require_disjoint(state, &spec.base)?;
                require_disjoint(&self.root, &spec.base)?;
                if tools.selected_tools.iter().any(|name| {
                    Tool::from_name(name).is_some_and(|tool| tool.action() != Action::Read)
                }) && !spec
                    .directories
                    .iter()
                    .any(|directory| directory.access == SourceAccess::ReadWrite)
                {
                    return Err(Error::Denied(
                        "native source tool selection exceeds independent approval",
                    ));
                }
                Ok(spec.actor.clone())
            },
        )?;
        let selected = aw::configured_provider_selection_for_actor(
            settings,
            actor.clone(),
            store.clone(),
            account,
        )?;
        let registry = SandboxDriverRegistry::new();
        let native = registry.register(
            DriverKey::builtin("native-filesystem")?,
            NativeFilesystemDriver,
        )?;
        let host = GitNativeSourceHost::new(approval, git, registry, native)?;

        // Fresh live authority cannot match a previous process's grant.
        // Only the explicit CAS requested at startup can publish it.
        host.publish_grant(manager, self.grant_rev)?;
        let (mut agent, source) = host.build_execution(
            &store,
            actor.clone(),
            execution,
            Agent::builder_from_selection(selected.selection, selected.options),
            tools,
        )?;
        agent.set_pre_dispatch_retries(pre_dispatch_retries)?;
        if let Some(rev) = self.open_rev {
            store.agent_open_with_source(manager, rev, &source)?;
        }
        let (view, handle) = store.activate_with_source(&source)?;
        agent.load_history(view.history, Some(Arc::new(handle.clone())))?;
        let owner =
            aw::actor_owner::Service::attach_for_agent_with_source(store.clone(), &agent, source)?;
        let persistent = PersistentSession {
            store,
            actor,
            handle,
            binding,
            allow_transfer: false,
        };
        Ok(Activated {
            persistent,
            agent,
            owner,
            withdrawal: Arc::new(Withdrawal(host)),
        })
    }
}
