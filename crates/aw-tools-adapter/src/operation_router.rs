//! Host-owned execution routing for already-validated semantic operations.
//!
//! The adapter owns the original Tool implementation and factual result. A host
//! router may coordinate execution, but it never receives authority merely from
//! being installed. Local/standalone callers leave the slot empty.

use crate::{Error, Result};
use aw_agent_core::{Tool, ToolContext, ToolFuture, ToolSpec};
use aw_tools::OperationId;
use serde_json::Value;
use sha2::{Digest as _, Sha256};
use std::{
    collections::BTreeMap,
    future::Future,
    pin::Pin,
    sync::{Arc, RwLock},
};

/// Host-private route for one exact original semantic operation. Implementations
/// must revalidate all policy/delegate/payer/disclosure authority themselves.
pub trait OperationRouter: Send + Sync {
    fn execute<'a>(
        &'a self,
        operation: OperationId,
        ctx: ToolContext,
        arguments: &'a Value,
        original: Arc<dyn Tool>,
    ) -> ToolFuture<'a>;
}

#[derive(Clone)]
pub(crate) struct GraphHostToolBinding {
    tool: Arc<dyn Tool>,
    runtime_binding: [u8; 32],
}

impl GraphHostToolBinding {
    pub(crate) fn tool(&self) -> Arc<dyn Tool> {
        self.tool.clone()
    }

    pub(crate) fn runtime_binding(&self) -> [u8; 32] {
        self.runtime_binding
    }
}

#[derive(Default)]
struct GraphHostToolState {
    next_generation: u64,
    tools: BTreeMap<OperationId, (Arc<dyn Tool>, u64)>,
}

/// Late-bound host Graph extensions. Merely installing a tool grants nothing:
/// JobHost still requires the exact current ToolBindingAuthority generation.
/// Initial AW use installs only the real Agent constructor; standalone hosts
/// leave the slot empty.
pub struct GraphHostToolSlot {
    inner: RwLock<GraphHostToolState>,
}

impl Default for GraphHostToolSlot {
    fn default() -> Self {
        Self {
            inner: RwLock::new(GraphHostToolState::default()),
        }
    }
}

impl std::fmt::Debug for GraphHostToolSlot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let count = self
            .inner
            .read()
            .map(|state| state.tools.len())
            .unwrap_or(0);
        f.debug_struct("GraphHostToolSlot")
            .field("installed", &count)
            .finish()
    }
}

impl GraphHostToolSlot {
    fn fingerprint(operation: OperationId, generation: u64) -> [u8; 32] {
        let mut hash = Sha256::new();
        hash.update(b"aw-graph-host-tool-runtime-v1\0");
        hash.update(operation.semantic_name().as_bytes());
        hash.update(generation.to_be_bytes());
        hash.finalize().into()
    }

    /// Install one trusted host-owned original executor. The operation identity
    /// and cached ToolSpec name must agree. Nested Graph construction remains
    /// forbidden even for trusted hosts.
    pub fn install(&self, operation: OperationId, tool: Arc<dyn Tool>) -> Result<()> {
        if operation == OperationId::Batch {
            return Err(Error::Denied("nested batch is not a Graph host tool"));
        }
        let spec = tool.spec();
        if spec.name != operation.model_name() {
            return Err(Error::Denied(
                "Graph host tool name differs from semantic OperationId",
            ));
        }
        let mut state = self
            .inner
            .write()
            .map_err(|_| Error::Protocol("Graph host tool slot poisoned"))?;
        if state
            .tools
            .get(&operation)
            .is_some_and(|(existing, _)| Arc::ptr_eq(existing, &tool))
        {
            return Ok(());
        }
        state.next_generation = state
            .next_generation
            .checked_add(1)
            .ok_or(Error::Limit("Graph host tool generations exhausted"))?;
        let generation = state.next_generation;
        state.tools.insert(operation, (tool, generation));
        Ok(())
    }

    pub fn remove(&self, operation: OperationId) -> Result<()> {
        let mut state = self
            .inner
            .write()
            .map_err(|_| Error::Protocol("Graph host tool slot poisoned"))?;
        if state.tools.remove(&operation).is_some() {
            state.next_generation = state
                .next_generation
                .checked_add(1)
                .ok_or(Error::Limit("Graph host tool generations exhausted"))?;
        }
        Ok(())
    }

    pub(crate) fn binding(&self, operation: OperationId) -> Result<Option<GraphHostToolBinding>> {
        let state = self
            .inner
            .read()
            .map_err(|_| Error::Protocol("Graph host tool slot poisoned"))?;
        Ok(state
            .tools
            .get(&operation)
            .map(|(tool, generation)| GraphHostToolBinding {
                tool: tool.clone(),
                runtime_binding: Self::fingerprint(operation, *generation),
            }))
    }
}

