# R8 — one portable Agent turn kernel and host-driven embedded no_std execution

Date: 2026-10-10. Status: implementation + R8 native/WASM fixtures validated; local checkpoint pending. Track: unnumbered one-way agentc → aw parity R8, implementing E2/E3 of the embedded architecture. Source baseline: R7 adf023cf0f298a051534bed267b7abc0b33eb91e. Rust source 0.15.0; SQLite schema v33; historical Phase68 tag at 953d6e1905d9968cce74137b4705b78d6f1aa49b. Not Phase69.

Historical authority: FULL-COMPREHENSIVE-HANDOFF-2026-10-09.md, immutable, including all historical annexes and hashes. Current-focused predecessor: [R7 extensions](AGENTC-PARITY-R7-EXTENSIONS-2026-10-09.md). Design source: [embedded no_std/libc plan](AW-EMBEDDED-NOSTD-LIBC-FREE-ARCHITECTURE-2026-10-09.md), [R0–R13 roadmap](AGENTC-PARITY-IMPLEMENTATION-PHASES-2026-10-09.md). Do not rewrite frozen handoff or skipped Phases64/65.

## Objective and key architectural decision

This is the first **functional embedded Agent turn** running the SAME turn/permission/intent/receipt state machine as the hosted std Agent. Keep full/minimal CLI, provider transports, SQLite/JSONL, web and TUI standard Rust. Do not create a second embedded Agent/tool policy engine, second store, alternate HTTP client, no_std libc shim or a negative no_std feature.

There are now three separate tiers:

- Tier K (already E1): aw-agent-kernel with --no-default-features, core-only no heap, borrowed capability Source/Grant/Catalog predicates.
- Tier E (R8 E2/E3): aw-agent-kernel with alloc exposes bounded TurnMachine; aw-embedded --no-default-features --features alloc exposes host-driven provider/tool/journal/clock/entropy ports and one real typed turn via caller-driven steps. Neither library has a host allocator, panic handler, libc/Tokio/Reqwest/filesystem/SQLite dependency.
- Tier S (normal, unchanged): aw-agent-core owns accounts, full conversations, async providers, hooks, ToolPolicy/ToolRegistry, SQLite/JSONL journaling and actual effects. Its real prompt_as loop now calls the SAME TurnMachine in every model round, batch, pre-intent, intent acknowledgement, pre-effect authorization recheck, tool settlement and result receipt step. No other independent production scheduling rule is added.

The optional independent aw-embedded-example implements fake host callbacks using no_std+alloc. It is NOT a new default CLI, background listener or provider account. An embeddable host must deliberately supply its own verified model transport, authenticated account, durable journal commit+flush, secure entropy, monotonic clock, current exact-sourced tool registration/grant, tool schema validation and actual tool executor. Missing capabilities fail closed. Installing a package does not start a model/tool call.

## Portable turn rules

The shared deterministic aw-agent-kernel::turn::TurnMachine is not coupled to Tokio, std or a task poller. It carries turn ID, next model ordinal, bounded model/tool budgets, unique tool IDs across rounds, current atomic batch and tool index, phases, and whether a non-read tool intent may require reconciliation. The stable event sequence for a successful one-tool turn is:

    ModelStarted(1)
    BatchProposed(1)
    BatchAdmitted(1)
    ToolIntent(0)
    IntentRecorded(0)
    EffectStarted(0)
    EffectSettled(0)
    ReceiptRecorded(0)
    ModelStarted(2)
    TextCompleted

This is not arbitrary logging: both runtime paths advance the actual one state machine or reject invalid transitions. Entire batches must pass ID/budget and current schema/actor/site/grant checks before first effect. Intent persistence must be acknowledged before start_effect; the grant must be rechecked after journaling. Tool receipts must be committed before next tool or paid model round. A model fragment, duplicate ID, invalid transition, missed deadline, canceled or ambiguous effect is terminal until explicit host inspection. No automatic provider/tool retry. Conservative unsafe-intent flag is set for Write/Process/Unknown, including a failed durable append attempt, and used by hosted recovery.

The hosted Agent also exposes Agent::portable_trace() for its most recent real turn: bounded data-free ephemeral Event values, cleared when history is replaced or reset, never persisted as authority. A new test directly compares the actual hosted execution trace with independent embedded execution, plus verifies that failed durable receipts cannot be replayed from the hosted Agent.

## Embedded host contracts and exact scope

