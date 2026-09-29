# agentc 0.6.0 to aw minimal Rust parity

**Later embedded requirement:** see AW-EMBEDDED-NOSTD-LIBC-FREE-ARCHITECTURE-2026-10-09.md. This file describes a std-based lightweight JSONL minimal binary, not a libc-free build. A separate embedded no_std artifact will share its Agent kernel but has host-injected services.

**Track:** unnumbered; not Phase 69. **State:** active, not feature-parity complete. Source version 0.15.0 and SQLite v33 remain unchanged.

**Reference:** live agentc 0.6.0 under /home/pvl/spaces/abird/src/agentc, inspected at commit f1b3f0b (2026-10-08); reference README, source, tests and .agents/design/{00-architecture,20-core-agent,30-extensibility,40-tui}.md.
**Target:** live aw under /home/pvl/spaces/abird/src/aw, base fb0902f plus this unnumbered change; current master authority is FULL-COMPREHENSIVE-HANDOFF-2026-10-08.md.

**Successor implementation:** [AGENTC-PARITY-IMPLEMENTATION-PHASES-2026-10-09.md](AGENTC-PARITY-IMPLEMENTATION-PHASES-2026-10-09.md) and [AGENTC-PARITY-R1-KERNEL-2026-10-09.md](AGENTC-PARITY-R1-KERNEL-2026-10-09.md) add the cross-profile trusted-source capability grant kernel. This baseline document remains the original gap register; no MCP transport/extensions are shipped merely because their origin types exist.

**2026-10-09 progress:** [AGENTC-PARITY-R3A-ANTHROPIC-2026-10-09.md](AGENTC-PARITY-R3A-ANTHROPIC-2026-10-09.md) supersedes the historical missing-Anthropic row: native API-key Messages, streaming and signed tool-continuation in standard minimal aw now exist and have mock evidence; Gemini remains missing, and consumer subscription OAuth/manual legacy thinking are not implied. The ledger below preserves its original pre-implementation status as a baseline.

**2026-10-09 R3b progress:** the historical Gemini-missing row in the original baseline ledger is superseded by [AGENTC-PARITY-R3B-GEMINI-2026-10-09.md](AGENTC-PARITY-R3B-GEMINI-2026-10-09.md). Native Gemini GenerateContent streaming/tool calls, key/header mapping and thoughtSignature preservation now have local fake-provider evidence; official live provider, Google Interactions and broader model capability/pricing certification remain outside R3b. The original ledger below is intentionally retained as prior history.

**2026-10-09 R4 successor:** [AGENTC-PARITY-R4-MODEL-CATALOG-2026-10-09.md](AGENTC-PARITY-R4-MODEL-CATALOG-2026-10-09.md) covers the baseline historical model catalog/cache/price gap with source-qualified unknowns, exact provider account metadata fencing, typed CLI list/refresh and existing onboarding. The original checklist below reflects the pre-implementation snapshot; unsupported cross-provider authenticated discovery, advanced auto-pickers, subscription quota and live API pricing must not be inferred.

**2026-10-09 R5a progress:** [AGENTC-PARITY-R5A-MCP-TOOLS-2026-10-09.md](AGENTC-PARITY-R5A-MCP-TOOLS-2026-10-09.md) supersedes older missing modern MCP tool-client rows for standard minimal/full: independent rmcp modern 2026/legacy 2025 HTTP and stdio mock tests now exist with explicit source grants. 2024 two-endpoint HTTP+SSE remains R5b; MCP resources, prompts and dynamic notifications remain R6, and old baseline ledger below is intentionally preserved.

**R5b 2026-10-09 successor:** [AGENTC-PARITY-R5B-LEGACY-SSE-2026-10-09.md](AGENTC-PARITY-R5B-LEGACY-SSE-2026-10-09.md) now covers the historical agentc deprecated 2024 HTTP+SSE transport gap via explicit same-origin legacy framing and the common RMCP/Agent tool engine. This does not imply MCP resources/prompts/list_changed (R6), live server compatibility, dynamic reconnect or full embedded no_std Agent. The original one-way checklist below is preserved as the pre-implementation snapshot.

## Definition of success

Only one-way agentc → aw parity counts. Every capability agentc actually ships must work in the minimal Rust feature closure with functionally equivalent user/developer workflows and test evidence. Additional aw features do not compensate for missing agentc features.

