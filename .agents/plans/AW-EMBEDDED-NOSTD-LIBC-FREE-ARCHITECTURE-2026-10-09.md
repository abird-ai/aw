# aw embedded: libc-free and no_std-native architecture

**Date:** 2026-10-09
**Status:** user-requested additional architecture requirement and implementation plan; NOT yet built.
**Scope:** embeddable aw Agent engine only. Standard aw CLI, TUI, Web, SQLite and distributed functionality remain Rust std-first.
**Current source:** aw 0.15.0 at plan base 31cfbd6; SQLite schema v33; latest numbered phase 68. Existing minimal JSONL build is std-based.
**Parent authority:** AGENTC-RUST-NATIVE-PARITY-MASTER-PLAN-2026-10-09.md and the current full handoff. This document strengthens, but does not replace, the one-way agentc functional parity objective.

**Execution progress (2026-10-09):** [AGENTC-PARITY-R1-KERNEL-2026-10-09.md](AGENTC-PARITY-R1-KERNEL-2026-10-09.md) adds the real shared core-only capability/grant kernel to aw-agent-core, plus an independent no_std consumer and linked wasm32v1-none no-import test. **Only E1 portable capability rules have source evidence**, not a no_std complete Agent; E2–E6 and pinned Rust 1.98.1 target validation remain outstanding.

## 1. Distinguish the promises

- **no_std** means the crate does not automatically link Rust std and relies on core, optionally alloc.
- **core-only** means no allocator, heap, std or OS assumptions. Fixed-size caller-owned buffers and pure protocol/state primitives only.
- **no_std + alloc** means Rust core+alloc with a host-supplied global allocator/panic policy at final link. This is the intended functional embedded Agent tier.
- **libc-independent library** means that the compiled library neither relies on libc/CRT symbols nor inserts dependencies that secretly require it.
- **libc-free final image** means the entire host+Agent+runtime linked artifact has no libc dependency; this requires symbol/import/ELF or WASM inspection, not just a successful crate compile.
- **static Linux musl** is NOT synonymous with libc-free. It can include statically linked musl, despite showing no shared libc at runtime.
- The std-based minimal agentc-parity profile is different from the embedded no_std profile. Full agentc functional parity is assessed on the former. Embedded capabilities also depend on which host services are actually supplied.

Neither a no_std attribute nor the absence of a direct libc Cargo dependency proves libc-free output. The normal aw application must not lose std features in pursuit of embedded support.

## 2. One shared Agent kernel — no independent embedded engine

Proposed source dependency flow:

    aw-agent-kernel (proposed portable shared crate)
       core-only: deterministic state, capability/grant rules,
                  typed transitions/events, effect ordering
       alloc:     bounded Agent context and tool state,
                  JSON/message data, pending operations
           /                               \
          /                                 \
    aw-agent-core (std)                aw-embedded (no_std)
      current std API, Tokio             host callback/driver adapter
      full/minimal CLI/Workspace         Rust rlib, optional C staticlib
      Reqwest/standard platforms         no std/libc in library
      SQLite or JSONL storage            host journal/transport/executor

**One semantic model/tool loop must remain authoritative.** Today crates/aw-agent-core/src/agent.rs, conversation.rs, provider.rs and journal.rs use std, Tokio, std-owned allocations and async trait objects. This is genuine refactoring work. Introduce a platform-neutral state/transition kernel only if the existing hosted Agent delegates to that very implementation in the same verified slice. Do not leave the hosted and embedded implementations to diverge.

We may initially keep aw-agent-kernel as an internal module, but a distinct crate is justified as soon as BOTH aw-agent-core and the embedded no_std host consume it. Do not prematurely create parallel aw runtime/tool registries or a shadow Session implementation.

## 3. Three supported tiers

| Tier | Environment | Scope |
| --- | --- | --- |
| K: core-only | core, no alloc/std/libc; no threads or OS | Stable typed IDs, permission/capability predicates, bounded state steps, buffer-based framing and validation |
| E: no_std + alloc | core + alloc; embedder supplies allocator and all effects | A functional bounded Agent; provider/tool/context handling through injected ports; libc-free link when host is libc-free |
| S: std default | std + Tokio + standard ecosystem | All existing CLI/TUI/Web/workspace/distributed features, preferred implementations for FS/network/OS/threading/alloc |