aw-embedded::EmbeddedTurn::new() is inert: no clock, RNG, model, I/O, journal or allocation-heavy background work. The embedder drives:

    let mut turn = EmbeddedTurn::new(turn_id, Identity{...}, Options::default())?;
    turn.start(&mut host, "Prompt")?;
    // caller selects each step based on Progress, no hidden Agent loop:
    turn.model_step(&mut host)?;
    turn.tool_step(&mut host)?;
    turn.model_step(&mut host)?;

HostPorts has seven precise ports: monotonic_ticks, secure_entropy, append (durable acknowledgement), model (host-selected verified identity), registration (exact current Capability + Grant), validate_tool (host schema/semantics), execute_tool (host effect). The embedded adapter never accepts tool grant/identity from a model response. It uses the same E1 Grant::permits actor/site/name/SourceKind/source-epoch/Effect check and the same E2 TurnMachine state transitions as std. All prompt/arguments/messages/output and event trace data are bounded and major operations have explicit clock/deadline gates. Synchronous host callbacks must enforce their own in-flight deadline because no embedded scheduler can preempt a callback. A host's journal must really commit+flush before returning success. Test fake journals use deliberate in-memory acknowledgement; this does NOT establish device flash durability.

This R8 slice supports ordinary typed text and host-defined JSON tool calls/results, not embedded signed-provider continuations, persistent Session resume/ContextFlow, subagent/guardian/planner, MCP transport, streaming tokens, asynchronous multi-tool execution, rich media or arbitrary installed extensions. Standard hosted product retains all its existing behavior. Future E4–E6 plus R9/R10 are separate.

## Real libc-free evidence and independent host

- Independent example: [examples/sdk-embedded/src/lib.rs](../../examples/sdk-embedded/src/lib.rs). The fake host has a deterministic monotonic clock, fake nonce, intentionally in-memory append/flush, model callback, exact Extension-source grant and one Unknown-effect tool. It exercises a real complete no_std+alloc text+tool turn, verifies journal ordering and returns a stable success code only if all effects and ten kernel transitions occurred.
- Real WASM harness compiles the noalloc kernel, alloc kernel, aw-embedded no_std+alloc and independent no_std consumer for wasm32v1-none with Cargo.lock, links a **full no_std cdylib** with host-only unsafe bump allocator/panic implementation and linker, inspects zero WASM imports, then executes the real model/tool/journal flow in Node WebAssembly VM. The reusable aw crates and consumer forbid unsafe; only this disposable host defines unsafe global allocation/panic. The final module has **no libc/OS host function imports**. Node is a test interpreter, not a dependency of the module or embedded Agent.
- Companion native [parity test](../../crates/aw-agent-core/tests/portable_embedded_parity.rs) runs an actual hosted Provider→Agent→Tool→Journal and independently drives the embedded host; their emitted portable transition traces and receipt ordering match. The hosted receipt-flush failure is also checked for sticky reconciliation/no replay.
- The live toolchain is Nix rustc **1.95.0**, not the repository-pinned 1.98.1. This is an actual built+executed import-free wasm32v1-none image under the installed version, not a claim about the missing pinned Rust, native ELF/ARM targets, C ABI, real firmware or production provider transport.

## Verification and closeout rules

Required gates:

1. cargo test --locked -p aw-agent-kernel; cargo test --locked -p aw-embedded; cargo test --locked -p aw-embedded-example; cargo test --locked -p aw-agent-core --test portable_embedded_parity.
2. python3 the removed agentc_r8_embedded_smoke helper must link a real final no_std+alloc WASM with zero imports AND execute its exported real turn in Node; no static no_std label/rlib build suffices.
3. Native hosted Agent/core tests and separate fresh full/JSONL-minimal aw binaries: both inherited MCP R5/R6, terminal/JSONL replay gates, portable parity and CLI suites. Whole-workspace check/Clippy warnings-denied, workspace docs, changed-file rustfmt, source-only reference gate.
4. Freeze original 2026-10-09 handoff verbatim and check all its original hashes against frozen commit. Add R8 as a separately approved successor, not a mutation of original annexes. No new phase tag, schema migration, automatic provider retry or Git push.
5. Be explicit about missing proof (pinned 1.98.1, native libc/ELF, C FFI, real firmware/scheduler, independent security audit). Monolithic cargo test --workspace --all-targets should only be claimed when it actually returns results, not if connector errors.

## Next phases

