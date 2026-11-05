# dotlink-tool-port

Base AW: `53206b2`; dotlink: `fc53fc9` plus uncommitted tool-hardening tree.

Design/decisions: [README.md](README.md).

## Committed

### `7ce39d9` — native dotlink-style tooling under original AW authority

- [x] Native `ls,grep,read,write,edit,patch,bread,bwrite,bedit` surface; retire
  model-facing `find`, leave human Find untouched
- [x] Keep existing AW exact-source/effect policy and model-directed
  parallelism; do not duplicate the tool executor
- [x] Concurrent scoped read/list paths; no global read lock
- [x] Platform staged writes; old create/edit APIs use the same backend
- [x] Cooperative Unix advisory cross-process directory mutation locks,
  root-local native write mutex and snapshot checks
- [x] Dotlink patch grammar, multi-edit, binary range operations and bounded
  output/error receipts
- [x] New cross-process, read/list race, symlink, CRLF, preflight and staging
  regression tests
- [x] Update tool-facing reference documentation

## Uncommitted

- [ ] Native Windows compile/tests including DACL preservation/recovery (no
  Windows target in current environment)
- [ ] Typed media `bread` transport/ToolOutput support (requires separate
  kernel-safe media contract; not part of native text tools)
- [ ] Dotlink wire interoperability fixtures through AW's `aw-mcp` client
  (separate external-source authorization remains mandatory)

## Tests and verification

- [x] `cargo fmt --all` (Linux)
- [x] `AW_BASH=/absolute/bash cargo test --offline --locked -p aw-tools --lib`
  (36 passed)
- [x] `cargo check --offline --locked --workspace --all-targets` (passed)
- [x] `cargo clippy --offline --locked -p aw-tools --all-targets -- -D warnings`
- [x] AW CLI binary: `AW_GIT=... AW_BASH=... cargo test --offline --locked
  --bin aw -q` — 383 passed, 1 ignored, zero failed
- [x] AW CLI library: 226 passed, 2 ignored with pinned `AW_GIT`/`AW_BASH`
  in the earlier workspace run
- [x] `git diff --check` (all modified tracked paths)
- [!] Whole-workspace strict Clippy: blocked by unchanged
  `crates/aw-workspace/tests/{git_containers,files_containers}.rs` and
  `src/web/dock.rs` preexisting lint violations; do not alter other owner's files
- [ ] Full `cargo test --workspace` completion across remaining packages;
  connector command timeout interrupted later split runs
- [ ] Native Windows compile, file ACL/atomic-write/symlink tests: no Windows
  Rust target available; target check reports missing `core`

## Hardening parity follow-up (2026-10-26)

- [x] Compare live dotlink schemas, tool implementation and new hardening
  against AW (including shell and `bread` formats).
- [x] Top-N `ls`, ignore/hidden discovery, bounded `grep` search and
  aggregate ignore budgets, shared text classification, exact-file filters.
- [x] Conditional/idempotent writes, cancellable read/lock budgets, staged
  parent-directory cleanup, mutation lock protocol matching dotlink.
- [x] Shell parameter parity and trusted Windows PowerShell registration.
- [x] Linux mandatory no-symlink/no-cross-mount `openat2`; Windows named
  kernel mutex coordination and ACL-preserving staged publication source.
- [x] Additional source-level compatibility and adversarial regression tests.
- [ ] Typed `bread` MCP ContentBlock media, distinct from encoded native JSON
  (requires explicit Agent/provider data contract; preserve existing policy).
- [ ] Host-selected sandbox execution separate from `trusted` Process:
  Bubblewrap mounts/network/security require dedicated platform validation.
- [ ] Windows/macOS native compile/runtime, ACL/recovery, process/mutex and
  unsupported platform checks.
- [ ] Cross-repo live MCP wire/media interoperability under AW's exact-source
  authorization, not remote read-only hints.

## Parity validation

- [x] `cargo fmt --all --check`
- [x] `AW_BASH=/absolute/bash cargo test -p aw-tools --offline --locked --lib`
  (53 tests passed, including unreadable-tree / missing-vs-inaccessible
  ignore-file regressions and full argument-schema fixtures)
- [x] `cargo clippy -p aw-tools --all-targets --offline --locked -- -D warnings`
- [x] AW CLI `--bin aw` 383 passed, 1 ignored; AW `--lib`
  226 passed, 2 ignored in an earlier full integration run
- [x] Platform/Agent/tools package group, MCP/providers/extension package
  group and workspace/editor/Git/terminal package group passed
- [x] `cargo check --workspace --all-targets --offline --locked` passed;
  strict tool/Agent Clippy and formatting passed
- [ ] Remaining complete workspace package groups (one monolithic runner
  invocation was interrupted by the connector, not a returned test failure)
- [ ] Native Windows/macOS runners and typed media integration checks
- [x] Review all dotlink `fc53fc9..9c771d2` commits and uncommitted
  native-tool hardening; record applicable vs non-applicable subsystems in
  README crosswalk
- [x] Integrate unreadable subtree/candidate and denied .gitignore reporting,
  bounded ignore loading and partial-result reasons
- [x] Windows managed PowerShell process contained in a Job Object before
  resuming suspended child; cleanup owned by existing Process service
