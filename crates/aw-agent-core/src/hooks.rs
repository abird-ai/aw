//! Nonblocking typed hook *service* over Runtime/Message, not another
//! authority engine. An asynchronous hook returns Defer and the host later
//! resumes its one-shot continuation; callbacks themselves must not block.
use crate::extension::{Extension, HookContext, HookEvent};
use crate::{Error, Result};
use aw_agent_kernel::{
    actor::ActorKey,
    hook_contract::{authorize_stage, Capability, Continuation, HookFlow, HookOutcome, Stage},
    message::{ActorMessage, ControlKind, Kind},
};
use futures_util::FutureExt;
use std::sync::{
    atomic::{AtomicU64, AtomicUsize, Ordering},
    Arc,
};
use std::{panic::AssertUnwindSafe, time::Duration};

/// The ONLY authority that can mint and resume a deferred hosted Agent
/// callback. The owner is the same HostRuntime as Actor/Message/Envelope;
/// direct Agent calls do not get this port and cannot mint continuations.
pub trait HookContinuationPort: Send + Sync {
    fn reserve(
        &self,
        turn_id: u64,
        stage: Stage,
    ) -> Result<(
        Continuation,
        tokio::sync::oneshot::Receiver<HookFlow<HookEvent>>,
    )>;
    fn abandon(&self, token: Continuation);
    fn verify(&self, token: Continuation) -> Result<()>;
}
struct Reservation<'a> {
    port: &'a dyn HookContinuationPort,
    token: Continuation,
    receiver: Option<tokio::sync::oneshot::Receiver<HookFlow<HookEvent>>>,
}
impl Drop for Reservation<'_> {
    fn drop(&mut self) {
        self.port.abandon(self.token);
    }
}
pub const MAX_HOOKS: usize = 64;
pub const MAX_HOOK_DEPTH: usize = 8;
struct HookDispatchGuard<'a>(&'a AtomicUsize);
impl Drop for HookDispatchGuard<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HookId(u64);
/// Only source-neutral, secret-free metadata. No credential, model account,
/// direct ToolPolicy mutation or durable receipt is exposed here.
#[derive(Clone, Copy, Debug)]
pub struct Context {
    pub stage: Stage,
    pub actor: ActorKey,
    pub envelope_revision: u64,
    /// Host-minted token, valid only if this handler defers the current item.
    pub continuation: Continuation,
}
pub trait RuntimeHook: Send + Sync {
    fn handle(&self, ctx: Context, message: &ActorMessage) -> HookOutcome<ActorMessage>;
}
pub struct Registration {
    pub owner: String,
    pub stage: Stage,
    pub capability: Capability,
    pub priority: i32,
    pub handler: Arc<dyn RuntimeHook>,
}
struct Entry {
    id: HookId,
    owner: String,
    stage: Stage,
    capability: Capability,
    priority: i32,
    handler: Arc<dyn RuntimeHook>,
}
pub enum Dispatch {
    Next(Option<ActorMessage>),
    Deferred {
        message: ActorMessage,
        next_index: usize,
    },
}
/// One bounded ordered registry; registration changes are generation-fenced
/// by version. The owning runtime rejects stale deferred continuations.
#[derive(Default)]
pub struct HookPipeline {
    handlers: Vec<Entry>,
    version: u64,
    sequence: u64,
    /// Observe-only errors must remain visible as diagnostics, never block
    /// factual execution or become a writable journal.
    observe_failures: AtomicU64,
    active_dispatches: AtomicUsize,
}
impl HookPipeline {
    pub fn version(&self) -> u64 {
        self.version
    }
    pub fn observe_failures(&self) -> u64 {
        self.observe_failures.load(Ordering::Relaxed)
    }
    pub fn register(&mut self, spec: Registration) -> Result<HookId> {
        authorize_stage(spec.stage, spec.capability)
            .map_err(|_| Error::Denied("observational lifecycle/event hooks cannot mutate"))?;
        if spec.owner.is_empty()
            || spec.owner.len() > 128
            || !spec
                .owner
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_.:-".contains(&b))
        {
            return Err(Error::Config("invalid Runtime hook owner".into()));
        }
        if self.handlers.len() >= MAX_HOOKS {
            return Err(Error::Limit("too many Runtime hooks"));
        }
        let id = self
            .sequence
            .checked_add(1)
            .ok_or(Error::Limit("hook sequence exhausted"))?;
        let version = self
            .version
            .checked_add(1)
            .ok_or(Error::Limit("hook registry revision exhausted"))?;
        self.sequence = id;
        self.version = version;
        let key = HookId(id);
        self.handlers.push(Entry {
            id: key,
            owner: spec.owner,
            stage: spec.stage,
            capability: spec.capability,
            priority: spec.priority,
            handler: spec.handler,
        });
        Ok(key)
    }
    pub fn unregister(&mut self, key: HookId) -> bool {
        let Some(index) = self.handlers.iter().position(|x| x.id == key) else {
            return false;
        };
        let Some(next) = self.version.checked_add(1) else {
            return false;
        };
        self.handlers.remove(index);
        self.version = next;
        true
    }
    pub fn has(&self, stage: Stage) -> bool {
        self.handlers.iter().any(|x| x.stage == stage)
    }
    /// A returned transformed message may change only its untrusted payload.
    /// Actor identity, target, control kind, priority and sequence are pinned.
    pub(crate) fn same_address(a: &ActorMessage, b: &ActorMessage) -> bool {
        a.id == b.id
            && a.from == b.from
            && a.to == b.to
            && a.kind == b.kind
            && a.turn == b.turn
            && a.priority == b.priority
            && a.source_sequence == b.source_sequence
    }
    pub fn dispatch(&self, ctx: Context, msg: ActorMessage, from_index: usize) -> Result<Dispatch> {
        // This admission path is normally serialized by its Runtime owner,
        // but cap reentry and malicious synchronous recursion explicitly.
        self.active_dispatches
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| {
                (n < MAX_HOOK_DEPTH).then_some(n + 1)
            })
            .map_err(|_| Error::Limit("Runtime hook recursion/concurrency ceiling"))?;
        let _depth = HookDispatchGuard(&self.active_dispatches);
        // Snapshot stable order; a handler is invoked without holding mutable
        // registration access. Owner teardown invalidates deferred version.
        let mut ordered: Vec<&Entry> = self
            .handlers
            .iter()
            .filter(|e| e.stage == ctx.stage)
            .collect();
        ordered.sort_by_key(|e| (e.priority, e.id.0));
        if from_index > ordered.len() {
            return Err(Error::Denied("stale hook continuation position"));
        }
        let mut current = msg;
        for (i, e) in ordered.into_iter().enumerate().skip(from_index) {
            // A panicking host hook may not escape admission and leave an
            // unverifiable partial decision. Observe fails open; gates deny.
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                e.handler.handle(ctx, &current)
            }))
            .unwrap_or(Err(aw_agent_kernel::hook_contract::Error::Denied));
            // Authoritative, already authenticated cancellation is never
            // optional-hook-vetoable. A plugin can observe it but must not
            // swallow, defer or edit it (even by failing or panicking).
            if matches!(current.kind, Kind::Control(ControlKind::Cancel(_))) {
                if !matches!(&result, Ok(HookFlow::Next(Some(m))) if *m == current) {
                    self.observe_failures.fetch_add(1, Ordering::Relaxed);
                }
                continue;
            }
            match result {
                Ok(HookFlow::Next(Some(next))) => {
                    if e.capability == Capability::Observe {
                        if next != current {
                            self.observe_failures.fetch_add(1, Ordering::Relaxed);
                        }
                        continue;
                    }
                    if !Self::same_address(&current, &next) {
                        return Err(Error::Denied("hook attempted to rewrite Actor authority"));
                    }
                    if next.payload.len() > aw_agent_kernel::message::MAX_PAYLOAD_BYTES {
                        return Err(Error::Limit(
                            "hook transformed payload exceeds message ceiling",
                        ));
                    }
                    current = next;
                }
                Ok(HookFlow::Next(None)) => {
                    if e.capability == Capability::Observe {
                        self.observe_failures.fetch_add(1, Ordering::Relaxed);
                        continue;
                    }
                    return Ok(Dispatch::Next(None));
                }
                Ok(HookFlow::Defer(token)) => {
                    if e.capability == Capability::Observe {
                        self.observe_failures.fetch_add(1, Ordering::Relaxed);
                        continue;
                    }
                    if token != ctx.continuation {
                        return Err(Error::Denied("hook returned a forged continuation"));
                    }
                    return Ok(Dispatch::Deferred {
                        message: current,
                        next_index: i + 1,
                    });
                }
                Err(_) => {
                    if e.capability == Capability::Observe {
                        self.observe_failures.fetch_add(1, Ordering::Relaxed);
                        continue;
                    }
                    return Err(Error::Denied("runtime hook gate rejected message"));
                }
            }
        }
        Ok(Dispatch::Next(Some(current)))
    }
    pub fn names(&self) -> Vec<String> {
        self.handlers.iter().map(|e| e.owner.clone()).collect()
    }
}
/// Single ordered async extension chain for Agent prompt/tool/decision views.
/// Async process callbacks return their *completed* verdict as Next/None;
/// explicit Defer is ONLY managed by the owning Actor Runtime's one-shot
/// continuation service, never by this stateless Agent evaluator.
async fn dispatch_report(
    extensions: &[Arc<dyn Extension>],
    ctx: HookContext<'_>,
    mut event: HookEvent,
    per_hook_timeout: Duration,
    port: Option<&dyn HookContinuationPort>,
) -> Result<(HookEvent, u64)> {
    if extensions.is_empty() {
        return Ok((event, 0));
    }
    let original = event.clone();
    let observe = matches!(original, HookEvent::Observation { .. });
    let mut failures = 0u64;
    // Bound the entire ordered chain in addition to each handler. A large
    // explicit extension set cannot multiply a provider/tool gate indefinitely.
    // Factual lifecycle observers fail open with counted diagnostics.
    let budget = per_hook_timeout
        .saturating_mul(4)
        .min(Duration::from_secs(60));
    let chain_deadline = tokio::time::Instant::now() + budget;
    for extension in extensions {
        let interested =
            std::panic::catch_unwind(AssertUnwindSafe(|| extension.wants_stage(original.stage())));
        match interested {
            Ok(false) => continue,
            Ok(true) => {}
            Err(_) => {
                if observe {
                    failures = failures.saturating_add(1);
                    continue;
                }
                return Err(Error::Denied("hook stage-interest probe panicked"));
            }
        }
        let remaining = chain_deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            if observe {
                failures = failures.saturating_add(1);
                continue;
            }
            return Err(Error::Timeout);
        }
        let mut reservation = if !observe {
            port.map(|owner| {
                owner
                    .reserve(ctx.turn_id, original.stage())
                    .map(|(token, receiver)| Reservation {
                        port: owner,
                        token,
                        receiver: Some(receiver),
                    })
            })
            .transpose()?
        } else {
            None
        };
        let mut handler_ctx = ctx;
        handler_ctx.continuation = reservation.as_ref().map(|r| r.token);
        let response = AssertUnwindSafe(tokio::time::timeout(
            per_hook_timeout.min(remaining),
            extension.on_hook(handler_ctx, event.clone()),
        ))
        .catch_unwind()
        .await;
        if observe {
            // Informational notifications cannot deny a completed Turn,
            // change factual receipts or block mandatory cancellation.
            // No private content is passed into this read-only projection.
            let valid = matches!(response,
                Ok(Ok(Ok(HookFlow::Next(Some(ref next)))))
                if HookEvent::validate_next(&original,next).is_ok());
            if !valid {
                failures = failures.saturating_add(1);
            }
            continue;
        }
        let flow = response
            .map_err(|_| Error::Denied("trusted Rust hook panicked"))?
            .map_err(|_| Error::Timeout)??;
        match flow {
            HookFlow::Next(Some(next)) => {
                HookEvent::validate_next(&original, &next)?;
                event = next;
            }
            HookFlow::Next(None) => return Err(Error::Denied("hosted hook vetoed this proposal")),
            HookFlow::Defer(returned) => {
                let Some(ref mut reservation) = reservation else {
                    return Err(Error::Denied(
                        "deferred hook requires authenticated owning Runtime",
                    ));
                };
                if returned != reservation.token {
                    return Err(Error::Denied("hook returned a forged Runtime continuation"));
                }
                let receiver = reservation.receiver.take().expect("fresh reservation");
                let remaining =
                    chain_deadline.saturating_duration_since(tokio::time::Instant::now());
                if remaining.is_zero() {
                    return Err(Error::Timeout);
                }
                let replied = tokio::time::timeout(remaining, receiver)
                    .await
                    .map_err(|_| Error::Timeout)?
                    .map_err(|_| Error::Denied("Runtime continuation cancelled/revoked"))?;
                reservation.port.verify(returned)?;
                match replied {
                    HookFlow::Next(Some(next)) => {
                        HookEvent::validate_next(&original, &next)?;
                        event = next;
                    }
                    HookFlow::Next(None) => {
                        return Err(Error::Denied("Runtime deferred hook vetoed proposal"))
                    }
                    HookFlow::Defer(_) => {
                        return Err(Error::Denied(
                            "nested/delegated Defer requires a new owned admission",
                        ))
                    }
                }
            }
        }
    }
    Ok((event, failures))
}
/// Unified async extension dispatch. The return type is always only the
/// typed next value; no side-band permission or new execution verb exists.
pub(crate) async fn dispatch_extension(
    extensions: &[Arc<dyn Extension>],
    ctx: HookContext<'_>,
    event: HookEvent,
    timeout: Duration,
    port: Option<&dyn HookContinuationPort>,
) -> Result<HookEvent> {
    dispatch_report(extensions, ctx, event, timeout, port)
        .await
        .map(|(next, _)| next)
}
/// Event-only read projection. Hooks are best-effort observers here; no
/// callback can withhold settled receipts or affect provider/tool admission.
pub(crate) async fn observe_extension(
    extensions: &[Arc<dyn Extension>],
    ctx: HookContext<'_>,
    stage: Stage,
    label: &str,
    timeout: Duration,
) -> u64 {
    if extensions.is_empty() {
        return 0;
    }
    if let Ok(event) = HookEvent::observation(stage, label) {
        return dispatch_report(extensions, ctx, event, timeout, None)
            .await
            .map_or(1, |(_, failures)| failures);
    }
    1
}
#[cfg(test)]
mod tests {
    use super::*;
    use aw_agent_kernel::{
        actor::{ActorKey, RuntimeKey},
        message::{Kind, TurnTarget},
    };
    use std::num::NonZeroU64;
    fn nz(n: u64) -> NonZeroU64 {
        NonZeroU64::new(n).unwrap()
    }
    fn key() -> ActorKey {
        ActorKey {
            runtime: RuntimeKey {
                id: nz(1),
                generation: nz(1),
            },
            id: nz(2),
            generation: nz(1),
        }
    }
    fn ctx(stage: Stage) -> Context {
        let key = key();
        Context {
            stage,
            actor: key,
            envelope_revision: 1,
            continuation: Continuation {
                owner: key,
                id: nz(3),
                generation: key.generation,
                envelope_revision: nz(1),
            },
        }
    }
    fn msg() -> ActorMessage {
        ActorMessage {
            id: 1,
            from: key(),
            to: key(),
            kind: Kind::Input,
            turn: TurnTarget::Auto,
            priority: 0,
            source_sequence: 1,
            payload: "raw".into(),
            content_type: aw_agent_kernel::message::ContentType::Text,
        }
    }
    struct Edit;
    impl RuntimeHook for Edit {
        fn handle(&self, _: Context, msg: &ActorMessage) -> HookOutcome<ActorMessage> {
            let mut m = msg.clone();
            m.payload.push_str("-edited");
            Ok(HookFlow::Next(Some(m)))
        }
    }
    struct Defer;
    impl RuntimeHook for Defer {
        fn handle(&self, c: Context, _: &ActorMessage) -> HookOutcome<ActorMessage> {
            Ok(HookFlow::Defer(c.continuation))
        }
    }
    struct Impersonate;
    impl RuntimeHook for Impersonate {
        fn handle(&self, _: Context, msg: &ActorMessage) -> HookOutcome<ActorMessage> {
            let mut m = msg.clone();
            m.priority = -5;
            Ok(HookFlow::Next(Some(m)))
        }
    }
    fn reg(
        stage: Stage,
        capability: Capability,
        priority: i32,
        handler: Arc<dyn RuntimeHook>,
    ) -> Registration {
        Registration {
            owner: "local".into(),
            stage,
            capability,
            priority,
            handler,
        }
    }
    #[test]
    fn transformed_text_and_defer_are_one_pipeline() {
        let mut p = HookPipeline::default();
        p.register(reg(Stage::Input, Capability::Transform, 0, Arc::new(Edit)))
            .unwrap();
        p.register(reg(Stage::Input, Capability::Transform, 1, Arc::new(Defer)))
            .unwrap();
        let Dispatch::Deferred {
            message,
            next_index,
        } = p.dispatch(ctx(Stage::Input), msg(), 0).unwrap()
        else {
            panic!("expected defer")
        };
        assert_eq!(message.payload, "raw-edited");
        assert_eq!(next_index, 2);
        let Dispatch::Next(Some(m)) = p.dispatch(ctx(Stage::Input), message, next_index).unwrap()
        else {
            panic!("expected next")
        };
        assert_eq!(m.payload, "raw-edited");
    }
    #[test]
    fn event_cannot_transform_and_any_authority_rewrite_denied() {
        let mut p = HookPipeline::default();
        assert!(p
            .register(reg(
                Stage::Lifecycle,
                Capability::Transform,
                0,
                Arc::new(Edit)
            ))
            .is_err());
        p.register(reg(
            Stage::Input,
            Capability::Gate,
            0,
            Arc::new(Impersonate),
        ))
        .unwrap();
        assert!(p.dispatch(ctx(Stage::Input), msg(), 0).is_err());
    }
    #[test]
    fn failing_observer_is_bypassed_with_failure_counter() {
        let mut p = HookPipeline::default();
        p.register(reg(Stage::Event, Capability::Observe, 0, Arc::new(Edit)))
            .unwrap();
        let Dispatch::Next(Some(out)) = p.dispatch(ctx(Stage::Event), msg(), 0).unwrap() else {
            panic!("expected next")
        };
        assert_eq!(out, msg());
        assert_eq!(p.observe_failures(), 1);
    }
    struct Recursive {
        parent: Arc<std::sync::Mutex<Option<Arc<HookPipeline>>>>,
    }
    impl RuntimeHook for Recursive {
        fn handle(&self, ctx: Context, msg: &ActorMessage) -> HookOutcome<ActorMessage> {
            let parent = self.parent.lock().unwrap().as_ref().cloned().unwrap();
            // Try recursively dispatching the very same stage; the bounded
            // Runtime hook guard must deny before an infinite stack or effect.
            parent
                .dispatch(ctx, msg.clone(), 0)
                .map(|_| HookFlow::Next(Some(msg.clone())))
                .map_err(|_| aw_agent_kernel::hook_contract::Error::Depth)
        }
    }
    #[test]
    fn recursive_hook_is_bounded_and_does_not_spill_into_nested_execution() {
        let parent = Arc::new(std::sync::Mutex::new(None));
        let mut pipeline = HookPipeline::default();
        pipeline
            .register(reg(
                Stage::Input,
                Capability::Gate,
                0,
                Arc::new(Recursive {
                    parent: parent.clone(),
                }),
            ))
            .unwrap();
        let pipeline = Arc::new(pipeline);
        *parent.lock().unwrap() = Some(pipeline.clone());
        assert!(pipeline.dispatch(ctx(Stage::Input), msg(), 0).is_err());
        assert_eq!(pipeline.active_dispatches.load(Ordering::Relaxed), 0);
        *parent.lock().unwrap() = None;
    }
}