pub(crate) fn validate_preseal_graph_revision(original: &Value, revised: &Value) -> Result<()> {
    let original_object = original
        .as_object()
        .ok_or(Error::Denied("original Graph admission is not an object"))?;
    let revised_object = revised
        .as_object()
        .ok_or(Error::Denied("revised Graph admission is not an object"))?;
    if original_object.len() != 1
        || revised_object.len() != 1
        || !original_object.contains_key("ops")
        || !revised_object.contains_key("ops")
    {
        return Err(Error::Denied(
            "pre-seal Graph revision may change arguments only",
        ));
    }
    let original_ops = original_object["ops"]
        .as_array()
        .ok_or(Error::Denied("original Graph ops are invalid"))?;
    let revised_ops = revised_object["ops"]
        .as_array()
        .ok_or(Error::Denied("revised Graph ops are invalid"))?;
    if original_ops.len() != revised_ops.len() || original_ops.is_empty() {
        return Err(Error::Denied(
            "pre-seal Graph revision cannot change node count",
        ));
    }
    for (original_node, revised_node) in original_ops.iter().zip(revised_ops) {
        let original_node = original_node
            .as_array()
            .ok_or(Error::Denied("original Graph node is invalid"))?;
        let revised_node = revised_node
            .as_array()
            .ok_or(Error::Denied("revised Graph node is invalid"))?;
        if original_node.len() != revised_node.len()
            || !(2..=3).contains(&original_node.len())
            || original_node[0] != revised_node[0]
            || !revised_node[1].is_object()
            || (original_node.len() == 3 && original_node[2] != revised_node[2])
        {
            return Err(Error::Denied(
                "pre-seal Graph revision cannot change tools/topology/when",
            ));
        }
    }
    Ok(())
}

pub type GraphPresealFuture<'a> =
    Pin<Box<dyn Future<Output = Result<GraphPresealDecision>> + Send + 'a>>;

#[derive(Clone, Debug, PartialEq)]
pub struct GraphPresealNode {
    pub coordinate: u32,
    pub operation: OperationId,
    pub arguments: Value,
    pub replay_identity: [u8; 32],
    pub footprint: aw_tools::filesystem::footprint::Footprint,
    pub dependencies: Vec<u32>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GraphPresealRequest {
    pub job_id: String,
    pub original_graph: Value,
    pub graph_fingerprint: [u8; 32],
    pub nodes: Vec<GraphPresealNode>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GraphFootprintOverride {
    pub coordinate: u32,
    pub footprint: aw_tools::filesystem::footprint::Footprint,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GraphPresealDecision {
    pub graph: Value,
    pub footprint_overrides: Vec<GraphFootprintOverride>,
}

impl GraphPresealDecision {
    pub fn unchanged(request: &GraphPresealRequest) -> Self {
        Self {
            graph: request.original_graph.clone(),
            footprint_overrides: Vec::new(),
        }
    }
}

pub type GraphPolicyFuture<'a> = Pin<Box<dyn Future<Output = Result<()>> + Send + 'a>>;
pub type GraphNodeExecutionFuture<'a> =
    Pin<Box<dyn Future<Output = Result<aw_tools::filesystem::batch::Status>> + Send + 'a>>;

pub trait GraphNodeOriginalExecutor: Send + Sync {
    fn execute<'a>(&'a self) -> GraphNodeExecutionFuture<'a>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GraphPolicyNode {
    pub coordinate: u32,
    pub operation: OperationId,
    pub replay_identity: [u8; 32],
    pub footprint_fingerprint: [u8; 32],
    pub dependencies: Vec<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GraphPolicyRequest {
    pub job_id: String,
    pub graph_fingerprint: [u8; 32],
    pub nodes: Vec<GraphPolicyNode>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GraphDependencyReceipt {
    pub coordinate: u32,
    pub status: aw_tools::filesystem::batch::Status,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GraphNodePolicyRequest {
    pub job_id: String,
    pub graph_fingerprint: [u8; 32],
    pub coordinate: u32,
    pub operation: OperationId,
    pub replay_identity: [u8; 32],
    pub dependency_receipts: Vec<GraphDependencyReceipt>,
}

/// Host policy around one already sealed Graph. No hook may mutate topology,
/// prepared arguments, when-expressions, NodePlan authority or JobManager state.
pub trait GraphPolicyRouter: Send + Sync {
    /// Admission-time policy over a side-effect-free, fully validated draft.
    /// Returned arguments are recompiled/revalidated by JobHost before sealing.
    fn before_seal<'a>(
        &'a self,
        _ctx: &'a ToolContext,
        request: GraphPresealRequest,
    ) -> GraphPresealFuture<'a> {
        Box::pin(async move { Ok(GraphPresealDecision::unchanged(&request)) })
    }

    /// Drop only transient live admission state after a pre-seal failure.
    /// Durable policy/replay evidence remains authoritative.
    fn abandon_preseal(&self, _job_id: &str) -> Result<()> {
        Ok(())
    }

    fn before_run<'a>(
        &'a self,
        _ctx: &'a ToolContext,
        _request: GraphPolicyRequest,
    ) -> GraphPolicyFuture<'a> {
        Box::pin(async { Ok(()) })
    }

    fn before_node<'a>(
        &'a self,
        _ctx: &'a ToolContext,
        _request: GraphNodePolicyRequest,
    ) -> GraphPolicyFuture<'a> {
        Box::pin(async { Ok(()) })
    }

    fn execute_node<'a>(
        &'a self,
        ctx: &'a ToolContext,
        request: GraphNodePolicyRequest,
        original: Arc<dyn GraphNodeOriginalExecutor>,
    ) -> GraphNodeExecutionFuture<'a> {
        Box::pin(async move {
            self.before_node(ctx, request).await?;
            original.execute().await
        })
    }
}

