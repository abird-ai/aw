# aw — Current architecture, decisions, completed phases, and outstanding work

**Authoritative current-state entry, 2026-10-11.** This document is included verbatim in the October 11 comprehensive master. It supersedes dated status statements in older phase plans without editing those plans. Older complete phase design and user decisions remain preserved in the frozen October 9 master and successor annexes, and every other archived plan and focus document is reproduced in the exhaustive source archive.

## 1. Why aw exists

aw is ONE Rust-first AI Agent and developer work product that supports an immediate lightweight text-first Agent, richer native terminal Workspace, durable Sessions/Domain/Machine/Fleet governance, optional collaboration and remote execution, and controlled web surfaces. Progressive disclosure: bare aw is clean, intuitive, fast and pleasing; advanced complexity appears only when requested.

The user repeatedly required elegant, intuitive, simple, composable and maintenance-light design. Pi-inspired minimal native UX without cloning another implementation. No gray-background chat boxes, no placeholder chrome, restrained status/tool success/failure colors, robust Inline/Scrollback/Fullscreen keyboard/mouse/resize, one source-of-truth composer/transcript, syntax/Markdown rendering and system/light/dark theme.

Core invariant: ONE Agent/model/tool/authorization/journal/turn engine, not a duplicate per CLI, browser, RPC or embedded host. All known history and pending user requirements are source-pinned in this master/annex archive rather than silently lost.

## 2. Current source and authority

- Live repo: /home/pvl/spaces/abird/src/aw, branch main.
- Source implementation baseline: a62383229318a262e8b1a03800a83953974046a0. New handoff documentation may create an additional local DOC-ONLY commit above it. Always verify HEAD, status and worktrees.
- Git origin is a local Git bundle; do not push, retag, sign or publish without instruction.
- Cargo product v0.15.0, MSRV 1.88, pinned rust-toolchain.toml 1.98.1, SQLite schema v33; last numbered Phase68 tagged at 953d6e1905d9968cce74137b4705b78d6f1aa49b. Tested local compiler Rust 1.95.0 does not establish pinned 1.98.1 platform release evidence.
- Phase64 indexing/watcher and Phase65 LSP were explicitly SKIPPED. Do not implement by default or synthesize tags; no Phase69.
- Truth priority: live source/Git/tests/migrations > this CURRENT state ledger > current focused docs > dated completed R6-R14/E4/E5 specs > frozen October 9 master > older original annexes.
- The original October 9 master MUST remain EXACT: 10,684 lines, 602,163 UTF-8 bytes, SHA256 39f10f52494e8691a78c25e4963222f9f29532b69a3897bed38e77c49f1db467, 12 verbatim annexes containing the Oct8 master/earlier original history. October 8 master has 8,602 lines. Historical phase.json handoff pointers intentionally stay dated and authenticated.
- User explicitly removed ALL current requirements, tool names and CI tests that compare aw to a separate researched executable. Older archived research is provenance ONLY, not a certification objective. All active tests and scripts use first-party aw names.
- User explicitly chose R14.5 canonical long names ONLY, with NO aliases, migration utility, compatibility branch or dual parser. Canonical CLI schema v2 is current.
- User explicitly PARKED E6 true bare-metal/firmware/QEMU/board work until separately resumed. Never start it as a default next phase.

## 3. Rust modules and feature cuts