Rust should be idiomatic: typed traits, explicit error ownership, structured async operations and a single canonical agent loop. Freestanding C23, no libc, custom TLS, process memory layout, 940 KiB payload or sub-millisecond startup are not language-agnostic contracts. Measure startup/RSS/binary size independently; never claim equal footprint without proof.

Build the light product with:

    cargo build --locked -p aw-cli --bin aw --no-default-features --features minimal

Cargo features are additive. Using --features minimal alone *also* enables the existing full default. Keep that default stable. Bare --no-default-features remains memory-only. The minimal feature selects JSONL durable local Sessions, but must not pull in SQLite, AwNet, browser, Domain, Fleet, or distributed control-plane features by implication.

## One-way capability checklist

**Y** = comparable implemented user ability, **P** = partial or behaviorally different, **N** = absent. These are code-based status markers, not a percentage or certification.

### Agent/runtime

- Y — one-shot chat, streaming, agent tool loop, safe cancellation, core TUI modes.
- P — tool-batch validation: aw prevalidates the entire batch and journals intents; agentc tolerates individual tool failures.
- N — parallel bounded tool jobs with source-ordered model results, preserving conservative aw external-effect recovery.
- P — HTTP retry and stream restart: agentc auto-retries 408/409/429/5xx, Retry-After and dropped streams; aw currently requires explicit new attempts.
- N — automatic threshold-triggered model summarization preserving retained recent context.
- P — manual compact: aw has explicit context-flow prepare/generate/apply but not the direct agentc interactive action.
- P — fully typed lifecycle and stream events, including message reset, command acceptance, and extension hook notification.

### Tools and host permissions

- P — read/edit/write/bash: aw has scoped read, create-only write, exact edit, Unix trusted bash with explicit grant; schemas and Windows shell behavior differ.
- N — built-in ls/find/grep for model consumption.
- N — selectable external rg/fd/grep/find backends with predictable internal fallback and normalized output.
- P — exact named tool allowlist versus aw's coarse current modes.
- P — parallel tool execution, spill output, per-job cancellation, tool-visible status and error semantics.
- Y — bounds and cancellation, checked effects, conservative no-replay state.
- N — equivalent default Windows shell/cmd/PowerShell selection as an authorized model tool.

### Providers, discovery, credentials

- Y — OpenAI Chat Completions, configurable OpenAI-compatible endpoints, Ollama local.
- P — OpenAI Responses and ChatGPT subscription: full aw has supported custody/OAuth, but minimal does not.
- N — native Anthropic Messages and subscription OAuth in minimal.
- N — native Google Gemini generateContent adapter.
- P — named provider presets (OpenRouter, xAI, DeepSeek, Groq, Mistral, Together, Google/OpenAI-compatible gateway): aw requires explicit profiles and lacks equivalent catalog.
- P — explicit setup-time model discovery: no common built-in/discovered catalog, provider metadata and 24h per-provider caching.
- N — common offline / force-refresh model listings, pricing sync and persisted user overrides.
- N — per-model usage cache read/write and rate-based known-cost status with explicit unknown handling.
- P — credential precedence and subscription login/logout operations: actual custody remains safely separated in aw.

### Runtime extensibility and MCP

- N — MCP stdio and Streamable HTTP client with lazy server connection, capability negotiation and bounded backoff.
- N — MCP tools, changed notifications and hot recompose, scoped under the exact current tool grant.
- N — MCP prompts, argument validation, resources/templates and generic text-only read tools.
- P — compile-linked Rust Extension and UI Extension trait contracts; not an equivalent runtime plugin ecosystem.
- N — versioned C/Rust external extension host with a useful equivalent to agentc's linked/runtime-loadable tools/providers/hooks.
- N — general agentc hook bus: input, pre/post provider, agent/turn/message/tool events, rewrite/veto, UI, session, resource and model lifecycle.
- N — runtime registered prompts, tools, custom providers, slash commands, system sections, status providers, resource roots and extension introspection.
- **Implementation preference:** share Rust-typed core traits and an optional versioned external process/MCP contract. Do not promise a stable Rust dylib trait-object ABI. Native dynamic loading may remain an explicitly trusted optional variant.

### Context, Sessions and UI