R9 bounded parallel read-only tool waves (still shared kernel, same exact effect journal); R10 safe automatic compaction/retry and long-lived durable Session; R11+ richer typed extension lifecycle (provider/status/command) if selected; R12 UI/UX/presentation parity; R13 portable release/policy validation. E4 fixed buffers, E5 optional C ABI and native libc-free ELF/ARM, and E6 cross-profile release certification remain independently gated. R2b Windows/external search optional. The no_std host is NOT a second CLI or a reason to remove standard Rust.


## R8 implementation and verification ledger (live worktree)

**Implemented and actually exercised:**

- The alloc-enabled shared TurnMachine owns model-round/batch limits, global model-proposed call ID uniqueness, entire batch admission, exact tool ordering, intent-before-effect, current grant recheck after journal, result receipt-before-next action, and conservative failed unknown-effect reconciliation. Hosted Agent delegates all those steps; tool MAX_BATCH is sourced from one kernel constant. The independent no_std+alloc host uses the same kernel and its E1 Source/Grant::permits. No second Agent/tool policy engine.
- The embedded constructor is inert. A deliberate host start + two model steps + one tool step completes a fake provider/tool/journal turn. The host port traits model secure nonce, monotonic clock/deadline, model, durable append, typed tool registration/grant, schema validation and executor. Missing clock, regressed clock, missing/zero entropy, missing journal, ungranted tool, tool grant revocation during journal flush, failed tool intent, failed receipt and explicit cancellation fail closed without automatic replay.
- Shared-kernel native Rust tests: **7/7**. aw-embedded native: **7/7**. Independent E3 SDK example: **1/1**. Actual standard hosted vs independently driven no_std embedded event trace parity and failed hosted receipt no-replay: **2/2**, matching **10/10** event values.
- Actual final no_std wasm32v1-none module: code built using Cargo.lock and installed rustc **1.95.0**, WASM linked with a disposable host-owned global allocator and panic implementation, **314,463 bytes after debuginfo stripping, zero function/import dependencies**, and **executed** through Node WebAssembly VM: returned **0xA80E0001** only on successful full mocked turn. the removed agentc_r8_embedded_smoke helper also verifies core-only and alloc kernel cuts and the embedded library/consumer targets. Earlier R1 linked noalloc kernel consumer stays zero-import. The module SHA-256 differs between temp links despite stable size/behavior; do NOT claim deterministic/reproducible release bytes.
- The hosted standard Rust CLI native suites after extraction: **JSONL-minimal 88/88**, **full CLI 260/260** and shared full library **108/108**. The initial full run was **255/260** due this Nix sandbox lacking /usr/bin/git; rerunning with repository-supported exported AW_GIT and AW_BASH trusted absolute executables yielded full **260/260**. That was an environment fixture issue, not a product regression.
- Actual independently rebuilt full AND minimal binaries: inherited R7 process tool/hook C/Rust fixture **21/21 each** and R6 MCP resources/notifications **32/32 each**. Minimal additionally passed R5 modern MCP **11/11**, R5b legacy SSE **11/11**, JSONL resume/no replay **6/6**, Phase2 rooted tools and actual native Inline/Scrollback/Fullscreen PTYs **3/3**.
- Whole workspace/all-target Cargo **check passed** and warning-denied **Clippy passed**. Workspace Rust docs with RUSTDOCFLAGS=-D warnings passed. All new/focused Rust modules passed rustfmt --check and git diff --check. R8 source-only structural checker **207/207**, historical Oct-9 full handoff integrity **699/699** (master **10,684 lines / 12 unchanged annexes**); the full reference profile **passed**. The reference profile is SOURCE-only; its report explicitly does not claim Rust execution; Rust and WASM tests above were independent native gates.
- Native monolithic cargo test --workspace --all-targets, optimized Nix/Windows/macOS releases, pinned 1.98.1 target compiler, native libc/ELF symbol audit, C ABI, verified durable device flash, real provider/TLS/authentication, and a third-party external security review were NOT run/NOT claimed. Do not promote zero-import WASM proof into blanket production firmware/complete aw no_std certification.

**History constraints:** Frozen 2026-10-09 master byte-for-byte unmodified; its original hashes and Phase 0–68 chronology retained. No Phase69, Phase64/65 indexing/LSP revival, schema migration, source version bump, auto-replay, or Git push. Next phase R9 is bounded parallel tool batches on this common machine. E4–E6 and richer embedded ports remain separate gated work.
