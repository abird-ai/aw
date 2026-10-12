# aw embedded C ABI — E5 version 1

A bounded, versioned C-facing adapter over the **existing** no_std+alloc
aw-embedded Agent and shared aw-agent-kernel TurnMachine.

The portable implementation is crates/aw-embedded-ffi/src/lib.rs and ports.rs.
Its public header is include/aw_embedded_ffi.h. No new model/tool/Session
engine is implemented here. The original kernel, hosted Agent and user CLI
stay unchanged and safe Rust. The small FFI crate is the ONLY unsafe pointer
boundary. It builds as an rlib with no_std + alloc and compiles with:

~~~sh
cargo check --locked -p aw-embedded-ffi \
  --no-default-features --features alloc --target wasm32v1-none
cargo test --locked -p aw-embedded-ffi --no-default-features --features alloc
~~~

The native standard C host example is separately packaged as a Rust staticlib
using an explicit std packaging wrapper, not as a bare-metal C executable:
examples/sdk-embedded-ffi-staticlib/README.md.

## ABI lifetime/permission contract

V1 uses only explicitly sized structs, uint32/uint64, size_t and borrowed
(pointer,length) byte buffers. No C strings, Rust types, bools or generated
layout assumptions cross the ABI. Caller checks aw_e5_abi_version and the
compiled struct sizes returned by aw_e5_layout_v1. Tools are stored in a
tightly packed V1 array, so each tool element needs EXACT sizeof tool;
configuration/callback structs permit a future larger compatible suffix.

Create makes private copies of all identity/source/tool strings and callback
function pointers, allocates exactly one Engine, and has ZERO host callbacks
or effects. The C host owns the user_data pointer and MUST keep it alive
until the handle is destroyed. The Rust Engine borrows stable privately
owned identity storage; it destroys the shared TurnMachine before freeing
the storage.

All operations are explicit and synchronous:
aw_e5_create_v1, aw_e5_start, aw_e5_model_step, aw_e5_tool_step,
aw_e5_state, aw_e5_answer_copy, aw_e5_trace_copy, aw_e5_cancel and
aw_e5_destroy. Calls do not automatically loop or start a provider/tool.
One opaque handle is single-owner. The BUSY atomic is disjoint from mutable
EngineState, so callbacks trying to reenter model/tool/state/destroy return
AW_E5_BUSY without making an overlapping mutable Rust reference.

The host supplies:
- a monotonic clock in host-defined ticks and fresh eight-byte secure entropy
- a synchronous journal callback, which MUST commit/flush each record before
  returning AW_E5_PORT_OK (record 5 is the tool intent preceding any effect)
- a model callback returning strict bounded JSON: text, finish final/tools/
  incomplete, calls (each id, name, object arguments)
- per-tool typed registration source ID/epoch/effect from explicit C config;
  a fresh tool_authorize callback on every pre-effect source/grant check;
  a separate tool_validate callback for complete schemas
- one synchronous tool_execute callback returning content and is_error

Callback request/reply JSON is bounded. Input and output pointers are
borrowed for the exact callback duration; hosts MUST NOT retain them and
must set output written <= capacity. Rust does not pass persistent raw
interior pointers to C. Answer and trace exports copy into C-owned buffers
and return BUFFER_TOO_SMALL with the exact required length/count if needed.
They are NOT NUL-terminated. Native tool/event IDs have stable numeric ABI
values; they are NOT Rust enum layout/discriminants.

No callback may implicitly grant a tool, select a model, choose credentials,
retry a paid request or replay an uncertain effect. Registration is source-
epoch/actor/site/effect-bound and checked both before journal ToolIntent and
AFTER it is committed. If authorization is revoked meanwhile, the tool is
not executed. The FFI wrapper conservatively treats ANY failure after a newly committed
ModelStarted as requiring inspection, including one after ModelSettled was
committed but the Assistant/TurnFinished record later failed. The request
may have incurred cost and its journal cannot be assumed complete. The existing shared kernel separately tracks uncertain
tool intent/receipt. A failure blocks further calls; destroy returns
RECONCILIATION_REQUIRED unless C deliberately uses discard_unreconciled=1,
which NEVER asserts the effect completed or was rolled back.

The caller must pass valid/live/aligned pointers; no software can safely
validate a forged/stale C address or out-of-bounds caller buffer. If an
allocator exhausts memory the embedded host's OOM policy applies. Final
staticlib integration MUST use panic=abort; no Rust panic may unwind across
the C ABI. There is no default TLS/network/clock/entropy, allocator/panic
handler, background task, SecretStore, or C runtime in the portable FFI rlib.

E5 is **not** an OS-independent C firmware artifact. The C executable test
uses a normal std/C runtime, intentionally distinct from E4's Linux raw-
syscall libc-free executable. Native no_std C-host ELF, x86_64-unknown-none,
ARM/RISC-V firmware, C++ integration, embedded transport/clock drivers, and
pinned Rust1.98.1 still require separate real evidence under E6/R13.
