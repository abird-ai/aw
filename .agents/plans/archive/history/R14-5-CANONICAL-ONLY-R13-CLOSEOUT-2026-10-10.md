# R14.5 + R13 — Canonical-only aw CLI and native certification closeout

Date: 2026-10-10. Current unnumbered continuation after E5 commit
3dcd62ff4edd4f3e6f8fdfbbc20d4489f38e5bd0.
Status: canonical-only CLI implemented and locally verified; cross-platform
and production release certification are separate, open work.
**E6 bare-metal firmware is explicitly PARKED by the user.**

## Current authority / preserved history

Read this file and the current .agents/plans/START_HERE.md.
The original immutable master,
.agents/plans/FULL-COMPREHENSIVE-HANDOFF-2026-10-09.md, is retained exactly
as written, with 10,684 lines and 12 byte-for-byte source annexes. All
previous per-phase implementation plans are archival history, not live
product requirements or current CLI guidance. Never rewrite the frozen
master, historical phase tags, or original numbered Phase68 checkpoint.
Phases64/65 (LSP and filesystem indexing) remain intentionally skipped.
No new numbered Phase69. No Git push/publication without instruction.

The external project once consulted during architecture research is
**NOT** an aw product target or certification dependency. The user
explicitly requested removing it from current implementation/validation
and documentation. We no longer ship, download, execute, schedule or require
an executable comparator. Active validation fixtures are first-party aw
behavior tests with isolated fake providers and independent actual C/Rust
clients; no competing executable defines aw's requirements.

## R14.5 — breaking canonical CLI names, NO migration

The user explicitly approved dropping ALL legacy long CLI aliases and
rejected migration support. The one original typed Clap CommandFactory /
FromArgMatches tree is normalized to canonical long flags in full,
JSONL-minimal and stateless builds. There is NO second parser, compatibility
feature, hidden deprecated flag, compatibility launch mode, migration
rewriter or old-name fallback. Each normalized flag retains its original
Arg ID, short spelling, environment variable NAME, static default,
value parser/range/conflicts and original model/tool/source/Session authority.

Six additional manually inherited tool spellings (tools, tool-allow,
tool-root, bash, parallel-reads and parallel-bash-limit) are removed.
The supported spellings are tools-mode, tools-allow, tools-root, tools-bash,
tools-parallel-read-limit and tools-parallel-bash-limit. Unknown long
names fail before provider invocation or tool execution. The minimal
one-shot --output-print parser preserves ordinary prompt words such as
model, mcp, rpc, extension and cli instead of interpreting them as
subcommands, using canonical Clap argument arity.

The read-only command aw cli schema reports **schema v2**, listing ONLY
currently supported flags, original typed Arg IDs, short names, env names,
static defaults, value limits and nested command paths. There are no
before, old_aliases, deprecated_aliases, removed_aliases or cli_mode
fields. Full includes 1,685 flag-path occurrences across 239 paths;
JSONL-minimal has 107/23, stateless 97/23. Every command path is
tested through an actual native process's --help. No config/Session/
provider/tool work is permitted during help/schema.

The old R14 flag mapping is preserved as an explicitly ARCHIVED historic
snapshot, NOT a current migration guide. Migration scripts and the
generated mapping gate were removed at the user's instruction. All
native mock-provider and RPC-client fixtures use canonical flags
directly, without an on-the-fly translation shim.

## R13 — first-party aw local conformance and performance

Real compiled full and minimal aw binaries each passed 17/17 independent
local mock-provider conformance scenarios after CLI alias removal.
Stateless passed the separate 6/6 memory-only executable assertions.
Covered components include scoped read/write/search, Anthropic/Gemini
native signed-tool continuations in isolated fixtures, modern MCP and
legacy SSE, MCP data/resources, process extensions, model-directed safe
parallelism, JSONL compaction/retry/no replay, source-pinned project
resources, typed headless JSONL/RPC with independent external Rust/Python
clients, and three actual terminal modes. These are aw's own executable
integration tests. Successful local fake SSE responses do not certify
live vendor accounts or full production deployment.

R13 binary benchmarks separately measure warm-cache startup, local
fake-model first-token/total latency, RSS and stripped/gzip size. These
are host/cache/build-profile dependent; release and debug measurements
must never be mixed. The minimal native release previously measured
approximately 19.4 MB; no competitor baseline or fixed product size
budget is imposed by this single-host test. The original no_std+alloc
shared Agent has executed in zero-import wasm32v1-none. E4 separately
linked and actually executed a Linux libc/CRT-free ELF, and E5's
portable C ABI was exercised by an independent C11 process (the C
test process intentionally uses libc). Each artifact has distinct scope.

Global/production requirements remain OPEN: native Windows x64/ARM64
and macOS Intel/ARM64 execution receipts, Linux ARM64 and optional
RISC-V evidence, pinned Rust 1.98.1 five-target release pipeline,
installer/signing receipts, live-vendor compatibility/billing,
multiuser owner/auth and browser project trust where explicitly
denied, complete native TUI coverage, and independent security audit.
The manual six-host CI workflow needs actual runs and per-host receipts
before marking any missing native platform certified. E6 is parked;
hardware firmware is NOT a blocking requirement for the current
hosted CLI closeout.