- src/main.rs / main_full.rs / main_minimal.rs / cli_taxonomy.rs own typed hosted CLI routing; single Clap CommandFactory/FromArgMatches projection. Main full CLI includes SQLite, web, Workspace and distributed capabilities; minimal JSONL and stateless share the exact underlying Agent.
- crates/aw-agent-core is the hosted Agent, tool policy, provider loop and journal. crates/aw-agent-kernel is shared no_std core-only/alloc deterministic TurnMachine + Source/Capability/Grant authorization.
- crates/aw-providers owns OpenAI-compatible Chat/Responses/Ollama, Anthropic Messages SSE, Gemini GenerateContent and model discovery/cost metadata; provider account/destination custody is not a model-discovered grant.
- crates/aw-tools implements no-follow rooted list/read/find/grep/edit and bounded explicit shell. aw-platform, aw-platform-unix/win enforce native filesystem/process differences.
- crates/aw-mcp handles explicitly selected stdio, modern Streamable HTTP and deprecated two-endpoint SSE transports through ONE rmcp client/Agent; aw-extensions hosts explicit bounded process extensions and hooks.
- aw-store SQLite v33, aw-store-jsonl, aw-store-api, aw-session-format, src/minimal_sessions.rs own original immutable Session history, leases, writer epochs, receipts, compaction and recovery. No alternate browser-owned DB.
- aw-editor, aw-ui-extension, src/tui, aw-workspace, aw-git, aw-terminal, aw-surfaces, src/web own inline/scrollback/fullscreen composer, native Workspace and SolidJS browser projection.
- aw-identity, aw-client, aw-inference, aw-collab, aw-routing, aw-pipeline and src/awnet own identity/device/client/teams/routing/pipeline/QUIC connectivity; network pairing is NOT application permission.
- crates/aw-embedded is a host-driven no_std+alloc Agent adapter; aw-embedded-ffi is the portable versioned C ABI, examples/sdk-embedded-native a separate Linux libc-free execution host.

Feature cuts:
- FULL: cargo build --locked -p aw-cli --bin aw. std-first, SQLite and optional full surfaces.
- JSONL-minimal: cargo build --locked -p aw-cli --bin aw --no-default-features --features minimal. std hosted Agent, no full SQLite/owner.
- Stateless: cargo build --locked -p aw-cli --bin aw --no-default-features.
- Embedded E1–E5: no_std core/alloc and portable C ABI, individually built/verified. Do NOT try to compile full Tokio/network/UI/SQLite into no_std.

## 4. Domain, Session, identity and remote ownership

Domain → Workspace → Session/Branch/Turn → immutable model attempts/results, external tool intents/receipts, child Sessions/planner provenance. Machine is execution host, Fleet is revision-pinned member machines (never pooled filesystem). AwNet QUIC is transport, not authorization.

Every accepted branch head has exact CAS/writer epochs. Paid model outputs must be stored before attachment; settled-but-unattached results are not regenerated. External effects journal INTENT before dispatch and factual RECEIPT before disclosure; ambiguous/unknown results are NEVER replayed automatically. Remote sync is semantic data and typed evidence, not raw SQLite/WAL files.

One owning writer, explicit owner/service vs trusted-local-direct locking, exact identity/provider/actor/site/tool fences. Credential SecretStore/OIDC issuer+subject, account/payer/model/destination and host execution site remain distinct. An advertised model price is neither invoice nor permission. Child Agents use normal Sessions; bounded root budgets, isolated writable Git worktrees and only explicit parent publisher. Reviewer, planner, team, routing and schedules cannot mint tool grants by returning text.

Configured pipeline intervals require owner grants, idempotent generation-fenced admission and no burst catch-up on missed schedules.

## 5. Models, tools, costs, retries and project sources

Model tool proposals never authorize themselves. Exact host-minted source ID/epoch, actor, site, effect and explicit grant required before AND after durable intent. MCP readOnlyHint and extension metadata are advisory only. ToolPolicy OFF by default. Model-directed parallel:true works only when explicit, for exact permitted Read/Bash waves under shared ceiling (default 16, range 1–128, 1MiB total trace/context); mutations/unknown effects remain serialized. Receipts and presentation stay original proposal order.

Native Anthropic signed thinking and Gemini thought signatures may remain only within current same-provider/model/destination private continuation; never export raw signed reasoning as ordinary conversation or replay across provider changes. User/private account model catalog is descriptive, stale/unknown price stays labeled unknown, no automatic profile/account failover.

R10 bounded compaction retains original append-only evidence. A settled paid summary applies from receipt after restart without paying twice; unknown paid states require user inspection. Generic 429/503/timeouts/partial streams are ambiguous and not automatically retried. Only positively attested BeforeDispatch retry may allow 0–2 extra attempts; default 0.

R11 project content is DATA, not execution permission. Explicit SHA256-pinned canonical-root trust/revoke of ancestor instructions, named prompt/skill/theme, rescan before prompts; no passive provider/MCP discovery. Browser/Workspace project-resource admission is intentionally denied pending authenticated owner policy.