Tier K is not required to implement arbitrarily long conversations or JSON documents without alloc. Tier E is the main embedded capability goal. Tier S must continue to use std::fs, std::time, std::process, standard synchronization, Tokio, Reqwest, mature parsers and existing platform crates rather than recreating these libraries.

## 4. Cargo design — positive features

Follow the official additive Cargo feature pattern. Do NOT implement a negative no_std feature. A future portable crate's manifest should use:

    [features]
    default = ["std"]
    std = ["alloc", ...optional_dependency_std_flags]
    alloc = [...optional_dependency_alloc_flags]

and its source entry should use:

    #![cfg_attr(not(feature = "std"), no_std)]
    #[cfg(feature = "alloc")]
    extern crate alloc;

No_std library selection uses --no-default-features; Tier E additionally selects --features alloc. The root aw CLI's existing --no-default-features --features minimal remains a **std** binary with JSONL. No universal root-level no_std CLI flag is needed; build the proposed aw-embedded library with the portable kernel instead.

Rust Cargo feature unification is additive. Requiring a no_std build of every workspace member would implicitly pull in std through dev-dependencies/full-feature consumers. Verify isolated package + target closures and avoid accidental std default features from Serde/JSON/hash/crypto dependencies.

No new crate is justified solely to claim a successful empty no_std compile; a future no_std library must run real shared Agent logic and receive host-created tool/model results.

### Target compile commands — FUTURE, not currently supported

    cargo check --locked -p aw-agent-kernel \
      --target wasm32v1-none --no-default-features

    cargo check --locked -p aw-agent-kernel \
      --target wasm32v1-none --no-default-features --features alloc

    cargo check --locked -p aw-embedded \
      --target wasm32v1-none --no-default-features --features alloc

    cargo test --locked -p aw-agent-core
    cargo build --locked -p aw-cli --bin aw \
      --no-default-features --features minimal
    cargo build --locked -p aw-cli --bin aw --features full

The current Nix connector has rustc/cargo 1.95.0 with wasm32v1-none already installed. The repository's rust-toolchain.toml requests 1.98.1, so actual release proof must use the pinned toolchain. x86_64-unknown-none and bare-metal ARM target components are not installed here; do not silently substitute a different target and call that proof.

## 5. Host-port contracts — small, optional, explicit

Do not reimplement libstd or build a giant Host interface with unsupported dummy functions. Design small Rust traits and a capability manifest consumed by the same shared Engine.

| Portable port | std implementation (default) | no_std implementation (embedder-provided) |
| --- | --- | --- |
| Clock | std::time::Instant / Tokio timer | hardware monotonic clock callback |
| Entropy | OS RNG / approved crate | host hardware RNG; failure if unavailable; never weak PRNG |
| ModelTransport | existing aw-providers, Reqwest and TLS | supplied authenticated streaming transport/RPC/TLS host callback |
| Journal | existing JSONL and SQLite stores | durable append/flush host contract, or explicit memory-only mode |
| ToolExecutor | aw-tools, Tokio/process/secure rooted filesystem | host-provided bounded tools and effects, or unsupported |
| Scheduler | Tokio task cancellation/timers | embedder-owned cooperative executor/event loop |
| CredentialAccess | existing SecretStore and local credential references | host-backed credential handles, not plaintext Session data |
| ResourceReader | std filesystem, explicit project trust/provenance | host-provided ROM/flash/byte buffer resources |
| Output | terminal/browser/headless typed event sinks | callback/ring buffer/UART; no raw authority |
| Memory | std allocator and Rust collections | host global allocator (E), caller buffers (K) |
| Panic/abort | std process strategy | embedding target's panic handler/reset policy |
| Identity/trust | native auth/OIDC/Account grants | host-verifiable principal and exact policy/grant IDs |

Missing host capabilities return an explicit Unsupported/Unavailable result. Tool registration remains separate from tool authorization. Lack of network never silently picks another account, executor or provider.

### Port interface mechanics

Prefer core::future, core::task and borrowed slices/generic state machines in the kernel, without Tokio handles. Use async I/O in host adapters. Avoid allocating a boxed future per token where an enum/generic future is sufficient. A no_std embedder may be single-threaded and should not have to implement Sync+Send everywhere by default; std wrappers retain thread safety and Tokio concurrency. Keep any target-required critical sections or atomics out of portable pure state.