pub struct GraphPolicyRouterSlot {
    inner: RwLock<Option<Arc<dyn GraphPolicyRouter>>>,
}

impl Default for GraphPolicyRouterSlot {
    fn default() -> Self {
        Self {
            inner: RwLock::new(None),
        }
    }
}

impl std::fmt::Debug for GraphPolicyRouterSlot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let installed = self
            .inner
            .read()
            .map(|guard| guard.is_some())
            .unwrap_or(true);
        f.debug_struct("GraphPolicyRouterSlot")
            .field("installed", &installed)
            .finish()
    }
}

impl GraphPolicyRouterSlot {
    pub fn replace(&self, router: Arc<dyn GraphPolicyRouter>) -> Result<()> {
        let mut guard = self
            .inner
            .write()
            .map_err(|_| Error::Protocol("Graph policy router slot poisoned"))?;
        *guard = Some(router);
        Ok(())
    }

    pub fn clear(&self) -> Result<()> {
        let mut guard = self
            .inner
            .write()
            .map_err(|_| Error::Protocol("Graph policy router slot poisoned"))?;
        *guard = None;
        Ok(())
    }

    pub(crate) fn current(&self) -> Result<Option<Arc<dyn GraphPolicyRouter>>> {
        Ok(self
            .inner
            .read()
            .map_err(|_| Error::Protocol("Graph policy router slot poisoned"))?
            .clone())
    }
}

/// Stable composition slot shared by Agent rebuilds. The slot is not a grant.
pub struct OperationRouterSlot {
    inner: RwLock<Option<Arc<dyn OperationRouter>>>,
}

impl Default for OperationRouterSlot {
    fn default() -> Self {
        Self {
            inner: RwLock::new(None),
        }
    }
}

impl std::fmt::Debug for OperationRouterSlot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let installed = self
            .inner
            .read()
            .map(|guard| guard.is_some())
            .unwrap_or(true);
        f.debug_struct("OperationRouterSlot")
            .field("installed", &installed)
            .finish()
    }
}

impl OperationRouterSlot {
    pub fn replace(&self, router: Arc<dyn OperationRouter>) -> Result<()> {
        let mut guard = self
            .inner
            .write()
            .map_err(|_| Error::Protocol("operation router slot poisoned"))?;
        *guard = Some(router);
        Ok(())
    }

    pub fn clear(&self) -> Result<()> {
        let mut guard = self
            .inner
            .write()
            .map_err(|_| Error::Protocol("operation router slot poisoned"))?;
        *guard = None;
        Ok(())
    }

    fn current(&self) -> Result<Option<Arc<dyn OperationRouter>>> {
        Ok(self
            .inner
            .read()
            .map_err(|_| Error::Protocol("operation router slot poisoned"))?
            .clone())
    }
}

/// Transparent tool wrapper. Schema, validation, liveness and conservative
/// parallelism remain those of the original tool; only actual execution may be
/// coordinated by the host.
pub(crate) struct RoutedTool {
    original: Arc<dyn Tool>,
    operation: OperationId,
    spec: ToolSpec,
    router: Arc<OperationRouterSlot>,
}