R12 headless typed JSONL/stdin-stdout RPC projects the SAME Agent/Session. Bounded IDs, frames, accepted/settled receipts, cancellation; lost pipe is unknown effect, not retry grant. Diagnostics expose event names/frame sizes only, not secret headers or private signed continuation.

## 6. Native TUI/web UX rules

One shared editor reducer and transcript. Inline, Scrollback and Fullscreen preserve correct scroll bottom/anchor after resize, hidden thinking/tool toggles, streaming changes and keyboard/mouse actions. Slash palette and Session/branch menus must remain coherent. Clean minimal user input, muted colored tool result blocks and proper syntax highlighter. System/light/dark auto, native palette and explicitly approved source theme accent; avoid flicker. Terminal raw/PTTY modes restore on exit/failure.

Web/SolidJS is a VIEW of authorized owner Session data, not independent provider/credential authority. No passive startup listeners, shells, model calls, MCP processes, skills or scheduler. Owner-level browser trust remains separately denied without explicit authenticated policy.

## 7. R14.5 current CLI contract (BREAKING)

Full compiled flag occurrences: 1,685 on 239 nested command paths; JSONL-minimal 107/23; stateless 97/23. One shared typed Clap normalization changes long names/categories only and keeps original Arg IDs, shorts, env-variable NAME, typed defaults/value limits, conflict and exact authorization. All deprecated long spellings and six pre-R14 tool aliases are REMOVED; no CLI migration command or compatibility mode. The old R14 old-to-new guide is explicitly ARCHIVED.

Supported example families: --provider-profile, --model-id, --provider-base-url, --tools-mode, --tools-root, --tools-allow, --output-print, --output-format, --state-memory-only, nested aw rpc --agent-stdio. Use aw --help and offline aw cli schema v2 for precise supported options and feature cuts. Schema v2 does NOT report before/old_aliases/deprecated_aliases/mode. Deprecated spellings MUST error before model or tool execution; command nouns in one-shot prompt remain plain user data.

R14.5 is a user-approved breaking SOURCE decision, but current product source version remains v0.15.0. DO NOT ship under unchanged prior release identity; review semver/versioning, actual target CI/installer and release signature before publishing.

## 8. Embedded E1–E5 verified scopes; E6 parked

E1 core-only no_std Source/Capability/Grant compiles zero-import wasm32v1-none. E2/E3 the SAME shared no_std+alloc TurnMachine and hosted Agent; independent zero-import linked WASM ACTUALLY runs two-model, source-bound one-tool, ten-event intent/receipt/final-text turn in Node.

E4 independently linked static native x86-64 Linux ELF, no libc/CRT/dynamic loader/unresolved symbols/relocs, host-owned bump allocator, raw Linux syscalls, W^X stack/load. Debug and release actually execute shared ten-event Agent, denied result and panic-abort controls. This is native Linux libc-free, NOT no-kernel freestanding firmware.

E5 portable no_std+alloc versioned opaque C ABI: eleven functions, bounded pointer/len and struct versions, copied labels, borrowed callbacks and C-owned outputs, BUSY reentrancy guard disjoint from mutable EngineState, exact source grant rechecks, paid ModelStarted failure/uncertain tool receipt reconciliation. Independent normal libc-using C11 host passed 710 positive/negative assertions in each debug/release variant. The underlying portable library remains no_std; native C fixture is NOT a libc-free C executable.

E6 bootloader/board/QEMU/native-target clock/entropy/IO/persistent power-loss journal/firmware integration is PARKED explicitly. Never claim E4 or E5 is a bare-metal board runtime.

## 9. Complete original numbered phases 0–68, with precise historical source

All original design plans and user decisions are preserved in the October 9 master verbatim, including Oct8 with complete Phase0–68 chronological notes and original Appendices A–O. The exhaustive source annex reproduces every separately authored numbered/focused plan and doc. This map is for orientation, not a replacement.

