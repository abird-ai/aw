# aw agentc parity — phased implementation roadmap (R0–R13)

Date: 2026-10-09. Status: R0/R1 complete; R2a committed; R3a native Anthropic implementation and focused validation complete; R3b Gemini implemented and focused-validated; R4 catalog implemented and under closeout; R5a modern MCP stdio/HTTP tools implemented and focused-validated; R5b legacy HTTP+SSE implemented and focused-tested; R6 resources/prompts next, R2b external/Windows optional. Base: ce7c816, source 0.15.0, SQLite v33, immutable phase-68 tag. This roadmap implements the previously approved AGENTC-RUST-NATIVE-PARITY-MASTER-PLAN-2026-10-09.md and AW-EMBEDDED-NOSTD-LIBC-FREE-ARCHITECTURE-2026-10-09.md. Read those and START_HERE/FULL-COMPREHENSIVE-HANDOFF before changes.

These are **parity implementation R-phases** and never reuse the repository's historical numbered phases 64/65/68/69 or their tags. No push unless requested. One-way agentc 0.6.0 to aw minimal functional parity; aw may use a better Rust design if actual user scenarios are preserved.

## Rules for every phase

- One canonical Agent loop shared across std full/minimal and future no_std embedded adapters. Never copy the execution loop into a shadow embedded agent.
- std remains preferred for the CLI, TUI, network, database, services and desktop/web. Separate no_std+alloc is for embedding; core-only rules require no heap.
- Discovery ≠ selection ≠ authorization ≠ execution. Third-party schemas, server names and read-only metadata grant nothing; grant exact actor, capability source, source epoch, site and permitted effect.
- Save intent before external effects; unknown effects never auto-replay. No implicit account/model rerouting; no hidden startup services.
- Before declaring a phase done: focused unit/negative tests; std minimal and full regression; real binary/mock/PTY proofs where affected; no_std compile and final linkage where affected; Clippy/docs and reference gates; docs and reviewable commit.
- Do not revive intentionally skipped filesystem indexing, watcher or LSP work. On-demand grep/find tools are different and remain allowed.
- A source-only checker cannot certify a real runtime behavior. Mark missing evidence explicitly.

## Phases and dependencies

| Work phase | Capabilities and ownership | Dependency / exit gate |
| --- | --- | --- |
| R0 — completed | Existing JSONL minimal Session, no-state, exact resume/lease/effect recovery | Commit 2d9dc55; inherited fake provider real-binary tests |
| **R1 — IMPLEMENTED (A1/E1)** | Add core-only no_std capability registry/selection/grant predicates, connect to existing hosted ToolRegistry and ToolPolicy; independent no_std consumer | R0; std builtins unchanged; remote extension tool never inherits name grant; wasm32v1-none build |
| R2a — coding search (implemented) | On-demand ls/find/grep via scoped Root + regex/globset/ignore; exact --tool-allow selection, no child grant widening | R1; actual fake-provider search and adversarial rooted file tests |
| R2b — optional external/Windows tool backends (pending) | Windows shell safe explicit execution policy and any benchmark-justified rg/fd executable backend; preserve std scoped-root security | R2a; Windows-native and no-escape tests if implemented |
| R3a — Anthropic (implemented, focused-validated) | Native Anthropic Messages SSE/tools/signed thinking, first-run API preset, SecretStore account-kind and per-provider selected env-key pin | R0/R1; independent fake HTTP + full/minimal provider/tool/credential/stream tests; [AGENTC-PARITY-R3A-ANTHROPIC-2026-10-09.md](AGENTC-PARITY-R3A-ANTHROPIC-2026-10-09.md) |
| R3b — Gemini (implemented, focused-validated) | Native Google Gemini GenerateContent SSE, typed tool ID/signature, API-key/SecretStore, discovery and native first-run setup | R3a shared provider facade, R0; real minimal-binary signed multi-step mock + provider/store/CLI tests, [AGENTC-PARITY-R3B-GEMINI-2026-10-09.md](AGENTC-PARITY-R3B-GEMINI-2026-10-09.md) |
| R4 — models (implemented, focused tests) | One typed provider/model catalog, dated optional references, discovery pagination, private per-account cached metadata/price, provider presets, shared aw model CLI and onboarding picker | R3; fake provider actual-binary catalog, account key rotation, user price/unknown safeguards; [AGENTC-PARITY-R4-MODEL-CATALOG-2026-10-09.md](AGENTC-PARITY-R4-MODEL-CATALOG-2026-10-09.md) |
| R5a — MCP tools (implemented, focused tests) | Official rmcp 3.5 client, stdio/Streamable HTTP 2026-07-28 and 2025-11-25 legacy negotiation, bounded tool list + JSON Schema, exact source grants and discovery-only aw mcp | R1; actual minimal-binary modern+legacy servers, stdio, invalid schemas, no read-only grant laundering; [AGENTC-PARITY-R5A-MCP-TOOLS-2026-10-09.md](AGENTC-PARITY-R5A-MCP-TOOLS-2026-10-09.md) |
| R5b — legacy HTTP+SSE (implemented, focused tests) | Explicit opt-in --mcp-sse 2024-11-05 GET SSE + same-origin POST, bounded event/handshake, rmcp typed initialize/tools, strict no fallback/no replay | R5a; actual binary legacy SSE fixture, 2024 negotiation, origin + bearer safety, failure/no-replay; [AGENTC-PARITY-R5B-LEGACY-SSE-2026-10-09.md](AGENTC-PARITY-R5B-LEGACY-SSE-2026-10-09.md) |
| R6 — MCP resources (A6) | Prompts/resources/templates, bounded pagination and subscriptions/invalidations | R5; resource revision, notification and cancellation tests |
| R7 — extensions (A7) | Typed extension/host process protocol, C and Rust SDK examples, hook lifecycle, permissions | R1/R5; crash/revoke/overrun/registration tests |
| R8 — portable Agent engine (E2/E3) | Extract current Agent transition/receipt state machine to shared no_std+alloc kernel and host-driven embedded real turn | R1/R3; identical std/embedded decision traces and actual no_std harness |
| R9 — parallel tool batches (A8) | Concurrent bounded read waves, serialize mutations and unknown effects, exact original result order | R2/R5/R8; cancellation/revocation/crash fault injection |
| R10 — compaction/retry (A9/A10) | Backend-neutral bounded context auto/manual compaction and durable provider attempt policy | R3/R4/R8; long Session, no paid/effect replay, proper tail |
| R11 — resources/UX/auth (A11/A12) | Trusted project instructions, skills/templates, named themes, keyboard/pickers/history, supported minimal secret custody | R4/R6/R7/R10; PTY/browser/PKCE/trust revocation tests |
| R12 — headless (A13) | JSONL event output, RPC stdio and external SDK client, safe redacted wire diagnostics | R7/R10; independent protocol subprocess and reconnect tests |
| R13 — certification (A14/A15/E4–E6) | Full minimal agentc one-way parity, libc-free final linked embedded output, cross-platform builds and benchmarks | All preceding phases; every agentc scenario certified or transparently open |

