# E5 — Versioned, bounded, independently tested C ABI for shared aw Agent

Date: 2026-10-10. Unnumbered embedded E5 successor after local E4 commit
bf8640e1b997aa0d9cdd29f70fc7812edcb6f277.
Status: **portable no_std+alloc Rust C ABI implemented and linked/executed by a
genuine, separately compiled native C11 client**, in debug and release.
Version 0.15.0, SQLite v33 and original Phase68 tag unchanged. No push.

## Authority, exact boundaries and read order

Read current .agents/plans/START_HERE.md, immutable
FULL-COMPREHENSIVE-HANDOFF-2026-10-09.md for exact original historical
architecture and 12 verbatim annexes (never modify that master),
AW-EMBEDDED-NOSTD-LIBC-FREE-ARCHITECTURE-2026-10-09.md,
AGENTC-PARITY-R8-EMBEDDED-AGENT-2026-10-10.md and
AGENTC-PARITY-E4-NATIVE-LIBC-FREE-2026-10-10.md. E4 already linked and
ACTUALLY EXECUTED the shared no_std+alloc Agent inside a libc-free **Linux**
raw-syscall ELF. E5 adds only a portable, audited pointer boundary to the
SAME kernel/host-driven Engine, not another runtime or a production provider.

The full aw CLI (std/Tokio), JSONL-minimal CLI, no-std embedded Agent and
their original capability, journaling and paid-effect rules are unchanged.
R14 complete canonical CLI taxonomy and old hidden long aliases remain intact;
R14.5 removal of compatibility aliases requires explicit future MAJOR release
approval. R13 global agentc 0.6 reference-provider/native cross-host parity
gaps remain open. E6 hardware/firmware/bootloader/peripherals remain future.

## Architectural decisions: one portable Agent, one isolated unsafe FFI crate

E5 creates crates/aw-embedded-ffi, a **no_std+alloc rlib** with exactly one
small isolated unsafe pointer boundary. aw-embedded, aw-agent-kernel and
aw-agent-core are NOT modified. The crate exposes a stable version-1 C header
at crates/aw-embedded-ffi/include/aw_embedded_ffi.h. No Rust struct/enum,
string, bool, Vec, panic, malloc or borrowed private Rust pointer is exposed
to the C host; all data uses explicitly sized structs, uint32/uint64, size_t
and (pointer,length) byte slices. Arrays of tool descriptors have an EXACT
V1 element stride, rejecting incompatible larger element sizes instead of
misparsing a next tool. Future config/callback extensions can append fields
under the explicit struct_size/version contract.

The Host owns its user_data pointer, allocator, durable journal implementation,
monotonic clock, hardware entropy, model transport/credentials, tool
executor and permission revocation. The FFI copies every actor/site/account/
model/source/tool string and the callback pointer table at create; no
configuration buffer is borrowed after return. A separate Box of immutable
identity strings stays allocated until after the shared EmbeddedTurn is dropped
so the Rust internal typed turn can borrow stable identity safely. This
FI-only extension of lifetime is narrowly justified and documented; no leaked
process-global identity or raw pointer becomes C-visible.

The public opaque AwE5Engine has a disjoint AtomicU8 BUSY gate and owned
EngineState. After taking BUSY, Rust mutably borrows ONLY EngineState, so an
attempted reentrant C callback accessing the BUSY field does not overlap an
exclusive Rust mutable borrow. Recursive/concurrent calls immediately return
AW_E5_BUSY before performing effects. The C caller must still provide a
valid, live, aligned handle and never concurrently destroy it or use a stale
pointer; arbitrary invalid C addresses cannot be safely inspected. The
pointer-to-handle destructor writes NULL and is idempotent for an already NULL
slot; it will not discard an uncertain effect unless a host explicitly chooses
discard_unreconciled=1 after its own investigation. Destruction makes no claim
that a paid request/tool effect succeeded or was undone.

E5 does NOT install a global allocator or panic handler into shared libraries
or its portable no_std rlib. The real native C proof uses a **separate native
std staticlib packaging crate**, examples/sdk-embedded-ffi-staticlib, linked to
a standalone C11 executable through the C host toolchain. The wrapper uses
panic=abort, and only creates an anchor export to force the actual portable
FFI symbols into a linkable native archive. The C executable intentionally
uses its normal OS C runtime: this is **not** the E4 libc-free final ELF nor
a native bare-metal C firmware build. An E6 host can link the same no_std
rlib with its independently supplied panic handler and allocator when a
suitable target compiler/platform is available.

## ABI v1 surface and bounds