- [x] AW Agent result budget now preserves valid JSON truncation envelopes
- [x] Windows Process service owns suspended native PowerShell children
  through a private Job Object before resume; no PID-only termination
  for those Process instances. Windows-native execution still pending.
- [x] Source-checked Windows Job Object and ToolHelp APIs in the pinned
  windows-sys 0.61.2 dependency; no claim of Windows target compilation.
- [x] Relevant `aw-store` exact-source tool-event regression passed (1 test);
  full 201-test store unit suite exceeded connector command lifetime.
- [x] Remaining pipeline/inference/identity/collaboration, store API/session
  format, workspace/editor/Git/terminal, platform/Agent/tool, MCP/provider
  and extension package groups passed in separate bounded runs.
- [x] No-std `aw-agent-kernel --no-default-features --features alloc`
  compiled; `aw-cli --bin aw` 383/383 active tests passed, 1 ignored;
  `aw-cli --lib` 226/226 active tests passed, 2 ignored.

## Audit-remediation validation — 2026-10-27

- [x] Reproduce oversized-grep match, >50 hits/file, read line marker,
  dotlink temporary leakage, invalid model-visible JSON, metadata loss,
  unsafe parent rollback, doubly linked create, shell/schema mismatch and
  Windows cross-target build failures
- [x] Fix search, result, schema, native publication and metadata behavior
  at shared tool/Agent/platform layers without a second executor
- [x] Native `aw-platform-unix` publication tests: atomic one-link create,
  create-only conflict, replaced-parent identity and owner/mode/xattr
- [x] `aw-tools` 59 unit/regression tests passed
- [x] `aw-platform-unix` 14 tests passed
- [x] `aw-agent-core` 70 active tests passed, 1 ignored
- [x] Windows GNU cross-compile `aw-platform-win` and `aw-tools`
  with `--all-targets` (Rust 1.98.1 and installed std target) passed
- [ ] Windows native Job Object, ACL, reparse-point, DACL-initial-create,
  metadata and rollback tests (requires actual Windows runtime)
- [ ] Full Windows CLI target build (requires MinGW C compiler for SQLite)
- [ ] macOS native ACL/xattr and filesystem publication tests
- [ ] Typed MCP media and sandboxed Process (separate authorized design)

## Shared toolset and 16-commit dotlink post-sync integration — 2026-10-30

Design: SHARED-TOOLSET-2026-10-30.md
Exact outcome: PORT-RESULTS-2026-10-30.md

- [x] Read all 16 dotlink commits since 9c771d2 through 70939fb;
      separate reusable tool hardening from OAuth/tunnel/release changes
- [x] Add crates/aw-tools-core synchronous-first Rust crate with independent
      opt-in async, durable and sandbox feature interfaces
- [x] AW native adapter explicitly enables async+durable while keeping the
      original single Agent/ToolRegistry/ToolPolicy/TurnJournal
- [x] Extract dotlink KMP sequence matcher, bounded text classification and
      in-place shrinking replacements, Codex patch grammar+EOF/CRLF
- [x] Share 64 MiB aggregate original/working mutation memory budget,
      checked expansion, bounded serde arrays, receipt preflight/paging
- [x] Raise exact multi-edit to 512 entries and 64 files with native AW
      request envelope and per-file limit intact
- [x] Extract shared bounded read window and remove duplicate platform Unix/
      Windows line-scan algorithms
- [x] Extract streaming original-relative binary splice/digest/no-op
      algorithms, use AW pinned platform staging on both Unix and Windows
- [x] Share grep work accounting, all-cause incomplete reporting, quoted
      ambiguous path labels; bound retained candidate paths to 4096
- [x] Define host-owned task status, quotas, spool accounting, safe ack
      high-water marks, sealed writer leases, bounded quiescence and durable
      journal interface behind opt-in feature
- [x] Define optional sandbox policy interface without changing trusted shell
- [x] Add tests for 512 edits/64 files, SHA-verified streamed 4 MiB binary,
      patch mixed-line-endings/EOF, search incomplete causes and stage
      integrity; no dotlink modifications
