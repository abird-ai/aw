# R9 — Bounded parallel exact-read tool waves

Date: 2026-10-10. Track: user-authorized unnumbered R9 (A8), one-way agentc → aw parity. Baseline: R8 1be9d4466bf7fdf4eef5e51d40fdf33f504074e7. **Not numbered Phase 69.** Version 0.15.0, SQLite schema v33, immutable Phase68 tag; Phases64/65 skipped. No push.

**Historical authority:** FULL-COMPREHENSIVE-HANDOFF-2026-10-09.md, frozen original master with all annexes/hashes. Its embedded R0–R13 roadmap is original intent; this focused successor documents R9 implementation. Predecessor: [R8 shared kernel](AGENTC-PARITY-R8-EMBEDDED-AGENT-2026-10-10.md). Standard R7 extensions/R6 MCP retain their same exact grants and journal.

## Goal and architectural invariants

Make independent host-classified reads faster without weakening account/provider identity, tool permissions, durable journaling, cancellation, deterministic result order or no-automatic-replay. One hosted Agent/provider/tool/journal engine serves std full and JSONL-minimal. The alloc-capable shared no_std kernel TurnMachine remains sole transition authority. The embedded adapter remains sequential until an actual embedded host needs an explicit safe scheduler.

- A read wave is 2–4 adjacent ToolEffect::Read calls from one already validated complete model tool batch; one read or --parallel-reads=1 uses the original serial E2 path. A host-selected read is not equivalent to a third-party readOnlyHint. Write, Process, Unknown, including untrusted MCP/extension descriptors, form hard barriers and never overlap.
- Validate the ENTIRE model tool proposal batch before first effect: ID uniqueness across rounds, bounds, schemas, current actor/site/source/epoch grants, hooks, worst-case context. Invalid proposals prevent every effect. The kernel also limits read wave size and enforces journal intent-before-effect and ordered receipt-before-next-action.
- Every ToolStarted intent in a wave is durably appended in model order BEFORE any read can start. Account and journal source binding are validated after flush, and each individual future again checks current origin and grant before entering tool code. If an intent append, callback or authorization fails, close every acknowledged intent with an Interrupted receipt where possible; persistence failures require explicit restart/recovery.
- The std Agent uses FuturesUnordered with at most four owned, nonspawned Futures, with the existing provider overall deadline plus per-tool timeout. On error/cancel/timeout, all unfinished futures are dropped and their cancellation tokens signaled. No detached tasks and no automatic effect replay.
- Hold bounded results privately until the ENTIRE read wave has completed and permissions are revalidated. Write ToolFinished receipts, model-visible context messages, post-tool hooks and events in original provider proposal order, regardless of actual read completion order. A host can revoke a source while a wave is running; the Agent discards its results and prevents further provider disclosure.
- A typed ToolOutput with is_error=true is a completed tool error result, not a transport/tool-execution Err. Failed or interrupted reads never leak partial model context. Earlier Write/Process/Unknown attempts still trigger the shared kernel reconciliation/no replay rule.

## API and CLI

The shared portable crate crates/aw-agent-kernel/src/turn.rs adds a MAX_READ_WAVE=4 constant, pure read_wave_len classifier and bounded read-wave phases, explicit durable intent acknowledgement, wave launch, settlement and original-order receipt transitions. Core-only kernel without alloc remains unchanged.

The existing crates/aw-agent-core/src/agent.rs drives the same TurnMachine through concurrent std futures. ToolPolicy.max_parallel_reads defaults to 4, with host validation 1..4. Agent::set_max_parallel_reads exposes an owner-controlled setting. Full and JSONL-minimal CLIs expose --parallel-reads 1..4 (default 4) and reapply it on model/config/effort rebuilds. It only affects explicitly granted tools; --tools and --tool-allow remain the authority controls.

Example:

    aw --profile llama --model MODEL --tools read-only --parallel-reads 4 --print "Compare four authorized files"
    aw --profile llama --model MODEL --tools workspace --parallel-reads 1 --print "Use strictly sequential tools"

Built-in read/ls/find/grep can execute concurrently when the provider proposes them as a single complete batch. Write/edit/bash do not. The kernel, no_std embedding, provider adapters, SQLite/JSONL storage, MFA/Account grants and UI are not duplicated.

## Actual acceptance