- Phases 0–10: initial Agent/SDK/providers/tools, Session tree, archive→Git history, Workspace, owner/device inference, revisioned context.
- 11–13: source vs runtime validation discipline, provider-effect recovery, Domain/Workspace/Machine/Fleet model.
- 14–23: AwNet QUIC pairing, reachability, typed MachineLink, remote reads/Sessions, Fleet bounded fanout, relay, lease and semantic sync.
- 24–29: remote materialization/orchestration, Domain quorum/failover, Workspace writer epochs and replica handoff.
- 30–37: stable identity, SecretStore/OIDC/OAuth, context projection, SQLite coordination, trusted local direct, typed CLI and deferred onboarding.
- 38–49: drafts, authorized pipelines/routing, recursive child Sessions/worktrees/root budgets, teams/reactivation, tool-free generative planner.
- 50–58: guardian reviews, native UX, rich Markdown/highlight, robust scroll/resize/terminal async restoration and UI regression review.
- 59–63: source provenance, explicit skill loader and registry, Solid browser Agent, rich Markdown/media.
- 64–65: SKIPPED indexing/watcher and LSP; do not revive.
- 66–68: portable skill bundles, governed pipeline interval schedules, release/installer hardening.
- Post-68: effect/scheduler recovery, generation idempotency, config CAS, skill path/export and terminal review fixes.

## 10. Unnumbered feature chronology R0–R14.5

Research phase names are historical; no external executable is a product target anymore.
- R0: JSONL-minimal Sessions, atomic lease/history and true memory-only.
- R1/E1: core-only no_std capability/provenance kernel with import-free WASM consumer.
- R2a: rooted ls/find/grep via mature regex/globset/ignore. Optional R2b Windows trusted shell/external acceleration not implicit; no LSP/index.
- R3a: Anthropic native Messages SSE and private signed tool continuation.
- R3b: Gemini native GenerateContent signed thought/function calls.
- R4: source/freshness-qualified model catalog, private cache and estimated cost.
- R5a/b: official rmcp modern stdio/HTTP and opt-in legacy SSE transport, same Agent.
- R6: MCP resources/prompts/templates bounded data-only source and list_changed invalidation.
- R7: opt-in trusted external process extensions and hooks, C/Rust examples.
- R8/E2–E3: shared portable Agent, no_std+alloc hosted/embedded semantic trace and actual import-free WASM.
- R9/R9a: bounded and model-directed parallel Read/Bash waves, mutation barriers.
- R10: bounded durable JSONL compaction + safe BeforeDispatch retry/no replay.
- R11: source-pinned project templates/instructions/skills/themes and native UX.
- R12: typed JSONL/RPC and independent Rust/Python clients over same Agent.
- R13: own real full/minimal/stateless conformance, portability/benchmark honesty.
- R14.1–R14.4: exhaustive one-source Clap taxonomy and nested CLI help; temporary aliases historical only.
- R14.5: user-ordered canonical-only, all aliases removed, no migrations.
- E4/E5: real native Linux libc-free ELF and separate portable C ABI independent C test. E6 PARKED.

## 11. Verified first-party evidence (source baseline a623832)

R13 real compiled full and minimal first-party mock integration: 17/17 each; stateless one aggregate with 6/6 executable memory-only checks. Canonical CLI real native executable: full 11/11 scenario groups across ALL 239 nested help paths, minimal 12/12 across 23, stateless 12/12 across 23; deprecated names fail before effects, scoped read and disabled-tool denial verified.

Native Rust CLI full 266/266, minimal 95/95, stateless 92/92; shared full Agent library 117/117; sandbox subagent tests require AW_GIT/AW_BASH exact absolute paths. Whole workspace check/Clippy --all-targets warnings-denied PASS, workspace Rustdoc warnings-denied PASS. E3 zero-import WASM executes; E4 native debug/release libc-free Linux ELF runs with failure controls; E5 independent C ABI native C11 710 assertions in EACH build.

October 8 frozen master checker 89, October 9 checker 710, source-only reference profile PASSED 98 steps. The reference profile explicitly does NOT perform Cargo/native/Nix/live-provider/security checks; independent actual runtime reports under ignored validation/host are separate evidence. Installed Rust host 1.95.0 is NOT pinned 1.98.1 release. CI workflow YAML is NOT evidence that six host runs occurred. No Git push.

The monolithic cargo test --workspace --all-targets was attempted but the connector stopped after an existing scheduler idempotency test exceeded 60 s; it has NO complete passing receipt. Claim targeted tests, not a fictitious workspace-wide all-test pass. Older unrelated global rustfmt drift exists; format edited files narrowly.

## 12. Known open register — 19 first-party R13 rows and product restrictions