- [ ] Integrate actual spool writer leases/terminal-seal cleanup and
      job_start/list/get/output/ack/cancel using AW's existing
      durable task/Session/Process owner and exact source grants (do not
      duplicate dotlink's manager or replay ledger)
- [ ] True MCP typed native media, live dotlink as shared-crate consumer and
      separately authorized sandbox launcher
- [ ] Native Windows/macOS runtime and complete original-lockfile AW
      workspace checks, once the l5 Cargo dependency cache supports them

### Current validation record

- [x] Shared core 23 sync-default and 30 all-features unit tests
      (includes sealed writer claims, last-drop notifications and bounded
      spool writer quiescence)
- [x] Native AW tool tests using actual source in disposable integration
      fixture (63 passed; no new source grants or mocked tool actions)
- [x] Unix platform source tests including stream digest preimage (15 passed)
- [x] Windows GNU cross-target aw-platform-win/aw-tools all-targets source
      compilation with Rust 1.98.1, 1.98.1 Windows std and cached dependencies
- [x] Strict Clippy for shared core and native tool adapters in fixture
- [x] rustfmt and non-UX path diff validation
- [ ] Full AW original Cargo.lock offline/online check on l5: rmcp 3.5.0 and
      other pinned crates absent from l5 registry; online index TLS issuer
      failure. Fixture dependency substitutions are confined to ignored
      target/review-tool-integration and NOT written to tracked manifests.

## Five sequential shared-toolset continuation phases — 2026-10-30

See `NEXT-FIVE-2026-10-30.md` for the phase-by-phase implementation record.

- [x] TS-N1 checked task lifecycle/retention/compaction accounting
- [x] TS-N2 real Process stdout/stderr sealed-writer leases and terminal quiescence
- [x] TS-N3 exact bounded Process output pages and gap-aware acknowledged prefix
- [x] TS-N4 complete-file signature-based media handoff, SHA-256 verification
      metadata, no filename MIME spoofing or unsupported typed-media claims
- [x] TS-N5 validated host-selected sandbox *policy* plus native tool/Windows
      cross-target tests; no ungranted sandbox execution
- [ ] Provider-native typed media, persistent host-owned job_* endpoints,
      genuine sandbox launcher and native Windows/macOS runtime tests
- [ ] Original-lockfile full AW workspace Cargo test once l5 cache/TLS fixed

Validation: shared all features 38/38, native AW tool 68/68, Unix 15/15;
strict core+tools Clippy and Windows GNU cross-target all-targets check passed
in ignored dependency-adjusted integration fixture; full original AW workspace
build not claimed green.

## Five sequential continuation phases TS-N6..N10 — 2026-10-31

Implementation / tests: `NEXT-FIVE-B-2026-10-31.md`.
AW main HEAD stays at `11bea90`; owner-managed changes remain uncommitted.

- [x] TS-N6 exact source/site/epoch/scope/call/effect TaskFence, CAS-like
  generations, revocation, journal-intent and unknown-state recovery proof
- [x] TS-N7 file-backed host-supplied Process spools, shared session quotas,
  actual writer leases, bounded output paging, host-only exact-source fence
- [x] TS-N8 MediaPacket complete-byte SHA-256/MIME revalidation and explicit
  TypedMediaHost authorization; native AW bread remains encoded JSON
- [x] TS-N9 pinned Linux bwrap runtime/workspace/executable FD plan and RAII
  child, no network by default and no new trusted-shell permission
- [x] TS-N10 original AW pinned Cargo.lock reconciliation and cross-feature,
  Windows compile, native Agent/CLI/tool tests with strict Clippy
- [ ] Monolithic workspace test run with terminal result; provider-native
  typed media, persistent model-facing job_* API, native sandbox runtime,
  Windows/macOS runtime and live dotlink-consumer integration remain open

### TS-N6..N10 test evidence

Original pinned-lockfile: shared 51 all-features / 28 default; AW tools 72;
Unix 15; Agent core 70 (1 ignored); AW CLI binary 383 (1 ignored);
AW CLI library 226 (2 ignored). Workspace all-target compilation, strict
Clippy, Windows GNU core/tool cross-check and no_std kernel passed. Bubblewrap
native smoke reached the launcher but host rejected /proc mount (EPERM);
runtime success not claimed. Full workspace monolithic test returned no terminal
summary and is not marked passing. All changes stayed in AW using dotlink-l5.

### Late post-sync dotlink drift — 2026-10-31

- [x] Review c44194f, 762dfa8 and abfc71d beyond the 70939fb base
- [x] Port Windows invariant UTF-16 mutation lock keys and sorted separate
  lexical admission-before-path mutex protocol in AW's lowest platform layer
- [x] Windows GNU aw-platform-win/aw-tools all-targets compilation
- [ ] Native Windows cross-process race/interoperability tests

### Fresh eight-pass tooling review/fix — 2026-10-31

See REVIEW-FIX-2026-10-31.md for source details and exact gates.

- [x] Irreversible spool truncation, exact source bytes, RAII quota
- [x] Unix/Windows opened file identity and read/write preflight
- [x] Distinct writer quiescence vs completed durable output
- [x] One bounded blocking writer per stream; no default spool allocation
- [x] Oversized grep scan and omitted context correctly reported
- [x] Pinned sandbox policy directory handles and eventual child reaping
- [x] Complete binary no-op preimage verification
- [x] Linear-time patch addition hunk lookahead
- [x] Accurate failed quota and poisoned/panicking writer settlement
- [x] Original-lockfile native/Agent/CLI, Windows cross and no_std gates
- [ ] Native Windows/macOS runtime and successful bwrap namespace launch
- [ ] Monolithic workspace suite with authoritative terminal result
- [x] Deterministic seal-before-observe handshake replaces flaky
      100-yield scheduling test; repeated 10 runs plus full core suite

### TS-N11..N20 tooling continuation (2026-10-31)

Details and commands: NEXT-TEN-2026-10-31.md.

- [x] N11 journal-first task transition proposals and unknown-commit fence
- [x] N12 versioned digest/identity spool checkpoints
- [x] N13 verified terminal spool recovery from host-opened files
- [x] N14 non-uncertain retention and confirmed cleanup authorization
- [x] N15 bounded task inspect/list/output/ack/cancel host contract
- [x] N16 exact-source Process bridge, source revocation and journal-first CAS
- [x] N17 explicit provider media MIME/size policy and delivery receipt
- [x] N18 no-nested-userns sandbox plan and checked launcher outcome
- [x] N19 irreversible shared StreamSummary record_chunk accounting
- [x] N20 adversarial task/retention/poison/reentrancy/API review
- [x] Pinned default/all-feature/native/Agent tests, strict Clippy,
      Windows GNU cross-target, workspace compilation and no_std
- [ ] Persistent model-facing job RPCs, storage restart/retention binding,
      provider typed media, native sandbox/Windows/macOS runtime and
      live dotlink shared consumer
- [x] Refreshed AW CLI binary 383/383 active and library 226/226 active
      (binary 1 ignored; library 2 ignored)
- [ ] Monolithic full-workspace test terminal result

### Fresh eight-pass post-N20 review/fix — 2026-10-31

Reference: REVIEW-FIX-N20-2026-10-31.md.

- [x] Positional FileExt checkpoint hashing; terminal-only snapshot
- [x] Ambiguous journal error always fences; explicit no-effect rejection
- [x] Immutable task fingerprint included in transition CAS proposal
- [x] Non-copyable, exclusive RetentionLease cleanup and persisted release
- [x] Provider host panic/error after attachment produces delivery-unknown
- [x] Spool quota reservation transferred only after accounting succeeds
- [x] Failed Completed checkpoint rejected; host callback unwind isolated
- [x] 128 deterministic random chunk/quota property regressions
- [x] Shared, native, Agent, CLI, workspace compile, strict lint,
      Windows cross-target and no_std verification as documented
- [x] Final 34 shared-sync, 85 shared-all-features, 81 native-tool,
      15 Unix, 70 active Agent-core, 383 CLI binary and 226 CLI library
      tests; original workspace compile and strict Clippy validated
- [ ] Monolithic workspace test run and native Windows/macOS/Bubblewrap
      runtime remain separate gates

### Sealed DL-150 convergence checkpoint — 2026-10-31

Owned plan: AI-NATIVE-FS-IMPLEMENTATION-2026-10-31.md.
Reference: dotlink sealed spec at 5942393, consulted read-only.

- [x] Canonical edit(path, edits=[[old,new,all?],...]) against one
      original snapshot, independent ranges and one atomic publish
- [x] Sealed public grep(context), strict patch(patch), cp(from,to)
      recursive tree copy by default; legacy knobs input-only
- [x] Canonical primary outcome and independent more/incomplete
      query modifiers; exact find/grep/read continuation cursors
- [x] Precise partial/cleanup status and non-double-counted rm ledger
- [x] Small shared batch(ops) schema and bounded dependency-graph
      compiler (pure, not model registered)
- [x] Shell writes scopes validated as advisory, never authority
- [x] Shared cumulative known/unknown progress snapshots
- [x] 41 read-only, 64 rw, 119 full core tests; 96 native tools;
      15 Unix, 70 active Agent-core; 383 CLI binary and
      226 CLI library current-run tests passed
- [x] Workspace all-target check, strict Clippy, no_std+alloc kernel,
      Windows GNU target and format/diff checks passed
- [ ] Durable per-node journal replay and model-visible batch
- [ ] Host integration of shell writes scheduling, durable job_start
      footprint, provider-native partial exact-data/status blocks
- [ ] Native Windows/macOS ACL/race and Bubblewrap runtime checks
- [ ] Monolithic workspace test terminal result; full aw-store suite
      timed out on its previous bounded attempt
- [ ] Native platform backend extraction into shared core and
      actual dotlink consumer replacement (dotlink untouched)

### Latest sealed durable-job spec follow-up — 2026-10-31

Dotlink reference: 8a3e7bb, consulted read-only.

- [x] Batch .err selects only known fail/partial; .uncertain is explicit
      and cannot accidentally trigger recovery/compensation branches
- [ ] One host-owned durable job substrate for shell and batch, with
      original AW Session/Turn/Process journal and shared task identity
- [ ] Explicit replay-protected job_restart only after terminal source,
      never implicit resumption by creating new processes
- [ ] Durable graph node claimed-before-effect / terminal-before-unlock
      checkpoint and exact shell runner reattachment
- [ ] Job output paging, ACK, cancel, retention and Task projection
      use one ID namespace; no duplicate executor or journal

### Sealed canonical Job/Graph and Windows shell update — 2026-10-31

Dotlink specification: a156142, consulted read-only.

- [x] Model-visible Windows native shell identity pwsh; powershell selection
      remains input-only compatibility; original executable option unchanged
- [x] Windows GNU aw-platform-win/aw-tools all-target cross compile
- [x] V2 single host-owned Job(Graph[Node]) substrate for shell/batch,
      current-authority restart and one ID/Task projection; process replacement
      intentionally does not reconstruct live Graph state
- [x] Graph scheduler and model-visible batch/job_* bound to the original outer
      AW journal replay fence and original Process owner

### Core-engine resumed verification — 2026-10-31

Full result: CORE-ENGINE-RESUME-2026-10-31.md

- [x] Local dotlink sealed 81a54ce verified against authenticated
      GitHub published main (older); no unsafe TLS bypass or pull
- [x] One shared operations layer for canonical write/edit/copy/mv/rm
      using original secured pinned Unix/Windows backend
- [x] Unix empty-stage pre-identity cleanup, no unverified recursion
- [x] Job replay from authenticated journal transitions without effects
- [x] Source-bound Job identity, claimed no-launch proof, fresh runner
      source/permit checks, cancellation escalation and fail-closed CAS
- [x] Original Cargo.lock: 43 RO, 71 RW, 141 full core; 98 native
      tools; 10 Unix backend; 7 Unix platform; 70 active Agent;
      383 CLI binary; 226 CLI library tests passed
- [x] Strict Clippy, workspace all-target compile, Windows GNU cross,
      no_std+alloc, fmt and diff checks passed
- [x] Registered batch/job_* and direct shells use the original TurnJournal
      outer invocation fence, exact nested live grants and original Process owner;
      per-node transitions remain intentionally bounded live-session state
- [ ] Native Windows/macOS/Bubblewrap, provider typed blocks, recovery
      and original full monolithic workspace integration gates

### Parity audit and next-agent handoff — 2026-10-31 (current)

- [x] Compare latest dotlink canonical c34ea5a spec with live AW
      shared engine, both secured backends, AW native tools and Job API
- [x] Snapshot complete c34ea5a README and TODO byte-identically in
      reference/DL150-SEALED-c34ea5a-{README,TODO}.md
- [x] Fix .gitignore requested-root isolation; malformed, unreadable,
      truncated or over-budget ignore files preserve safe results with
      explicit incomplete gitignore rather than hard failure
- [x] Keep slash/basename glob selection based on source glob, not
      compiled regex text
- [x] Reject discovery first-row over-ceiling instead of more offset=0
- [x] Mark invalid-UTF8 native filenames incomplete path_encoding;
      no lossy reusable path labels
- [x] Grep selected primary hit and context atomic, no silent long-line
      replacement or incomplete context substitution; fail actionably
      when first complete unit cannot fit
- [x] Pinned original-lockfile verification: 51 RO, 83 RW, 153 full
      core; 105 AW tools; 10 Unix backend, 7 Unix platform, 71 active
      Agent; 383 CLI binary, 226 CLI library tests; strict Clippy,
      workspace check, no_std+alloc and Windows GNU cross pass
- [x] Signed owned-source parity review record and next-agent handoff:
      PARITY-AUDIT-2026-10-31.md and
      NEXT-AGENT-HANDOFF-2026-10-31.md
- [x] P0 final-symlink lexical + resolved target authorization and protected
      descendant/exclusion policy implemented in shared secured backends
- [x] P0 original host-owned Job execution, model-facing batch/job_* and one
      original TurnJournal/Process owner implemented without a second database
- [ ] P1 adversarial grep context-coalescing/page-boundary correctness,
      provider native data/status ContentBlocks, native ignore edge cases
- [ ] Native Windows/macOS security/runtime, Bubblewrap, crash-fault
      injection and monolithic workspace-suite completion

## Shared `aw-tools` convergence plan — 2026-10-31

Design: `SHARED-AW-TOOLS-CONVERGENCE-2026-10-31.md`.
Cross-repo canonical implementation architecture: dotlink DL-150 `SHARED-IMPLEMENTATION.md`.
Baseline: AW `6f7845d` / shared implementation `e337f0d`; dotlink semantic spec `c34ea5a`.
Job persistence boundary: shared Jobs are live-session durable only. AW relies on its existing unresolved outer `ToolStarted` Session journal to forbid cross-process redispatch; do not add a persistent Graph/NodePlan/result workflow database. A transport that cannot prove retired-session non-redelivery may use only the sealed minimal replay tombstone strategy.

- [x] Latest AW parity pass already fixed requested-root gitignore isolation, invalid-UTF8 `path_encoding`, source-glob semantics, first-record non-progress, atomic grep hit/context result units and write-only no-equality behavior — `e337f0d` + `6f7845d`
- [ ] A0 record/finalize cross-repo plan; after dotlink's small pre-implementation spec clarifications land, refresh the exact canonical reference snapshot here.
- [x] A1 mechanically rename `aw-tools-core -> aw-tools` and current `aw-tools -> aw-tools-adapter`; canonical shared crate and thin AW adapter names are now in place.
- [x] A1 update AW root/surfaces/UI/subagent/process consumers to the adapter where they require AW-specific integration, while secured FS crates depend on canonical `aw-tools`.
- [x] A1 baseline was rerun after the mechanical move before the semantic convergence phases.
- [x] A2 move canonical grep, binary planning, strict patch execution, validation/result fitting and common mutation/lifecycle decisions into shared `aw-tools`; AW adapter retains projection/compatibility only.
- [x] A2 keep AW Process/PTY custody, browse/UI helpers, ToolPolicy/TurnJournal/provider/config/compatibility behavior in `aw-tools-adapter`.
- [x] A3 prepared operation-class targets use lexical + resolved authority; content final symlinks follow safely while lifecycle remains no-follow.
- [x] A3 remove workspace-user-file `nlink==1` restrictions while retaining single-link integrity checks for private spool/control artifacts.
- [x] A3 admit legal UTF-8 Unix control-character paths through reversible quoting; invalid-UTF8 remains explicit `path_encoding` incompleteness and native Windows restrictions remain.
- [x] A3 preserve no-cross-mount/reparse and pinned-handle invariants with symlink/alias/hardlink/race coverage in the shared/native layers.
- [x] A4 replace prefix-based staging/tombstone protection with exact bounded identity-owned private artifact records and restart reconciliation; user-created prefix lookalikes are not hidden.
- [x] A4 complete shared cp/mv/rm descendant policy plus crash/failure, case-only/cross-device and hardlink semantics at the implementation/test layers.
- [x] A5 consolidate Job + task fence/checkpoint/spool/retention primitives into one canonical live-session `aw-tools::JobManager`; no third manager.
- [x] A5 immutable NodePlan captures canonical args, semantic budgets, registry/authority/runtime bindings, conservative native footprints and deterministic lower-coordinate conflict admission.
- [x] A5 shared JobManager owns raw-byte output paging/ACK/owners, exact pre-effect result reservation, replay claims, cancellation/revocation, Task projection, live-session restart and paged job listing.
- [x] A6 bind JobManager to original AW authority without a second journal/database/executor. One live `JobRuntime` owns mixed native + ShellNode Graph execution; exact outer call/source/effect identity, per-grant live `ToolBindingAuthority` generations, Session/branch lifecycle rebinding, deterministic conflict scheduling, claim-before-effect, terminal-before-dependent, explicit cancellation/revocation, current-authority restart, interpreter object identity, post-spawn unknown fencing, and process stop proof all reuse AW's existing authority + `PipeProcess` custody. Internal Graph state is intentionally live-session-only: the outer AW `ToolStarted/ToolFinished` journal is the durable replay fence, so process loss leaves the invocation uncertain rather than reconstructing/replaying a persistent Graph database.
- [x] A6 enable the canonical model-facing runtime surface: `batch`, `job_start`, `job_list`, `job_get`, `job_output`, `job_ack`, `job_cancel`, `job_restart`, plus direct `bash`/`pwsh`/`powershell` through the same one-ShellNode Job path. Oversized/abandoned optimistic calls hand off the same Job; explicit cancellation records Job cancellation first. Native exact results and ShellNode streams use bounded JobManager-owned retention with raw-byte paging/ACK; fixed process terminal metadata survives result-payload ACK.
- [x] A7 produce one reviewed pinned shared-crate revision and reusable conformance fixture for dotlink. Earlier external-consumer pin: `3cb3cea` (semantic convergence `75f6f00`; ShellNode spool correction `c0d8ee3`). Later shared direct-execution, runtime-admission and private-record durability hardening supersede that candidate locally; dotlink must pin the final reviewed AW revision after it is reachable from the remote, never a sibling path. `aw-tools`/secured backends have no AW Agent/adapter normal dependency; opt-in host-neutral `conformance` vectors cover canonical schemas/requests/Graph/status semantics; matching Unix/Windows `CapabilitySet` APIs cover multiple pinned read/write grants, absolute/base-relative model paths, write-only destinations, cross-root lifecycle, final-symlink target identity, lexical/resolved exclusions, durable host-owned private-artifact registries/reconciliation, and exact grant-object identities. An isolated standalone consumer executes generic shared cross-root operations with no AW Agent types, and a disposable exact-Git-revision consumer resolves `aw-tools` + `aw-tools-fs-unix` from `656ce69` and executes the v1 conformance fixture. Windows grant selection is exact/case-sensitive while namespace safety remains invariant-folded. The final review also fixed ShellNode maximum-Graph spool admission to reserve terminal result plus both retained streams, including empty-selection trusted mode. UNC/network-share model paths use canonical `//server/share/...` spellings in the shared grammar/router; native Windows execution CI remains a separate release verification item.
- [ ] A8 support dotlink differential integration without importing dotlink transport/policy types into `aw-tools`. **Scaffolded:** dotlink now has a test-only fixture reporter consuming AW conformance JSON without a committed sibling path dependency. First explicit run reports the expected legacy gap: missing `find/cp/mv/rm`, schema drift for `ls/grep/read/write/edit/patch/bread/bwrite/bedit`, and four unreachable request vectors. Shared fixture generation accepts host `SchemaCaps` so configurable ceilings do not create false parity failures. Actual shared-engine execution remains gated on the exact pinned Git revision from A7.

### Fresh pre-pin cross-repo review — 2026-11-01

- [x] Revalidated AW against its larger architecture: one Agent/ToolPolicy/
      TurnJournal, current Session authority, minimal/stateless builds and
      no_std+alloc kernel boundaries remain intact.
- [x] Unified host concurrency: the same trusted read/shell ceilings now govern
      top-level Agent waves and nested Graphs; dynamic SDK narrowing is
      transactional and live Jobs block a conflicting policy change.
- [x] Added one AW `LocalToolLimits` narrowing contract so direct schemas,
      validators, execution, Graph nodes, retained native results, shell streams
      and Job admission share one host configuration. A Job-producing config is
      rejected up front unless its aggregate spool can retain the exact result
      reservations of one maximum-size Graph, so a valid Graph cannot self-starve
      later nodes into quota-driven `skip` outcomes.
- [x] Kept product policy out of shared safety bounds: shared render/result pages
      and raw Job pages support up to 64 MiB, Graph JSON up to 64 MiB, and the
      shared aggregate Job spool up to 8 GiB; AW retains its historical smaller
      defaults while dotlink can map its larger configured ceilings. Internal
      consumers now use `default-features = false` plus explicit `aw-tools`
      features so future convenience-default changes cannot silently widen a host.
- [x] Private-artifact custody now distinguishes volatile from durable stores:
      volatile registries may own exact stage/backup names but can never authorize
      an `rm` tombstone detach. AW therefore omits `rm` when secure durable mutation
      records are unavailable instead of either weakening recovery or disabling the
      rest of the safe filesystem toolset.
- [x] Multi-root `rm` reserves every exact identity-bound tombstone record before
      the first detach, revalidates source identity immediately before rename and
      retires every unused reservation on pre-publication failure; injected failure
      on reservation #2 leaves all requested roots intact.
- [x] Direct tool presentation now receives the Agent's actual per-call result
      budget before execution. Read/search/binary pages preserve advancing cursors,
      mutation receipts preflight against the live budget, and oversized batch/shell
      presentation hands off the same Job. Graph NodePlans retain their independently
      captured semantic result ceilings, so outer context pressure cannot change
      node status or dependent effects.
- [x] Fresh post-pin review on `656ce69` (2026-11-03): `aw-tools` all-features 219/219, AW adapter 177/177, Agent core 78 passed + 1 intentional ignore, Unix secured backend, strict Clippy, stateless/no-default, minimal, no_std+alloc kernel, full workspace all-target compile, rustfmt and diff checks all pass. Shared engine ceilings remain product-neutral for dotlink (64 MiB render/read/per-stream safety, 8 GiB session spool) while AW defaults stay narrower. A fresh Windows GNU cross-check could not start because the current toolchain image lacks the `x86_64-pc-windows-gnu` standard library; prior matching-toolchain cross-build evidence remains valid but native Windows is still a release gate.
- [x] Exact Git-consumer readiness revalidated from `656ce69`: a disposable independent Cargo workspace resolved the shared crates by exact Git revision after the local Git source was cached and executed the host-neutral conformance fixture. The first online attempt was blocked only by the environment's known crates.io CA-chain failure, not package layout or shared dependencies.
- [x] `3cb3cea` moves the remaining canonical filesystem execution into the shared layer: streamed `bedit` now owns digest/preimage/no-op/staged publication in `aw-tools`; strict canonical `patch` owns full preflight/planning/staging/publication there too. Native backends coalesce Add-file sections beneath one absent publication root into one atomic namespace unit, preserve per-line Add payload LF/CRLF bytes, and return complete contributing-input coordinates so partial/unknown patch commits retain exact 1-based section ledgers. AW's legacy `dir`/`best_effort` patch compatibility remains adapter-only and unadvertised. Final gates: `aw-tools` 223/223, Unix secured backend 34/34, AW adapter 177/177, strict Clippy/fmt, full workspace all-target compile, stateless, minimal and no_std+alloc all green.
- [x] Fresh dotlink normal matrix passed: 759 main tests with only the explicit
      differential gate ignored, plus all integration binaries; fmt/check/strict
      Clippy clean. Regenerated differential remains exactly the expected legacy
      gap (missing find/cp/mv/rm, nine schema drifts, four unreachable vectors).
      The temporary reporter also keeps Graph/aggregate/marker vectors explicitly
      unverified, so schema parity alone can never turn the cutover gate green.
- [x] Fresh AW product/config matrix passed after the final pre-pin fixes:
      shared `aw-tools` 241/241, Unix backend 38/38, AW adapter 188/188,
      Agent-core all-target suites green (78 active lib tests + 1 intentional
      ignore), AW library 226/226 active + 2 intentional ignores, and AW binary
      384/384 active + 1 intentional ignore. No-default/stateless, `minimal`,
      no_std+alloc kernel, full-workspace all-target compile, rustfmt/diff and
      strict Clippy for shared/core/adapter/Unix are green. The current Windows
      GNU shared/backend/platform/Agent/adapter stack also cross-compiles cleanly;
      native Windows/macOS execution remains a separate release gate.
- [x] Fresh config-readiness review caught and fixed narrow-only composition and
      SDK/runtime seams: direct-only selections ignore uninstantiated Job quotas;
      Graph parallelism cannot bypass the Agent scheduler; Session-scope and tool
      policy lifecycles are separate; public JobRuntime construction and later
      shell registration use the same maximum-Graph spool preflight as normal AW
      configuration. Disabled/unselected capabilities therefore cannot constrain
      direct tools, while every published Job producer retains fail-fast capacity.
- [x] Final private-artifact review closes the remaining pre-finalization window:
      Stage names are durably reserved before namespace creation, identity is
      finalized before payload, Unix/Windows recovery refuses non-private/nonempty
      same-name replacements, and Windows Stage/Backup/Tombstone names are all
      OS-random. Existing-parent Windows directory copy now uses the same
      reserve-before-create protocol; the current Windows cross-stack is green.
- [x] Final dotlink-consumer review split canonical text mutation preimage-read
      and replacement-write file ceilings so hosts with asymmetric read/write
      profiles never have to collapse authority to one size bound. Identity-only
      exact edits remain `unchanged` even when the readable source exceeds the
      write ceiling because they publish nothing; grep keeps its independent
      read-only scan ceiling. Dotlink's historical binary edit source/result size
      intentionally follows its write ceiling, so no artificial binary split was
      added. The shared JobManager crosswalk also covers dotlink's required replay,
      file-spool, ACK/paging, Task, restart, cancellation/revocation and terminal
      expiry primitives; transport-attempt/session cleanup stays host-adapter work,
      not a second semantic manager. Validation: `aw-tools` 242/242, Unix backend,
      AW adapter 188/188 with `AW_BASH`, strict Clippy, full workspace all-target
      compile, no-default/stateless, `minimal`, no_std+alloc, rustfmt/diff and the
      Windows GNU shared/backend/platform/Agent/adapter cross-stack are green.

### Fresh pre-commit convergence review — 2026-11-05

- [x] Re-read live shared semantics, secured Unix/Windows capability routing,
      AW adapter/Job runtime, minimal/full CLI and subagent composition against
      AW's one-Agent/ToolPolicy/TurnJournal and narrow-only runtime-config goals.
      No new authority, scheduler, path, recovery or direct-vs-Graph correctness
      defect remained after the current local implementation series.
- [x] Fresh native/shared gates: `aw-tools --all-features --lib` 246/246,
      `aw-tools-fs-unix --lib` 41/41, `aw-tools-adapter --lib` 189/189 with
      `AW_BASH`, `aw-agent-core --all-targets` all active tests green (one
      explicit measurement test ignored), strict Clippy for shared/backend/
      adapter/Agent, rustfmt/diff and full workspace all-target compilation.
- [x] Fresh AW product/configuration gates: root no-default/stateless and
      `minimal` checks, no_std+alloc `aw-agent-kernel`, CLI library 228/228
      active tests and CLI binary 384/384 active tests. Tool authority remains
      runtime-owned and off by default; child Agents inherit and may only narrow
      the parent's shared limits/concurrency/tool set.
- [x] Fresh Windows GNU cross-stack check passed for `aw-tools`,
      `aw-tools-fs-win`, `aw-platform-win`, `aw-platform`, `aw-agent-core` and
      `aw-tools-adapter`. This remains compile evidence; native Windows/macOS
      execution stays an explicit release gate.
- [x] Fresh dotlink-facing source review revalidated canonical request parsing,
      direct execution, final-symlink path intent, multi-root CapabilitySet
      locking/exclusions and private-artifact recovery. Missing-parent namespace
      publication is serialized by the nearest existing parent lock, including
      through CapabilitySet composition; no duplicate scheduler workaround is
      required.

### Cross-process private-artifact recovery fix — 2026-11-05

- [x] Fresh dotlink stdio integration exposed a real multi-process race: a
      newly opened durable registry could reconcile another process's live Stage
      record between reserve/create/finalize, causing `cannot durably finalize
      private mutation artifact` and losing an otherwise valid mutation.
- [x] Recovery now acquires the native namespace mutation barrier for each exact
      private record before cleanup, then revalidates that record against the
      authoritative durable Store. A stale recovery snapshot cannot delete or
      retire work another registry/process has already completed.
- [x] Windows mutation locking now includes a separately namespaced,
      invariant-folded ancestor overlap chain in addition to lexical admission
      and exact-path mutexes. This gives Stage/tombstone recovery the same
      parent/ancestor exclusion that Unix gets from nearest-parent flocks,
      including overlapping prepared roots.
- [x] Regression: shared durable-Store revalidation test; `aw-tools` 247/247,
      Unix backend 41/41, strict shared/backend Clippy, full workspace all-target
      check, Windows GNU shared/platform/adapter cross-stack, and dotlink
      `independent_processes_do_not_lose_each_others_mutations` all pass.
- [x] No broad private-name prefix hiding or retry-on-uncertain mutation was
      introduced. Independent private-state roots remain independent recovery
      authorities; the tested cross-process mutation protocol remains safe even
      when their XDG roots differ.

### Shared convergence verification

- [ ] Pre/post mechanical-rename behavior/test matrix identical.
- [ ] Final symlink lexical/resolved authorization matrix on native supported platforms.
- [x] User regular hardlink read/write/edit/cp/mv/rm tests and private-state single-link integrity tests.
- [x] UTF-8 quoted control-path + invalid-UTF8 path_encoding tests.
- [x] Exact private stage/backup/tombstone crash recovery and identity-mismatch tests, including atomic no-replace durable record publication, OS-random exact-name Stage reservation before namespace creation, identity finalization before payload, restart reconciliation of provisional/final records, and reclamation of interrupted record-publication temporaries.
- [x] Recursive lifecycle protected-descendant, native Unix no-cross-mount and Windows reparse/cross-target race coverage; native Windows/macOS runtime remains tracked separately below.
- [x] Graph conflict scheduling deterministic across runner timing.
- [x] Node results/dependent effects invariant across outer result/transport budgets; oversized aggregate presentation hands off the same completed Job without changing node modifiers.
- [x] Outer-journal no-replay ambiguity, current-authority restart, explicit Job cancellation/revocation, ShellNode stop proof, post-spawn unknown fencing, and exact-grant live revocation tests.
- [x] Raw output offsets, lossy/base64 projection, ACK idempotence, explicit multi-owner retention and pressure cleanup tests.
- [ ] Native Windows/macOS runtime gates. The latest Windows GNU
      shared/backend/platform/Agent/adapter source cross-build is green with the
      matching cached toolchain, including the final provisional/private-name
      hardening. This is compile evidence only; native platform execution CI
      remains required.
- [x] Full AW workspace all-target test matrix passed with terminal results via bounded connector-safe partitions (including `aw-cli` lib/bin, core/tools/agent/platform, storage/client/surfaces, `aw-store` 201/201 in exhaustive partitions, and workspace/examples). A single monolithic command exceeds the connector response ceiling, so validation is partitioned without reducing target coverage.