There are exactly 11 exported core ABI functions:
aw_e5_abi_version, aw_e5_layout_v1, aw_e5_create_v1,
aw_e5_start, aw_e5_model_step, aw_e5_tool_step, aw_e5_cancel,
aw_e5_state, aw_e5_answer_copy, aw_e5_trace_copy, aw_e5_destroy.
The native example staticlib has one additional packaging-only anchor, not
a separate public Agent API.

AwE5ConfigV1 contains version, struct_size, host identity labels, turn_id,
explicit max_rounds/max_calls/deadline_ticks and a bounded array (max 16)
of source_kind, source_id, source_epoch, exact effect and tool name records.
Every name is copied and checked through the EXISTING Source/Capability/Grant
validators. A matching tool name alone is never authorization: an explicit
tool_authorize callback must approve a freshly source-bound exact actor/site/
source-epoch/effect record every time the existing Engine calls registration.
A separate tool_validate callback checks structured arguments. Authorization
is checked again AFTER durable ToolIntent append and BEFORE execute; revocation
then blocks without running the effect. No callback manufactures an Agent
grant on behalf of untrusted model data.

AwE5CallbacksV1 provides synchronous borrowed-host ports: monotonic clock;
fresh secure entropy; committed+flushed journal append; bounded model
request/reply JSON; dynamic tool_authorize; tool_validate; and tool_execute
with explicit caller-owned output buffers/capacity/required length. There is
NO Tokio, extra scheduler, network/auth fallback, thread for model execution,
background callbacks, automatically selected account or automatic retry.
The exact existing EmbeddedTurn controls model/tool/journal state transitions
and stores actual source-bound receipts. Host callback JSON input maximum
1,572,864 bytes, output maximum 786,432 bytes; model replies strictly require
text, finish and calls, reject unknown keys, and all nested call arguments
must be object JSON within existing MAX_ARGUMENT_BYTES. Host callback output
must set 0<=written<=capacity and cannot retain buffers. Failed/invalid
callbacks fail closed and leave the Agent blocked.

AwE5StateV1 exposes status, nonce, trace length, answer byte length and
needs_reconciliation. Every effect and output remains bounded by the original
Engine limits. Stable AwE5EventV1 numeric kind/ordinal is a portable projection
of the shared TurnMachine trace (including currently reserved read/Bash waves)
rather than Rust enum discriminants. Answer and trace use exact copy-into-
caller buffers with BUFFER_TOO_SMALL and required size, no NUL or borrowed
interior memory. The C header documents pointer validity/alignment and
callback lifetime restrictions; malformed pointers/out-of-bounds host writes
cannot be safely detected by any FFI library, and OOM can abort according
to the embedder's allocator policy.

### Paid and tool effect uncertainty

The original kernel reports uncertain tool intent/receipt state; C E5 mirrors
it and requires explicit reconciliation before destructive teardown. E5 adds
one conservative host callback journal tracker: a newly *fully committed*
ModelStarted record makes needs_reconciliation TRUE if model_step later fails,
EVEN IF ModelSettled was committed and a subsequent Assistant/TurnFinished
journal write then failed. This covers ambiguous paid model callbacks,
malformed/oversized replies, grant-denied tool proposals, model-settlement
journal loss and an incomplete assistant/turn receipt after a paid request. C cannot silently destroy these unresolved
turns except with explicit discard_unreconciled=1, and the handle never
automatically replays a model or tool request. It is possible for an arbitrary
C host to lie about durability; its journal callback contract requires real
commit+flush before returning OK. Tests use a deterministic synchronous
in-memory mock journal, not a verified production disk.

## Real independent C implementation and test

The C-only test source at
examples/sdk-embedded-ffi-staticlib/c/fixture.c imports ONLY the committed
public C header, no Rust types or Agent implementation. It compiles with
strict C11 -Wall -Wextra -Wpedantic -Werror -O2 -fno-common and links the
real native panic=abort staticlib, THEN ACTUALLY runs through the original
shared Engine. Its independent callback context supplies a deterministic
clock, entropy, model, tool, validation, journal, dynamic source-grant
authorization and explicit host-owned buffers.

Positive: 2 real model callbacks, 1 tool execution, 10 shared kernel events,
exact ordered committed journal kinds 1,2,3,4,5,6,2,3,4,7, original C identity
and source strings copied before stack memory is overwritten, no creation
effects, strict schema/layout/version checks, caller-owned answer/event
buffers, and refused model_step after completion.