impl RoutedTool {
    pub(crate) fn new(original: Arc<dyn Tool>, router: Arc<OperationRouterSlot>) -> Result<Self> {
        let spec = original.spec();
        let operation = OperationId::from_model_name(&spec.name).ok_or(Error::Denied(
            "registered local tool has no canonical OperationId",
        ))?;
        Ok(Self {
            original,
            operation,
            spec,
            router,
        })
    }
}

/// Apply the same semantic execution router to a host-registered built-in
/// tool such as `agent`. The wrapped tool keeps its exact schema/effect and the
/// host must still grant/register it normally.
pub fn routed_host_tool(
    tool: Arc<dyn Tool>,
    router: Arc<OperationRouterSlot>,
) -> Result<Arc<dyn Tool>> {
    Ok(Arc::new(RoutedTool::new(tool, router)?))
}

impl Tool for RoutedTool {
    fn spec(&self) -> ToolSpec {
        self.spec.clone()
    }

    fn is_available(&self) -> bool {
        self.original.is_available()
    }

    fn validate(&self, arguments: &Value) -> Result<()> {
        self.original.validate(arguments)
    }

    fn can_run_parallel(&self, first: &Value, second: &Value) -> bool {
        self.original.can_run_parallel(first, second)
    }

    fn execute<'a>(&'a self, ctx: ToolContext, arguments: &'a Value) -> ToolFuture<'a> {
        let original = self.original.clone();
        let router = self.router.clone();
        let operation = self.operation;
        Box::pin(async move {
            if operation == OperationId::Batch && ctx.execution_scope.is_some() {
                return Err(Error::Denied(
                    "batch cannot start while this work is owned by an active Graph node",
                ));
            }
            match router.current()? {
                Some(router) => router.execute(operation, ctx, arguments, original).await,
                None => original.execute(ctx, arguments).await,
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aw_agent_core::{
        AccountOwner, BillingOwner, CancellationToken, CredentialCustody, InvocationIdentity,
        PrincipalId, ToolEffect, ToolOutput, ToolSource,
    };
    use serde_json::json;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Echo {
        calls: Arc<AtomicUsize>,
    }
    impl Tool for Echo {
        fn spec(&self) -> ToolSpec {
            ToolSpec {
                name: "read".into(),
                description: "fixture".into(),
                parameters: json!({"type":"object","properties":{"value":{"type":"string"}},"required":["value"],"additionalProperties":false}),
                effect: ToolEffect::Read,
            }
        }
        fn validate(&self, arguments: &Value) -> Result<()> {
            arguments
                .get("value")
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
                .map(|_| ())
                .ok_or(Error::Denied("fixture needs value"))
        }
        fn can_run_parallel(&self, first: &Value, second: &Value) -> bool {
            first.get("value") != second.get("value")
        }
        fn execute<'a>(&'a self, _ctx: ToolContext, arguments: &'a Value) -> ToolFuture<'a> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Box::pin(async move {
                Ok(aw_agent_core::ToolOutput::text(
                    arguments["value"].as_str().unwrap_or_default(),
                ))
            })
        }
    }

    struct Router {
        calls: Arc<AtomicUsize>,
        seen: Arc<std::sync::Mutex<Option<OperationId>>>,
    }
    impl OperationRouter for Router {
        fn execute<'a>(
            &'a self,
            operation: OperationId,
            ctx: ToolContext,
            arguments: &'a Value,
            original: Arc<dyn Tool>,
        ) -> ToolFuture<'a> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            *self.seen.lock().unwrap() = Some(operation);
            Box::pin(async move { original.execute(ctx, arguments).await })
        }
    }

    fn context() -> ToolContext {
        let actor = PrincipalId::new("local").unwrap();
        ToolContext {
            actor: actor.clone(),
            turn_id: 1,
            call_id: "call-route".into(),
            source: ToolSource::trusted_host(),
            effect: ToolEffect::Read,
            execution_site: "local".into(),
            cancellation: CancellationToken::new(),
            result_budget_bytes: aw_agent_core::tool::MAX_TOOL_RESULT_BYTES,
            identity: InvocationIdentity {
                actor: actor.clone(),
                account_id: "fixture".into(),
                account_owner: AccountOwner::User(actor.clone()),
                payer: BillingOwner::User(actor),
                credential_custody: CredentialCustody::UserDevice("fixture".into()),
                credential_version: None,
                inference_executor: "local".into(),
                model: "fixture".into(),
                grant_revision: 1,
            },
            scope: None,
            execution_owner: Some(
                aw_agent_core::AgentExecutionBinding::from_trusted_host(
                    "/d:0/i:0/p:0/a:0",
                    "fixture-session",
                    "main",
                )
                .unwrap(),
            ),
            execution_scope: None,
        }
    }

    #[tokio::test]
    async fn empty_router_slot_preserves_exact_local_executor() {
        let original_calls = Arc::new(AtomicUsize::new(0));
        let original: Arc<dyn Tool> = Arc::new(Echo {
            calls: original_calls.clone(),
        });
        let routed =
            RoutedTool::new(original.clone(), Arc::new(OperationRouterSlot::default())).unwrap();
        let args = json!({"value":"local"});
        let out = routed.execute(context(), &args).await.unwrap();
        assert_eq!(out.content, "local");
        assert_eq!(original_calls.load(Ordering::SeqCst), 1);
        assert_eq!(routed.spec().name, original.spec().name);
        assert_eq!(routed.spec().effect, original.spec().effect);
        assert_eq!(routed.spec().parameters, original.spec().parameters);
        assert!(routed.is_available());
        routed.validate(&args).unwrap();
        assert!(routed.can_run_parallel(&args, &json!({"value":"other"})));
        assert!(!routed.can_run_parallel(&args, &args));
    }

    struct BatchEcho {
        calls: Arc<AtomicUsize>,
    }

    impl Tool for BatchEcho {
        fn spec(&self) -> ToolSpec {
            ToolSpec {
                name: "batch".into(),
                description: "fixture batch".into(),
                parameters: json!({"type":"object","additionalProperties":false}),
                effect: ToolEffect::Control,
            }
        }

        fn validate(&self, arguments: &Value) -> Result<()> {
            if arguments.as_object().is_some_and(serde_json::Map::is_empty) {
                Ok(())
            } else {
                Err(Error::Denied("fixture batch arguments"))
            }
        }

        fn execute<'a>(&'a self, _ctx: ToolContext, _arguments: &'a Value) -> ToolFuture<'a> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Box::pin(async { Ok(ToolOutput::text("unexpected")) })
        }
    }