For I/O, prefer established embedded-io and embedded-io-async traits plus adapters, rather than a custom byte-stream protocol layer. Only add an adapter if a real consumer needs it. Keep std::io/Tokio paths for the hosted build.

Serde and serde_json support alloc-only no_std when configured with default-features=false and features including alloc. Reuse them for Tier E where suitable. Tier K uses borrowed validated slices or small fixed buffers; do not force a second bespoke JSON parser across the product.

Bound memory, message counts, tool calls, frames, JSON depth, retries, summaries and time. All host callbacks return typed failures and cannot invent completed results. A failed journal flush must prevent the subsequent external effect; a lost callback cannot imply the effect never occurred.

## 6. Embeddable ABI and memory ownership

### Rust host

Expose the shared Engine with typed HostPorts; the embedder calls admit/submit, poll/drive, cancel, observe, inspect receipt and drop. The same state machine drives std aw-agent-core. No resource grant or provider credential restored from model text/Session history.

### C host (optional separately validated adapter)

Build a versioned C ABI around a no_std artifact only when there is an independent C consumer:
- opaque engine handle, ABI version and struct size for forwards compatibility;
- pointer + explicit byte length for data, no Rust layout/types over FFI;
- host supplies allocator or memory buffers, with paired free owner, alignment and lifetime rules;
- no panics/unwind across ABI, stable error status and bounded diagnostics;
- explicit reentrancy and thread-affinity contract, no storing borrowed callback buffers;
- exact event and receipt ordering; destructor/cancellation cannot claim rollback of uncertain effects.

A no_std library built as staticlib might have unresolved host memory/runtime symbols until final link. Library-level independence and final host executable libc-freedom are audited separately. Do not install a global panic handler or allocator in the reusable kernel; the host owns them.

## 7. libc-free proof and supported targets

Primary no_std target: **wasm32v1-none**, supported with core+alloc but no std, and intentionally imports nothing from the host. It gives a strict target where accidental std usage fails compilation, and final linked artifact imports can be inspected.

Native bare-metal follow-up: **x86_64-unknown-none**, a standard no_std target with no default allocator; separately inspect ELF headers, symbols and linked entrypoint. ARM follow-up: **thumbv7em-none-eabi** or a supported platform chosen by a real embedder.

Optional standalone Linux libc-free executable: **x86_64-unknown-linux-none** is a separate Tier-3 target requiring extra build infrastructure and no shipped core/alloc. A runnable standalone raw-syscall Linux process is NOT necessary for initial embedding and must never be confused with static-musl packaging.

Accept a libc-free claim only with:
1. real Agent state machine compiled in Tier K and Tier E while std is unavailable;
2. a linked no_std executable/WASM harness exercising a real fake-provider turn and journal/tool callbacks;
3. final WASM imports, ELF needed libs, undefined symbols, libc/CRT references and compiler builtins inspected;
4. appropriate host allocator/panic/stack/alignment validation; no helper secretly requiring libc;
5. same behavioral trace as the std Agent for identical fake transport and tool decisions;
6. clear record of selected target/toolchain/Cargo.lock and the actual artifact hashes.

An rlib compiling alone is **not** enough. A static musl binary with no DT_NEEDED does not prove libc-free if it incorporates musl itself.

### Current toolchain feasibility evidence (2026-10-09; NOT an aw proof)

Using the available Nix Rust **1.95.0** sysroot and an explicitly found Nix wasm-ld binary, a disposable **standalone** no_std core-only rlib and a standalone no_std + alloc rlib both compiled with warnings denied for wasm32v1-none. A separate no_std panic-handler probe linked as a **311-byte wasm32v1-none module with no import section**. Temporary inputs were cleaned up. This demonstrates the present environment has a usable target and linker, but **none of those inputs contained aw code**. Therefore this is only E0 target feasibility and does not advance E1–E6 implementation/certification. The repository requests Rust 1.98.1, which has not yet been tested for this target on this host.

### R2 portable search research update (2026-10-09)