Negative: wrong ABI/struct sizes and incompatible tool stride, missing
callback, duplicate source/tool grants, invalid source epoch/kind, malformed
UTF8, invalid/oversized prompt and buffer bounds, invalid JSON, callback
output overflow, no implicit tool grant, source grant revoked after intent
flush, failed tool intent (ZERO effects), missing tool receipt after actual
effect (must reconcile), tool callback failure, ambiguous paid model failures
(must reconcile), failed clock/entropy, cancellation terminal state, reentrant
callback attempts at state and destroy (both BUSY), invalid old/new
model stage and explicit destroy choices. The test currently passes
**710 assertions** in EACH independent debug and optimized release native
C executable. These are isolated deterministic tests, not production
OpenAI/Anthropic/Gemini clients, credential exposure, firmware or concurrency
stress/cross-platform certifications.

Reproduction:
    python3 the removed agentc_e5_c_abi_smoke helper --variant debug
    python3 the removed agentc_e5_c_abi_smoke helper --variant release

Each execution independently checks wasm32v1-none no_std+alloc compile,
the original shared kernel/embedded Agent Cargo dependency closure, fresh
Rust panic=abort native staticlib link, the exact eleven exported C symbols
and C executable ABI sizes, and actual successful C process exit and count.
The native C process does use libc; the underlying portable Rust FFI rlib
does NOT. Host-only unsigned SHA256, target compiler, source Git HEAD + dirty
flag, Cargo.lock/header/ABI/library/C source and final staticlib/C executable
hashes are stored under ignored validation/host/e5/native-{debug,release}.json
and outputs under ignored target/e5. These are machine-local reproducible
evidence rather than shipped releases or global security attestation.

## E5 closeout/gates, honest remaining work

Files: crates/aw-embedded-ffi/{Cargo.toml,README.md,include/aw_embedded_ffi.h,
src/{lib.rs,ports.rs}}, examples/sdk-embedded-ffi-staticlib/{Cargo.toml,
README.md,src/lib.rs,c/fixture.c}, the removed agentc_e5_c_abi_smoke helper and
the removed test_agentc_e5_c_abi helper. Cargo workspace/Cargo.lock add exactly these
two eligible packages. All shared Agent libraries and CLI remain unmodified.
Update .agents/docs/ARCHITECTURE.md, SDK.md, CLI.md if relevant,
BUILD-STATUS.md, VALIDATION-GATE.md, README and START_HERE with this handoff;
preserve master and all preceding E4/R14/R13 records. Linux-x86_64 real C
debug/release tests are conditional native Rust validation gate steps, and
can be exercised by the manually triggered CI job only on compatible
native platforms; workflow source itself is NOT execution evidence.

## E5 local execution / closeout evidence — 2026-10-10

**Scope: independently compiled C ABI consumer on Linux x86-64, portable
Rust no_std+alloc rlib. The native C executable is a normal hosted C
process with libc and is NOT the E4 native libc-free ELF.** The following
reports were produced from the E5 dirty source above the prior E4 commit;
rerun after the local E5 checkpoint to bind evidence to the exact clean
revision. All generated executable/JSON evidence stays under ignored
target/e5 and validation/host/e5.

- Actual native C DEBUG via the removed agentc_e5_c_abi_smoke helper --variant debug:
  **710 independently evaluated assertions**, including the additional
  independent C control proving a failed Assistant journal append after
  ModelSettled STILL requires reconciliation, 2 model callbacks, 1 original
  source-authorized tool, 10 real shared kernel events, and original strict
  journal kinds (1,2,3,4,5,6,2,3,4,7). Verified 11 actual exported C ABI
  functions. Native staticlib **17,505,038 bytes**, SHA256
  **0b55215844905dca06c7939939dbf1ce59b9cef1f1c8c625de8450271bc50593**.
  Linked C executable **5,249,376 bytes**, SHA256
  **b120e84de747966820640742930fba45d5673cdb73b08ae0812e5e6cce925ed3**.
  Actual C process exit 0; stdout is E5_C_ABI_NATIVE_OK checks=710
  model_calls=2 tool_calls=1 events=10.
- Actual native C RELEASE --variant release: same 710 passing independent
  C assertions, two model callbacks, one tool, ten shared events, original
  source/actor/site/epoch and durable journal order, executable exit 0.
  Native optimized staticlib **2,088,598 bytes**, SHA256
  **1132fb762e81b4e867cfc3595c78773b2550b3028b606bac75fb70934482c78f**;
  linked optimized C executable **697,976 bytes**, SHA256
  **35e3d9f4c106b39d150a0674f3677dc7914d37a7af855e9bfbb1177845db68e2**.
  Note final executable size is not the size of only the no_std Engine;
  the C executable includes the ordinary hosted C runtime/staticlib wrapper.