    #[tokio::test]
    async fn graph_owned_batch_rejects_before_delegate_router_or_original_executor() {
        let original_calls = Arc::new(AtomicUsize::new(0));
        let router_calls = Arc::new(AtomicUsize::new(0));
        let seen = Arc::new(std::sync::Mutex::new(None));
        let slot = Arc::new(OperationRouterSlot::default());
        slot.replace(Arc::new(Router {
            calls: router_calls.clone(),
            seen,
        }))
        .unwrap();
        let routed = RoutedTool::new(
            Arc::new(BatchEcho {
                calls: original_calls.clone(),
            }),
            slot,
        )
        .unwrap();
        let mut ctx = context();
        ctx.execution_scope = Some(
            aw_agent_core::ExecutionOwnershipScope::from_trusted_host(
                "/d:0/i:0/p:0/a:0/t:1/j:g/n:1",
                CancellationToken::new(),
            )
            .unwrap(),
        );
        let result = routed.execute(ctx, &json!({})).await;
        assert!(matches!(
            result,
            Err(Error::Denied(
                "batch cannot start while this work is owned by an active Graph node"
            ))
        ));
        assert_eq!(router_calls.load(Ordering::SeqCst), 0);
        assert_eq!(original_calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn installed_router_receives_exact_semantic_id_and_original_once() {
        let original_calls = Arc::new(AtomicUsize::new(0));
        let router_calls = Arc::new(AtomicUsize::new(0));
        let seen = Arc::new(std::sync::Mutex::new(None));
        let slot = Arc::new(OperationRouterSlot::default());
        slot.replace(Arc::new(Router {
            calls: router_calls.clone(),
            seen: seen.clone(),
        }))
        .unwrap();
        let routed = RoutedTool::new(
            Arc::new(Echo {
                calls: original_calls.clone(),
            }),
            slot.clone(),
        )
        .unwrap();
        let args = json!({"value":"routed"});
        let out = routed.execute(context(), &args).await.unwrap();
        assert_eq!(out.content, "routed");
        assert_eq!(router_calls.load(Ordering::SeqCst), 1);
        assert_eq!(original_calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            *seen.lock().unwrap(),
            Some(OperationId::Fs(aw_tools::filesystem::schema::Tool::Read))
        );

        slot.clear().unwrap();
        routed.execute(context(), &args).await.unwrap();
        assert_eq!(router_calls.load(Ordering::SeqCst), 1);
        assert_eq!(original_calls.load(Ordering::SeqCst), 2);
    }
}
