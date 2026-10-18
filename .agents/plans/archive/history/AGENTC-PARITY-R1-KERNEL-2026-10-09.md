# R1 — shared Rust capability kernel (A1 + E1)

Date: 2026-10-09. This is an unnumbered parity implementation work phase, NOT historical project Phase 69. Parent roadmap: AGENTC-PARITY-IMPLEMENTATION-PHASES-2026-10-09.md. The original Phase 68 tag and the intentionally skipped Phase 64/65 indexing/LSP remain unchanged.

## Scope delivered

- New **aw-agent-kernel** dependency cut: Cargo default std -> alloc; opt-out core-only; alloc is independent of std. No OS, Tokio, HTTP, SQLite, Serde, filesystem or allocator implementation in its core-only code.
- New immutable core-only typed capability definitions: source kind (Host, MCP, Extension), host-issued source epoch, effect (Read, Write, Process, Unknown), bounded validated name/source IDs, bounded borrowed Catalog with duplicate rejection and independent revision, and exact actor/site/source/epoch/effect Grant.
- Independent **aw-kernel-embed-example** no_std source consumer uses that *same* capability Grant predicate. This demonstrates portable actual logic, not a second agent loop.
- Existing std aw-agent-core ToolRegistry caches validated ToolSpec/effect once at registration. The model-visible schema and pre-execution receipt use this cached value, not later calls to a potentially stateful Tool::spec.
- ToolPolicy preserves legacy named grants **only** for the exact trusted host source. Tool sources from MCP/extensions/custom hosts may be registered without granting authority; exact source/epoch/effect/site+actor grant required. Grant revoke hides the tool and denies future dispatch.
- Agent now checks registered provenance at *both* previous tool policy checkpoints: whole proposed batch validation and just before effect dispatch. Existing provider/account/context/journal ordering remains unchanged.
- Hosted APIs for trusted Source registration, exact source grant and revocation use the same kernel authority predicate. Native code must explicitly invoke these; no model, project resource, schema or browser payload gains permission to do so.
- Added conservative ToolEffect::Unknown; it is never downgraded to Read on receipt/recovery unless a trusted host explicitly classifies/grants it.
- Existing full std build remains default, lightweight std/JSONL remains the separate minimal cut. No new listeners, host polling or startup scans.

## Security/invariant analysis

- Registered capability name collisions reject; no replacing a formerly granted tool by a new remote origin.
- Host name allowlist still authorizes only trusted-host, not an MCP tool with the same name.
- Source ID, host-minted epoch, effect, actor and site are exact-match gates. Descriptions and read-only hints are not grants.
- Catalog revision is distinct from source epoch. A registry mutation changes the catalog revision, but per-actor grant revocation does not currently produce a persistent config/journal revision; avoid claiming fully versioned dynamic authorization.
- The mutable Agent borrow pins registration while a model turn executes. Future asynchronous remote capability reload should apply at a safe boundary and bind the exact source epoch to external-effect receipts before MCP tools are shipped.
- Authority and tool-selection controls stay in the host: remote server can only provide metadata. An arbitrary Rust host-provided Tool is still trusted code, not a sandbox.
- No new persistent schema or external tool/protocol execution is introduced. This is one real shared authority component toward the larger portable Agent E2/E3 migration.

## Tests and proof categories

- Kernel unit tests: exact source/epoch/effect/actor/site, catalog bounds/duplicates/invalid names, no heap requirement.
- Hosted Agent integration tests: name-grant laundering denied, explicit source grant and revocation work, stale epoch rejected, site/actor/effect mismatch denied, Unknown cannot inherit Read, duplicate names rejected, cached ToolSpec cannot mutate after admission, traditional trusted host registration preserved.
- Independent no_std consumer: build for wasm32v1-none without the standard library. Test both core-only and alloc-enabled kernel configurations; linked real consumer exports the grant check as a WASM symbol and has **zero imports**.
- This confirms a portable **capability kernel**, not a complete no_std Agent, firmware, Linux libc-free executable or stable C extension ABI. Rust 1.95.0 is available for the target here; repo pins 1.98.1, so pinned-target release proof remains a later E-phase.
- Existing std full/minimal/memory-only CLI, tool loop, provider, source/static/Clippy/PTTY gates must remain green. No live remote provider, MCP, multi-platform release or formal security audit is inferred.

## Fresh verification evidence and limits

- aw-agent-kernel native core-only unit suite **3/3**, standalone embedded consumer suite **1/1**.
- aw-agent-core scoped source/invocation suite **6/6**, inherited tool-loop tests **13/13**, complete agent-core all-target integration suites green.
- Main binary tests: full **258/258**, JSONL minimal **86/86**, stateless **83/83** (explicit AW_GIT/AW_BASH Nix toolchain paths when required).
- Tool crate **5/5**, JSONL store **3/3**, full SQLite store library **75/75** with host paths supplied.
- Actual minimal-binary fake HTTP workflows: Phase 2 scoped file tools **4/4**, JSONL/Session resume and no-replay **6/6**, Inline/Scrollback/Fullscreen PTY **3/3**. No production provider contacted.
- Core-only and alloc-only wasm32v1-none checks, independently linked **86,165-byte** consumer export with **zero WASM imports** on available Rust 1.95.0; the repository-pinned Rust 1.98.1/embedded Agent/E2 remains untested.
- R1 source architecture checker **42** checks; static source validation **1,060 files / 37 manifests / 1,033 authored Rust test annotations / 766 links / 0 errors or warnings**.
- Whole-workspace/all-target compilation, Clippy -D warnings, rustdoc -D warnings and workspace doctests passed.
- Formatting check **passed for all changed R1 Rust files**, but repository-wide cargo fmt --all -- --check remains red on **six unrelated pre-existing files**: aw-providers/config.rs; aw-store/lib.rs, pipelines.rs and schedules.rs; src/config_persistence.rs and src/skills.rs. No unrelated formatting rework is included in R1. Therefore the opt-in full Rust validation profile has not been claimed fully green.
- One monolithic cargo test --workspace --all-targets attempt failed at the connector transport level. Tests covering the changed core, tools, and SQLite store were rerun in smaller real host commands and passed. This is not a claim of complete workspace runtime certification.
- No live MCP, dynamic extension host, native provider API account, multi-target libc-free Agent binary, or independent security audit was executed.

## What remains

R2: add bounded on-demand ls/find/grep and tool engine backends without indexing. R3 onward: provider adapters, discovery, modern MCP, runtime extensions, portable Agent transition kernel E2/E3, safe read concurrency, compaction, retries, resources, headless RPC and full cross-platform certification. The common tool registry can already represent remote origins but **does not yet start MCP connections or load extensions**.

## R1 validation commands

    cargo test --locked -p aw-agent-kernel --no-default-features
    cargo test --locked -p aw-kernel-embed-example --no-default-features
    cargo test --locked -p aw-agent-core --test capability_sources
    cargo test --locked -p aw-agent-core --test tool_loop
    python3 the removed agentc_r1_no_std_smoke helper
    python3 the removed test_agentc_parity_r1 helper

Run the actual-binary scoped-file fixture against a fresh minimal compile, then full/minimal native builds, Clippy, docs, source reference profile and Git whitespace checks. Historical tags remain immutable; no push.
