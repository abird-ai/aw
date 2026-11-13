//! Host-injected Agent/Turn routing for the unified jobs_* surface.
//!
//! The adapter owns finite Process/Graph Jobs. AW may inject one authoritative
//! Session/Actor/Turn router for Agent refs. The slot is only composition state:
//! it carries no authority and every request must revalidate the supplied
//! ToolContext at the authoritative owner.

use crate::{Error, Result};
use aw_agent_core::{ToolContext, ToolOutput};
use aw_tools::job_event::{EventRuleSet, JobEventDelivery};
use serde_json::Value;
use std::{
    future::Future,
    pin::Pin,
    sync::{Arc, RwLock},
};

pub type AgentJobFuture<'a> = Pin<Box<dyn Future<Output = Result<Option<ToolOutput>>> + Send + 'a>>;
pub type AgentJobListFuture<'a> =
    Pin<Box<dyn Future<Output = Result<Option<AgentJobListPage>>> + Send + 'a>>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FiniteExposureOwner {
    pub canonical_agent_ref: String,
    pub actor: String,
    pub session_id: String,
    pub branch_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentJobListRequest {
    pub cursor: Option<String>,
    pub limit: usize,
    pub authority_fingerprint: [u8; 32],
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentJobListItem {
    pub reference: String,
    /// Cursor that resumes strictly after this exact root.
    pub after_cursor: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentJobListPage {
    pub items: Vec<AgentJobListItem>,
    /// Present only when additional roots exist after the returned page.
    pub next_cursor: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentJobGet {
    pub reference: String,
    pub revision: Option<u64>,
    pub wait_ms: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AgentJobRead {
    pub reference: String,
    pub view: Option<String>,
    pub filter: Option<Value>,
    pub cursor: Option<String>,
    pub limit: usize,
    pub wait_ms: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgentSendMode {
    Auto,
    Queue,
    Steer,
    Interrupt,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AgentSendInput {
    Text(String),
    Eof,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentJobSend {
    pub reference: String,
    pub revision: Option<u64>,
    pub mode: AgentSendMode,
    pub input: AgentSendInput,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentJobCancel {
    pub reference: String,
    pub revision: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentJobSubscribe {
    pub reference: String,
    pub revision: Option<u64>,
    pub rules: EventRuleSet,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentWakeRecipient {
    pub canonical_ref: String,
    /// Opaque host-issued admission identity bound to the original
    /// participant, membership/audience epoch and exact jobs_subscribe grant.
    pub authority_ref: String,
}

pub type AgentEventFuture<'a> = Pin<Box<dyn Future<Output = Result<()>> + Send + 'a>>;

/// AW-hosted authoritative Agent/Turn router. Returning Ok(None) means the
/// reference is not an Agent/Turn resource owned by this router and the adapter
/// may continue with finite-Job resolution.
pub trait AgentJobRouter: Send + Sync {
    fn reserve_finite(
        &self,
        _owner: &FiniteExposureOwner,
        _canonical_ref: &str,
        _capacity: usize,
    ) -> Result<()> {
        Ok(())
    }
    fn publish_finite(&self, _owner: &FiniteExposureOwner, _canonical_ref: &str) -> Result<()> {
        Ok(())
    }
    fn release_unpublished_finite(
        &self,
        _owner: &FiniteExposureOwner,
        _canonical_ref: &str,
    ) -> Result<()> {
        Ok(())
    }
    fn list<'a>(
        &'a self,
        _ctx: &'a ToolContext,
        _request: AgentJobListRequest,
    ) -> AgentJobListFuture<'a> {
        Box::pin(async { Ok(None) })
    }

    /// Resolve one control target exactly once on first admission. Returning
    /// None means the ref belongs to the finite Job owner instead.
    fn resolve_control_ref(&self, _ctx: &ToolContext, _reference: &str) -> Result<Option<String>> {
        Ok(None)
    }

    fn wake_recipient(&self, _ctx: &ToolContext) -> Result<Option<AgentWakeRecipient>> {
        Ok(None)
    }

    fn revalidate_wake_recipient(
        &self,
        _ctx: &ToolContext,
        _recipient: &AgentWakeRecipient,
    ) -> Result<()> {
        Err(Error::Denied("Agent wake recipient is unavailable"))
    }

    fn get<'a>(&'a self, ctx: &'a ToolContext, request: AgentJobGet) -> AgentJobFuture<'a>;
    fn read<'a>(&'a self, ctx: &'a ToolContext, request: AgentJobRead) -> AgentJobFuture<'a>;
    fn send<'a>(&'a self, ctx: &'a ToolContext, request: AgentJobSend) -> AgentJobFuture<'a>;
    fn cancel<'a>(&'a self, ctx: &'a ToolContext, request: AgentJobCancel) -> AgentJobFuture<'a>;

    fn subscribe<'a>(
        &'a self,
        _ctx: &'a ToolContext,
        _request: AgentJobSubscribe,
    ) -> AgentJobFuture<'a> {
        Box::pin(async { Ok(None) })
    }

    /// Recipient-side admission for a source-owned factual event. The source
    /// commit already happened; rejection here must never rewrite it.
    fn admit_event<'a>(&'a self, _delivery: JobEventDelivery) -> AgentEventFuture<'a> {
        Box::pin(async { Err(Error::Denied("Agent wake recipient is unavailable")) })
    }
}

/// Stable host composition slot shared across Agent rebuilds. Replacing the
/// router does not authorize anything: the newly installed router still has to
/// authenticate/authorize every ToolContext and target itself.
pub struct AgentJobRouterSlot {
    inner: RwLock<Option<Arc<dyn AgentJobRouter>>>,
}

impl Default for AgentJobRouterSlot {
    fn default() -> Self {
        Self {
            inner: RwLock::new(None),
        }
    }
}

impl std::fmt::Debug for AgentJobRouterSlot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let installed = self
            .inner
            .read()
            .map(|guard| guard.is_some())
            .unwrap_or(true);
        f.debug_struct("AgentJobRouterSlot")
            .field("installed", &installed)
            .finish()
    }
}

impl AgentJobRouterSlot {
    pub fn replace(&self, router: Arc<dyn AgentJobRouter>) -> Result<()> {
        let mut guard = self
            .inner
            .write()
            .map_err(|_| Error::Protocol("Agent Job router slot poisoned"))?;
        *guard = Some(router);
        Ok(())
    }

    pub fn clear(&self) -> Result<()> {
        let mut guard = self
            .inner
            .write()
            .map_err(|_| Error::Protocol("Agent Job router slot poisoned"))?;
        *guard = None;
        Ok(())
    }

    pub fn is_installed(&self) -> Result<bool> {
        Ok(self
            .inner
            .read()
            .map_err(|_| Error::Protocol("Agent Job router slot poisoned"))?
            .is_some())
    }

    pub(crate) fn current(&self) -> Result<Option<Arc<dyn AgentJobRouter>>> {
        Ok(self
            .inner
            .read()
            .map_err(|_| Error::Protocol("Agent Job router slot poisoned"))?
            .clone())
    }
}