- P — lightweight JSONL session creation/resume/list/new/title with exact reference fencing: first implemented in this track.
- P — compatible session-file opening (agentc supports path or ID; aw's lightweight selector currently uses an ID).
- P — project instructions: aw requires explicit instruction admission; agentc loads trusted ancestor/project AGENTS/CLAUDE files.
- P — named skills: aw explicit installed/path admission, agentc discovers SKILL.md and has slash invocation.
- N — prompt templates with arguments and slash invocation.
- P — themes: built-in light/dark/system supported; named JSONC theme files absent.
- P — live pickers for model, thinking, session, setup and command completion.
- P — persistent prompt history, @file completion, paste collapsing, Ctrl+T transpose and Ctrl+O cards: inspect/implement missing interactions.
- P — streaming Markdown, syntax, tool-card backgrounds and terminal resize: comparable core UX but detailed PTY parity remains unproven.
- P — privacy and project trust: implement the equivalent user workflow through an explicit trusted-root decision, not silent authority expansion.

### Headless interfaces and distribution

- N — machine-readable headless JSONL event mode matching the agentc event flow.
- N — JSONL RPC stdin/stdout control: prompt, prompt_template, abort, get_state, set_model, set_thinking, list_models, compact, bash, new_session, get_messages.
- P — Rust embeddable SDK exists; foreign-language embedding via stable protocol still needs work.
- N — opt-in wire recording/debugging with credential-redacted output.
- P — release target set: aw's five-target Nix graph lacks Linux riscv64 and macOS x86-64 from agentc's common six; full cross-target proof not yet completed.
- P — static Linux musl and dependency-light installers are designed but complete artifact validation has not been executed.
- N — reproducible on-host side-by-side benchmark for startup, RSS, payload and TUI first paint.

## Minimal implementation currently underway

- Added explicit Cargo minimal = JSONL feature closure; full default remains unchanged.
- Reused aw-store-jsonl and aw-store-api SessionCursor/TurnJournal, not a new Session journal or model executor.
- Shared one canonical secret-free provider destination fingerprint with full aw's session controller.
- Implemented new/resume exact/resume latest/list/title/lightweight Sessions, with explicit --no-state.
- Provider account, working directory, tool root and system text are rechecked before old context is bound to a new Agent.
- A model change within unchanged custody may preserve public history; no private model continuation crosses models.
- Unresolved same-tool-root external effects block another JSONL Session until explicit inspection/acknowledgement.
- Actual-binary mock-provider regression covers journaled read/edit/write, no replay on resume, destination mismatch, list/latest and explicit no-state.
- This is **not** full agentc parity. The missing rows above stay missing even if this subset passes.

## Work packages (unnumbered)

1. **Tool parity:** bounded on-demand ls/find/grep and backend choice; no indexing/watchers/LSP. Exact per-tool grants and call/result ordering.
2. **Provider and identity parity:** native Anthropic/Google; presets, model catalog, cache and discovery, known/unknown prices, lightweight OAuth.
3. **MCP and extensions:** explicit stdio/HTTP client, tools/prompts/resources, live capability refresh, bounded hook/registration host with C/Rust consumer proofs.
4. **Agent behaviors:** automatic safe compaction, classified pre-effect HTTP retry, orderly concurrent tool batches, stream reset.
5. **TUI/resources:** project trust, prompts, SKILL.md interoperability, named themes, pickers, keybindings, @file completion and persistent history.
6. **Headless/release:** event JSONL, interactive RPC JSONL, SDK bridge, six-target production proof, measured footprint.

## Non-negotiables and acceptance

- One aw product and one canonical model/tool executor shared by minimal and full.
- Do not replay uncertain tool/provider effects automatically; no silent rerouting; no authority from an MCP tool description or extension name.
- Root/tool/identity/account boundaries survive session switching and cancellation.
- Tool and skill discovery do not grant use; explicit host/actor approvals remain decisive.
- Phase 64 and Phase 65 indexing/LSP remain skipped; no Phase 69 inferred and no historical tag changed.
- A green feature flag compile is **not** agentc parity. Final acceptance requires each listed reference capability operational in minimal build, tested with independently constructed actual-binary and extension/MCP/provider/PTY fixtures on supported platforms.
- Use cheap source checks for normal development; opt-in Rust/full validation includes minimal binary build and mock-provider Session smoke. Record blocked production tests without inventing results.