Do not link fd or rg CLI programs into no_std. Std aw-tools now uses ignore/globset/regex library components while all file enumeration/read passes through aw-platform's scoped Root. For a future embedded host, host callbacks supply paths and bounded bytes. We separately compiled a disposable regex-automata 0.4.18 no_std+alloc matcher (features alloc, syntax, meta) for wasm32v1-none on Rust 1.95.0, offline with no std dependency. That is a **temporary dependency feasibility test**, not a compiled aw-embedded search host or end-to-end libc-free Agent. Precompiled DFAs may support allocation-free matching under stricter feature cuts later. No custom regex parser, Gitignore engine or second Agent loop is approved.

### Native Gemini adapter and embedded boundary (2026-10-09)

R3b [AGENTC-PARITY-R3B-GEMINI-2026-10-09.md](AGENTC-PARITY-R3B-GEMINI-2026-10-09.md) adds native Google GenerateContent to aw-providers using standard Rust/Reqwest/SSE. It does **not** link networking, TLS, provider sessions or CredentialStore into aw-agent-kernel core-only no_std. A future E2/E3 host-driven Agent can reuse portable model/tool/content authority types, but no full libc-free Agent or Google embedded-network stack is claimed.

### R4 model catalog/embedded boundary (2026-10-09)

[AGENTC-PARITY-R4-MODEL-CATALOG-2026-10-09.md](AGENTC-PARITY-R4-MODEL-CATALOG-2026-10-09.md) uses standard Rust filesystem/HTTP/SecretStore user metadata for descriptive model discovery and cache. The portable core-only aw-agent-kernel and future no_std+alloc host must not depend on these std services or bundled commercial rates; a host may supply only permitted verified model capability metadata to the shared Agent transition kernel. R4 adds no libc-free Agent build or embedded network/OS dependency.

### R5a stdio/Streamable HTTP vs embedding boundary (2026-10-09)

[AGENTC-PARITY-R5A-MCP-TOOLS-2026-10-09.md](AGENTC-PARITY-R5A-MCP-TOOLS-2026-10-09.md) adds rmcp 3.5, Tokio process and Reqwest 0.13.2 **only to a standard Rust aw-mcp crate**. The existing no_std portable capability source/epoch/grant model is reused through hosted aw-agent-core. No MCP/TLS/process std dependency is added to core-only aw-agent-kernel, and no separate Agent loop, libc-free MCP transport or embedded device network implementation is claimed.

### R5b deprecated MCP HTTP+SSE is standard-host-only (2026-10-09)

[AGENTC-PARITY-R5B-LEGACY-SSE-2026-10-09.md](AGENTC-PARITY-R5B-LEGACY-SSE-2026-10-09.md) adds a Tokio/Reqwest long-lived SSE/POST bridge only to standard aw-mcp. It reuses the canonical no_std-friendly capability SourceKind::Mcp/exact authority predicates through the existing hosted aw-agent-core, without importing networking, TLS, libc, Tokio or MCP protocol engines into the core-only kernel. No full embedded no_std Agent or libc-free final executable is certified by these transport tests.

## 8. Acceptance matrix and regression gates

- Core-only portable crate compiles with no std and no alloc.
- Alloc-tier portable Agent compiles with no std on wasm32v1-none.
- The embedder runs one meaningful end-to-end tool-bearing Agent request: begin receipt, provider turn, tool proposal, authorization, effect intent, tool result, final answer, cancellation/recovery.
- std and embedded traces have matching semantic sequencing, tool-call/result pairing, policy checks, and conservative no-replay handling (timing and host-defined details may differ).
- A host with missing network/clock/entropy returns explicit failure before effects and never supplies insecure defaults.
- Out-of-memory/fixed buffer overflow yields safe refusal where feasible; no silent truncation of important authority fields.
- C host independent test (if FFI is shipped) checks ABI, callback ownership, thread affinity and FFI no-unwind.
- No allocation, model/network call, filesystem crawl, subprocess or daemon solely from creating an engine without an explicit host request.
- Standard minimal/full native tests, Clippy, doctests, PTY/browser, release manifest and actor/account/ContextFlow security regression tests remain unchanged.
- No new filesystem indexing, watcher or LSP: explicitly skipped in Phase 64/65.

Use optional no_std target gates only when portable kernel/embedding code changes. Ordinary daily development keeps cheap std-native tests; full platform/libc audit remains opt-in CI / release profile. Missing target toolchains are blockers, not reported successes.

## 9. Phases integrated with the agentc plan