1. Windows native trusted shell/job cancellation.
2. Full SQLite automatic ContextFlow compaction, beyond proven explicit full/JSONL behavior.
3. Minimal encrypted credential vault/PKCE beyond explicit env refs.
4. Browser/Workspace authenticated owner project-resource trust: explicitly CLOSED until approved policy.
5. Complete browser/native TUI picker/editor/theme cross-platform UX.
6. Privileged direct RPC Bash/extension operations: unsupported pending exact authorized intent/receipt API.
7. Linux ARM64 native executable evidence.
8. Optional Linux RISC-V native/emulator evidence (not default release target).
9. Windows x64 native/console evidence.
10. Windows ARM64 native proof.
11. macOS ARM64 native proof.
12. macOS Intel optional native proof (not current release asset).
13. Pinned Rust 1.98.1 five-target Nix release-all actual build and execution.
14. Comparable release-mode size/startup/RSS/latency/PTY performance across targets.
15. Live vendor behavior/credential/billing/paid account integration, beyond fake fixtures.
16. E4 Linux-native libc-free already PROVEN; genuinely kernel-free bare metal deferred E6.
17. E5 portable C ABI Linux C-host already PROVEN; standalone freestanding C firmware deferred E6.
18. E6 real board/firmware/clock/entropy/drivers/power-loss recovery PARKED.
19. Complete cross-feature, multiuser/auth, platform/production security independent review.

Rows are honest unverified scope, NOT a command to implement all optional or deliberately denied features. Keep unauthorized browser/privileged RPC grants closed unless user approves and tests them.

## 13. Next-work sequencing (E6 remains parked)

Prioritize first-party aw release/security quality: stable CLI/schema and closeout docs, authentic native target CI (Windows/macOS/ARM etc) with actual per-host receipts, pinned cross-target Nix release-all and installer/signing, explicit versioning for breaking R14.5 source, real vendor/credential/billing validation and independent security review. Missing native host evidence remains OPEN, never source-check-green.

Do not resume E6, indexing/LSP, external executable comparisons, compatibility aliases or automatic remote side effects without user direction. Do not silently publish current breaking source as unchanged v0.15.0 binary.

## 14. Next-agent working method

1. Read FULL-COMPREHENSIVE-HANDOFF-2026-10-11.md current section and verbatim annexes in chunks, plus the exhaustive source annex and source manifest for relevant historical phases. Live source/Git/test receipts outrank dated status.
2. Re-anchor git status --short --branch, git rev-parse HEAD, git worktree list, git log --oneline --decorate -20. Do not reset dirty user work.
3. Inspect src/, crates/, examples/, tools/, migrations, relevant current .agents/docs. Preserve one Agent/turn machine, current exact grants and no-replay; no unapproved tool/provider/distributed privileges.
4. Propose a focused next approved slice, implement with real negative/recovery tests and correct full/minimal/stateless compiled binary. Run Clippy, Rustdoc and source-only validations separately; do not claim unrun targets.
5. Write all design decisions, code paths, alternative rejections, test receipts and pending items into .agents/docs or .agents/plans. Never modify frozen historical masters or immutable tags; add documented successor instead.
6. Commit only verified work locally with clean Git status and exact SHA. Never push without authorization. If interrupted, re-anchor live repo and resume rather than guessing.
7. For every future handoff record exact tests, missing platform/genuine provider proof, approved/deferred status and a copyable starting prompt.

## 15. Navigation

Live product README.md, source ownership and the removed validation_gate helper. Focused current docs .agents/docs/ARCHITECTURE.md, CLI.md, SDK.md, TOOLS.md, STATE.md, DATABASE-ARCHITECTURE.md, DOMAIN-MODEL.md, AWNET.md, FLEETS.md, WORKSPACE.md, BROWSER-WORKSPACE.md, CREDENTIAL-PLACEMENT.md, BUILD-STATUS.md and VALIDATION-GATE.md.

The frozen FULL-COMPREHENSIVE-HANDOFF-2026-10-09.md and all later R6–R14.5/E4/E5 plans are copied byte-for-byte in the new comprehensive master. The companion exhaustive source archive includes every other focused top-level historical plan and document with checksum; use its inventory rather than ignoring a decision.