1. Shared core no_std+alloc TurnMachine tests verify bounded waves; incomplete intents cannot start effects, incomplete receipts cannot trigger next tool/model; Unknown/Write/Process are barriers. Core-only and alloc wasm32v1-none compiles and existing E1 zero-import consumer remain valid.
2. crates/aw-agent-core/tests/parallel_read_waves.rs uses actual Tokio barriers and real native Agent/Provider/Tool/Journal to PROVE 4 concurrent reads, 5th read waiting, original result and receipt order, serial write/process/unknown barriers, complete batch preflight, typed error outputs, ordered after hooks, revocation during intent flush or in-flight, storage failures, cancellation and simulated tool crash with no detached tasks or automatic replay. R8 portable hosted/embedded 10-event parity test remains green.
3. the removed agentc_r9_parallel_smoke helper runs independently against freshly rebuilt full and JSONL-minimal aw binaries and an isolated fake OpenAI streaming tool-call provider. One model response contains 4 distinct tool calls: read/ls/find/grep or mixed read/write/read. Assert exact real tool_call_id pairing and ordered model inputs, explicit --parallel-reads 4/1 and out-of-range rejection. No live provider or credentials.
4. Full and minimal CLI native test suites, inherited R6 modern/legacy MCP R7 independent C/Rust process extension, JSONL no replay, Phase2 tools and native PTY. Whole workspace compilation, Clippy warnings denied, Rust docs, source/handoff/reference validation. Never claim reference-only gate as a native runtime proof.

## Deliberate deferred scope

- The separate aw-embedded E2/E3 host drives the same kernel sequentially; it has no invented async executor. Future hardware with a real scheduler can use bounded read-wave transitions without a copied Agent loop.
- Never parallelize Write/Process/Unknown, promote remote advisory readOnlyHint, auto-retry providers or ambiguous effects, grant new tools, persist trust decisions, start daemons, or bypass existing context/trace limits.
- Cooperative Future cancellation cannot forcibly stop misbehaving blocking native code. Such hosts require independent OS/process containment. No claims for real paid providers, full cross-platform release, pinned Rust 1.98.1, firmware ELF libc audit or independent security certification.
- Next phase R10: bounded context compaction and safe provider-attempt policy. Optional R2b, R11–R13, E4–E6 remain open. Never start historical Phase69 or revoked indexing/LSP without explicit user direction.

## Verification ledger

**Verified in the current R9 worktree against actual source and independently compiled binaries:**

- Shared aw-agent-kernel tests: 10/10, including four-read ceiling, incomplete journal intent/receipt barriers, Unknown/Write/Process serialization and cancellation; core-only and alloc wasm32v1-none continued to compile.
- Actual hosted Agent R9 Tokio/Provider/Tool/Journal adversarial integration: **14/14**. Barriers prove real concurrent reads, fifth-read queueing and out-of-order tool completion; tests prove exact ordered receipts/model context/hooks, Process/Write/Unknown barriers, sequential=1, journal intent and receipt failure, source revocation both before wave and mid-flight, cancellation without detached tasks, typed tool error vs transport error, and post-read veto **after** all completed factual receipts were persisted. R8 standard-vs-embedded actual decision-trace parity stays **2/2**.
- Freshly rebuilt full **and** JSONL-minimal binaries: R9 mock multi-tool OpenAI SSE integration **5/5 each**, R6 MCP data/notification real-binary fixture **32/32 each**, and R7 independent C/Rust process-extension fixture **21/21 each**. Per-mode --parallel-reads 1/4 validated through actual CLI; invalid 0 rejected before networking.
- Existing Rust main CLI suites after implementation: minimal **88/88**; full **260/260** plus shared lib **108/108** (exported AW_GIT and AW_BASH Nix trusted executables for the existing subagent fixture). Previously run minimal JSONL Session resume **6/6**, Phase2 rooted tools and Inline/Scrollback/Fullscreen terminal **3/3**, R5 modern/legacy MCP **11/11 each** remain green.
- R8 linked no_std+alloc WASM **actually executed** a provider/tool/journal/final-answer turn under Node using installed rustc **1.95.0**, and linked with **zero host imports**; E1 core-only WASM consumer also zero imports. The reusable kernel does not link a native OS/runtime because of R9.
- R9 source-only checker **141/141**, whole-workspace/all-target Cargo check and Clippy with -D warnings **passed**, workspace Rust docs with RUSTDOCFLAGS=-D warnings **passed**, all focused new/changed standalone modules rustfmt --check and git diff --check **passed**. Native monolithic workspace/all-target Rust test and repo-wide blanket rustfmt were **not claimed**; the affected Agent/kernel and both CLI suites were separately tested and the large historical main_full formatting drift is unchanged.
- Frozen October 9 complete handoff integrity **700/700**, preserving its **10,684 lines / 12 exact verbatim annexes** and immutable Phase68 tag. R9 whole reference profile **passed**, report at validation/host/20261009T223313Z-d9ff6372f7fe/report.json. Its own manifest explicitly states it is SOURCE/reference-only, not Cargo/Rust runtime proof; actual Rust CLI, concurrency, C/Rust process, MCP and linked WASM were verified separately.
- No live paid provider, actual external security audit, pinned 1.98.1 runtime proof, optimized Nix crossbuild/native libc-free ELF proof, or production device attestation claimed. A single monolithic cargo test --workspace --all-targets result is not claimed unless separately observed; changed crates and primary suites have been tested independently.

R9 is ready for a single unnumbered local commit: version 0.15.0 and SQLite schema v33 unchanged, no new persistent grant, release tag, numbered Phase69, skipped indexing/LSP or Git push. Next is R10 compaction/retry unless user reprioritizes.