- Positive and negative executable C outcomes cover exact struct size/version
  including rejected oversized tool-array stride; bad encoding/duplicate
  grants/missing callbacks; copied stack identities and granted source labels;
  no callback at create; strict borrowed callback buffer/written ownership;
  callback reentry BUSY (state and destroy) with disjoint Atomic gate and
  exclusive mutable EngineState; caller-owned answer and full ten-event trace
  with BUFFER_TOO_SMALL and required sizes; no-tool denial; incomplete
  model/oversized model reply failures; time/entropy failures; cancellation
  phase checking; tool permission revoked AFTER committed ToolIntent;
  failed ToolIntent prevents ANY effect; failed ToolReceipt after executing
  ONE effect requires inspection; and paid model callback/ModelSettled
  ambiguity is conservatively marked needs_reconciliation and refuses normal
  destroy without explicit host inspection/choice. **No automatic replay or
  privilege widening occurs.** These are isolated fake model/tool callbacks,
  NOT paid production provider security certification or a real disk fsync.
- Portable aw-embedded-ffi --no-default-features --features alloc compiled
  independently with --target wasm32v1-none; no Rust std/Tokio/SQLite/network
  dependency pulled into the portable rlib. E5 Rust native unit tests
  **3/3**, native separate staticlib wrapper built from the same original
  Engine, package-level and full-workspace Clippy with -D warnings passed.
  Cargo.lock includes only the two new owned E5 packages; no modifications
  to the existing aw-agent-kernel, aw-embedded, aw-agent-core or CLI source.
- Unchanged full aw CLI Rust tests **266/266**, JSONL-minimal CLI **95/95**,
  stateless CLI **92/92** and full shared Agent library **117/117** all passed
  with the existing explicit AW_GIT/AW_BASH environment fixture. Existing
  R14 actual compiled full CLI schema/alias/permission **11/11** across
  239 paths and minimal **12/12** across 23 paths also remain green.
- E3 separate actually executed linked zero-import wasm32v1-none Agent
  with the same model→tool→journal turn remains green. The independent
  E4 libc-free static Linux ELF debug and release were ACTUALLY rebuilt
  and executed successfully, including debug wrong-result rejection and
  panic-abort control. The standard E5 native C executable intentionally
  DOES use libc; it does not supersede or conflate E4.
- Cargo --locked --workspace --all-targets check PASSED; complete
  --workspace --all-targets Clippy -D warnings PASSED; Rust workspace
  doctests with RUSTDOCFLAGS=-D warnings PASSED. Scoped rustfmt for new
  FFI Rust and wrapper sources, Python source syntax and git diff --check
  passed. No blanket workspace rustfmt or all-target monolithic workspace
  Rust test green is claimed (historical formatting drift and connector
  limits remain recorded in R13).
- E5 source-only the removed test_agentc_e5_c_abi helper passed **326** structural
  pointer/ABI/permission/negative/handoff assertions. The ORIGINAL immutable
  2026-10-09 history checker passed **709** (10,684 original lines and all
  12 byte-exact verbatim annexes). Existing R14 source checker **197**
  and E4 source checker **203** passed. The SOURCE-ONLY reference validation
  gate PASSED; report
  **validation/host/20261010T121258Z-42428c57e53b/report.json**.
  Its explicit cargo_executed=false/rust_executed=false fields are honest:
  the independently documented actual C/Rust/WASM/native ELF executions above
  are the REAL runtime evidence.
- Manual read-only Linux x86-64 CI is configured for debug+release E5 real
  C process proofs and retained per-host receipts. It has NOT been executed,
  and offers NO native Windows/macOS/ARM/RISC-V or standalone bare-metal
  firmware certification. Original global R13 reference agentc 0.6 parity,
  pinned Rust 1.98.1 release-all, E6 actual boards/drivers, live provider
  integration and independent production-security audit remain open.

**E5 implementation complete as an independently proven v1 portable C ABI;
no general network/firmware, native libc-free C executable, cross-platform
C ABI qualification, production provider billing/security or device driver
claim. Next E6 is hardware-credible firmware/board porting with actual
target-specific no_std core/alloc, entrypoint/allocator/clock/entropy/durable
journal/transport and independent QEMU or board execution receipts. R14.5
breaking removal of old long CLI aliases remains deferred.** Source version
0.15.0, SQLite v33 and historical phase-68 tag unchanged; no push.

**E5 is a portable embedded C ABI, NOT a native libc-free C executable,
not a general asynchronous/multi-user tool runner, and not an audited ABI on
Windows/macOS/ARM. E6 is the next hardware firmware/driver transport phase,
requiring real target stdlib, boot/board or emulator and clock/entropy/host
integration evidence. R14.5 destructive old CLI alias removal remains
DEFERRED. No Phase69, skipped 64/65 indexing/LSP, version/schema/phase-68
retag, provider-billing claim, release publication or push.**
