//! E5: one bounded versioned C ABI over the exact no_std+alloc shared Agent.
//!
//! Only this FFI crate owns unsafe pointer glue. aw-embedded and the shared
//! aw-agent-kernel remain safe Rust and retain every transition and grant.
//! C callers own callback context, memory buffers, credentials and committed
//! journal. Pointer validity is a caller requirement; arbitrary stale C
//! addresses cannot be safely validated by the callee. Compile panic=abort
//! in final C staticlib integration; NEVER unwind across C.
#![cfg_attr(not(feature = "std"), no_std)]
#![deny(unsafe_op_in_unsafe_fn)]
#[cfg(not(feature = "alloc"))]
compile_error!("aw-embedded-ffi requires the alloc feature; select --features alloc");

extern crate alloc;

mod ports;
use alloc::{
    boxed::Box,
    string::{String, ToString},
    vec::Vec,
};
use aw_agent_kernel::{
    capability::{Capability, Effect, Source, SourceKind},
    turn::Event,
};
use aw_embedded::{
    EmbeddedTurn, Error as EngineError, Identity, Options, Status, MAX_PROMPT_BYTES,
};
use core::{
    ffi::c_void,
    mem::size_of,
    ptr, slice,
    sync::atomic::{AtomicU8, Ordering},
};
use ports::Ports;

const VERSION: u32 = 1;
const MAX_TOOLS: usize = 16;
const MAX_CALLBACK_JSON: usize = 1_572_864;
const MAX_RESULT_JSON: usize = 786_432;