## Release boundary and next milestones

The canonical-only CLI change is BREAKING. Source still reports
0.15.0 / SQLite v33 because no release/version bump was authorized.
Before distribution, select an appropriate breaking release version,
complete native target CI, installer/signature and product regression
reviews, and explicitly approve publication. Do NOT silently ship a
breaking CLI under the same previously distributed artifact version.
No compatibility profile or migration tool will be added.

Remaining work is **aw-native** platform/release/security certification,
not an external-comparison program. Resume E6 only when the user
explicitly authorizes firmware work.

## Completed local verification ledger — 2026-10-10

**First-party source and runtime evidence, not a third-party comparator:**

- Canonical-only CLI is the default in the real full, JSONL-minimal and
  stateless binaries, with no old-name aliases, compatibility Cargo toggle
  or argument migration utility. The public read-only aw cli schema is v2,
  and retains only actual supported typed Arg IDs, shorts/env/defaults,
  correct canonical headings and feature-cut-dependent command paths.
- Actual compiled CLI taxonomy executable tests passed all 239 full
  nested help paths (11 scenario groups) and 23 paths each in minimal
  (12 groups) and stateless (12 groups), with old options rejected before
  provider/tool execution and canonical model/scoped-read/denial behavior.
- Fresh native aw real-binary integration matrix: full **17/17**, JSONL-
  minimal **17/17**; stateless one aggregate row with **6/6** actual
  memory-only checks. Isolated local fake providers only. Source fingerprint
  and actual compiled binary hash stored in ignored
  validation/host/r13/{full,minimal,stateless}-aw-final.json; regenerate
  against the clean commit after the local checkpoint.
- Native Rust full CLI **266/266**, minimal **95/95**, stateless **92/92**;
  full shared library tests **117/117**. The sandbox environment explicitly
  provided AW_GIT and AW_BASH absolute binaries for five subagent cases.
  Rust Cargo workspace --locked --all-targets check and whole-workspace
  warning-denied Clippy PASSED. Rust workspace doctests with
  RUSTDOCFLAGS=-D warnings PASSED.
- E3 no_std+alloc Agent WASM linked with **zero imports** and actually
  executed the original fake model/tool/journal turn. E4 Linux x86-64
  libc/CRT-free ELF debug+release actual native executions passed, with
  negative invalid-witness and panic-abort tests. E5 independent C11 host
  executed **710/710** assertions in debug and release; 11 versioned ABI
  exports were checked and portable no_std wasm32v1-none compilation passed.
  These targets are distinct; E6 true bare metal is PARKED.
- The R13 portability report verifies minimal/stateless normal Rust Cargo
  dependency isolation and the linked WASM turn. It **correctly reports 0
  additional native target executions, 0 five-target pinned Nix release
  artifacts, and no Rust 1.98.1 pinned-toolchain execution**. This is
  local evidence, not signing, macOS/Windows/ARM or production vendor proof.
- Source-only validation profile passed 98 focused checks, including
  archived-phase integrity and current canonical CLI architecture.
  October-08 historical handoff integrity is **89**, immutable October-09
  complete master integrity **710** with exact **10,684 original lines
  and twelve verbatim historical annexes**. Source/static syntax,
  manifests and documentation-link validation are green. The static
  link report explicitly identifies TWO old, frozen phase-note tool
  links which were superseded by the renamed current aw-native scripts;
  neither original archived note was silently edited. Source-only
  validation does NOT claim to compile or execute Rust binaries.
- The native monolithic workspace cargo test --workspace --all-targets
  was attempted but the connector failed internally after an existing
  scheduler idempotency test exceeded 60 seconds. There is NO complete
  monolithic PASS receipt; the separately completed targeted Rust, aw
  integration, embedded, Clippy and docs gates are the proof claimed.
- All **45** operational test scripts previously prefixed by a reference
  project are now aw-owned in both filename and source. All **321**
  operational Python/workflow tool script references currently resolve.
  Obsolete comparison tool and ignored cached downloaded reference binary
  were deleted. The new live README, START_HERE and this current plan
  have no external-project brand/requirements. The original archival plan
  filenames and complete byte-frozen historical annexes are intentionally
  preserved for reproducibility, rather than fabricated as new filenames.
- Current local worktree before checkpoint: based on
  3dcd62ff4edd4f3e6f8fdfbbc20d4489f38e5bd0; source changes are
  uncommitted until the final local commit. After commit, regenerate ignored
  proof reports for exact clean-head/dirty=false provenance.

**Still OPEN:** actual Windows/macOS/ARM and optional RISC-V native tests,
pinned five-target Nix release-all, signed installer/release verification,
live-vendor model/credential/billing checks, independent production security
review and intentionally restricted multiuser/browser grants. These gaps
remain real; no competitor or reference executable comparison is required.

**User instructions preserved:** E6 is explicitly PARKED; canonical CLI
long names only, no old aliases or migration. Version remains 0.15.0 and
SQLite v33, no numbered Phase69, no phase-68 tag rewrite and no Git push.