R3/R4 and R5/R6 can progress independently after shared types settle. E2/E3 must be addressed before mature R9/R10 execution models. Do not create a new crate for each phase without a demonstrated independent dependency boundary.

## R1 exact implementation contract (implemented source; see R1 closeout)

- New shared no_std kernel with positive Cargo std/alloc features. Core-only borrowed portable descriptors: bounded ID, source kind (trusted host/MCP/extension), host-assigned source epoch, effect (read/write/process/unknown), immutable catalog view and exact-source actor+site grant matching.
- One trusted hosted tool registry owns the Arc tool and a validated **cached** ToolSpec+source descriptor. Source collisions rejected globally. Source metadata cannot replace a previously admitted name.
- Existing trusted host named permits keep working unchanged. Remote MCP/extension source registration does **not** inherit that grant: a separate exact source/epoch/site grant is mandatory.
- Recheck the same capability in both provider-visible tool projection and immediately before effect. Existing journal/account/context checks remain intact. Immutable call identity and source epoch pinned for the invocation.
- Host registration stays a trusted explicit API, not model, prompt, project or browser authority. No remote transports/extensions implemented in R1.
- Independent no_std consumer uses the shared kernel to validate permissions; compile wasm32v1-none core-only without std or alloc. This is **not** a libc-free aw Agent executable.
- Add negative tests: invalid names/source/epoch, duplicates, owner/site/revision mismatch, remote same-name grant laundering, permission revocation, write effects not downgraded by untrusted metadata, before-effect denial, and ordinary host tools continued success.
- Rust minimal/full, Clippy, docs, actual-binary mock tool smoke and source-bound reference profile green. No SQLite migration or version change.

## Evidence discipline and sequencing

A green Cargo compile or a source checker does not certify the no_std embedded Agent. The final embedded engine must run a real tool-bearing loop and its final linked artifact must be audited for libc symbol/imports. std must remain the preferred implementation; do not write a new libc, TLS stack, executor or embedded networking system to accommodate no_std.

Every R-phase updates a one-way agentc scenario ledger with reference source, target implementation, std minimal actual-binary evidence, full-product evidence, embedded applicability, platform evidence and status (missing/partial/implemented/certified). Preserve the current comprehensive handoff; no historical tag rewriting. Keep phases reviewable, independently revertible and source-validated.

R2a implementation and accepted mature-library design deviation: see [R2 search plan](AGENTC-PARITY-R2-SEARCH-2026-10-09.md). Core no_std regex feasibility is separate from std search tools. R2b remains intentionally open; R3 providers can begin independently once R2a is committed.

R1 implementation evidence: see AGENTC-PARITY-R1-KERNEL-2026-10-09.md. The next authorized work phase is R2, bounded on-demand coding tools. A future no_std Agent remains E2/E3, not completed by the R1 capability kernel.