pub mod result_code {
    pub const OK: u32 = 0;
    pub const INVALID: u32 = 1;
    pub const ABI_MISMATCH: u32 = 2;
    pub const DENIED: u32 = 3;
    pub const UNAVAILABLE: u32 = 4;
    pub const TOO_LARGE: u32 = 5;
    pub const WRONG_PHASE: u32 = 6;
    pub const FAILED: u32 = 7;
    pub const BLOCKED: u32 = 8;
    pub const BUFFER_TOO_SMALL: u32 = 9;
    pub const BUSY: u32 = 10;
    pub const RECONCILIATION_REQUIRED: u32 = 11;
}
type Clock = extern "C" fn(*mut c_void, *mut u64) -> u32;
type Entropy = extern "C" fn(*mut c_void, *mut u8, usize) -> u32;
type Journal = extern "C" fn(*mut c_void, u32, *const u8, usize) -> u32;
type Exchange = extern "C" fn(*mut c_void, *const u8, usize, *mut u8, usize, *mut usize) -> u32;
type Gate = extern "C" fn(*mut c_void, *const u8, usize) -> u32;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct AwE5ToolV1 {
    pub struct_size: u32,
    pub source_kind: u32,
    pub effect: u32,
    pub reserved: u32,
    pub name: *const u8,
    pub name_len: usize,
    pub source_id: *const u8,
    pub source_id_len: usize,
    pub source_epoch: u64,
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct AwE5ConfigV1 {
    pub struct_size: u32,
    pub abi_version: u32,
    pub turn_id: u64,
    pub actor: *const u8,
    pub actor_len: usize,
    pub site: *const u8,
    pub site_len: usize,
    pub account: *const u8,
    pub account_len: usize,
    pub model: *const u8,
    pub model_len: usize,
    pub max_rounds: u32,
    pub max_calls: u32,
    pub deadline_ticks: u64,
    pub tools: *const AwE5ToolV1,
    pub tools_count: usize,
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct AwE5CallbacksV1 {
    pub struct_size: u32,
    pub abi_version: u32,
    pub context: *mut c_void,
    pub clock: Option<Clock>,
    pub entropy: Option<Entropy>,
    pub journal: Option<Journal>,
    pub model: Option<Exchange>,
    pub tool_authorize: Option<Gate>,
    pub tool_validate: Option<Gate>,
    pub tool_execute: Option<Exchange>,
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct AwE5StateV1 {
    pub struct_size: u32,
    pub abi_version: u32,
    pub status: u32,
    pub needs_reconciliation: u32,
    pub reserved: u32,
    pub trace_length: u32,
    pub answer_bytes: usize,
    pub nonce: u64,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AwE5EventV1 {
    pub kind: u32,
    pub ordinal: u32,
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct AwE5LayoutV1 {
    pub struct_size: u32,
    pub abi_version: u32,
    pub config_size: u32,
    pub callbacks_size: u32,
    pub tool_size: u32,
    pub state_size: u32,
    pub event_size: u32,
    pub max_tools: u32,
    pub max_callback_json: u32,
}

struct IdentityData {
    actor: String,
    site: String,
    account: String,
    model: String,
}
pub(crate) struct Binding {
    pub(crate) name: String,
    pub(crate) source_id: String,
    pub(crate) kind: SourceKind,
    pub(crate) effect: Effect,
    pub(crate) epoch: u64,
}
pub struct AwE5Engine {
    // The lock lives in a DISJOINT field/allocation from the mutably used
    // Agent state, so callback reentry can inspect BUSY without borrowing a
    // field of an already-exclusive &mut whole Engine. This is essential to
    // avoid C callbacks accidentally violating Rust aliasing requirements.
    busy: AtomicU8,
    state: EngineState,
}
struct EngineState {
    // Drop order is crucial: borrower turn is dropped before its stable
    // owned_identity Box. C never holds references to either Rust allocation.
    turn: EmbeddedTurn<'static>,
    _owned_identity: Box<IdentityData>,
    ports: Ports,
    // Separate from kernel's tool-effect uncertainty. A provider callback
    // can incur cost while its reply is lost; no automatic replay is safe.
    model_uncertain: bool,
}
struct Guard(*mut AwE5Engine);
impl Drop for Guard {
    fn drop(&mut self) {
        // SAFETY: caller supplied a valid live handle, and destroy rejects
        // BUSY; this allocation remains alive until the guard releases.
        unsafe { (*self.0).busy.store(0, Ordering::Release) };
    }
}
fn lock_engine(handle: *mut AwE5Engine) -> Result<Guard, u32> {
    if handle.is_null() {
        return Err(result_code::INVALID);
    }
    // SAFETY: valid live well-aligned handle is an explicit C contract.
    // Atomic BUSY is the only memory accessed prior to exclusive admission.
    let busy = unsafe { &(*handle).busy };
    if busy
        .compare_exchange(0, 1, Ordering::Acquire, Ordering::Relaxed)
        .is_err()
    {
        return Err(result_code::BUSY);
    }
    Ok(Guard(handle))
}
fn with_engine(handle: *mut AwE5Engine, f: impl FnOnce(&mut EngineState) -> u32) -> u32 {
    let _guard = match lock_engine(handle) {
        Ok(guard) => guard,
        Err(code) => return code,
    };
    // SAFETY: only the disjoint state field is mutably borrowed after BUSY
    // admission. A recursive C callback checks the separate AtomicU8 field
    // and returns BUSY, never creating an alias to state.
    f(unsafe { &mut (*handle).state })
}
unsafe fn borrow_bytes<'a>(input: *const u8, len: usize, max: usize) -> Result<&'a [u8], u32> {
    if len > max || len > isize::MAX as usize {
        return Err(result_code::TOO_LARGE);
    }
    if len == 0 {
        return Ok(&[]);
    }
    if input.is_null() {
        return Err(result_code::INVALID);
    }
    // SAFETY: caller guarantees readable initialized bytes for the entire
    // method, and no overlapping mutation; slice never outlives invocation.
    Ok(unsafe { slice::from_raw_parts(input, len) })
}
unsafe fn owned_utf8(input: *const u8, len: usize, max: usize) -> Result<String, u32> {
    let source = unsafe { borrow_bytes(input, len, max) }?;
    let decoded = core::str::from_utf8(source).map_err(|_| result_code::INVALID)?;
    Ok(decoded.to_string())
}
fn binding_from_c(value: &AwE5ToolV1) -> Result<Binding, u32> {
    if value.struct_size != size_of::<AwE5ToolV1>() as u32 || value.reserved != 0 {
        return Err(result_code::ABI_MISMATCH);
    }
    let kind = match value.source_kind {
        1 => SourceKind::Host,
        2 => SourceKind::Mcp,
        3 => SourceKind::Extension,
        _ => return Err(result_code::INVALID),
    };
    let effect = match value.effect {
        1 => Effect::Read,
        2 => Effect::Write,
        3 => Effect::Process,
        4 => Effect::Unknown,
        _ => return Err(result_code::INVALID),
    };
    let name = unsafe { owned_utf8(value.name, value.name_len, 64) }?;
    let source_id = unsafe { owned_utf8(value.source_id, value.source_id_len, 128) }?;
    let source =
        Source::new(kind, &source_id, value.source_epoch).map_err(|_| result_code::INVALID)?;
    Capability::new(&name, source, effect).map_err(|_| result_code::INVALID)?;
    Ok(Binding {
        name,
        source_id,
        kind,
        effect,
        epoch: value.source_epoch,
    })
}
fn error_code(value: EngineError) -> u32 {
    match value {
        EngineError::InvalidConfiguration | EngineError::InvalidResponse => result_code::INVALID,
        EngineError::TooLarge => result_code::TOO_LARGE,
        EngineError::Denied => result_code::DENIED,
        EngineError::JournalUnavailable
        | EngineError::ToolUnavailable
        | EngineError::ProviderUnavailable
        | EngineError::UnavailableClock
        | EngineError::UnavailableEntropy => result_code::UNAVAILABLE,
        EngineError::NotStarted | EngineError::RegressedClock | EngineError::Deadline => {
            result_code::WRONG_PHASE
        }
        EngineError::Transition(_) => result_code::BLOCKED,
        EngineError::Failed => result_code::FAILED,
    }
}
fn engine_status(value: Status) -> u32 {
    match value {
        Status::Created => 1,
        Status::NeedsModel => 2,
        Status::NeedsTool => 3,
        Status::Completed => 4,
        Status::Incomplete => 5,
        Status::Blocked => 6,
    }
}
fn make_engine(cfg: AwE5ConfigV1, cb: AwE5CallbacksV1) -> Result<AwE5Engine, u32> {
    if cfg.turn_id == 0 || cfg.tools_count > MAX_TOOLS {
        return Err(result_code::INVALID);
    }
    if cb.clock.is_none()
        || cb.entropy.is_none()
        || cb.journal.is_none()
        || cb.model.is_none()
        || (cfg.tools_count > 0
            && (cb.tool_authorize.is_none()
                || cb.tool_validate.is_none()
                || cb.tool_execute.is_none()))
    {
        return Err(result_code::UNAVAILABLE);
    }
    let identity = Box::new(IdentityData {
        actor: unsafe { owned_utf8(cfg.actor, cfg.actor_len, 128) }?,
        site: unsafe { owned_utf8(cfg.site, cfg.site_len, 128) }?,
        account: unsafe { owned_utf8(cfg.account, cfg.account_len, 256) }?,
        model: unsafe { owned_utf8(cfg.model, cfg.model_len, 256) }?,
    });
    // SAFETY: Box points to a stable separate allocation throughout the
    // handle lifetime, and handle drops the EmbeddedTurn BEFORE owned_identity.
    // Nothing can mutate these strings after construction, nor borrow them
    // through C. No static leak: Box is dropped by destroy after the turn.
    let stable: &'static IdentityData = unsafe { &*(&*identity as *const IdentityData) };
    let id = Identity {
        actor: &stable.actor,
        site: &stable.site,
        account: &stable.account,
        model: &stable.model,
    };
    let turn = EmbeddedTurn::new(
        cfg.turn_id,
        id,
        Options {
            max_rounds: cfg.max_rounds,
            max_calls: cfg.max_calls,
            deadline_ticks: cfg.deadline_ticks,
        },
    )
    .map_err(error_code)?;
    let mut bindings = Vec::new();
    bindings
        .try_reserve_exact(cfg.tools_count)
        .map_err(|_| result_code::TOO_LARGE)?;
    if cfg.tools_count > 0 {
        if cfg.tools.is_null() {
            return Err(result_code::INVALID);
        }
        // SAFETY: C caller supplies a readable array of exactly tools_count
        // initialized AwE5ToolV1 objects. All fields are copied on create.
        let tools = unsafe { slice::from_raw_parts(cfg.tools, cfg.tools_count) };
        for tool in tools {
            let binding = binding_from_c(tool)?;
            if bindings
                .iter()
                .any(|existing: &Binding| existing.name == binding.name)
            {
                return Err(result_code::DENIED);
            }
            bindings.push(binding);
        }
    }
    Ok(AwE5Engine {
        busy: AtomicU8::new(0),
        state: EngineState {
            turn,
            ports: Ports {
                cb,
                actor: identity.actor.clone(),
                site: identity.site.clone(),
                bindings,
                model_started_committed: 0,
                model_settled_committed: 0,
            },
            model_uncertain: false,
            _owned_identity: identity,
        },
    })
}
#[no_mangle]
pub extern "C" fn aw_e5_abi_version() -> u32 {
    VERSION
}
/// # Safety
/// The C caller must provide valid, correctly aligned live pointers for each
/// declared length/struct and honor exclusive engine ownership. Any input
/// or output buffers must not overlap the engine allocation, and the host
/// must preserve callback context until destroy. See the v1 C header.
#[no_mangle]
pub unsafe extern "C" fn aw_e5_layout_v1(out: *mut AwE5LayoutV1) -> u32 {
    if out.is_null() {
        return result_code::INVALID;
    }
    // SAFETY: caller provides writable aligned AwE5LayoutV1, full size.
    unsafe {
        out.write(AwE5LayoutV1 {
            struct_size: size_of::<AwE5LayoutV1>() as u32,
            abi_version: VERSION,
            config_size: size_of::<AwE5ConfigV1>() as u32,
            callbacks_size: size_of::<AwE5CallbacksV1>() as u32,
            tool_size: size_of::<AwE5ToolV1>() as u32,
            state_size: size_of::<AwE5StateV1>() as u32,
            event_size: size_of::<AwE5EventV1>() as u32,
            max_tools: MAX_TOOLS as u32,
            max_callback_json: MAX_CALLBACK_JSON as u32,
        })
    };
    result_code::OK
}
/// # Safety
/// The C caller must provide valid, correctly aligned live pointers for each
/// declared length/struct and honor exclusive engine ownership. Any input
/// or output buffers must not overlap the engine allocation, and the host
/// must preserve callback context until destroy. See the v1 C header.
#[no_mangle]
pub unsafe extern "C" fn aw_e5_create_v1(
    config: *const AwE5ConfigV1,
    callbacks: *const AwE5CallbacksV1,
    out: *mut *mut AwE5Engine,
) -> u32 {
    if out.is_null() {
        return result_code::INVALID;
    }
    // SAFETY: caller supplies a writable live pointer slot.
    unsafe { out.write(ptr::null_mut()) };
    if config.is_null() || callbacks.is_null() {
        return result_code::INVALID;
    }
    // SAFETY: caller supplies correctly aligned readable v1 C struct headers.
    let config = unsafe { &*config };
    let callbacks = unsafe { &*callbacks };
    if config.struct_size < size_of::<AwE5ConfigV1>() as u32
        || callbacks.struct_size < size_of::<AwE5CallbacksV1>() as u32
        || config.abi_version != VERSION
        || callbacks.abi_version != VERSION
    {
        return result_code::ABI_MISMATCH;
    }
    match make_engine(*config, *callbacks) {
        Ok(engine) => {
            // SAFETY: only C owns this boxed opaque handle until destroy.
            unsafe { out.write(Box::into_raw(Box::new(engine))) };
            result_code::OK
        }
        Err(code) => code,
    }
}

/// # Safety
/// The C caller must provide valid, correctly aligned live pointers for each
/// declared length/struct and honor exclusive engine ownership. Any input
/// or output buffers must not overlap the engine allocation, and the host
/// must preserve callback context until destroy. See the v1 C header.
#[no_mangle]
pub unsafe extern "C" fn aw_e5_start(
    handle: *mut AwE5Engine,
    prompt: *const u8,
    len: usize,
    progress: *mut u32,
) -> u32 {
    if progress.is_null() {
        return result_code::INVALID;
    }
    let payload = match unsafe { borrow_bytes(prompt, len, MAX_PROMPT_BYTES) } {
        Ok(value) => value,
        Err(code) => return code,
    };
    let text = match core::str::from_utf8(payload) {
        Ok(value) => value,
        Err(_) => return result_code::INVALID,
    };
    with_engine(handle, |engine| {
        let code = engine
            .turn
            .start(&mut engine.ports, text)
            .map_or_else(error_code, |_| result_code::OK);
        // SAFETY: valid aligned writable u32 provided by C.
        unsafe { progress.write(engine_status(engine.turn.status())) };
        code
    })
}
/// # Safety
/// The C caller must provide valid, correctly aligned live pointers for each
/// declared length/struct and honor exclusive engine ownership. Any input
/// or output buffers must not overlap the engine allocation, and the host
/// must preserve callback context until destroy. See the v1 C header.
#[no_mangle]
pub unsafe extern "C" fn aw_e5_model_step(handle: *mut AwE5Engine, progress: *mut u32) -> u32 {
    if progress.is_null() {
        return result_code::INVALID;
    }
    with_engine(handle, |engine| {
        // A paid model attempt may already have been dispatched after its
        // durable ModelStarted intent. Even if ModelSettled succeeds, a later
        // Assistant/TurnFinished journal failure can leave the saved turn
        // incomplete. Mark *any* failure after a newly committed model-start
        // as requiring inspection, not only missing ModelSettled.
        let started_before = engine.ports.model_started_committed;
        let code = engine
            .turn
            .model_step(&mut engine.ports)
            .map_or_else(error_code, |_| result_code::OK);
        if code != result_code::OK
            && (engine.ports.model_started_committed > started_before
                || engine.ports.model_started_committed > engine.ports.model_settled_committed)
        {
            engine.model_uncertain = true;
        }
        // SAFETY: the C caller supplied a valid writable output u32.
        unsafe { progress.write(engine_status(engine.turn.status())) };
        code
    })
}
/// # Safety
/// The C caller must provide valid, correctly aligned live pointers for each
/// declared length/struct and honor exclusive engine ownership. Any input
/// or output buffers must not overlap the engine allocation, and the host
/// must preserve callback context until destroy. See the v1 C header.
#[no_mangle]
pub unsafe extern "C" fn aw_e5_tool_step(handle: *mut AwE5Engine, progress: *mut u32) -> u32 {
    if progress.is_null() {
        return result_code::INVALID;
    }
    with_engine(handle, |engine| {
        let code = engine
            .turn
            .tool_step(&mut engine.ports)
            .map_or_else(error_code, |_| result_code::OK);
        // SAFETY: the C caller supplied a valid writable output u32.
        unsafe { progress.write(engine_status(engine.turn.status())) };
        code
    })
}
/// # Safety
/// The C caller must provide valid, correctly aligned live pointers for each
/// declared length/struct and honor exclusive engine ownership. Any input
/// or output buffers must not overlap the engine allocation, and the host
/// must preserve callback context until destroy. See the v1 C header.
#[no_mangle]
pub unsafe extern "C" fn aw_e5_cancel(handle: *mut AwE5Engine) -> u32 {
    with_engine(handle, |engine| match engine.turn.status() {
        Status::Created | Status::NeedsModel | Status::NeedsTool => {
            engine.turn.cancel();
            result_code::OK
        }
        Status::Completed | Status::Incomplete | Status::Blocked => result_code::WRONG_PHASE,
    })
}
/// # Safety
/// The C caller must provide valid, correctly aligned live pointers for each
/// declared length/struct and honor exclusive engine ownership. Any input
/// or output buffers must not overlap the engine allocation, and the host
/// must preserve callback context until destroy. See the v1 C header.
#[no_mangle]
pub unsafe extern "C" fn aw_e5_state(handle: *mut AwE5Engine, out: *mut AwE5StateV1) -> u32 {
    if out.is_null() {
        return result_code::INVALID;
    }
    // SAFETY: out is a valid readable/writable C struct V1 header; only
    // read the version and size before writing other fields.
    let header = unsafe { &*out };
    if header.struct_size < size_of::<AwE5StateV1>() as u32 || header.abi_version != VERSION {
        return result_code::ABI_MISMATCH;
    }
    with_engine(handle, |engine| {
        // SAFETY: C supplies a complete aligned writable struct V1.
        unsafe {
            out.write(AwE5StateV1 {
                struct_size: size_of::<AwE5StateV1>() as u32,
                abi_version: VERSION,
                status: engine_status(engine.turn.status()),
                needs_reconciliation: u32::from(
                    engine.turn.needs_reconciliation() || engine.model_uncertain,
                ),
                reserved: 0,
                trace_length: engine.turn.trace().len() as u32,
                answer_bytes: engine.turn.answer().len(),
                nonce: engine.turn.nonce().unwrap_or(0),
            })
        };
        result_code::OK
    })
}
fn copy_slice(data: &[u8], out: *mut u8, cap: usize, written: *mut usize) -> u32 {
    if written.is_null() {
        return result_code::INVALID;
    }
    // SAFETY: C provides writable aligned count pointer.
    unsafe { written.write(data.len()) };
    if cap < data.len() {
        return result_code::BUFFER_TOO_SMALL;
    }
    if data.is_empty() {
        return result_code::OK;
    }
    if out.is_null() {
        return result_code::INVALID;
    }
    // SAFETY: caller owns valid cap-sized writable buffer nonoverlapping
    // the Agent's internal storage. The length was checked above.
    unsafe { ptr::copy_nonoverlapping(data.as_ptr(), out, data.len()) };
    result_code::OK
}
/// # Safety
/// The C caller must provide valid, correctly aligned live pointers for each
/// declared length/struct and honor exclusive engine ownership. Any input
/// or output buffers must not overlap the engine allocation, and the host
/// must preserve callback context until destroy. See the v1 C header.
#[no_mangle]
pub unsafe extern "C" fn aw_e5_answer_copy(
    handle: *mut AwE5Engine,
    out: *mut u8,
    cap: usize,
    written: *mut usize,
) -> u32 {
    if written.is_null() {
        return result_code::INVALID;
    }
    with_engine(handle, |engine| {
        copy_slice(engine.turn.answer().as_bytes(), out, cap, written)
    })
}
fn event_code(value: Event) -> AwE5EventV1 {
    let (kind, ordinal) = match value {
        Event::ModelStarted(n) => (1, n),
        Event::TextCompleted => (2, 0),
        Event::TextIncomplete => (3, 0),
        Event::BatchProposed(n) => (4, n as u32),
        Event::BatchAdmitted(n) => (5, n as u32),
        Event::ToolIntent(n) => (6, n as u32),
        Event::IntentRecorded(n) => (7, n as u32),
        Event::EffectStarted(n) => (8, n as u32),
        Event::EffectSettled(n) => (9, n as u32),
        Event::ReceiptRecorded(n) => (10, n as u32),
        Event::Blocked => (11, 0),
        Event::ReadWaveProposed(n) => (12, n as u32),
        Event::ReadWaveIntent(n) => (13, n as u32),
        Event::ReadWaveIntentRecorded(n) => (14, n as u32),
        Event::ReadWaveStarted(n) => (15, n as u32),
        Event::ReadWaveSettled(n) => (16, n as u32),
        Event::ReadWaveReceiptRecorded(n) => (17, n as u32),
        Event::BashWaveProposed(n) => (18, n as u32),
        Event::BashWaveIntent(n) => (19, n as u32),
        Event::BashWaveIntentRecorded(n) => (20, n as u32),
        Event::BashWaveStarted(n) => (21, n as u32),
        Event::BashWaveSettled(n) => (22, n as u32),
        Event::BashWaveReceiptRecorded(n) => (23, n as u32),
        // Additive codes only; original E5 C ABI event IDs 1..23 do not move.
        Event::BatchRejected(n) => (24, n as u32),
        Event::DenialRecorded(n) => (25, n as u32),
    };
    AwE5EventV1 { kind, ordinal }
}
/// # Safety
/// The C caller must provide valid, correctly aligned live pointers for each
/// declared length/struct and honor exclusive engine ownership. Any input
/// or output buffers must not overlap the engine allocation, and the host
/// must preserve callback context until destroy. See the v1 C header.
#[no_mangle]
pub unsafe extern "C" fn aw_e5_trace_copy(
    handle: *mut AwE5Engine,
    out: *mut AwE5EventV1,
    cap: usize,
    written: *mut usize,
) -> u32 {
    if written.is_null() {
        return result_code::INVALID;
    }
    with_engine(handle, |engine| {
        let trace = engine.turn.trace();
        // SAFETY: caller provides a valid writable length slot.
        unsafe { written.write(trace.len()) };
        if cap < trace.len() {
            return result_code::BUFFER_TOO_SMALL;
        }
        if trace.is_empty() {
            return result_code::OK;
        }
        if out.is_null() {
            return result_code::INVALID;
        }
        for (i, event) in trace.iter().enumerate() {
            // SAFETY: capacity checked, caller owns writable aligned array
            // with cap entries; no internal pointers retained.
            unsafe { out.add(i).write(event_code(*event)) };
        }
        result_code::OK
    })
}
/// # Safety
/// The C caller must provide valid, correctly aligned live pointers for each
/// declared length/struct and honor exclusive engine ownership. Any input
/// or output buffers must not overlap the engine allocation, and the host
/// must preserve callback context until destroy. See the v1 C header.
#[no_mangle]
pub unsafe extern "C" fn aw_e5_destroy(
    slot: *mut *mut AwE5Engine,
    discard_unreconciled: u32,
) -> u32 {
    if slot.is_null() || discard_unreconciled > 1 {
        return result_code::INVALID;
    }
    // SAFETY: caller guarantees a valid writable pointer-to-handle slot
    // and that a stale pointer will never be passed after destruction.
    let handle = unsafe { *slot };
    if handle.is_null() {
        return result_code::OK;
    }
    let guard = match lock_engine(handle) {
        Ok(g) => g,
        Err(code) => return code,
    };
    // SAFETY: exclusive BUSY gate held until destruction.
    if unsafe { (*handle).state.turn.needs_reconciliation() || (*handle).state.model_uncertain }
        && discard_unreconciled == 0
    {
        return result_code::RECONCILIATION_REQUIRED;
    }
    // Must not unlock AFTER free: destroy consumes this guard. EmbeddedTurn
    // drops BEFORE its identity backing Box, so no dangling borrow.
    core::mem::forget(guard);
    unsafe {
        slot.write(ptr::null_mut());
        drop(Box::from_raw(handle));
    }
    result_code::OK
}

#[cfg(test)]
extern crate std;
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn c_layout_and_source_validation() {
        assert_eq!(VERSION, 1);
        assert_eq!(size_of::<AwE5EventV1>(), 8);
        assert_eq!(core::mem::align_of::<AwE5EventV1>(), 4);
        let missing = AwE5ToolV1 {
            struct_size: size_of::<AwE5ToolV1>() as u32,
            source_kind: 99,
            effect: 4,
            reserved: 0,
            name: ptr::null(),
            name_len: 0,
            source_id: ptr::null(),
            source_id_len: 0,
            source_epoch: 1,
        };
        assert_eq!(binding_from_c(&missing).err(), Some(result_code::INVALID));
    }
    #[test]
    fn progress_status_denial_codes() {
        assert_eq!(engine_status(Status::Blocked), 6);
        assert_eq!(error_code(EngineError::Denied), result_code::DENIED);
        assert_eq!(error_code(EngineError::TooLarge), result_code::TOO_LARGE);
        assert_eq!(ports::effect_name(Effect::Unknown), "unknown");
        assert_eq!(ports::source_code(SourceKind::Extension), 3);
        assert_eq!(
            event_code(Event::TextCompleted),
            AwE5EventV1 {
                kind: 2,
                ordinal: 0
            }
        );
        // E5 ABI v1 event codes are immutable; new denial states are additive
        // and never claim a ToolIntent/EffectStarted/ReceiptRecorded.
        assert_eq!(
            event_code(Event::BatchRejected(2)),
            AwE5EventV1 {
                kind: 24,
                ordinal: 2,
            }
        );
        assert_eq!(
            event_code(Event::DenialRecorded(1)),
            AwE5EventV1 {
                kind: 25,
                ordinal: 1,
            }
        );
    }
}