| Work item | Design/implementation | Required proof |
| --- | --- | --- |
| E0 (NOW, planning only) | Establish boundaries, tiers, Cargo additive feature contract, host ports, proof standard | Reviewed, linked plan; no runtime changed |
| E1 (alongside A1) | One core-only typed capability/grant registry, consumed by std Agent and a no_std test host | Rust unit tests + wasm32v1-none core-only compile |
| E2 | Move existing Agent event/turn/tool transition rules into shared no_std+alloc kernel; hosted Agent delegates | Exact std/embedded trace parity, no alternative loop |
| E3 | Implement host-driven model/journal/tool/time/entropy ports for embedded | Real no_std fake Agent request with durable effect receipts |
| E4 | Optional fixed-buffer core-only protocol/state helpers | No-heap core build; safe bounds |
| E5 | Optional C ABI, native ELF/ARM target and final-linked libc-free audit | C/Rust host fixtures, actual final binary symbol and import proof |
| E6 | Cross-profile release/maintenance certification | standard full/minimal parity and independently linked embedded tests green |

**Main parity plan sequencing change:** E0 is a precondition for A1. Design A1 capability types as core-only from the outset, and begin E2 while moving the Agent execution logic toward advanced A8 (parallel tools), A9 (compaction) and A10 (retry). std-based provider/MCP adapters can be implemented before embedded network drivers; they share provider wire types and output events. Embedded no_std does not require writing its own HTTP/TLS implementation.

## 10. Anti-goals

- No requirement to run Clap/Ratatui/Tokio/SQLite/Reqwest under no_std.
- No second embedded Agent loop, independent policy engine or Session database.
- No custom libc/allocator/runtime/TLS/HTTP written into the shared core.
- No negative no_std Cargo feature and no feature that silently removes std behavior from normal builds.
- No giant mandatory Host trait requiring invented stubs.
- No implicit permission when a host provides an effect callback.
- No fake libc-free claims from static-musl output or rlib compilation.
- No allocator/panic globals defined in reusable libraries.
- No weakened provider/account/tool/Session identities during embedded continuation.
- No unsafe FFI transmutation, panic unwinding or silently retained caller pointers.
- No unbounded data structures or hidden allocations in hot parse/render paths.

## 11. Research references, verified October 2026

- Rust core/no_std: https://doc.rust-lang.org/stable/core/attribute.no_std.html
- Rust embedded book: https://doc.rust-lang.org/embedded-book/intro/no-std.html
- Rust alloc: https://doc.rust-lang.org/stable/alloc/
- Cargo features/unification: https://doc.rust-lang.org/cargo/reference/features.html
- Serde no_std: https://serde.rs/no-std.html
- serde_json no_std+alloc: https://docs.rs/serde_json/latest/serde_json/
- embedded-io: https://docs.rs/embedded-io/latest/embedded_io/
- embedded-io-async: https://docs.rs/embedded-io-async/latest/embedded_io_async/
- embedded-io-adapters: https://docs.rs/embedded-io-adapters/latest/embedded_io_adapters/
- WASM no-std no-import target: https://doc.rust-lang.org/beta/rustc/platform-support/wasm32v1-none.html
- x86_64-unknown-none: https://doc.rust-lang.org/rustc/platform-support/x86_64-unknown-none.html
- ARM thumb bare metal: https://doc.rust-lang.org/rustc/platform-support/thumbv7em-none-eabi.html
- Optional Linux libc-free target: https://doc.rust-lang.org/rustc/platform-support/x86_64-unknown-linux-none.html

## 12. Suggested implementation starting prompt

Read this no_std design, the AGENTC-RUST-NATIVE-PARITY-MASTER-PLAN-2026-10-09.md, the M0 minimal plan, START_HERE, FULL-COMPREHENSIVE-HANDOFF-2026-10-08.md and focused SDK/TOOLS/STATE sources. Confirm live Git and target availability. Implement **only E1 together with A1**: one shared typed, core-compatible capability/grant contract consumed by the current std Agent and a real no_std test host; do not clone the Agent loop. Run rust unit/feature/Clippy/doc, full native compatibility, source-bound validation, and the wasm32v1-none core-only build; record the real compiler/target and actual evidence. Do not begin Phase 69, revive skipped 64/65 or push.
