# AW + dotlink / DL-200 R6 — comprehensive Phase-3 continuation handoff

**2026-11-25 · L5 · Store v60 · Git recovery schema v4**

This is the current onboarding document after `AGENTS.md`. It consolidates
the initial design, signed history, owner boundaries, actual validation,
unfinished work and continuation instructions. It supersedes the *current
status and next actions* in earlier handoffs, not their historical provenance.
The sealed SPEC remains normative. Live source and executed tests outrank prose.

The new detailed chronological record below preserves every post-November-24
late-handoff implementation cut. Appendix A reproduces the complete previous
comprehensive handoff, including the original architectural decisions, R1–R6
lineage, Phase 0/1/2 foundations and earlier Phase 3 ledger. The historical
references in that appendix are still the original design records. Nothing in
the appendix changes the current continuation state at the top of this file.

## 1. Exact continuation state

Latest verified signed **implementation** checkpoint:

```text
2b4fac33d8345f284ecfd3e0585f332c72037dc6
Enforce trusted minimum Process containment before launch
parent: f3a7535262d588e08a8d058201467fcd3decf0e7
Reconcile exact partial Sandbox replacements through original custody
```

At handoff, HEAD and main agreed at 2b4fac3; the interrupted previous turn
completed the commit while the continuation was inspecting its durable
validation job. Signature independently verified, the six checkpoint paths
clean, and parent exact. Do not duplicate this implementation or re-run its
entire validation merely because the previous chat appeared stuck. A signed
documentation-only checkpoint may sit above it; revalidate HEAD/main before
every subsequent write. All pending implementation is below; no background
implementation is promised or authorized by this document.

Live schema: `crates/aw-store/src/lib.rs`, LATEST_SCHEMA_VERSION = 60.
Git SourceDriver supports recovery schema 4 (derive), retaining compatible
schemas 1–3. No schema bump in the containment checkpoint. AGENTS.md's older
v33 statement is historical; follow its operating rules and the actual schema.

| Phase | Current state |
|---|---|
| 0 / 0B | Complete and published; shared Process and release corrections; real Bubblewrap worker-retirement and Windows-native runtime qualification still explicitly deferred |
| 1 | Complete and published through 7f4724f: Agent identity, principal/group ACL, immutable history, disclosure, logical Sandbox, input plan, provenance and state RPC foundation |
| 2 | Complete and published through e0c6bb3 / v54: one Event owner, typed Events and Handlers, durable delivery, same-store universe and v36 cutover; provider audience expansion remains Unknown absent its actual fence |
| 3 | Advanced but **not complete**: source/checkpoint/Vault resource owners and Git construction, structural mutations, native/Process/Vault effect custody and minimum containment implemented at trusted lower boundaries; source-aware production admission, independent provisioning/containment, remaining projections and public surface still held |
| 4 | Formal unified Jobs/Agent lifecycle and input/control cutover pending; considerable underlying Agent/Jobs runtime already exists |
| 5 | Final native ExtensionRegistry composition pending; typed registry leaves and staged registration already available |
| 6 | Dynamic Process Extensions and TypeScript/JavaScript SDK integration pending |
| 7 | Compatibility contraction, drain/restart, final Rust/Nix and AW/dotlink qualification pending |

Phases 0–7 are the sealed DL-200 implementation sequence, not new numbered AW
product phases. Do not invent Phase 69, an eighth DL-200 phase, or a second runtime.
The earlier status estimate (roughly 85% foundations / 60–70% overall) was
qualitative, before the subsequent construction and containment work. Do not
treat it as measured task-count progress or equate foundation coverage with
production readiness. Phase 3 remains unchecked.

## 2. Operating constraints and continuation procedure

- Source authoring: **dotlink---l5 only**.
- AW: `/home/pvl/spaces/abird/src/aw`.
- dotlink: `/home/pvl/spaces/abird/src/dotlink`.
- Read-only frozen release reference: `/home/pvl/spaces/abird/src/dotlink-017-mounts`.
- Do not use A1 or another worktree. Never reset, stash, overwrite or incorporate
  another owner's changes or staging. Never list/read/inspect/write/stage/reset/
  stash/commit `AW/.agents/plans/ux/**` or its staging. UX is separately owned.
  Shared plan queue is coordinator-owned. The active DL-200 plan is the relevant
  plan; do not rewrite other owners' plans.
- Physical Sandbox mount/config-v13 and release/0.17 Sandbox implementation
  remain frozen unless explicitly promoted. No thaw by implication from R6 or
  from a new strict containment requirement.
- No push, tag, release, publish, deploy or destructive history operation without
  separate user authorization. Prior push authorization was for the earlier
  committed AW line, not subsequent checkpoints.
- One implementation turn approximately 30 minutes, checkpoint at the next
  coherent tested point and pause. Choose useful end-to-end owner/consumer
  cuts rather than endless marginal refinements.
- Elegant, simple, clear, intuitive Rust library design. Reuse composable
  primitives across libraries, hosts and future models. Prefer static and
  zero-cost abstractions when practical. Take simple performance wins;
  otherwise keep clarity and target actual hot paths. No speculative
  micro-layer, broad evidence pinning, repeated fixtures or test-only ledgers.
  Prefer one grouped real failure-boundary regression per owner.
- Preserve no_std/alloc, minimal/stateless, full and durable feature boundaries.
  XDG project paths per AGENTS.md. No secrets, real grants, provider tokens,
  runtime databases/transcripts, caches or build outputs in Git.

**Checkpoint discipline:** read exact live owner source/tests before editing.
Check exact parent and owned paths including real-index staging. Execute focused
real tests and relevant strict Clippy, workspace checks, scoped formatting and
diff validation. Record actual successes, failures, unexecuted gates and
limitations in CURRENT_HANDOFF and active plan TODO. Use a private
`GIT_INDEX_FILE` to validate the exact expected-parent tree. Create one terse
signed commit of only the owned paths, verify parent, tree, signature and owned
cleanliness. Do not disturb the real index outside owned paths.

Signing identity:

```text
Good "git" signature for pvl@prasannavl.com with ED25519 key
SHA256:4aB+NvRb1shCx4xjtZDr2Y7FzfHGph80eFtpF+pHuCI
```

**Git workflow note from November 25:** the earlier combined low-level
reference-update call received a tool safety refusal, with no specific Git
error. The attachment candidate 5764644 was valid but not yet on main. A later
ordinary signed path-scoped Git commit (04d058d) had the identical validated
tree and parent; independent verification confirmed it. Subsequent checkpoints
used normal `git commit --only -S` for the exact owned paths after private-index
validation and expected-parent check, with intent-to-add only for a newly
created owned file if necessary. Verify actual result rather than trusting
the tool's process-cleanup report: the connector sometimes reported leftovers
despite exit 0 and a valid signed commit. Do not retry blindly; do not evade
a safety refusal. If an operation is denied or another owner moves HEAD,
stop, revalidate and coordinate. No direct ref mutation is necessary.
The original procedure and exact earlier history remain in Appendix A.

If a response is interrupted, first inspect HEAD, owned diffs, staging and
the relevant original durable job (job_list/job_get/job_output). Jobs can
belong to other clients; don't cancel, acknowledge or alter unrelated jobs.
Read results before repeating a command. Acknowledge only your own completed
job after retaining its factual result. Do not overwrite active source or
claim a failed/zero-test filter as passing.

## 3. Git and publication truth

| Ref / role | Exact observed or previously verified commit |
|---|---|
| AW signed implementation HEAD/main at handoff | `2b4fac33d8345f284ecfd3e0585f332c72037dc6` |
| AW last independently verified remote main (November 25 push) | `4ddeed8d0023cd0222633fc496f0d252e3dda54c` — Share finite source Process settlement with Vault projection |
| AW local origin/main at handoff | `4ddeed8d0023cd0222633fc496f0d252e3dda54c` |
| AW shared Process/R6 corrective publication | `20a63a4d4547cc20b2a154787823125b82f5e385` |
| dotlink main/HEAD; carried verified publication | `85ceccc1088e201c98c994a46a68834440eeab32` |
| dotlink AW dependency pin (carried verified baseline) | `20a63a4d4547cc20b2a154787823125b82f5e385` |
| Frozen release/0.17 checkout | `576d0a1a3fe0a5522a02d51f1d0598c3ddc160c2` — v0.17.9 |

75 outstanding AW commits were pushed and remote independently verified at
4ddeed8 in the earlier separately authorized action. Phase 1/2 and the Phase 3
line through that tip are therefore published. Eight later implementation
commits through 2b4fac3 are local (plus this handoff's documentation checkpoint
if committed). Both dotlink checkouts were clean at the read-only observation;
do not repin or publish. Local tracking ref is not current remote truth;
a new remote query is needed before making a new publication claim.

At observation the original journal owners remain dirty under concurrent work:
`crates/aw-agent-core/src/agent.rs`, `crates/aw-agent-core/src/journal.rs`,
`crates/aw-store/src/journal.rs`. Other concurrent work includes networking,
remote workspaces, child actors, minimal sessions, resource CLI, routing,
skill package, TUI and Workspace code. Recheck exact scope; no permission to
touch those paths. No new worktree, A1 or UX-plan inspection was used here.

## 4. Read order and source-of-truth hierarchy

1. AW/AGENTS.md, .agents/plans/README.md, then the active DL-200 queue line.
2. This handoff, current §§1–7 first, then the detailed ledger and historical
   appendix as needed. The full history is included to avoid dependence on
   unavailable conversations; use section-level reads for long files.
3. `.agents/plans/dl200-unified-jobs-agent-events-2026-11-20/README.md`.
4. Same plan `SPEC.md`: normative R6, particularly §0.1–0.3 (shared roles),
   §18/19 (checkpoint/construction), §25 (Agent/Sandbox/Vault authority),
   §26 (Events), §27–29 (public surfaces and examples), §30–32 (invariants/seal).
   Do not silently modify sealed requirements to fit a partial implementation.
5. Same plan `IMPLEMENTATION.md` and `TODO.md` in that order (phase boundaries,
   live acceptance queue).
6. `.agents/docs/CURRENT_HANDOFF.md` (incremental factual owner/evidence record).
7. Historical provenance in Appendix A §4/5/8/10, including the original
   DL-150, R1–R5 resolved checklist, Nov 14 unified runtime, Nov 21/22 R6,
   Nov 23 Phase 1 and Nov 24 Phase 3 documents. Original plans and decisions
   remain at their indexed .agents/docs / .agents/plans paths.
8. Cross-repo only: dotlink/AGENTS.md, dotlink CURRENT_HANDOFF and actual pin/
   consumer source; frozen release reference read-only.
9. Actual owner source and tests for the precise chosen cut. Do not infer
   current signatures or authority from old prose.

The current plan index and AGENTS.md contain historical status/version wording;
the explicit live state above supersedes that wording for this effort.
The documentation is not a new normative protocol or a new persistence owner.

## 5. Current architecture and owner map

Core decisions remain the sealed R6 ones: one original Agent/Actor/Envelope/
TurnMachine/TurnJournal, one shared Process lifetime owner, one authoritative
resource owner per truth, six descriptive shared internal roles (not base
classes), typed registry leaves and scoped authority. Agents and finite Jobs
share exposure/namespace but not execution semantics. StorePos is local to the
StoreScope/restore generation, not a global clock. History is immutable and
disclosure manifests/ACL are not execution grants. Events are generic, with
distinct original Input/Handler completion. Preserve accepted attribution,
writer epochs and exact Actor/site/effect/ToolBinding permissions. The original
full design, including rejected alternatives and R1–R6 resolution, is in
Appendix A and the normative SPEC.

| Owner / paths (Store paths under crates/aw-store/src unless qualified) | What is implemented and what remains |
|---|---|
| `crates/aw-agent-core/src/{source_driver,sandbox_driver,registry,vault_provider}.rs` | Stable DriverKey, independent schema pins, typed native staged registry leaves, Prepared and evidence contracts. Derive uses original descriptor, not a new runtime. Leaves add-only until exact live/retained/unresolved dependency proof permits removal/replacement. |
| `agent_owner.rs`, `agent_host.rs`, `history_owner.rs`, `actor_queue.rs`, principal/group and Event owners | Authoritative Agent envelope and allow-only ACL, staged encrypted initial tasks, immutable history/attribution and disclosure, generic EventScan and Events/Handler/typed delivery. No new admission abstraction. Original journal/context/queue integration of live source authority is pending. |
| `sandbox_owner.rs`, `sandbox_owner/source_bindings.rs`, `source_compatibility.rs`, `sandbox_owner/grants.rs` | Logical Sandbox, named versioned bindings, exact layout and compatibility, independent SandboxGrant current policy/evidence. Metadata mutation and layout reservation prevent custody loss; Agent↔Sandbox association immutable. No ambient fallback from a source-bearing or unresolved layout. |
| `construction_receipt.rs`, `source_barrier.rs`, `coordination.rs` | Original immutable indexed Prepared/Unknown/Applied/Rejected owner, full source identity barrier, exact layout scopes for first-source attachment, Project origin in v60 (Session XOR Project). Claim before effect, original recovery only. |
| `sandbox_owner/construction.rs`, `sandbox_owner/source_bindings/{attachment,release}.rs` | Standalone retained baseline, required-source parent derivation, versioned attach (including empty), release and replacement in unused isolated named layouts. Per-binding original Prepared provenance. Replacement one atomic binding publication. Exact partial continuation supported; ambiguous/surviving effects fenced. Bound-Agent/whole-root and unversioned/mixed policy cases remain. |
| `checkpoint_owner.rs`, `checkpoint_owner/{capture,restore,retirement}.rs` | Immutable Checkpoint/SourceSet, exact capture/restore and retained anchor cleanup. History-only, empty Auto, versioned baseline restore, encrypted initial task + custom initial ACL. Replay before new selection/approval; no inherited mutable state. Public host RPC coverage still pending. |
| `crates/aw-git/src/source_driver.rs`, `source_driver/{tree,restore}.rs`, `source_driver/restore/replacement.rs` | Real exact clean/dirty Git capture, private synthetic commit/index, isolated marked restore, composite Derive (recovery v4), exact retention, partial cleanup/continuation, predecessor-release proof. Changed, foreign, unmarked or ambiguous data stays protected. No automatic destructive repair. |
| `crates/aw-host/src/source_checkpoint.rs` | GitCheckpointRestoreHost actual registered-driver capture/restore/retirement and standalone construction/derive/attach/replacement/release composition; GitSourceFence and GitRestoreFence provided independently, no ambient discovery. No production authority inferred from descriptors. |
| `sandbox_owner/effects.rs`, `sandbox_owner/effects/journal.rs` | One original journal-linked direct ToolStarted claim, independent grant and exact ToolBinding/actor/writer/site, physical proof before release with ToolFinished. Deferred worker settlement and interrupted original Turn recovery; no second execution ledger or guessed termination. Broader Run hazards and graph/job/control routes still held. |
| `crates/aw-host/src/source_{approval,git,filesystem,authority,process}.rs` | Registered Git resolver, bounded disjoint named roots, per-root RO/RW and private artifacts, durable native registry/recovery, independent live revocable approval and exact Store-backed provisioning. Worker custody survives awaiter loss; root-free native registration. Ordinary and Vault finite/interactive Process launch share the one fence, static projection, terminal and source custody. Source-bearing production Actor admission still held. |
| `crates/aw-tools-adapter/src/{native_filesystem,private_artifacts,process}.rs` and platform filesystem crates | Same capability/registry backend; source-owned marker excludes ambient automatic cleanup; original exact private-record reconciliation; prepared command/pins preserve monotonic containment requirement through custody. No raw physical paths granted by a model. |
| `crates/aw-process/src/lib.rs`, existing platform backends | One async Process, actual spawn lane, cancellation/deadline, factual Occupancy/stop/output. ContainmentRequirement BestAvailable/ClosedDescendants; unsupported strict Unix refuses before fence/lane/spawn, Windows suspended Job is the existing closed backend (runtime qualification deferred). No promotion of frozen Linux mount backend. |
| `vault_grants.rs`, `vault_signing.rs`, migration 059, core VaultProvider leaf | Agent-owned grant references/evidence; concrete local signing provider with broker and reserved derived proof env projection through actual launch; future-only rotation/revocation. Generic/persistent provider configuration and other broker/socket/file/env integrations pending. |
| `src/agent_lifecycle.rs`, original Agent/Actor/journal/context/queue owners | Explicit native host selection exists; current production source-bearing admission remains HELD. Coordinate concurrent owners. Do not enable general source-bearing Graph, Job, Process or control routes without actual full runtime participation. |

Schema lineage (read old historical schema details in Appendix A): v38 Agent/ACL,
v39 principal/groups, v40 History/Disclosure, v41 Sandbox, v42–43 input/replay,
v44–54 generic Events/Handlers/cutover, v55 bindings, v56 Checkpoint/SourceSet,
v57 construction custody, v58 independent SandboxGrant, v59 VaultGrant,
v60 Project construction origin in the original immutable audit owner. No
parallel migration/ledger for structural source changes. Keep v36 Event
compatibility provenance until Phase 7 drain/restart evidence.

### Non-negotiable cross-cut invariants

- Stable source identities and independent Binding/Anchor/Recovery schemas;
  typed handles are not durable authority. Effective source limit defaults
  128; host may lower it, defensive absolute 1024. Recovery cannot be stranded
  by a lower admission limit. Claim complete sets; replacement uses old+new
  backing identities but resulting named-slot count.
- Original Prepared before external effects; no blind retry, no invented
  completion or new planner during inspection. Unknown retains exact custody.
  Explicit continuation only after original cleanup and positive absence of
  surviving effects. Completed incoming materialization retained.
- Immutable evidence or actual provider-held fence through the final
  transaction; a persisted fence/descriptor is identity, never authority.
  Exact actor, writer epoch, policy, ToolBinding, source binding generation,
  layout and live physical identity all participate. Authority and custody
  independent of factual tool result. No Agent-to-Sandbox rebinding.
- Final atomic transaction alone publishes logical resource/binding/result.
  Original receipt/result preserved across errors and restart; wakes are
  reconstructible. No duplicate execution or recovery ledger. Same name after
  release/attachment gets a fresh binding revision; provenance per binding.
- No stale or ambient grants, source-free fallback, provider-secret logging,
  arbitrary model-controlled env/mount/socket/file injection, or implied
  retroactive erasure of already materialized Process proof.
- Git retains exact snapshot without changing parent HEAD/index/content;
  published cleanup never discards unaccounted edits. Parent physical backing
  remains protected while retained children need it. Structural mutation
  holds complete old bindings until publication. Empty layout reservations
  protect exact Store-known root/cwd; physical aliases/ancestry/mounts/
  cross-Store exclusion still independent.
- Finite worker owns actual wait/stop/reap/settlement on dropped awaiter;
  interactive custody explicit. Source ToolFinished alone cannot prove
  physical quiescence. Occupancy/stop evidence is factual, never inferred
  from an exit/error or policy requirement. Required containment fails closed;
  no weaker fallback or arbitrary shell/Graph dispatch.

## 6. Remaining work — priority and practical next cut

**Highest priority is exact live source-aware Actor/Turn admission.** Revalidate
current journal ownership before choosing this path. If available, follow the
original Agent/Actor/TurnJournal/context/queue gates through actual live host
approval and immutable source/ToolBinding/actor/site/writer participation.
Qualify the real approved native Actor end-to-end and across restart before
promoting only its supported route. Do not introduce speculative wrappers to
avoid a conflicting owner or open admission from persisted SandboxGrant,
cached preflight, registered driver or configured router alone.

If these owners are still occupied, select one independent actual production
consumer of the existing primitives. The structural-mutation foundation is now
substantial; avoid redoing it or inventing increasingly narrow tests for it.

| Remaining boundary | Closure required |
|---|---|
| Independent production physical authority / host provisioning | Configure the actual trusted source/provider/layout authority and revocation; use registered driver and approved exact roots with guards held through effect/publication. Existing provision_approval is trusted composition, not automatic policy. Do not infer physical authority from paths, ACL, pin, stored grant or empty artifact registry. |
| Actual Process containment/dispatch | Strong minimum is enforceable and refuses unsupported Linux Direct; do not claim real strong Linux scope or Windows runtime evidence. Production trusted command/descendant containment and complete multi-source physical policy through original aw-process and Job owners, without thawing mount/config-v13/release work. Unsupported routes stay held. |
| Bound-Agent structural mutation, other baseline/layout cases | Keep current unused named-child support. Exact quiescent Actor integration required for bound-Agent changes; whole-root, unversioned assets, mixed/ephemeral derivation, alternate backing identities require explicit destination/provider policy and original custody. No Agent rebind or implicit source omission. |
| Broader interrupted effects and recovery | Reconcile original journal/Run hazards and actual physical/provider proof. Marked exact Git partial/scratch recovery and native artifact recovery exist; ambiguous/unmarked/modified effects remain Unknown. Do not fabricate terminal evidence or prune foreign artifacts. |
| Remaining Vault / provider authority | Persistent provider configuration, actual scoped broker/socket/ephemeral-file/other env integration; provider-held revocable proof through use. Existing signing and finite/interactive projection already implemented. |
| Protected metadata and host RPC exposure | Implement sealed §18/§25/§28–29 using original host Port/constructor/ACL/task, checkpoint/Sandbox/Vault owners; replay first, bounded metadata, current independent disclosure; no extra restore constructor or raw public source locators. |
| Registry dependency closure | Only remove/replace SourceDriver/Vault/Sandbox definitions after owner enumeration and complete durable compatibility/recovery dependencies. Compose final registry in Phase 5, not a parallel catalog. |
| Phase 3 acceptance | Actual grouped production admission/provisioning/containment/recovery and public surface qualification; keep phase unchecked until these pass. |
| Phase 4 | Unified Agent lifecycle, staged inputs, Jobs cursor/high-water/tree/ancestor fencing, exact control/wait/revision semantics using existing primitives. |
| Phase 5 | Compose typed leaf registries and scoped context/state/views/hooks into final atomic ExtensionRegistry. |
| Phase 6 | Dynamic Process Extensions, scoped callbacks, reload/failure, TS/JS SDK over the same original Process owner. |
| Phase 7 | Contract compatibility only after drain/restart and real cross-repo Rust/Nix qualification; repin/publish separately only if authorized. |

The detailed new and historical ledgers below explicitly retain limitations at
each checkpoint; later entries supersede earlier "pending" statements about
the same implemented cut. Do not use an old checkpoint's limitation as a reason
to reimplement completed work.

## 7. Validation and blockers at this handoff

Latest original durable validation job completed (exit 0), recovered after the
interrupted turn. Independently verified the resulting signed implementation
commit 2b4fac3, expected parent and clean owned paths.

| Gate | Actual result and scope |
|---|---|
| Shared Process | 8/8 unit tests, doc target passed, minimum-containment refusal |
| Adapter | 258/258 library tests, prepared fenced/unfenced containment/custody |
| Host | 16/16 all-feature library; real original source Process refusal and full Git/native lifecycle |
| Source-effects-only Host | 2/2 focused source-effect tests (not whole native-only library) |
| Strict affected Clippy | Passed, including workspace compilation gates as recorded in current handoff |
| Workspace | All targets/all features check passed |
| Formatting / diff | Scoped checks passed |
| Git | 33/33 repository tests at immediately prior affected checkpoint f3a7535; no rerun in minimum containment cut |
| Store | Earlier combined 239/244, five recorded legacy failures; not rerun in latest cut |

Seven ENOENT child-spawn failures in the first-source attachment broad run
were due to an overlapping focused rebuild replacing the test executable;
stable-executable coordination 24/24 passed. The stronger alias gate required
one checkpoint and two Host fixture updates; those retests passed. Report
combined coverage as combined, not one all-green run. The five remaining Store
legacy failures: retired Job subscription in Agent shell, v38 principal
downgrade, v35 remote-start downgrade, remote-view reconnect and v34
remote-view downgrade. Do not weaken intended cutover to appease old tests.
Prior adapter ACK suffix assertion fixed with actual finite Job provenance;
full adapter suite now green. Real Bubblewrap worker-retirement and
Windows-native Process runtime qualification remain deferred. No claim of
strong Linux containment, production source-aware Actor dispatch or full
Phase-3 completion. Existing test coverage uses real Git operations and grouped
fault boundaries; fixture-only journal setup is not production admission.

The original user preference is to keep evidence practical: run relevant
strict gates and focused real regressions, don't repeat unrelated suites merely
for another pin. Record meaningful failures rather than hide or over-engineer.

## 8. Exact subsequent signed implementation chronology

The following 24 entries cover every first-parent commit between the previous
comprehensive documentation checkpoint `c5104dbb0fbffb3a24ccf0d0a11d454e05eac47e`
and the verified implementation checkpoint `2b4fac33d8345f284ecfd3e0585f332c72037dc6`.
Their detailed notes are taken from the actual committed CURRENT_HANDOFF record,
grouped by commit, with exact owned changed paths from Git. Read an entry's
actual diff and tests for implementation detail. The preceding complete history
and original designs are preserved in Appendix A, with the historical source
references. For any commit use `git show --stat <sha>`, `git show <sha> -- <path>`
and `git show <sha>:.agents/docs/CURRENT_HANDOFF.md`. No synthetic evidence
ledger is required.

### 123d1e7 — Compose fenced Git checkpoint restore

Commit: `123d1e7843aba0bbd8642be161837fcb44e41140`. Changed paths: `.agents/docs/CURRENT_HANDOFF.md`, `.agents/plans/dl200-unified-jobs-agent-events-2026-11-20/TODO.md`, `crates/aw-git/src/source_driver.rs`, `crates/aw-git/src/source_driver/restore.rs`, `crates/aw-host/src/lib.rs`, `crates/aw-host/src/source_checkpoint.rs`.

Concrete captured-source Git checkpoint restore now composes the registered
`GitSourceDriver` with the existing Store owners in
`aw-host::source_checkpoint::GitCheckpointRestoreHost` (git-source-effects).
Fresh restore replays Applied before selecting layout or asking for authority;
otherwise the trusted host supplies an inert layout and one live, independent
whole-set `GitRestoreFence` covering current disclosure, repository
administration/retention, destination isolation and physical/provider authority.
The driver checks approved canonical checkout and parent identities, exact
anchor/descriptor locations, and refuses ambient ancestor discovery. All named
locations must match the captured SourceSet, without overlapping destinations
or origins. Plan read-only, then use the original durable Prepared receipt
and source barrier before worktree effects. The actual host fence remains
held through driver revalidation, fresh Agent/Sandbox/history/optional initial
task/Applied publication and cleanup settlement. A saved fence handle, directory
pin or Git worktree lock is not authority. Explicit resume freshly authorizes
and inspects the original descriptors only; no blind retry, replan or new
candidate. Explicit abandon invokes exact Git candidate release under current
cleanup authority through Rejected, with Applied protected. No new schema,
registry, execution ledger or Agent-to-Sandbox rebinding.

One grouped disk regression captures two dirty Git sources via the ordinary
Store owners, rejects incomplete approval before effects, aborts final
publication, reopens the Store, refuses a different destination, resumes the
original materializations with a live guard held through publication, replays
Applied and abandons a second unpublished candidate. Both original HEADs
and indexes remain unchanged. Host **16/16**, Git **32/32**, Store checkpoint
owner **4/4**; strict Host/Git all-target/all-feature Clippy, workspace all-target/all-feature
check, scoped rustfmt and diff-check green. This is a trusted blocking host composition seam, not production
authority provisioning or RPC admission; the embedding authority owner must
supply the real physical/provider fence and ensure cross-Store/alias exclusion.
Only captured Git SourceSets are supported here; effectful baseline, mixed
driver coordination and arbitrary partial/scratch cleanup remain separate.
Source-bearing Agent/Turn admission stays held.

### 188fb94 — Compose fenced Git Sandbox retirement

Commit: `188fb9410dc761a72e3c9268d009854f824d24ba`. Changed paths: `.agents/docs/CURRENT_HANDOFF.md`, `.agents/plans/dl200-unified-jobs-agent-events-2026-11-20/TODO.md`, `crates/aw-git/src/source_driver/restore.rs`, `crates/aw-host/src/source_checkpoint.rs`.

Managed closed-Sandbox Git retirement now composes the original Store
`sandbox_reconcile_closed_sources` and Applied restore-provenance lookup with
`GitCheckpointRestoreHost::reconcile_closed_sandbox`. The independent whole-layout
`GitRestoreFence` covers exact original source/parent identities and physical
cleanup authority through the existing settlement transaction. Shared driver
checks bind the approved origin/target and retained binding to the original
descriptor even after checkpoint-anchor release. Only the complete unchanged
initial Git binding set is supported; changed generations, mismatched locations
and overlapping destinations refuse. Support one managed checkout root or a
private directory whose direct children are exactly the managed checkout
locations, with cwd contained in the root. Foreign entries refuse before any
removal. Exact driver release proves each worktree/registration absent, then
the entire root must be absent or empty under the live fence. No parent removal,
unrelated-file deletion, broad prune, shared backing GC or new cleanup ledger.
Partial cleanup keeps the original full binding set and layout reservation;
reconcile only that custody, including after restart and a lower admission
limit. Already released Sandboxes replay without another authorization or probe.
The extended grouped Host regression uses two real dirty Git restores, actual
checkpoint retention release, normal Agent/queue/Sandbox close, wrong location
and foreign file refusal, real partial cleanup with later user work preserved,
and exact recovery/replay. Host **16/16**, Git **32/32**, strict affected
all-target/all-feature Clippy, workspace all-target/all-feature check and scoped
fmt/diff passed. Production physical/provider fences remain supplied
by the embedding authority owner; arbitrary layouts, changed bindings, mixed
drivers and general partial/scratch cleanup are not promoted.

### e1fb670 — Restore versioned baselines through Prepared custody

Commit: `e1fb6709f7dd9bd9a7aa75a65a6c49b8945468d5`. Changed paths: `.agents/docs/CURRENT_HANDOFF.md`, `.agents/plans/dl200-unified-jobs-agent-events-2026-11-20/TODO.md`, `crates/aw-host/src/source_checkpoint.rs`, `crates/aw-store/src/checkpoint_owner.rs`, `crates/aw-store/src/checkpoint_owner/restore.rs`, `crates/aw-store/src/construction_receipt.rs`, `crates/aw-store/src/lib.rs`.

Effectful versioned baseline restoration now uses the original Prepared restore
constructor, source barrier, encrypted initial-task custody and atomic publication.
`CheckpointRestoreLayout` distinguishes independently selected retained destination
versions from the historical checkpoint SourceSet. The original receipt stores the
exact baseline anchors once as operation inputs, not checkpoint history or a second
source owner. Explicit None accepts required or ephemeral versions; empty Auto
accepts only ephemeral versions. Neither can substitute captured sources. Bounds,
canonical names and registered schemas are checked before driver preparation;
startup compatibility and recovery use the original materialization inputs.
`GitCheckpointRestoreHost::restore_versioned_baseline` shares the existing replay-first,
fenced execution/inspection/publication path; resume and abandon need no new route.
The public result preserves original history coverage, the checkpoint remains empty,
and source-bearing admission stays held. A new claim must fit the existing receipt
encoding bound with room for its terminal outcome. No migration or new ledger.
The grouped Host regression uses independently selected already-retained dirty Git
versions; checks captured-source/empty-Auto refusal, failed publication with task,
history retention, missing-driver restart, exact resume and replay, and task-free
ephemeral Auto abandonment. Host **16/16**, checkpoint owner **4/4**, construction
receipt **1/1**, strict affected all-target/all-feature Clippy, workspace
all-target/all-feature check and scoped fmt/diff passed.
Production baseline selection and retention authority
remain the embedding host's responsibility; creating a new version/snapshot or
unversioned baseline assets is not implemented by this slice.

### ec3a45b — Compose fenced Git checkpoint capture and retirement

Commit: `ec3a45b16a4fb9025f0ef733fe87f808bc5e21ac`. Changed paths: `.agents/docs/CURRENT_HANDOFF.md`, `.agents/plans/dl200-unified-jobs-agent-events-2026-11-20/TODO.md`, `crates/aw-git/src/source_driver.rs`, `crates/aw-git/src/source_driver/restore.rs`, `crates/aw-host/src/source_checkpoint.rs`, `crates/aw-host/src/source_checkpoint/capture.rs`.

Concrete fenced Git checkpoint capture and retention reconciliation now compose
`GitCheckpointCaptureHost` with the same registered driver and original Store
Prepared/replay/inspection/publication/abandonment owners. `GitSourceLocation`
records exact named independently approved checkout identity. Shared checkout pin
verification refuses noncanonical paths, changed directory identity and ambient
ancestor discovery before Git probes; approved capture planning reuses the same
read-only driver planner. The whole selected set and original descriptor/binding/
checkpoint identities are checked before first effects and on recovery. New capture
replays before approval and runs only after Prepared; resume inspects only, while
abandon conditionally releases exact unpublished retention. Immutable Git commit
content does not imply immutable authority or retention: the actual independent
`GitSourceFence` remains held through the Store commit. Logical retirement remains
`checkpoint_delete`; `reconcile_retired` checks the exact full retained anchor set
against approved Git repositories and holds that same authority through existing
Store settlement, including failure. Already absent refs may reconcile; no broad
prune, backing GC or worktree removal. No new Store owner, schema or ledger.
The existing grouped two-source Host lifecycle regression now uses this host
instead of hand-composing capture callbacks: incomplete approval, failed capture
publication, restart, wrong location, exact resume/replay, separate abandoned
capture, retention deletion followed by failed Store settlement, and exact retry
of reconciliation with absent refs. Restore, baseline and closed-Sandbox recovery
remain in that same fixture; original HEAD/index unchanged. Host **16/16**,
Git repository **32/32**, strict affected all-target/all-feature Clippy, workspace
all-target/all-feature check and scoped fmt/diff passed. Production authority
provisioning and arbitrary scratch cleanup remain explicitly open.

### eaf4b47 — Own Agent Vault grants and fenced signing broker

Commit: `eaf4b47d5578cb764cd66c4bd38edadaa3c6f2ef`. Changed paths: `.agents/docs/CURRENT_HANDOFF.md`, `.agents/plans/dl200-unified-jobs-agent-events-2026-11-20/TODO.md`, `crates/aw-agent-core/src/lib.rs`, `crates/aw-agent-core/src/vault_provider.rs`, `crates/aw-store/migrations/059_vault_grants.sql`, `crates/aw-store/src/checkpoint_owner.rs`, `crates/aw-store/src/construction_receipt.rs`, `crates/aw-store/src/lib.rs`, `crates/aw-store/src/sandbox_owner.rs`, `crates/aw-store/src/sandbox_owner/grants.rs`, `crates/aw-store/src/vault_grants.rs`, `crates/aw-store/src/vault_signing.rs`.

v59 owns named Agent VaultGrant references only in `agent_vault_grants_current`.
Final `VaultProviderRegistry` uses the existing staged publication and typed native
implementation custody; registration advertises projection kinds, not authority.
Agent Manage + current independent provider attach guard + exact Agent rev govern
attach/replace. One Agent rev advance mints a unique grant generation; no-op is
idempotent, null removes even after close. Actor stable identity, existing access
generation, exact entry/projection/scope, provider authority generation and original
Store restore scope are checked; no secret bytes or key locator in Agent metadata.
Metadata lists are bounded and redacted, require independent binding-metadata authority,
and do not assert provider availability from saved evidence. No implicit Vault grant
copy on shared Sandbox, history/ACL inheritance or checkpoint restore.
Concrete `SigningVaultProvider` uses the existing SecretStore implementation under a
provider-owned private root, registered Broker projection and explicitly provisioned
dedicated signing keys. `with_vault_broker` is a trusted local synchronous consumer
boundary, not model dispatch: the real provider read guard and original Store writer
transaction span exact grant/actor/entry/scope/admission validation and the bounded
domain-separated HMAC operation. No raw secret read API, arbitrary target or Process
projection. Rotation/revocation govern future use without Agent fanout; provider
restart requires new independent authorization (fresh authority identity). Provider
configuration/key lifecycle and root maintenance remain host-owned, not backed up or
restored with Agent grants; failed key deletion does not undo factual withdrawal.
Grouped real SecretStore regression covers populated v58->59 migration, signing,
registered definition/subject checks, independent attach authority, ACL broadening,
Store reopen, provider rotation/restart, non-inheritance, held admission and closed
cleanup. Four existing source-owner migration fixtures now simulate v59 downgrade
correctly and check the current schema constant. Final focused Vault **1/1**,
checkpoint **4/4**, construction receipt **1/1**, Sandbox **4/4**, Host **16/16**,
strict affected all-target/all-feature Clippy, workspace all-target/all-feature
check and scoped fmt/diff passed. The one broad default-feature Store lib run
was **235/244** before the
four v58 assertion corrections; all four now pass in the focused rerun. Five
unchanged legacy fixture failures remain: Agent shell invokes the disabled legacy
Job Event subscription; v38 principal downgrade drops referenced tables; v35
remote-start downgrade leaves agent_state_current; v34 remote-view downgrade
leaves policy_admission_current; remote-view reconnect asserts obsolete v35.
The new migration adds no domain-principal dependency and changes none of those
owners. Full suite not claimed green or rerun on the corrected tree; parent build
was not rerun. Do not repair unrelated owners as part of this checkpoint.

### cdb197a — Fence Process launch and project scoped Vault proof

Commit: `cdb197a0f6f3ef7c7f84d61ea734209e774d3c11`. Changed paths: `.agents/docs/CURRENT_HANDOFF.md`, `.agents/plans/dl200-unified-jobs-agent-events-2026-11-20/TODO.md`, `crates/aw-host/src/source_process.rs`, `crates/aw-process/src/lib.rs`, `crates/aw-process/src/linux_spawn.rs`, `crates/aw-store/src/lib.rs`, `crates/aw-store/src/sandbox_owner.rs`, `crates/aw-store/src/sandbox_owner/effects.rs`, `crates/aw-store/src/sandbox_owner/effects/journal.rs`, `crates/aw-store/src/source_barrier.rs`, `crates/aw-store/src/vault_grants.rs`, `crates/aw-store/src/vault_signing.rs`, `crates/aw-tools-adapter/src/process.rs`.

Process-level derived Vault proof projection now composes the original
`PreparedSandboxEffect`/ToolStarted claim, exact source lease set, current Sandbox
and ToolBinding authority, registered SigningVaultProvider and current named Env
VaultGrant. `SandboxEffectLaunch` is an owned one-shot revalidation boundary, not
new admission or settlement ownership. The lane rechecks the original intent,
source bindings, writer and complete held leases; the common Vault check verifies
exact grant/actor/access/Store/provider generations. Its synchronous read-only
IMMEDIATE transaction and live provider/tool guards cover OS spawn and scope
attachment. `SpawnControl::new_fenced` and `SpawnOnce` are Agent-neutral,
opt-in; the original Process retains all factual launch results. Cancellation and
absolute deadline are checked again before OS spawn. Driver pins stay held through
launch/attachment; a refused unused token is definite no-spawn. No second Process,
ledger, durable proof or schema change. Ordinary launch uses the same spawn/attach
helper without allocating a launch fence. `start_source_process_with_vault` selects
one explicitly authorized provider-defined integration, `AW_VAULT_SIGNING_PROOF`:
a domain-separated HMAC over length-framed Store ID/generation, exact Agent/actor,
binding/entry/scope, original effect ID, grant revision and bounded challenge.
The key remains encrypted in the provider backend; only the derived proof reaches
the approved child environment. Explicit or ambient reserved-name collisions refuse;
no arbitrary model target, generic credential/env read or retroactive revocation.
The consuming trusted driver must actually support the reserved integration and
isolate its child; this is not general Bubblewrap realization or a relying-service
freshness/expiry protocol. The original grouped source Process fixture validates
stale rotated grant refusal, collision refusal and real proof-bearing child with
ordinary ToolFinished/source settlement (held source association remains fixture-only).
Process regression exercises cancellation during the final fence. Final Process
**7/7**, adapter Process **27/27**, Vault **1/1**, Sandbox **4/4**, Host **16/16**,
strict affected all-target/all-feature Clippy, workspace all-target/all-feature
check and scoped fmt/diff passed. Linux runtime qualified; no Windows-native
runtime or production containment claim. The earlier broad Store failures remain
recorded above, not rerun here. No new production source admission, raw key
projection, socket/file delivery or public RPC.

### 63417f3 — Recover interrupted source custody through original Turn

Commit: `63417f31614b622c880270a155c18af134d782c3`. Changed paths: `.agents/docs/CURRENT_HANDOFF.md`, `.agents/plans/dl200-unified-jobs-agent-events-2026-11-20/TODO.md`, `crates/aw-host/src/source_process.rs`, `crates/aw-store/src/lib.rs`, `crates/aw-store/src/sandbox_owner.rs`, `crates/aw-store/src/sandbox_owner/effects.rs`, `crates/aw-store/src/sandbox_owner/effects/journal.rs`, `crates/aw-store/src/sandbox_owner/source_bindings.rs`, `crates/aw-store/src/source_compatibility.rs`.

Interrupted source-effect custody recovery now reuses the original settled Run's
immutable Turn projection when ToolFinished is absent. `SandboxToolTerminalRef`
distinguishes the actual numeric ToolFinished sequence from the original interrupted
Turn node; release facts retain only that closure identity and the factual physical
outcome, with existing numeric receipt encoding unchanged. The exact original call,
effect, execution site, Interrupted status, absent output and unique membership are
verified against the settled non-completed Run result. Running or pending-attachment
state is not closure; exact settled attachment conflict is supported. No new ToolFinished, inferred Process exit,
second journal or Run-hazard acknowledgement. Current external disclosure and
permanent physical quiescence/no-start proof are still independently required;
Unknown retains the original complete source claims. The existing release transaction
advances Sandbox rev once for settled write-capable effects, with lookup-only replay.
Exclusive owner startup compatibility permits held source-bearing unresolved Runs
only when each such Run has original journal-linked source custody for its exact
Agent/Sandbox and Store scope. Original Store recovery settles the Run; compatibility
never authorizes a new effect. Direct/staged backup validation remains quiescent-only,
open admission/unowned associations remain denied, and missing SourceDriver stays
recovery-required before physical inspection. Extended existing grouped Process
regression exercises loss before ToolFinished, exclusive Store reopen, Unknown,
missing-driver refusal and exact interrupted custody release without changing original
journal/Turn or broader inspection hazard. Terminal-linked recovery and lookup-only
replay across degraded reopen remain supported. Final Sandbox **4/4**, checkpoint
**4/4**, source barrier **1/1**, Host **16/16**, strict affected all-target/all-feature
Clippy, workspace all-target/all-feature check and scoped fmt/diff passed. Earlier
broad Store failures remain recorded, not rerun. Production physical proof acquisition
and whole-Run hazard reconciliation are distinct.

### bf8a536 — Retain source custody through native worker settlement

Commit: `bf8a536b6e738ee26d4f42130217b1966991a173`. Changed paths: `.agents/docs/CURRENT_HANDOFF.md`, `.agents/plans/dl200-unified-jobs-agent-events-2026-11-20/TODO.md`, `crates/aw-host/src/source_filesystem.rs`, `crates/aw-host/src/source_process.rs`, `crates/aw-store/src/checkpoint_owner/capture.rs`, `crates/aw-store/src/checkpoint_owner/restore.rs`, `crates/aw-store/src/construction_receipt.rs`, `crates/aw-store/src/sandbox_owner/effects.rs`, `crates/aw-store/src/sandbox_owner/effects/journal.rs`, `crates/aw-store/src/source_barrier.rs`.

Native filesystem physical completion now belongs to the actual blocking worker,
not the async result awaiter. The existing non-cloneable `SourceMutationBarrier`
owns one cheap SessionStore reference; `SandboxEffectLease` uses that same Store
owner rather than a second reference. No new lease type, registry, journal or schema.
`PreparedSandboxEffect::admit_with_authority` uses the original admission/claim
transaction and transfers capability, live policy and owned lease together; existing
async Process handoffs and checkpoint custody reuse that owner. The concrete native
worker records its original positive quiescence or Unknown before it releases the
live authority and returns. Dropping the awaiter no longer drops worker settlement
custody or the Store owner. Caught executor panic is Unknown, never inferred success;
actual process loss or failed physical proof commit still leaves unresolved custody.
The grouped native Host regression pauses an actual staged write through the existing
artifact reservation seam, drops its awaiter and verifies live source exclusion and
withdrawal waiting on the real worker. The worker completes, records its proof and
releases authority; the original journal recovers an Interrupted Turn after reopen
and consumes that proof without external reinspection or rewriting the tool receipt.
Ordinary ToolFinished/missed-hook recovery and Git exclusions remain exercised.
Final source barrier **1/1**, construction receipt **1/1**, checkpoint **4/4**,
Sandbox **4/4**, Host **16/16**, strict affected all-target/all-feature Clippy,
workspace all-target/all-feature check and scoped fmt/diff passed. Earlier broad
Store failures remain recorded, not rerun. This is native-worker completion,
not arbitrary Process containment, crash-time
private artifact cleanup or automatic whole-Run hazard acknowledgement. The source
bearing association remains fixture-only. Shared barrier/result API no longer needs
borrow lifetimes, allowing other trusted workers to retain the real owner.

### a9b0eaf — Fence ordinary Process launch and retain finite worker settlement

Commit: `a9b0eaf8c4540fb8490bde5f7e47bdc1c9039891`. Changed paths: `.agents/docs/CURRENT_HANDOFF.md`, `.agents/plans/dl200-unified-jobs-agent-events-2026-11-20/TODO.md`, `crates/aw-host/src/source_process.rs`, `crates/aw-store/src/sandbox_owner/effects.rs`.

Ordinary source Process launch now uses the same actual OS spawn/attachment
fence as the Vault projection. `SandboxEffectLaunch::with_source_authority`
reuses the exact original intent, binding/grant/writer observations, complete lease
pin and live ToolBinding/source authority transaction; Vault adds its independent
projection check inside that same boundary. Revoke/regrant after initial claim
cannot authorize the old launch. `admit_with_launch_authority` transfers the
original launch and settlement custody to the real worker. `run_source_process`
is a trusted finite noninteractive consumer of the same PipeProcess owner:
its local result-await cancellation token does not cancel the parent Turn, and
loss of the awaiter cannot abort the original wait/stop/reap and proof settlement.
Return the factual Process receipt (or definite/uncertain launch refusal) separately
from the source proof result. Positive proof is recorded before ordinary ToolFinished
release; no receipt is fabricated if the result is lost. Proof refusal/panic leaves
Unknown custody. A known exit/group scan alone is not proof; the reviewed driver
must independently establish permanent source quiescence. Existing interactive and
Vault start APIs still return explicit custody, not automatic terminal settlement.
Grouped Process regression exercises grant revoke/regrant between claim and spawn,
no-start proof settlement, actual successful terminal with refused source proof,
and dropped awaiter after a real fixed single-child workload enters; it observes
cancellation and durable quiescence, reopens the interrupted Run and reuses the
proof without rewriting history or acknowledging broader hazards. Sandbox **4/4**,
Vault **1/1**, Host **16/16**, strict affected all-target/all-feature Clippy,
workspace all-target/all-feature check and scoped fmt/diff passed. Earlier broad
Store failures remain recorded, not rerun. No schema, second Process/Job owner
or source-bearing admission change. Runtime loss and failed
proof commit still require reconciliation; production containment remains separate.

### faa6436 — Recover marked Git private-index scratch custody

Commit: `faa64366d6c60c7ad46042b276d488332a6040ac`. Changed paths: `.agents/docs/CURRENT_HANDOFF.md`, `.agents/plans/dl200-unified-jobs-agent-events-2026-11-20/TODO.md`, `crates/aw-git/src/source_driver.rs`, `crates/aw-git/src/source_driver/restore.rs`, `crates/aw-git/src/source_driver/tree.rs`, `crates/aw-git/tests/repository.rs`, `crates/aw-host/src/source_checkpoint.rs`, `crates/aw-host/src/source_checkpoint/capture.rs`.

Git capture and dirty restore now share explicit private-index scratch custody.
The original Prepared descriptor still owns the operation. `PrivateIndexes` creates
its private directory once and, on platforms with verified private files, syncs a
small sibling ownership marker containing the prepared identity and exact parent/
directory physical identities before Git writes any index. This is external artifact
ownership evidence only, not a new operation ledger, grant or completion record.
Successful execution explicitly removes the known scratch files and directory before
publication/retention; failed subprocesses leave custody for authorized recovery, not
recursive Drop cleanup. Read-only inspection and final revalidation require both
scratch and marker absent. Abandonment of unpublished capture/restore verifies the
original descriptor and exact repository, reconciles only the protected namespace's
known regular single-link index/lock files, syncs removal before retiring the marker,
and reuses the existing conditional ref or exact worktree cleanup. Replaced directory,
foreign content, malformed/missing marker, unaccounted alias or still-live Git writer
is not deletion authority. The independent host must guarantee the original namespace
remained protected and exclude surviving writers; this is not cryptographic protection
against hostile same-UID code. The pre-marker creation gap, legacy unmarked scratch,
partial/changed checkout and unsupported private-file platforms remain conservative
Unknown. No descriptor/schema or dependency change. Existing grouped real Git
capture/restore regressions inject failure after an actual private-index write or
checkout, verify refusal of same-mode replacement and foreign entries, preserve the
real HEAD/index/files, and settle the exact marked scratch through original abandonment.
Final Git repository **32/32**, Git library and doc tests, Host **16/16**, strict
Git/Host all-target/all-feature Clippy, workspace all-target/all-feature check and
scoped fmt/diff passed. Earlier broad Store failures remain recorded, not rerun.

### 11ad45f — Recover exact-subset unpublished Git materializations

Commit: `11ad45f9df64bcb3d4d4f50dc657058eee5fbaf0`. Changed paths: `.agents/docs/CURRENT_HANDOFF.md`, `.agents/plans/dl200-unified-jobs-agent-events-2026-11-20/TODO.md`, `crates/aw-git/src/lib.rs`, `crates/aw-git/src/source_driver/restore.rs`, `crates/aw-git/src/source_driver/tree.rs`, `crates/aw-git/tests/repository.rs`, `crates/aw-host/src/source_checkpoint.rs`.

Unpublished partial Git restore abandonment now uses the shared physical tree
verifier. Original locked detached registration, exact parent/repository and base
index, absent unresolved index lock and independent protected private-destination
custody remain prerequisites. Enumerate bounded actual namespace without following
links or depending on Git's untracked enumeration: every directory is an expected
ancestor or empty unchanged gitlink, every leaf belongs to the captured tree, and
missing leaves are permitted only for unpublished abandonment. Partial files must
match raw captured blob bytes; complete trees retain existing Git normalization and
mode semantics. Complete publication/revalidation and published retirement still
require the entire captured tree. Unexpected empty directories, ignored content,
changed bytes/types, nonempty gitlinks, invalid index and uncertain registration
remain fenced. Reuse existing exact worktree removal, scratch reconciliation and
Prepared release; no new descriptor, schema, effect or replay. Host must guarantee
the private candidate stayed unpublished/protected and exclude surviving writers
and unaccounted aliases. Legacy unmarked scratch, truncated/converted partial file
content without exact byte proof and ambiguous registration remain Unknown.
Grouped real Git regression checks actual partial checkout from captured index,
changed leaf/foreign empty directory refusal, exact abandonment/replay (including
after anchor retirement), and refusal of missing leaf on published retirement.
Final Git repository **32/32**, Git library/doc tests, Host **16/16**, strict
Git/Host all-target/all-feature Clippy, workspace all-target/all-feature check and
scoped fmt/diff passed. The initial focused run reached an obsolete fixture assertion
expecting the partial candidate to remain; updated the assertion and retained replay
after anchor retirement. Earlier broad Store failures remain recorded, not rerun.

### 8e3688c — Verify source-free layout before final source detachment

Commit: `8e3688c1346c1d318ae88c304136f73d2225046f`. Changed paths: `.agents/docs/CURRENT_HANDOFF.md`, `.agents/plans/dl200-unified-jobs-agent-events-2026-11-20/TODO.md`, `crates/aw-git/src/source_driver.rs`, `crates/aw-host/src/source_checkpoint.rs`, `crates/aw-host/src/source_git.rs`, `crates/aw-store/src/sandbox_owner/grants.rs`, `crates/aw-store/src/sandbox_owner/source_bindings.rs`.

Metadata-only final source detachment now uses the original `sandbox.sources.set`
mutation owner. Trusted `sandbox_sources_detach_final` is its final-null specialization,
not another RPC or durable operation. Exact sole binding, Project writer, open Sandbox,
CAS, no retained Agent binding or known shared open layout, complete source-set
exclusion and legacy run/hazard quiescence precede physical verification. The original
immediate transaction holds the independently approved verifier guard through commit.
`source_git::detach_final_source` uses the actual registered Git driver, an independent
whole-source fence and approved `GitSourceFreeLayout`. Existing cwd/root must be the
same canonical private empty directory, disjoint from the exact old checkout and Git
administration/backing directories by path and identity. Host must exclude aliases,
mounts, other writers and ambient source mappings; pins are not authority. No delete,
remount, changed layout, Agent admission/rebind or additional receipt. Normal unverified
final removal still refuses. Exact current-rev empty no-op skips verification, stale
rev conflicts, reattach mints a fresh binding generation. First-source attachment also
checks exact shared cwd/root reservations. Existing grouped Store/Host regressions
cover refusal for retained Agent, live source custody, shared layout, stale rev,
wrong physical identity and nonempty root; real detach and reattach leave Git HEAD,
index and files unchanged. Final Sandbox **4/4**, Git repository **32/32**, Git
library/doc tests, Host **16/16**, strict affected all-target/all-feature Clippy,
workspace all-target/all-feature check and scoped fmt/diff passed. Earlier broad
Store failures remain recorded, not rerun. This is only the already-source-free
layout case; actual materialization removal/replacement still requires Prepared physical effects. Lower
017 mount/Bubblewrap convergence remains frozen and untouched.

### 35d1d3f — Compose named multi-source native realization

Commit: `35d1d3f5059e9a23d7eecceecdb195d7a926be3a`. Changed paths: `.agents/docs/CURRENT_HANDOFF.md`, `.agents/plans/dl200-unified-jobs-agent-events-2026-11-20/TODO.md`, `crates/aw-host/src/source_approval.rs`, `crates/aw-host/src/source_filesystem.rs`, `crates/aw-host/src/source_git.rs`, `crates/aw-host/src/source_process.rs`.

Multi-source native filesystem realization extends the original live approval,
registered NativeFilesystemDriver and SourceFilesystemRoute rather than creating
another policy/execution owner. NativeSourceApprovalSpec now carries the exact
relative-path base and bounded named NativeSourceDirectorySpec set (including each
root's original private-artifact registry). One read guard owns the full canonical
approval and actual pins; revoke waits for the admitted worker. Original whole-root
single-source physical/policy encoding is retained; disjoint named direct children
use a framed whole-set digest. No parent/ambient capability for child layouts.
NativeSourceRoot is a reusable physical observation plus effective binding, not
an authority. ApprovedSourceLayout pins the exact narrowed set through the original
CapabilitySet backend, checks canonical root and object identity, rejects duplicates,
and is compared against the complete actual effect grant and Sandbox layout before
admission. Whole-root grants remain forbidden on a multi-binding Sandbox. Git resolver
checks current Store policy and exact named binding set before inspecting each approved
checkout, retains all repository pins until capability construction, and excludes Git
administration across the selected roots (rejects roots overlapping administration).
Read effects narrow all roots to RO; mutating effects expose only the permitted modes,
require at least one writable source, and claim every exposed source including RO roots.
No arbitrary nested/root mapping, implicit ambient parent access, lower mount changes,
new source-bearing admission or new persistence. Physical alias/mount/external writer
exclusion remains independent host authority. Updated the unreleased host approval
constructor and resolver to the common set form; no parallel single-root executor.
Grouped existing native regression uses actual sibling Git checkouts (generic native
variant without Git), verifies mixed RO/RW ceilings, denial of parent/unapproved sibling,
Git administration and escaped path, per-root source barriers and dropped-await worker
settlement/recovery through original Turn. Final non-Git native **1/1**, Host
**16/16**, strict Host all-target Clippy with all features and source-effects-only,
workspace all-target/all-feature check and scoped fmt/diff passed. Earlier broad
Store failures remain recorded, not rerun. Existing single-root physical encoding
remains compatible; general multi-source Process containment and host provisioning
still open.

### 9ce306e — Recover durable native artifacts through original source custody

Commit: `9ce306ed9f8ed740dc84f5fcc35487255f4b9656`. Changed paths: `.agents/docs/CURRENT_HANDOFF.md`, `.agents/plans/dl200-unified-jobs-agent-events-2026-11-20/TODO.md`, `crates/aw-host/src/source_approval.rs`, `crates/aw-host/src/source_filesystem.rs`, `crates/aw-host/src/source_git.rs`, `crates/aw-host/src/source_process.rs`, `crates/aw-tools-adapter/src/native_filesystem.rs`, `crates/aw-tools-adapter/src/private_artifacts.rs`.

Durable native artifact custody composes the existing adapter FileStore, registry
cache and lower exact private-artifact owner, not another ledger. A bounded private
`.source-owned-v1` namespace discriminator selects deferred source-owner recovery;
legacy ambient open refuses that same persistent namespace (including after cache
loss), and a source opener cannot adopt nonempty unmarked ambient records. Original
FileStore root digest/record format remains unchanged. `open_source_artifacts`
provisions in separately protected, canonical disjoint private state; recovery-only
`reopen_source_artifacts` requires the existing marked namespace. Missing or unsupported
state cannot become empty/volatile custody. Opening never reconciles source files.
`NativeSourceDirectorySpec::durable` composes the actual artifact registry with trusted
approval; custom providers can still supply the original Registry contract. Native
worker keeps the narrowed ApprovedSourceLayout through completion and checks that
writable private-artifact records are retired before recording positive source proof.
A factual successful ToolOutput survives unresolved artifact custody (Unknown).
`GitNativeSourceResolver::recover` uses the original journal-linked recovery owner,
typed registered native driver, original trusted-host filesystem intent, exact
claimed named roots, independent quiescence/namespace guard and the actual registered
Git driver/pins. Normal preparation and recovery share Git administration exclusions;
recovery refuses unexpected RO records or records overlapping excluded administration.
No arbitrary source file deletion, replay, replacement effect, new journal receipt or
Run-hazard acknowledgement. The original lower artifact reconciler uses exact durable
records; all writable records must retire before source release. Independent physical
and provider authority stays held through settlement, including failure. The host
must know the actual original durable artifact state namespace and exclude all surviving
workers, aliases and external writers; marker/empty registry alone is never quiescence,
no automatic policy reconstruction or proof from a wrong pre-provisioned namespace.
Unsupported private-file platforms remain denied; lower mount convergence unchanged.
Grouped adapter regression drops/reopens the actual durable registry, refuses legacy
automatic cleanup and reconciles an exact stage record. Existing native regression
now uses real durable namespaces and actual mutation lock instead of test-only
pausing Store. Under original source custody it simulates an interrupted Stage
finalization, verifies successful ToolFinished with Unknown custody, missing namespace
refusal, exact cleanup preserving unowned lookalikes and target contents, and terminal
replay. Existing detached worker/reopen settlement remains exercised. Final artifact
owner **8/8** (also green within adapter library run), non-Git native **1/1**, Host
**16/16**, strict Adapter/Host all-target/all-feature Clippy, source-effects-only Host
Clippy, workspace all-target/all-feature check and scoped fmt/diff passed. Broad
adapter library **254/255**: unchanged `job_tools::tests::skipped_node_read_does_not_wait_for_sibling`
fails the `finished.content.ends_with(" unchanged ack=all released=true")` assertion
both in the full run and isolated rerun. Parent baseline not rerun; do not claim this
failure pre-existing or the adapter suite fully green. Previously recorded Store
failures not rerun. This does not claim a real process crash at every boundary or
production Bubblewrap containment.

### 8efc5dc — Provision native approvals from exact Store source bindings

Commit: `8efc5dc95827423f862b9f3f6a379a7c55d2585e`. Changed paths: `.agents/docs/CURRENT_HANDOFF.md`, `.agents/plans/dl200-unified-jobs-agent-events-2026-11-20/TODO.md`, `crates/aw-host/src/source_approval.rs`, `crates/aw-host/src/source_git.rs`, `crates/aw-host/src/source_process.rs`, `crates/aw-store/src/sandbox_owner/grants.rs`, `crates/aw-tools-adapter/src/job_tools.rs`.

Trusted Git native approval provisioning now resolves the original current Store
bindings rather than requiring a copied complete binding payload or a prior grant.
`sandbox_grant_sources` checks Project writer, exact canonical open Agent/Sandbox,
actor Submit, exact expected Sandbox rev, bounded nonempty unique named selection
and live SourceDriver compatibility in one read transaction. This is trusted host
input only; caller must already independently authorize source metadata. No raw
locators added to ordinary metadata responses and no grant written. The generic
`NativeSourceApprovalSpec<D>` reuses the original subject/generation/base fields;
`GitSourceApprovalDirectory` carries independently approved exact location,
binding revision and access. `GitNativeSourceResolver::provision_approval` verifies
complete selected bindings, exact named layout, modes, registered Git implementation
and physical identities before provisioning durable artifact namespaces. It retains
Git pins through original NativeSourceApproval construction, checks the actual pins
and rereads the exact current Store observation before returning the live revocable
approval. The caller transfers independent host authority into that approval, not a
copy of persisted grant evidence. Existing explicit grant CAS remains the only
publication owner and revalidates actor and binding generations. No auto-activation,
new registry, persistence, or physical source mutation. Host must still independently
exclude mounts/aliases/external writers and own the decision; no automatic discovery
of authority or public protected metadata endpoint. Grouped existing Store/Host
regressions cover denied manager, stale revision, duplicate selection, wrong physical
pin, no artifact state on these refusals, actual two-source approval and subsequent
ordinary grant/route/recovery. Generic non-Git approval API remains usable.
Final Sandbox **4/4**, non-Git native **1/1**, Host **16/16**, strict Store/Adapter/Host
all-target/all-feature Clippy and Host source-effects-only Clippy, workspace
all-target/all-feature check and scoped fmt/diff passed.

The prior adapter failure was a stale test assertion: finite-Job ACK receipts append
the canonical provenance suffix. The grouped skipped-node test now checks the exact
ACK portion and parses/verifies the existing provenance fields; idempotent ACK and
nonexistent explicit stream refusal remain unchanged. No Job runtime change.
Isolated ACK regression and complete adapter library **255/255** passed. Previous
Store-wide legacy failures were not rerun.

### 4ddeed8 — Share finite source Process settlement with Vault projection

Commit: `4ddeed8d0023cd0222633fc496f0d252e3dda54c`. Changed paths: `.agents/docs/CURRENT_HANDOFF.md`, `.agents/plans/dl200-unified-jobs-agent-events-2026-11-20/TODO.md`, `crates/aw-host/src/source_process.rs`.

Finite Vault proof-bearing Processes now reuse the exact ordinary Process
launch and worker settlement owners. A statically selected private projection
callback composes the one-shot spawn token, original source/tool/Store fence
and actual signing-provider guard. Both interactive start APIs and both finite
run APIs share the same spawn preparation and cancellation logic; no extra
runtime policy dispatch, separate task owner, dependency or durable schema.
`run_source_process_with_vault` retains the original source lease through
wait/stop/reap and deferred physical settlement, including lost awaiters. The
existing `SourceProcessCompletion` keeps factual Process receipts and source
quiescence results separate. The finite path refuses stale Vault grant and
reserved-env collision without spawning or inspecting a nonexistent terminal.
Existing interactive Vault success path remains exercised. Grouped real Process
regression now loses the finite Vault awaiter after a fixed child verifies the
proof and signals entry. Provider withdrawal remains future-only and cannot
release that child's source claim; awaiter loss cancels only the execution,
not parent Turn. Original worker records quiescence and interrupted Turn
recovery uses it without reinspection, rewriting receipt or acknowledging
broader Run hazards. Driver proof is still independently required; the fixed
single-child fixture is not general containment. Interactive settlement stays
explicit, runtime loss/failed proof commits remain recovery cases. Socket/file
and other provider integrations remain distinct; no lower mount convergence
change. Standalone effectful Sandbox construction still requires a genuine
Project-owned receipt scope, not a synthetic Agent audit origin.
Final Host **16/16**, strict Host all-target Clippy with all features and
source-effects-only, workspace all-target/all-feature check and scoped fmt/diff
passed. Unchanged Store/adapter suites were not repeated.

### a446b86 — Own standalone Sandbox construction in Project-scoped receipts

Commit: `a446b8632487620e8291db64f115c12fa10c40ba`. Changed paths: `.agents/docs/CURRENT_HANDOFF.md`, `.agents/plans/dl200-unified-jobs-agent-events-2026-11-20/TODO.md`, `crates/aw-host/src/source_checkpoint.rs`, `crates/aw-store/migrations/060_project_construction_origin.sql`, `crates/aw-store/src/agent_host.rs`, `crates/aw-store/src/checkpoint_owner/capture.rs`, `crates/aw-store/src/checkpoint_owner/restore.rs`, `crates/aw-store/src/construction_receipt.rs`, `crates/aw-store/src/lib.rs`, `crates/aw-store/src/sandbox_owner.rs`, `crates/aw-store/src/sandbox_owner/construction.rs`.

Standalone versioned Sandbox creation now consumes the actual shared construction
owner. Schema v60 widens the original immutable audit/receipt table to accept either
its existing Session foreign key or a canonical Project origin for construction;
the migration preserves legacy audit sequence/data and recreates the same replay and
unique Prepared identity indexes. No additional operation ledger or fake Session.
Project receipts omit Agent origin fields, carry the exact candidate Sandbox,
authenticated creator/request/fingerprint, writer epoch, layout digest and independently
retained baseline SourceSet + original Restore descriptors. The common claim,
complete-set source barrier, inspection validation, fenced publication and reconciled
abandonment primitives are reused. Existing Agent/checkpoint receipt encodings and
legacy security audit mapping are retained; Project security audit contains only
redacted operation/state. One atomic publication inserts just the fresh Sandbox and
its source bindings and transitions the original receipt Applied. The exact current
Project authority, writer epoch, fresh layout, source/schema compatibility and
candidate identity are rechecked at publication. Replay uses the original result;
resume inspects the original descriptor without reselecting versions or executing
again. Abandon uses the original Rejected receipt; no physical retry or Applied
cleanup. Git host shares materialization planning, execution, proof and cleanup with
checkpoint restore; independently authorized live whole-set fence still spans commit.
Original Applied standalone provenance now participates in closed-Sandbox retirement.
Grouped real Git lifecycle tests force failed publication after both materializations,
prove no Agent/Session/visible Sandbox was created, recover across restart (missing
driver refuses), replay, close and reconcile the original retained layout, and
abandon a no-effect unresolved candidate. Existing receipt regression checks exact
legacy audit fact preservation through migration. Broad Store library **239/244**:
the exact same five earlier failures (legacy Agent shell disabled Job subscription,
v38 principal downgrade, v35 remote-start downgrade, remote-view reconnect and v34
remote-view downgrade); all other Store tests passed, including construction,
migration, source custody and backup/restore qualification. Final Host **16/16**,
strict Store/Host all-target/all-feature Clippy (after redundant-borrow cleanup),
workspace all-target/all-feature check and scoped fmt/diff passed. This is the retained-version
fresh Project baseline case, not mutable parent derivation, unversioned baseline
capture, effectful replace/remove, public RPC or source-bearing execution admission.

### 950965b — Derive isolated Sandboxes through original source custody

Commit: `950965b98b9f24b0c9f5f7af078f507f62c5d1d0`. Changed paths: `.agents/docs/CURRENT_HANDOFF.md`, `.agents/plans/dl200-unified-jobs-agent-events-2026-11-20/TODO.md`, `crates/aw-agent-core/src/source_driver.rs`, `crates/aw-git/src/source_driver.rs`, `crates/aw-git/src/source_driver/restore.rs`, `crates/aw-git/tests/repository.rs`, `crates/aw-host/src/source_checkpoint.rs`, `crates/aw-store/src/construction_receipt.rs`, `crates/aw-store/src/lib.rs`, `crates/aw-store/src/sandbox_owner.rs`, `crates/aw-store/src/sandbox_owner/construction.rs`, `crates/aw-store/src/sandbox_owner/source_bindings.rs`.

Same-Project mutable parent Sandbox derivation now uses the same v60 Project
construction receipt, source barrier, inspection validation, publish and abandon
owners. `SandboxCreateFrom::{Baseline,Parent}` and borrowed `SandboxCreateSources`
select the exact retained anchors or original versioned parent bindings; existing
baseline JSON/fingerprint remains compatible. Parent lineage and exact open parent
revision, complete nonempty required binding set, Project writer, legacy layout
quiescence, fresh destination and compatibility are checked before planning, at
claim and publication. Derive requests include the exact parent in their replay
fingerprint. No public checkpoint, Agent, Session, extra receipt stage or ledger.
Core `SourceOperationKind::Derive` and Git recovery schema 4 preserve old schemas
1–3. One Git descriptor fixes the original capture observation/identity and exact
reserved destination before either effect. Driver reuses private-index capture,
verifies retained deterministic snapshot, and composes existing RestorePlan
materialization using the outer descriptor's stable worktree marker. No new plan
or capture on inspection/recovery; capture-only or partial child is Unknown.
Abandon/retire removes only the original child first, then its exact private
capture retention. When retention is already absent, require target and original
marked registration absent before release; no prune or guessed cleanup. Parent
HEAD/index/content remain unchanged; live independent whole-set physical and
provider authority remains mandatory, including backing custody. Parent logical
close is allowed once quiescent, but retained children (including closed awaiting
cleanup) block physical parent retirement and structural binding edits. Derived
binding edits are held until effectful structural custody exists. Publication
retains exact parent lineage and original Applied provenance; retirement uses
same registered driver and existing closed-Sandbox owner. First realization
supports nonempty all-required same-SourceRef isolated Git parent sources; mixed
or ephemeral/source-free parents and alternative new-backing source identities
remain held rather than guessing a baseline policy. Native COW drivers may reuse
the core operation and Store contract with explicit compatible semantics. No DB
schema change. Focused real Git capture-only failure regression refuses retry,
protects foreign target and releases exact original retention after proven absence.
Expanded existing Host lifecycle derives from dirty linked parent, tests partial-set
abandonment, failed publication/restart/resume/replay, no public checkpoint or
Session, unchanged parent HEAD/index, independently changing parent contents and
retained child/parent retirement ordering. Final validation: core source contracts
1/1, Git 33/33 (library and doc targets also passed), construction receipts 1/1,
Sandbox owners 4/4, Checkpoint owners 4/4, Host 16/16; strict affected-crate
Clippy, workspace all-target/all-feature check, scoped fmt and diff checks passed.
The earlier broad Store result (239/244, five recorded legacy failures) was not
rerun.

### 17fdc07 — Release managed Sandbox sources through original materialization custody

Commit: `17fdc07e16efc65040a219b00245e06a9f793646`. Changed paths: `.agents/docs/CURRENT_HANDOFF.md`, `.agents/plans/dl200-unified-jobs-agent-events-2026-11-20/TODO.md`, `crates/aw-host/src/source_checkpoint.rs`, `crates/aw-store/src/construction_receipt.rs`, `crates/aw-store/src/lib.rs`, `crates/aw-store/src/sandbox_owner.rs`, `crates/aw-store/src/sandbox_owner/source_bindings.rs`, `crates/aw-store/src/sandbox_owner/source_bindings/release.rs`, `crates/aw-store/src/source_barrier.rs`.

Effectful removal of one originally constructed source is now a trusted realization
of sources.set(name,null). Uses the original immutable Applied restore/derive
descriptor, not a fresh delete/materialization plan, and the existing source
coordination owner. A reserved release claim identifies the exact Sandbox CAS
revision and original selected descriptor; the whole old source set stays
retained/fenced until settlement. No new schema, receipt ledger or driver operation
kind. Read-only authorization/physical preflight before claim; recheck exact
Project writer, open Sandbox, complete original bindings, quiescence, layout and
dependencies before release and before publication. A failed effect/commit retains
original rows and hazard; explicit same-revision recovery verifies the original
release, including proven absence, without re-materializing. One shared binding
mutation publication helper advances Sandbox rev once and emits sources.changed;
the release uses a stable original-descriptor fact identity and released=true.
Later closed retirement selects the remaining original descriptors only after
verifying exact release facts for absent names. Legacy metadata-only deletions are
not physical release proof. Ordinary metadata mutation refuses managed
construction changes that would lose original custody. Parent retained children
and shared open materializations remain fenced. First concrete Git realization
requires an unused Sandbox, disjoint private named children, unchanged cwd=root,
independently held complete physical/provider fence, exact original selected
checkout (or proven absence); verifies retained siblings by identity, not content,
and releases only selected worktree plus its own derived retention if applicable.
Final removal keeps the empty original parent directory; no root deletion or Agent
admission. Prior CAS revision conflicts; ordinary current-rev absent-name null
remains a no-op. New materialization attachment/replacement, bound-Agent removal,
whole-root removal and legacy ambiguous custody still need explicit handling.
Grouped existing Host lifecycle exercises a real derived source release, failed
publication after physical removal, restart/wrong claim refusal, successful
reconciliation preserving independently edited sibling, remaining subset
retirement and final-source empty parent. Final construction receipts 1/1,
Sandbox owners 4/4, Checkpoint owners 4/4, source barrier 1/1, Host 16/16;
strict Store/Host all-target/all-feature Clippy, workspace all-target/all-feature
check and scoped fmt/diff passed. Earlier broad Store 239/244 not rerun.

### 04d058d — Attach versioned Sandbox sources with exact materialization provenance

Commit: `04d058de343f96e12e79a543195ddbb35d245215`. Changed paths: `.agents/docs/CURRENT_HANDOFF.md`, `.agents/plans/dl200-unified-jobs-agent-events-2026-11-20/TODO.md`, `crates/aw-host/src/source_checkpoint.rs`, `crates/aw-store/src/construction_receipt.rs`, `crates/aw-store/src/lib.rs`, `crates/aw-store/src/sandbox_owner.rs`, `crates/aw-store/src/sandbox_owner/source_bindings.rs`, `crates/aw-store/src/sandbox_owner/source_bindings/attachment.rs`, `crates/aw-store/src/sandbox_owner/source_bindings/release.rs`, `crates/aw-store/src/source_barrier.rs`.

Effectful attachment of an independently retained version uses the original v60
Project-scoped construction owner with a private SandboxAttachment candidate
and exact old Sandbox CAS, writer, layout, full original bindings and incoming
anchor. One new Restore descriptor is claimed in the existing Prepared receipt
before effects; same old+incoming SourceMutationSet excludes participating writers.
Explicit resume inspects, never replans; abandon reconciles the exact private
unpublished candidate. Applied replay matches the original authenticated request,
Sandbox revision and anchor. No schema, new driver operation or parallel ledger.
Publication atomically inserts the new binding at the next Sandbox rev, advances
the existing source CAS and emits sources.changed with opaque Prepared provenance;
same transaction marks original receipt Applied and releases the barrier.
Managed mutation/retirement now resolves exact Prepared descriptors per current
binding. Rev-1 bindings retain initial construction identity; later attachment
bindings resolve the original immutable claim by their exact changed fact and
unique Prepared index, not by name or path. A reattached old name must have the
prior original descriptor's release fact. Managed release and closed retirement
support heterogeneous retained descriptors; the older homogeneous
sandbox_restore_preparation view still validates one PreparedSourceSet.
Shared read-only managed layout observation and original claim lookup reused.
Concrete Git attachment requires nonempty already-managed unused open Sandbox,
private named-child layout, cwd=root, complete independently approved old+incoming
locations, absent target on first planning, sibling identity (not unchanged
content) and held physical/provider authority through settlement. Existing
restore, inspect, publish and private cleanup primitives reused. No implicit
materialization of ephemeral or unversioned inputs, bound-Agent mutation,
first-source attachment on an empty layout or replacement of an occupied name.
Those require their own exact reservation/publication semantics, not a fake
fresh Sandbox or a destructive remove+attach presented as atomic replace.
Existing grouped real Host lifecycle exercises same-name reattachment after
proven release, abandoned private candidate, failed publication/restart,
no new execution on replay, missing-driver refusal, edited sibling preserved,
new binding revision and mixed Restore/Derive closed retirement. Final construction
receipts 1/1, Sandbox owners 4/4, Checkpoint owners 4/4, source barrier 1/1,
Host 16/16; strict Store/Host all-target/all-feature Clippy, workspace
all-target/all-feature check and scoped fmt/diff passed. Earlier broad Store
239/244 not rerun.

### 3181bd0 — Replace managed Sandbox sources through one Prepared materialization

Commit: `3181bd01163f0b2be4da7bbdc15bf6e77449c242`. Changed paths: `.agents/docs/CURRENT_HANDOFF.md`, `.agents/plans/dl200-unified-jobs-agent-events-2026-11-20/TODO.md`, `crates/aw-git/src/source_driver/restore.rs`, `crates/aw-git/src/source_driver/restore/replacement.rs`, `crates/aw-host/src/source_checkpoint.rs`, `crates/aw-store/src/sandbox_owner/source_bindings.rs`, `crates/aw-store/src/sandbox_owner/source_bindings/attachment.rs`, `crates/aw-store/src/sandbox_owner/source_bindings/release.rs`, `crates/aw-store/src/source_barrier.rs`.

Versioned replacement of an occupied managed source reuses the SandboxAttachment
receipt with an explicit replace selection (old attach encoding/fingerprint
unchanged). Original old binding set and provenance plus incoming Restore
descriptor are claimed before any effect. Complete old+incoming identities share
one barrier; admission count is the resulting named count, not the temporary
union of backing identities. No new schema, ledger or driver descriptor kind.
One publication removes the exact old binding generation, inserts the new at
next Sandbox rev, records incoming Prepared provenance and old release identity
in the same sources.changed fact, marks Applied and settles the barrier.
Existing per-binding lookup and later retirement require exact prior release;
no intermediate visible remove/attach. Git read-only planning admits an occupied
destination only after verifying the exact releasable original checkout; verifies
incoming independently retained version and both independently approved origins.
First effect releases original with existing restore/derive cleanup, then
materializes the exact new Restore descriptor. Old private derived retention
cannot serve as the incoming anchor. Publication checks exact incoming checkout,
absence of old registration/marker and old derived retention/scratch (same
common repo allows only the verified new registration). Normal resume is
inspection-only; untouched original with no incoming registration can be
abandoned. Explicit post-release continuation requires target and incoming
registration/marker absent, reconciles exact old release, verifies positive
incoming absence again, then executes the original descriptor under a newly
authorized complete-set fence. No automatic retry or rejection after release;
ambiguous or partial incoming effects stay held. No post-publication old-tree
edits discarded. First concrete path remains unused private named-child layout,
cwd=root, same destination, no unaccounted aliases/dependencies or external
writers; independent old disposal + incoming metadata/materialize authority
mandatory. Grouped real Host lifecycle covers distinct third backing at
two-name limit, changed old refusal, untouched abandonment, actual gap,
inspect-only/unsafe-abandon refusal, explicit continuation, failed publication,
restart/resume/replay, prior derive retention released, sibling edits preserved,
mixed-origin retirement, same-common-repository replacement and final release.
Final Git 33/33, construction 1/1, Sandbox 4/4, Checkpoint 4/4, source barrier
1/1, Host 16/16; strict Git/Store/Host all-target/all-feature Clippy, workspace
all-target/all-feature check and scoped fmt/diff passed. Earlier broad Store
239/244 not rerun.

### 2541805 — Reserve empty Sandbox layouts through first-source attachment

Commit: `2541805a68b1e9e87340da1713f7f461b912a7cf`. Changed paths: `.agents/docs/CURRENT_HANDOFF.md`, `.agents/plans/dl200-unified-jobs-agent-events-2026-11-20/TODO.md`, `crates/aw-host/src/source_checkpoint.rs`, `crates/aw-host/src/source_process.rs`, `crates/aw-store/src/checkpoint_owner/restore.rs`, `crates/aw-store/src/construction_receipt.rs`, `crates/aw-store/src/coordination.rs`, `crates/aw-store/src/sandbox_owner.rs`, `crates/aw-store/src/sandbox_owner/source_bindings.rs`, `crates/aw-store/src/sandbox_owner/source_bindings/attachment.rs`, `crates/aw-store/src/sandbox_owner/source_bindings/release.rs`, `crates/aw-store/src/source_barrier.rs`.

First-source attachment into an empty Sandbox reuses the existing Project
Prepared attachment receipt and original Restore descriptor. Empty old sets
add exact tool-root/cwd layout scopes to the same durable source coordinator
claim, independent of incoming backing identity and without consuming named
source slots. Scope keys are separately namespaced hashes of the Store-known
paths; physical aliases, ancestry, mounts and cross-Store writers still require
the independent host fence. Nonempty old receipts retain their existing scope
encoding. Empty layout observation requires unused open Sandbox, no retained
Agent or Session at either exact path, no open alias/dependency or closed
retained layout, current writer and legacy run/hazard quiescence. Shared
layout admission checks reject reservations in normal Sandbox creation,
structural metadata mutation, closure, ambient Agent binding and execution.
Legacy ambient selection also refuses exact source-bearing layout aliases
across Projects after successful first publication. Recheck the original
layout/revision and complete claim through the same final transaction; no
new reservation table, migration, operation kind or ledger. Recovery derives
exact layout scopes from the immutable original empty layout and reclaims
the original source hazard, never a new plan. Abandon settles that same
reservation only after exact unpublished candidate reconciliation. Managed
materialization provenance now permits absence of an initial restore/derive
receipt, requiring exact attachment provenance for each later binding; rev-1
legacy bindings without original receipt remain unsupported. Managed
attachment history prevents metadata-only custody loss even after final
release. Git reuses existing named-child layout and restore checks: private
canonical root, cwd=root, no foreign entries and absent target on initial
planning, independently authorized whole-layout physical fence mandatory.
Existing grouped real Host lifecycle now covers pristine empty Sandbox,
foreign entry refusal, competing first attachment from another backing,
blocked closure/alias/legacy Agent creation, untouched abandonment, failed
publication and restart/missing-driver recovery, exact replay, subsequent
replacement/removal, reattachment and closed retirement without an initial
construction receipt. The stronger open-layout alias gate intentionally
changes old fixture assumptions: checkpoint regression now asserts early
ambient rejection; Process/native fixtures publish their fixture-only source
layout after ordinary source-free journal admission. Broad Store 231/244
initially: the five unchanged legacy failures, one old alias expectation and
seven ENOENT child spawns after a concurrent focused rebuild replaced the
test binary. Corrected alias regression 1/1, stable-executable coordination
24/24 resolve all eight new failures; combined coverage 239/244, not a
single all-green run. Host 14/16 initial (two old fixture-order assumptions),
corrected Process/native tests 2/2; grouped Git lifecycle passed. Strict
Store/Host all-target/all-feature Clippy, workspace all-target/all-feature
check and scoped fmt/diff passed.

### f3a7535 — Reconcile exact partial Sandbox replacements through original custody

Commit: `f3a7535262d588e08a8d058201467fcd3decf0e7`. Changed paths: `.agents/docs/CURRENT_HANDOFF.md`, `.agents/plans/dl200-unified-jobs-agent-events-2026-11-20/TODO.md`, `crates/aw-git/src/source_driver/restore/replacement.rs`, `crates/aw-host/src/source_checkpoint.rs`.

Explicit replacement continuation now also reconciles an exact unpublished partial
incoming checkout. Reuses the original Prepared receipt and whole-set fence,
existing exact restore inspection/partial cleanup and the predecessor-release
proof shared with final publication. First verify incoming registration and
prove original old registration and private derived retention/scratch absent.
An exact completed incoming checkout is retained, not destroyed/recreated.
Otherwise existing private cleanup accepts only proven original contents,
registration and scratch; retry the original descriptor only after positive
absence of target and registration. Foreign entries, modified/truncated leaves,
ambiguous registration/scratch and retained predecessor remain held. Normal
resume remains inspect-only; unsafe post-release rejection remains denied.
No new schema, operation or recovery state machine. Existing real grouped Host
lifecycle exercises the post-release gap, an exact partial candidate (fixture
omits an expected leaf), foreign entry refusal/preservation, explicit cleanup
and continuation, failed publication, completed candidate retention and restart
inspection/publication. Host 16/16, Git 33/33, strict Git/Host all-target/all-feature
Clippy, workspace all-target/all-feature check and scoped fmt/diff passed. Broad
Store not rerun; previous combined 239/244 remains the recorded result.

### 2b4fac3 — Enforce trusted minimum Process containment before launch

Commit: `2b4fac33d8345f284ecfd3e0585f332c72037dc6`. Changed paths: `.agents/docs/CURRENT_HANDOFF.md`, `.agents/plans/dl200-unified-jobs-agent-events-2026-11-20/TODO.md`, `crates/aw-host/src/source_authority.rs`, `crates/aw-host/src/source_process.rs`, `crates/aw-process/src/lib.rs`, `crates/aw-tools-adapter/src/process.rs`.

Trusted minimum Process containment now has one Agent-neutral owner:
aw_process::ContainmentRequirement::{BestAvailable,ClosedDescendants}.
Process::spawn_with_containment refuses an unsupported minimum before entering
the launch fence/lane or invoking OS spawn, with factual ContainmentUnavailable;
existing spawn defaults to BestAvailable. The current Windows suspended Job
backend is the only existing closed-descendant backend; Direct Unix group
containment (including Linux) cannot satisfy the strict requirement. No fallback,
no stronger backend promotion and no changes to Occupancy or stop evidence.
PreparedPipeLaunch carries the minimum by value through both fenced/unfenced
adapter paths and additional custody. Independent SourceToolAuthority minimum
combines monotonically with the driver's prepared requirement at the single
shared source Process handoff, covering finite, interactive and Vault projection.
Unsupported minimum becomes PipeLaunchFailure::NotStarted and settles the
original Tool effect through its existing journal; no fabricated terminal or
quiescence proof. Ordinary weaker execution remains available only under its
existing trusted host policy and independent physical quiescence requirement.
No mount/pinning/Bubblewrap changes, schema, dependency or new Process owner.
Focused runtime/adapter checks plus grouped Host source-effect regression prove
no downgrade, no launch fence/OS execution on unsupported Unix and original
NotStarted settlement. Windows runtime qualification and strong Linux backend
remain deferred; this is an enforceable policy minimum, not a physical authority
provider or production source-aware dispatch. Shared Process 8/8, adapter
258/258, Host 16/16, source-effects-only Host 2/2; strict affected-crate
all-target/all-feature Clippy, workspace all-target/all-feature check and scoped
fmt/diff passed. Broad Store not rerun; previous combined 239/244 remains.

## 9. Copy-ready prompt

See [AW-DL200-PHASE3-NEXT-AGENT-2026-11-25.md](AW-DL200-PHASE3-NEXT-AGENT-2026-11-25.md). This prompt is the current handoff; the historical prompt below is not current.

---

# Appendix A — complete historical November 24 late handoff

**Historical snapshot only.** The following is preserved verbatim from the signed documentation checkpoint c5104db. All old current-state, publication, pending-work and continuation instructions are superseded by §§1–9 above and the live plan. Retained here to preserve original decisions, rejected designs, exact prior chronology, validation and historical source index, not to require repeating old work. The original file remains independently available at [AW-DL200-PHASE3-FULL-COMPREHENSIVE-HANDOFF-2026-11-24-LATE.md](AW-DL200-PHASE3-FULL-COMPREHENSIVE-HANDOFF-2026-11-24-LATE.md).

# AW + dotlink / DL-200 R6 — comprehensive Phase-3 continuation handoff

**2026-11-24 late checkpoint · L5 · Store schema v58**

Canonical onboarding document after `AW/AGENTS.md`. This replaces the earlier
November 24 Phase-3 handoff as current guidance, not as historical provenance.
It records the sealed design lineage, the complete implementation chronology,
owner boundaries, qualification, remaining work and the exact next-agent
procedure. It is an orientation document, not a competing normative SPEC.

## 1. Read this first: exact continuation state

Last signed **implementation** checkpoint (beneath this documentation update):

```text
0bf9bf6cbdce3d0929885198f93528572b85cf2b
Restore baseline initial tasks through existing encrypted custody
parent: 85028a4ed680760c9dbf3e198a1ec18d1c006421
Share initial task custody with captured-source checkpoint restore
```

At handoff creation, HEAD and refs/heads/main both resolved to the implementation
checkpoint. Its signature was verified with the expected ED25519 signing key
(see §2). A signed documentation-only commit may sit above it; do not mistake
that for new implementation. Revalidate the live parent before every write.

**Important correction to the last chat and older prompt:** task-only baseline
restore/recovery is already implemented at 0bf9bf6. Do not redo it. The older
184ea6b handoff is the first SourceDriver prerequisite, not current HEAD.
Current schema is confirmed by `crates/aw-store/src/lib.rs`:
`LATEST_SCHEMA_VERSION = 58`. AGENTS.md's older v33 text is historical; obey its
operating/ownership rules and use live code for schema. No new schema migration
was needed for the later Phase-3 checkpoints.

| DL-200 phase | Status at this handoff |
|---|---|
| 0 / 0B | Sealed and published; shared Process/release correction and replay/disclosure hardening complete; real Bubblewrap and Windows-native runtime evidence deferred |
| 1 | Complete locally through 7f4724f (Agent, ACL, history, Sandbox, staged inputs, provenance, state RPCs) |
| 2 | Complete locally through e0c6bb3 / schema v54 (Event owner/cutover) |
| 3 | Underway at v58; foundations, capture/restore, native host composition, retention, baseline, initial ACL/tasks live at trusted lower boundaries; production admission and provisioning still held/pending |
| 4 | Pending formal Jobs high-water, own cursor/tree traversal, ancestor fencing |
| 5–6 | Pending final typed registry composition and dynamic Process Extensions/SDK |
| 7 | Pending compatibility contraction, drain/restart evidence, final cross-repo qualification |

The TODO's combined “Phase 1/4” item is not evidence that Phase 1 is undone;
the local Phase-1 close and later Phase-4 work are distinct. README's older seal
heading mentions a pending Phase-0 corrective gate; its publication was completed
at the recorded signed baseline. Do not rewrite the sealed design to update status.

### Priority next implementation boundary

**Exact live source-aware Actor / Turn admission**, only when its original
journal/context/queue owners are available. Do not open the held gate based on
a persisted SandboxGrant, registered driver, cached preflight or configured router
alone. Use the actual current independent host approval plus original Agent,
ToolBinding, writer epoch, Actor Envelope, source-effect and journal owners.
Inspect the exact source files and the concurrent ownership before choosing the cut.

At the handoff observation, these paths remain under unrelated concurrent work:

- `crates/aw-agent-core/src/agent.rs`
- `crates/aw-agent-core/src/journal.rs`
- `crates/aw-store/src/journal.rs`

There is also unrelated uncommitted AW networking, remote workspace, child-actor,
minimal-session, resource CLI, routing, skill-package, TUI and Workspace work.
Do not infer authorization to inspect or edit those diffs from this handoff.
Do not touch reserved UX plan files or staging. Coordinate through the user/owner
if required; do not invent a mechanism for inter-agent communication.

If the journal owner is still busy, choose **one independent real consumer cut**:
production host/restore composition with explicit independently supplied physical
authority, or effectful fresh-baseline Prepared custody if the sealed contract and
live owner support it without overlapping work. VaultProvider/VaultGrant is also
a sealed Phase-3 requirement: take only a coherent actual owner/consumer slice
when prerequisites are satisfied. Do not create speculative admission abstractions
or open unsupported Graph/Job/Process/control paths to make progress look complete.
Keep Phase 3 unchecked until the actual grouped qualification succeeds.

## 2. Non-negotiable operating constraints

- Source authoring: **dotlink---l5 only**.
- AW: `/home/pvl/spaces/abird/src/aw`.
- dotlink: `/home/pvl/spaces/abird/src/dotlink`.
- Read-only release reference: `/home/pvl/spaces/abird/src/dotlink-017-mounts`.
- Do not use A1, create another worktree, reset/stash/revert unrelated work, or
  overwrite another agent's staged/unstaged changes.
- Never list/read/inspect/write/stage/reset/stash/commit the reserved
  `AW/.agents/plans/ux/**` subtree or its staging. UI is separately owned.
- **Frozen:** Sandbox physical mount/config-v13 and release/0.17 physical Sandbox
  work. Phase 3 is the logical owner layer. No thaw without explicit promotion.
- No push, tag, release, publish, deploy or destructive history action without
  separate user authorization.
- Keep each implementation turn around 30 minutes; stop at the next coherent
  tested, signed checkpoint, not an arbitrary percentage or a speculative future cut.
- Elegant, simple, clear, intuitive Rust. Static/zero-cost abstractions when
  practical; take easy performance wins without maintenance debt. Keep reusable
  primitives for other models/hosts. Prefer clean/simple first and targeted
  hot-path optimization. One grouped real failure-boundary regression per
  owner; no fixture/evidence explosion, repeated artificial pinning or unneeded
  test infrastructure. Never count a zero-test filter as a pass.
- Preserve no_std/alloc, minimal/stateless, full and durable feature boundaries.
  XDG project-owned data follows AGENTS.md. No credentials, real grants, runtime
  databases, transcripts, caches or build outputs in Git.

Expected signing identity:

```text
Good "git" signature for pvl@prasannavl.com with ED25519 key
SHA256:4aB+NvRb1shCx4xjtZDr2Y7FzfHGph80eFtpF+pHuCI
```

At each coherent checkpoint: focused real regression(s), relevant strict
check/Clippy/fmt/diff, update CURRENT_HANDOFF + active plan TODO factually, commit
only exact owned paths in a private GIT_INDEX_FILE with an expected-parent CAS on
main, verify signature, normalize only owned paths, pause. Push separately only
if requested. See §9 for a safe practical sequence.

## 3. Git / publication truth

These published refs were verified earlier in the active handoff chain and are
**carried publication baselines**, not newly remote-queried at this docs update:

| Ref / role | Exact commit |
|---|---|
| AW last verified remote main | `78339410710472ec51d5ff46a5c9e586b67ff30d` — Own logical Sandboxes |
| AW published shared Process / R6 base | `20a63a4d4547cc20b2a154787823125b82f5e385` |
| dotlink local/published main baseline | `85ceccc1088e201c98c994a46a68834440eeab32` — Publish DL-200 R6 consumer and repin AW |
| dotlink AW pin | `20a63a4d4547cc20b2a154787823125b82f5e385` |
| frozen release/0.17 | `576d0a1a3fe0a5522a02d51f1d0598c3ddc160c2` — v0.17.9 |

Current local observations (read-only at handoff):
AW HEAD/main `0bf9bf6...`; AW tracking origin/main
`0f3f3ebbfaa12e66c1aeb0f5c2a27faf53e84e2e` is stale relative to the carried
remote baseline. dotlink main/HEAD `85ceccc...`, tracking origin/main
`fb1b7b97891d45e84d2a729f3eeb1a7cfe0eaa94`; release checkout HEAD at
`576d0a1...`, both dotlink checkouts clean. Committed dotlink Cargo.toml and
Cargo.lock pin the published AW 20a63a4 source. No fetch, remote publication,
repin or release performed. **A local tracking ref is not remote truth.**
Phase-1/2/3 as a whole remain unpublished. Separate verified remote query is
required before making new publication claims.

## 4. Mandatory reading and historical design lineage

Read in order (paths relative to AW unless indicated):

1. `AGENTS.md` and the plan index `.agents/plans/README.md`; inspect the
   active plan/ownership line in `.agents/plans/TODO.md` without entering reserved
   UX paths.
2. This comprehensive handoff (the §10 chronological record is available for
   exact per-cut details; its interim pending statements are historical).
3. `.agents/plans/dl200-unified-jobs-agent-events-2026-11-20/README.md`
4. Same plan: `SPEC.md` (normative; especially §0.1–0.3, §18, §21.5, §25.4–25.6,
   §28/29 and relevant admission/Process sections).
5. Same plan: `IMPLEMENTATION.md` and `TODO.md`.
6. `.agents/docs/CURRENT_HANDOFF.md` (live incremental owner record).
7. `.agents/docs/DL200-R1-R5-RESOLVED-DESIGN-CHECKLIST-2026-11-21.md`.
8. `.agents/docs/UNIFIED-JOBS-AGENT-FINAL-HANDOFF-2026-11-14.md`.
9. `.agents/docs/AW-DL200-R6-FULL-COMPREHENSIVE-HANDOFF-2026-11-22.md`.
10. `.agents/docs/AW-DL200-PHASE1-FULL-COMPREHENSIVE-HANDOFF-2026-11-23.md`.
11. Relevant earlier handoff/design references below; do not reread large
    historical dumps if the current task does not need them.
12. For cross-repo work: dotlink/AGENTS.md, dotlink/.agents/docs/CURRENT_HANDOFF.md
    and actual consumer/pin source.
13. Live source/tests and owner paths for the precise implementation slice.

Live source and executed tests outrank dated prose. Sealed SPEC governs
unimplemented behavior; IMPLEMENTATION supplies phase boundaries and grouped
acceptance. TODO is the actual queue. CURRENT_HANDOFF is the current incremental
record. This document is a dated orientation and historical record; prior
handoffs are history, not current status. No extra design authority is created.

### Beginning-to-current provenance index

The underlying work did not begin in Phase 3. The original AW foundations and
all prior handoffs remain in Git under the following live paths; these are the
detailed records of designs, rejected options, scope, implementation and tests.

| Era | Authoritative/historical reading |
|---|---|
| Original AW foundations, numbered phases 0–68, Four Primitives, Hooks/Trust, Agent/Turn runtime | [October 19 comprehensive handoff](AW-COMPREHENSIVE-HANDOFF-2026-10-19.md), [runtime primitives](RUNTIME-FOUR-PRIMITIVES-AND-SERVICES-2026-10-11.md), [hooks/trust](RUNTIME-HOOKS-TRUST-AND-POLICY-2026-10-11.md), [architecture](ARCHITECTURE.md), [SDK](SDK.md), [state](STATE.md) |
| DL-150 exact remote/tool authority and replay | [dotlink tool port plan](../plans/dotlink-tool-port/), [DL-150 reference](../plans/dotlink-tool-port/reference/DL150-LIVE-CA89F959-README.md) |
| Nov 9–14 unified Jobs/Agent (Agents folded into Jobs, provenance, control, wake) | [Unified runtime plan](../plans/ai-native-unified-jobs-agent-runtime-2026-11-11/), [Nov 14 comprehensive handoff](UNIFIED-JOBS-AGENT-FINAL-HANDOFF-2026-11-14.md), [operations/transcript plan](../plans/ai-native-operations-agents-jobs-transcript-2026-11-10/), [Job control/wake plan](../plans/ai-native-job-control-wake-2026-11-10/) |
| Nov 15–17 shared Toolset, job output, stop and ACK/EOF | [Nov 15 AW handoff](AW-FULL-COMPREHENSIVE-HANDOFF-2026-11-15.md), [Nov 17 AW/dotlink tools handoff](AW-DOTLINK-TOOLS-FULL-COMPREHENSIVE-HANDOFF-2026-11-17.md), [Job output plan](../plans/ai-native-tool-output-grep-2026-11-15/), [Toolset builder plan](../plans/ai-native-toolset-builder-2026-11-15/) |
| Nov 18–20 release/0.17 convergence and shared runtime | [Nov 18 handoff](AW-DOTLINK-017-FULL-COMPREHENSIVE-HANDOFF-2026-11-18.md), [Nov 19 handoff](AW-DOTLINK-017-FULL-COMPREHENSIVE-HANDOFF-2026-11-19.md), [Nov 20 cross-repo handoff](AW-DOTLINK-FULL-COMPREHENSIVE-HANDOFF-2026-11-20.md), [shared runtime](../plans/017-shared-runtime-convergence-2026-11-18/), [stop provenance](../plans/017-stop-provenance-2026-11-18/), [pre-017 convergence](../plans/pre-017-shared-convergence-2026-11-18/) |
| DL-200 R1–R5 design/reviews and explicit decisions | [Nov 20 full handoff](AW-DL200-FULL-COMPREHENSIVE-HANDOFF-2026-11-20.md), [R1–R5 resolved checklist](DL200-R1-R5-RESOLVED-DESIGN-CHECKLIST-2026-11-21.md) |
| R6 seal, shared Process correction, publication and phase-0B | [Nov 21 R6 handoff](AW-DL200-R6-FULL-COMPREHENSIVE-HANDOFF-2026-11-21.md), [Nov 22 R6 handoff](AW-DL200-R6-FULL-COMPREHENSIVE-HANDOFF-2026-11-22.md), [active normative plan](../plans/dl200-unified-jobs-agent-events-2026-11-20/) |
| Phase 1 Agent/state/ACL/history foundation | [Nov 23 Phase-1 handoff](AW-DL200-PHASE1-FULL-COMPREHENSIVE-HANDOFF-2026-11-23.md) |
| Phase 2 Events and first Phase-3 DriverKey prerequisite | [Earlier Nov 24 Phase-3 handoff](AW-DL200-PHASE3-FULL-COMPREHENSIVE-HANDOFF-2026-11-24.md) |
| All later Phase-3 cuts through task-only baseline | This document (§6, §7, §10), active TODO and CURRENT_HANDOFF, signed commits |

The older handoffs are deliberately not rewritten as current status. Original
archived pre-rewrite material and its bundle locator remain described in
AGENTS.md / plan index; do not reconstruct deleted massive evidence trees,
invent Phase 69, or pull frozen or separately owned work into this effort.
Additional relevant live docs: DOMAIN-MODEL, DATABASE-ARCHITECTURE,
LOCAL-OWNER, LOCAL-PROTOCOL, TOOLS, CONTEXT-FLOW, CONTEXT-FLOW-IMPLEMENTATION,
CREDENTIAL-PLACEMENT, GIT-WORKFLOW, RESTORED STATE authority plan, and the
rest of the indexed product plans. Scope is the AW/dotlink DL-200 continuation,
not a claim of completion of every unrelated AW initiative.


## 5. Sealed architecture and initial design decisions

The following design synopsis retains the earlier comprehensive handoff's
architecture and design lineage. The active SPEC remains normative.

### Original architecture north star

#### One truth, one owner

Do not introduce second mutation/authority owners.

Examples:

- Agent envelope/ACL: agent_state_current + agent_acl_current.
- Session is backing storage, not public Agent truth.
- Event history: immutable Event projection/log, not per-consumer payload queues.
- Event subscription: progress only.
- EventHandler: activation/progress + one receipt, not product state.
- EventInput: accepted typed Event input fact.
- ActorQueue: queue custody, not EventInput identity.
- SourceDriverRegistry: driver definition/compatibility identity, not SourceBinding state.
- logical Sandbox: binding membership/lifecycle owner.
- Checkpoint/SourceAnchor tables: immutable checkpoint/source recovery facts, not live binding state.

#### Permanent public concepts

Keep the public mental model small:

Domain -> Workspace -> Agent/Turn/Jobs plus logical Sandbox, Sources, Checkpoints, Vaults and typed
Extensions.

Do not create a public universal resource/service abstraction merely because internal mechanics share code.

#### Canonical identity

Keep canonical slash refs and typed handles distinct from display/presentation identity.

Root/current/stable aliases are resolution conveniences, not competing durable identities.

#### Agent vs Session

Agent is public semantic truth.

Session remains backing storage/runtime history compatibility.

Do not move Agent revision/access/lifecycle truth back into Session title/participation/audit.

#### Jobs vs Events

Jobs are hierarchical and need their own Phase-4 high-water/tree frontier.

Events are one Store-local ordered stream and use EventScan.

Do not reuse EventScan for Jobs.

#### Logical Sandbox vs physical sandbox

Logical Sandbox is the resource/environment envelope.

Physical mount/config-v13 work is separate and frozen.

#### Leaf registries

Final leaves are final from first implementation.

Final leaves already live:

- EventTypeRegistry
- EventHandlerRegistry
- SourceDriverRegistry
- SandboxDriverRegistry

Later ExtensionRegistry composition must compose these exact owners rather than replace/migrate them.

---

### Original historical design lineage

#### AW foundations

AW already had one Agent runtime, TurnMachine/ToolRegistry/ToolPolicy, immutable journal/history,
Domain/Workspace/Session storage, Process/tool execution and explicit actor/source/site/effect grants.

DL-200 is not a second runtime.

#### DL-150 and dotlink

DL-150 established remote execution receipts, exact authority binding, replay/no-rerun behavior and Job
output/control semantics. DL-200 reuses those lower facts.

#### Unified Jobs + Agent — November 9-14

The model surface converged on flat Jobs operations:

- jobs_list
- jobs_get
- jobs_read
- jobs_send
- jobs_cancel
- jobs_ack

Agent was folded into that model rather than becoming a parallel model-plane control family.

The November 14 handoff is the best historical record.

#### November 15-20 cross-repo convergence

AW and dotlink converged on shared Process/Job semantics, actor/site/effect grants, cancellation/timeout/
NoTarget/Unknown distinctions and the shared lower runtime.

#### DL-200 R1

R1 simplified the public architecture around:

- public Agent / internal Session;
- unified Jobs;
- generic Events;
- immutable history/forks;
- logical Sandbox/Source/Checkpoint/Vault;
- typed registries/extensions.

#### R2-R5

R2-R5 hardened access/revisions/history/disclosure/groups, Event ordering and cutover, SourceSet/checkpoint
semantics, Sandbox/Vault boundaries, typed registry identity and dynamic-extension/Process behavior.

The R1-R5 checklist is design provenance, not implementation evidence.

#### R6

R6 simplified private mechanics without creating a framework:

- StoreScope/StorePos;
- EventScan for actual Event consumers only;
- staged immutable registry publication;
- stable DriverKey separate from runtime generation;
- shared Prepared/ExternalEvidence vocabulary;
- shared Process owner.

R6 is sealed and normative.

---

### Explicitly rejected designs

Do not add:

- TransactionManager
- OperationManager
- global replay ledger
- universal revision clock
- generic Jobs+Events traversal engine
- distributed/global Event router
- public spawn-pool framework
- second Process/Event/Project/Session/Source/Agent-ACL owner
- unused registry implementations
- Agent-to-Sandbox rebind
- cross-store Event-to-Agent delivery in v1
- cross-store ordered recursive Jobs traversal in v1
- restore-specific Agent/Sandbox constructor
- runtime registry generation as durable driver identity
- arbitrary Vault env/path/mount target language
- provider fence handle treated as cached permission
- task-local/global Event causal owner
- per-subscriber Event payload queues.

---

### Exact public and internal contracts to preserve

Canonical resource refs: `/d:<domain>/i:<principal>/p:<project>/a:<agent>`
with `/t:<turn>/j:<job>/n:<node>` where meaningful; `/s:<sandbox>` is a
Project resource, not an ExecutionRef. Public Agent owns security/history/work;
Project is a Workspace-backed view; Session is backing storage. Original
Runtime/Actor/Message/Envelope, Hook Next/Defer, ToolPolicy/ToolRegistry and
immutable TurnJournal remain. Jobs model API is list/get/read/send/ack/cancel;
no restart/background modes or Jobs-only subscription. Agent input/staged
inputs, Process stdin/EOF and finite Graph semantics remain distinct.
Events use one ordered Store-local stream, source-first authorization, source
revisions, immutable typed EventInput and original delivery owners. V1 has no
cross-store Event→Agent delivery or ordered recursive cross-store Jobs.

Owner / Fact / Operation / Projection / Cursor / Scope are roles, not base
classes or generic frameworks. StoreScope uses existing store_id +
restore_generation; StorePos is commit-log seq in that Store. No global clock.
Agent rev, ACL/security generation, input rev, Job owner rev, Event source rev
and byte offsets remain distinct. The Process Scope is one dispatch lifetime;
Sandbox governs physical/logical environment, not Process ownership.

One `aw-process` owns child/scope/pipe/stop/terminal; async bounded admission,
private logical Linux launch facility, O(1) process-lifetime lanes (N=1), no
retirement/shrink or thread-per-Process. Launch checks cancellation and the
canonical absolute Running deadline before fork; no-start is not a ProcessTerminal.
Launched Process uses factual stop/exit/output semantics; Unknown/NoTarget/Stopped
stay distinct. Stop reaching a target does not prove descendants quiescent.
Windows uses suspended/no-window launch and exact occupancy settlement; real
native qualification remains deferred. Preserve original tool and Actor grants,
participant provider/payer identity, payload disclosure on replay, and compact
payload-free tombstones for original effect identity. See R6 amendment for
exact gates and no duplicate Process owner on dotlink.

### Phase-1 owner summary

#### Agent owner

agent_state_current owns frozen canonical Agent handles/backing Session+branch, owner, title, Agent rev,
ACL rev, lifecycle/admission, immutable parent lineage and history/Sandbox pointers.

Agent rev advances only on factual Agent mutation.

Reads, cursor reads, Process output and group membership churn do not manufacture Agent rev.

#### Agent ACL

Seven allow-only permissions:

- view
- contribute
- submit
- control
- derive
- share
- manage

Effective permissions are owner UNION direct user grant UNION matching active group grants.

access_generation is an opaque security/observation fence, not Agent rev.

#### Principals/groups

One Domain principal namespace, no nested groups, immutable group handles, complete-set membership
replacement, group rev separate from Agent rev.

#### Immutable history/disclosure

v40 owns HistorySpan/Base/Grant and a persistent content-addressed DisclosureManifest DAG.

History sealing does not advance Agent rev.

Forks share immutable history exactly but create fresh mutable/security state.

#### Logical Sandbox

v41 owns logical Sandbox identity and immutable Agent->Sandbox binding.

Physical mount/config-v13 remains separate/frozen.

#### Staged input plan

v42/v43 own staged intent and mutation replay.

Send is permission-checked, replay-safe and atomic.

#### Provenance/exposure

Turn/model/tool and finite Job/Process provenance are immutable facts.

Agent exposure is a rebuildable projection from authoritative owner state.

#### Native Agent state RPCs

Phase 1 closes with one AgentStatePort over existing Store methods for:

- agent.list
- agent.get
- agent.edit
- agent.admission
- agent.access.list
- agent.access.set

No second state/ACL owner.

---

### Phase-2 Event owner summary

#### Event projection / EventScan

Store-local immutable Event ordering over StorePos.

One private EventScan serves read, subscription and Handler.

Source visibility precedes deeper disclosure.

Unknown holds progress.

#### Durable subscriptions

Identity is recipient Agent + normalized subject.

Only progress/generation/authority state is persisted; no payload queue.

#### EventInput

Immutable Agent input fact, not Event backlog.

Admission rechecks recipient/source/disclosure/placement and dedupes before causal budget charge.

#### Causal scope

Fresh Event ID is the root when none exists.

Accepted EventInput can bind to one factual Turn.

Descendant Events inherit root and increment hop.

No global/task-local causal owner.

#### EventType / custom Events

Typed schema identity rides the ordinary Event envelope.

Same-subject raw Event remains generic without matching schema tag.

#### EventHandler

Definition registration is inert.

Store activation owns one cursor and at most one deterministic in-flight receipt.

No Handler product-state queue.

#### Same-store broad universes

Only currently-authorized matching Domains participate in completeness.

Hidden remote Domains do not perturb affinity.

Authorized remote placement stales/lags rather than returning a partial world.

#### v54 cutover

Whole source revisions belong to exactly one delivery owner.

Compatibility sidecars are retained until Phase 7.

Do not remove them in Phase 3.

---

### Schema evolution

| Schema | Existing owner |
|---|---|
| 38 | authoritative Agent envelope + ACL |
| 39 | Domain principal/group owner |
| 40 | HistorySpan/Base/Grant + DisclosureManifest DAG |
| 41 | logical Sandbox owner |
| 42–43 | staged input plan and mutation replay |
| 44–46 | Event projection, source revs, durable read cursors |
| 47–50 | exact Project placement, subscriptions, typed EventInput, disclosure/causality |
| 51–54 | typed custom Events, Handler receipts, same-store universes, v36 delivery cutover |
| 55 | named SourceBindings and durable compatibility |
| 56 | immutable Checkpoint / SourceSet / anchor retention |
| 57 | indexed construction receipt with Prepared source custody |
| 58 | independent SandboxGrant evidence |

Compatibility contraction belongs to Phase 7 after drain/restart evidence.
Do not delete the v36 provenance sidecars or old state/Job compatibility during
Phase 3. The previous accepted checkpoint 47ee8a03abecbb3aee84a49c0a590096d76ed1a7
remains historical validation provenance, not proof of unchecked gates.


## 6. Phase 3: current implementation and precise owner map

Use these paths as a map, not a mandate to edit all of them. The detailed
chronological checkpoint notes are preserved in §10; the table below gives
the **latest** meaning after all those cuts.

| Boundary | Actual owner / reusable primitives | Status and limits |
|---|---|---|
| Typed driver definitions | `crates/aw-agent-core/src/source_driver.rs`, `registry.rs`, `sandbox_driver.rs` | final add-only leaves; DriverKey + independent artifact schema; no replacement/removal yet |
| Live logical source state | `crates/aw-store/src/sandbox_owner.rs`, `sandbox_owner/source_bindings.rs`, `source_compatibility.rs`, migration 055 | names, access, checkpoint policy, binding generation, startup degraded diagnostics, metadata-only mutation, closed retained release |
| Git physical source implementation | `crates/aw-git/src/source_driver.rs`, `source_driver/tree.rs`, `source_driver/restore.rs`, `tests/repository.rs` | exact clean/dirty capture, Gitlink semantics, isolated restore, materialization identity, conservative exact cleanup |
| Immutable checkpoints | `crates/aw-store/src/checkpoint_owner.rs`, `checkpoint_owner/retirement.rs`, migration 056 | metadata list/get, history-only/empty Auto, live history GC roots, tombstones, shared retention |
| Durable construction custody | `construction_receipt.rs`, `source_barrier.rs`, `coordination.rs`, migration 057 | original indexed Prepared/Unknown/Applied/Rejected receipts, complete source claims, exact recovery/publication/abandonment; no second ledger |
| Capture / restore | `checkpoint_owner/capture.rs`, `capture/recovery.rs`, `restore.rs` | actual capture+restore lower paths; baseline and task-only recovery; same original constructor |
| History/ACL/initial input | `history_owner.rs`, `agent_host.rs`, `agent_owner.rs`, `actor_queue.rs`, `secret_store.rs` | one initial audience and HistoryGrant, task custody and atomic Pending; no checkpoint secrets or inherited mutable inputs |
| Independent source grant | `sandbox_owner/grants.rs`, migration 058 | exact current policy/authority/actor/binding evidence; does not grant from ACL alone |
| Original effect custody | `sandbox_owner/effects.rs`, `sandbox_owner/effects/journal.rs` | typed driver preparation, whole set barrier, exact ToolStarted/ToolFinished, deferred physical settlement, Unknown retained |
| Native and Process integration | `crates/aw-host/src/source_{authority,process,filesystem,git,approval}.rs`, `aw-tools-adapter/src/{process,native_filesystem,operation_router,lib}.rs` | lower trusted explicit route, one source dir, same native engine and Process owner; no production unrestricted shell or graph |
| Actor activation | `src/agent_lifecycle.rs`, original Actor/Store journal/context/queue/admission | explicit Git native host selection checks admission before ambient root; actual source-bearing execution still held; coordinate dirty owners |
| Vault | SPEC §25.5, §28.10 | design sealed; provider/grant owner, safe projection and real Process integration pending |

Store paths in this table after the first fully qualified Store path are relative
to `crates/aw-store/src/`; migration paths are under `crates/aw-store/migrations/`.
Inspect real source/tests for function signatures; prose is not a code interface.

### A. Source identity and effect invariants

- `DriverKey` stable built-in/Extension namespace + local ID; adjacent Binding,
  Anchor, Recovery schema pins independent. Typed handles have registration
  revision/TypeId, not a durable authority. Resolve persisted state by stable key
  plus supported artifact schema. Leaf add-only until the complete durable
  dependency set (live/retained bindings, anchors, unresolved recovery) can be
  proven. Missing/incompatible definitions retain exact data and fail closed;
  compatibility diagnostic is not actual materialization/retention evidence.
- SourceName `[a-z][a-z0-9_-]{0,63}`, ReadOnly/ReadWrite, Required/Ephemeral.
  Effective max default 128, host may only lower, defensive absolute 1024.
  New admission checks full set before external preparation/effects. Existing
  retained recovery remains available when limits lower.
- Store-local complete-set source mutation claims key stable source identity,
  block live and unresolved hazards. Avoid per-source partial acquisition.
  Retained cleanup shares claim-or-recover logic, reserved claim namespaces.
- Prepared capture/restore descriptor before first external effect; non-secret
  bounded identity fields only. Recovery inspects exact original descriptor;
  NotStarted/Completed/Unknown, no blind retry; NotStarted is not inferred from
  lost retention/materialization. ExternalEvidence Immutable may support final
  commit under current local authority; revocable HeldFence needs actual live
  provider fence valid through whole final transaction. Persisted handle is not
  permission. No provider-wide generic secret/evidence manager.
- Final AW transaction alone exposes the checkpoint or fresh Agent/Sandbox.
  Postcommit scheduler wakes/cache refreshes reconstructible; no wake-gated
  discoverability. Original receipt identity and caller disclosure/replay remain
  independent of subsequent ambient target context.

### B. Git and checkpoint semantics

- Built-in Git key `builtin/git`, binding schema 1, anchors 1–2, recovery 1–3.
  Absolute trusted executable, explicit registry registration. No implicit
  ambient Git config, credentials, hooks, filters, network/fetch or alternate
  backing. Local checkout/common-dir identity pin. Exact tracked plus nonignored
  untracked regular files; tracked symlinks retained, untracked link traversal
  refused; clean initialized gitlinks exact HEAD, uninitialized empty preserves
  index link; no implicit nested fetch. Unsupported changes fail closed.
- Dirty capture uses private indexes, deterministic synthetic commit, exact
  `refs/aw/checkpoints/<checkpoint>/<source>` absent-old CAS. Never mutate
  user HEAD/index or expose synthetic commit as restored HEAD.
- Restore to isolated fresh worktree at original detached base, base real index,
  captured dirty tree materialized via private index. Verify exact parent, origin,
  target/registration/operation lock, contents and index. Missing/changed/partial
  candidates are Unknown unless exact reconciliation proves release.
- Checkpoint captures exact committed HistoryBase + immutable SourceSet.
  Explicit None = current independently authorized destination baseline.
  Empty Auto = no required anchors, optional current ephemeral baseline only.
  Nonempty Auto = exact captured required sources; Selected = exact named captured
  sources, no silent fallback. No implicit task/ACL/credential inheritance.
  Every restore creates a new Agent + new Sandbox, shares immutable history only.
  No inherited Vault secrets, credentials, live leases, network/device authority,
  active Processes, staged inputs or mutable security state.
- Public metadata is redacted; source locator/disclosure independently authorized.
  Logical retirement blocks new restores, retains original replay tombstone, and
  refuses unresolved restore dependencies (including task-only). Cleanup of
  retained anchors is separate, exact, fenced and idempotent. Live shared anchors
  prevent release. Rejected unpublished candidate never replans; Applied cannot
  be abandoned.
- Quiescent binding mutation is metadata-only, old full set plus incoming
  fenced, original Sandbox rev used for monotonic changed binding generation.
  Removing/reinserting identical source cannot revive old grants. Initial attach
  only unused isolated layout, existing source-bearing Agents held. Final-source
  removal blocked until safe source-free layout proof. Effectful mutation needs
  original Prepared custody.
- Closed Sandbox retains original bindings and known root/cwd reservation
  until independently authorized, proven entire-layout release. No live bound
  Agents or source hazards. Closure works without drivers; diagnostics revalidate
  retained rows on restart. Cleanup exact old bindings and shared dependencies,
  live external guard through commit. Historical source-bearing Session remains
  effect-inert after release; fresh authorized clean baseline may use released
  root. Exact Store path fencing is not physical alias/ancestor/cross-Store
  containment; trusted host must supply that.
- Managed Git cleanup only exact unchanged original restore (clean or captured
  dirty) with original Applied/Prepared descriptor and independent physical
  fences. Refuse later user edits, ignored extras, changed index/HEAD, moved/
  changed registration, missing original objects or uncertain scratch. Conditional
  private ref deletion only expected OID, no broad prune, shared object deletion
  or generic recursive deletion. Git's exact removal of locked worktree needs
  two force flags, gated on proven original content and external authority.
  Unpublished untouched registered no-checkout and exact completed candidates
  can release; arbitrary partial/scratch ownership remains unresolved.
  Production automated cleanup not claimed. Real deletion tests only in fixtures.

### C. Source-effect / runtime authority

SandboxGrant is independent of Agent ACL and ToolBinding. Live exact actor,
Sandbox, complete effective binding set + version, current host policy evidence,
physical pin and existing tool authority must intersect. Agent→Sandbox immutable.
Driver preparation is inert until final admission with complete claims. Shared
`SourceToolAuthority` uses original ToolBinding and Actor Envelope publication
fence. No model-selected ambient path or duplicate Process owner. Launch pins
and host guard travel into actual async Linux launch lane; cancellation and
revocation use original lifecycle. Pre-spawn refusal neutral; uncertain setup/
unproven descendant quiescence stays Unknown. Native blocking worker owns pinned
capability and live host guard even if awaiter drops. Real physical quiescence
record precedes logical ToolFinished; original receipt hook or restart uses
same atomic source release, no pending completion map.

Direct journal-linked claims derive reserved effect identity from exact original
ToolStarted (active Turn, actor, effect/source/site, original arguments and
writer epoch). No relaunch for same intent, even after release. Original
ToolFinished logical receipt plus separate factual physical settlement required.
Missing terminal belongs to original reconciliation owner, never fabricate.
Deferred positive proof can settle after restart; Unknown preserves barriers.
One direct journaled tool effect supported, not multiple Graph/Job nodes.

Native route: canonical parser/limits/executor/result projection via pinned
platform CapabilitySet, exact single directory, effective read/write mode,
Git admin/.git/common dir exclusions. No ambient fallback, legacy native
argument shapes, shell, Job/control/wake/Graph/remove in root-free registration
(remove requires separate durable artifact admission). Real Git resolver uses
actual Store registry, independently approved root and verified materialization
identity, blocking work off async scheduler. Weak registration-side tool
authority breaks router cycle. Each builder gets a fresh slot and exact limits.
No second policy registry. NativeSourceApproval independently pins directory,
Agent/session/branch/Store/actor; explicit grant CAS; live withdrawal waits
for admitted guards, denies new; persisted grant alone insufficient. Trusted
host owns physical resolver and provisioning. HostAgentActivation explicitly
selects route and checks original Store activation first. Held gate stays shut
across activation/Turn/context/queue until original owner integration is complete.

### D. Initial ACL and task — most recent implementation

Custom history access uses `AgentHistoryForkAccess::Custom(Vec<AgentCreateAccessGrant>)`
(Clone, no longer Copy; Private/Inherit encoding unchanged). Current destination
owner resolves exact principals in destination Domain, rejects invalid/duplicate/
owner grants, and validates explicit disclosure expansion before planning and
again at publication. Ordinary agent.create(from=...) and both restore modes
publish one initial ACL + HistoryGrant in shared constructor. No source grant
from history ACL.

InitialTask mints candidate-local queue identity, encrypted payload ref/digest,
pinned trusted execution site; old receipts decode omitted site, no-task request
fingerprint unchanged. Captured-source restore: original Prepared receipt and
complete source barrier commit before encrypted write, verify custody before
atomic publication of new Agent/Sandbox/history/ACL/Pending/Applied. Queue
remains held for source-bearing Agent. Abandonment proves source cleanup first
then discards only exact private unpublished task; no deletion of Applied input.
Normal queue owner handles published cancellation/deletion.

**Latest (0bf9bf6): task-only baseline initial input is implemented too.**
`checkpoint_restore_baseline(..., request_id, execution_site)` supports optional
task, `checkpoint_resume_baseline(..., request_id, binding, baseline_sources)`
and `checkpoint_abandon_baseline(..., request_id)` recover exact receipt.
Source-free or empty-Auto already-materialized independently authorized baseline:
no fictitious source barrier. Source-set digest pins exact canonical baseline
selection and original history/layout. Existing exclusive Store connection
guard serializes bounded local encrypted write and final publication/explicit
recovery. Vault initialized/opened before claim; Prepared before ciphertext;
reuse existing PreparedQueueCustody and AEAD checks, refuse master regeneration
while original custody exists. Resume inspects original encrypted input and
current independent authorization; never rewrites missing ciphertext or
rematerializes/replans. Applied replay before current context; Rejected terminal,
Unknown retains original custody and history dependency. No-task remains one
atomic Applied transaction. Abandon deletes private task only, never baseline
files or Applied queue item. Source-free ordinary admission, any sources held.
Do not redo this cut.

### E. Lower trusted APIs vs promoted public host surface

Current trusted Rust lower owner entry points include checkpoint list/get metadata,
history-only/empty Auto create, prepare/inspect/replay/abandon capture,
prepare/inspect/replay/abandon captured restore, baseline restore/resume/abandon,
checkpoint delete and retired-source reconciliation; Sandbox create/fresh sources,
grant set/status/preflight, named source mutation, close, restore preparation
lookup, closed-source reconciliation, typed source-effect prepare and journal-linked
settlement/recovery. These do not by themselves implement normative checkpoint/
Sandbox/Vault RPCs or production runtime provisioning. Verify replay/preflight
ordering at the actual public host boundary; do not expose locators or credentials.
Actual method names and signatures in live source are authoritative.

## 7. Remaining work and suggested next cuts

The following queue is a dependency/ownership map, not permission to promote a
new phase or to bypass concurrently owned files.

| Remaining boundary | Required closure |
|---|---|
| Source-aware Actor/Turn admission | Original Agent/Actor/TurnJournal/context/queue gates participate with live exact approval, same writer/authority and immutable binding; preserve source-free route and all current denial/recovery paths. Qualify end-to-end real approved native Actor and restart before opening only that route. Coordinate journal owner first. |
| Production independent approval | Trusted host/config/RPC supplies actual physical approval + source/provider fences; never infer from stored grant, paths or ACL. Register actual driver in owner open. Wire exact builder/activation, startup/revocation, replay-first current disclosure. No global policy registry. |
| Public checkpoint/Sandbox host RPCs | Implement normative §18/§25.4/§28–29, original Port/constructor/ACL/task owners, request replay before target/expensive preflight, bounded metadata, actual approved baseline and SourceDriver effects; no restore-specific constructor. Public list/get/create/restore/delete and associated host selection/grant/lifecycle surfaces remain unpromoted. |
| Effectful baseline materialization | Already-materialized baseline is green; actual external effects must use original Prepared descriptors and complete-set recovery before write, proper custody/fences and atomic publication. Task-only path is not a shortcut. |
| Multi-source policy and source-bearing Job/Process | Complete logical layout/physical authorization including overlapping roots, aliases, artifacts and descendants. Actual contained command integrated with original aw-process and Job dispatch/receipt owners. No frozen Bubblewrap change without promotion. |
| Incomplete original effects | Original journal/reconciliation recovers missing terminal and factual Process/native custody; never infer physical completion from ToolFinished or generic error; no second ledger. |
| Cleanup composition | Trusted host composes exact original descriptors and independently authorized source/destination/provider fences, safe whole-layout release; conservative partial/scratch reconciliation, no automatic deletion of changed user work. Verify current live dependencies. |
| Final-source removal/effectful structural mutation | Prove new safe baseline/physical isolation and exact source mutation custody; no Agent rebind or ambient fallback. |
| Concrete provider fences | For revocable proof, actual provider-held lease valid through commit. Retained identity handles alone insufficient. Materialized audience expansion remains Unknown until provider-held principal fence. |
| VaultProvider/VaultGrant | Follow sealed §25.5 and §28.10: separate Agent-owned grant refs/evidence, provider owns secret value/rotation, actor/principal + access-generation ceiling and per-use current provider authority. Safe provider-defined broker/socket/ephemeral-file/env projections into original Process; no arbitrary targets or wholesale mount. Rotation/revocation governs future projections, never claim retroactive erasure of Process material. No secrets in checkpoint, journal/receipt, logs or public metadata. |
| SourceDriver replacement/removal | Only after owner enumeration and proof of every live/retained binding, anchor and unresolved recovery schema dependency; same final registry, no new catalog. |
| Phase-3 grouped qualification | Real end-to-end physical/admission/failure-boundary tests plus strict affected checks; no false parity or publication claim. Remain Phase 3 underway. |
| Phase 4–7 | Follow active plan: Jobs own tree/cursor; final registry composition/dynamic Extensions; compatibility contraction only after drain/restart and final cross-repo qualification. |

Explicitly not green: real Bubblewrap worker-retirement and Windows-native
Process runtime tests; full production source-aware Actor dispatch; generic
provider principal/physical fence; multi-source and graph/process route;
remaining cleanup/physical policy; Vault; complete public RPC surface; full
Phase-3 qualification or publication; Phase 4–7. Broad historical Store/remote
downgrade fixtures are not claimed as new passes. The earlier broad Agent state-shell
regression fails at an unchanged retired legacy Job Event subscription call;
do not weaken intentional cutover to appease it. Fix test/owner only as its own
reviewed slice. Last focused qualification excludes it and zero-test filters.

## 8. Exact implementation chronology and validation

All signed implementation and documentation checkpoints since the published
R6 base are listed below, generated from the actual first-parent Git ancestry
at the signed implementation checkpoint. The subject is not a substitute for
the diff or grouped evidence. For any row:
`git show --stat <sha>`, `git show <sha> -- <owned-path>`, and
`git show <sha>:.agents/docs/CURRENT_HANDOFF.md` recover the exact implementation
and contemporaneous detailed record. No new synthetic tracking ledger is needed.
Historical phase-0 and release chronology remains in §4's prior handoffs.

### Phase 1 (including docs-only refreshes)

```text
97846191f45186977d262e010d540e22283dc916 Start DL-200 Phase 1 store-scope foundation
d9f53db3a96b5c8e7402b8d8b732c06a184b6dec Wire Agent owner seam
77fced26b68924db45d9251111b87725a748f5ed Make Agent recovery exact
b7059fb4c338cf3356bb9cee99f8c0ebcc6ac6ce Mark accepted Agent input
0e82f734919d6de68fa3ddbc1bba7b4d95e1775c Activate Agent owner state
10aa467b9fe73103194640991520a4f1fb8183f1 Add Domain principal groups
bcca6dd9a5aa049b3e1a20d0873ca09e7a70e533 Add group Agent access
706f6f1fd9fc3b9e431ace43014bcf0f09b71c82 Add immutable history owner
a5aa8fdd21d67358da2ab74e6c10ee8a7306a740 Add full Agent access
0db11447cb699fae64d5b1396f11dad353b4a83b Fork Agent history
0f3f3ebbfaa12e66c1aeb0f5c2a27faf53e84e2e Wire Agent from refs
05d8a74d8e22b243ae90082be99157607d9508b0 Refresh DL-200 handoff
15968db7d98cfb16553cc1bb19f48af532bc401b Add host Agent state shell
6bc8f6ffd2d68e71281811fb9825b6ad8e888efa Preempt active Agent close
e0a9562ca3e515cb5c3344e60673131501df464b Retire Agent wake recipients
91e79c44e41aca0001c58b493187fd91d713fc4e Close Agent finite work
c9501e5e42f39545414d14ccc0925040318de42b Compose stable Agent close
6bc8c979bc8a34f55d7229160d9d9bf0a26b4322 Retain root Agent Job runtime
0af6a63aad928e293c9005305eb27162e8746786 Add root Agent close seams
086ed74429073b976ac0fedc4bc61c112939676c Compose root Agent close
ed0332f432b40605b2b1814b2a9ac4c5d7e4ee61 Activate host-created Agents
53f1201e65c6b8bba6c4008af4e525cd8dd8b9f0 Publish initial Agent task
0c695a6f71a5fd0c626c13a804c263682a47bb6d Expose host Agent create
78339410710472ec51d5ff46a5c9e586b67ff30d Own logical Sandboxes
9633dfb2700911a40bc657f3c69980d01381e63e Expose Sandbox reads and close
1d4988bc0726eb9626365c46018253a233b6d332 Route non-current Agent close
426d1f894a097062c48a1b6cf8fb02bc1a31195f Authorize cross-Project Agent create
c6718fc20198550b3065f4544919a4f741ee4b56 Own staged Agent input plans
e546a75016d78b3bff78bb6e7e9833dd8f6410fc Send staged Agent inputs atomically
503706d1d3ffd3bbe5a3d4d87a943d75b567b0f0 Expose staged Agent input host API
838ea82b46417984f7e356a66e8aec1ec6eaf646 Project immutable Turn provenance
90a1b863827b1f631db8aa73fb3c75c0d9b5e889 Project finite Job provenance
03d1224ef7b2054b3856fa559dcc23ac666f341f Rebuild Agent exposure from owner state
53cb41f22c72dc47226a4a186bf627b842a56e7c Refresh Phase 1 comprehensive handoff
22df683a16841cbba6564be3672538bc909c72a5 Clarify handoff implementation boundary
7f4724f942e4d90a6eaaca77af6c528b37a2262d Expose Agent state RPCs
```

### Phase 2

```text
14b53872236d2af262d24a2ad9529c4b652c2756 Start Event projection foundation
7e54a9d248a3b9536e95a62ba1fa1886673fe346 Add private Event scan
9958b7e80b8aca8c25218b1b96893048f51e29b0 Own source Event revisions
57d3940d1c50d912e6a39058faee2fa7022f6485 Add exact Agent Event reads
b2a63f2e99af44509983052658a632ec28ccdd8c Scope Event cursor cleanup
639cebdb86ba729ff992bac4767efab0495f7a54 Wake exact Agent Event reads
1c777dcf95b7269016d77d53ce884eb8ed187911 Broaden Event reads to exact Projects
06a2164551c27531174c0a6fd4be344fb3dcaf66 Own durable Event subscriptions
016b806ff28cb6dba703d7ad30439ba026c791e2 Deliver typed Event inputs
bd18dcd2dbbcd77641d9da8a437710754fbab38a Propagate Event disclosure and causality
5a0473203e21d91915758f8c33613617aa3ed281 Publish typed custom Events
a0549147f2c2bee2791dcca5760b3057deea21d3 Expose model Event tools
53490a6d19d542c98416a6f3a67f085b4ffc73ed Own durable EventHandler receipts
8fc29093037a8e6540cdfd0da4294a4db58512d2 Prove same-store Event universes
e0c6bb3184bf87fc1bb2186ddc7f5ffb97a39c96 Cut over Event delivery ownership
```

### Phase 3 (including earlier docs-only refresh)

```text
184ea6b05643c49d34a7f3bfdbfed86f26753162 Establish SourceDriver recovery contracts
8aa536f5a093f5b49d56c78efc76eeeb928e2f9b Refresh Phase 3 comprehensive handoff
f4e0491602570039f09a2abd331fd38063523c22 Persist Sandbox source bindings
2ef25d38a94281ab86dcc1e67ba271964f0fe35e Add Git SourceDriver clean capture
fa251cca29611bece3669aeaaa2096f25126cb5e Capture exact dirty Git source trees
2f2897179134b14ffdcd10e7c8f49c7bc3307217 Restore exact Git source trees
150734408deed5ad930d43c7be9fd67459a4838d Persist immutable checkpoint source sets
e4052106a66615df01b71713aee8c851c97eb0fe Persist prepared source recovery custody
a720c066c1bd9363c610979837530defe228aac1 Fence complete source mutation sets
82015a447c60959057843db64e69409b56caca19 Bind checkpoint capture to prepared custody
21442aa443fc49d0cabbcf2d28bf10722baa84de Recover exact prepared source captures
b3e689aa06af4464277a4a08a57ef3bbc465dab7 Publish prepared checkpoints under live source fences
c0aa38e7f1c656fb4632e3a32a51eb7b9294f6af Prepare exact non-destructive checkpoint restores
f29fa6ca4ae5ef25daf76e7226767608d0783e54 Publish fresh restored Agents with held source admission
718f7982fcb54447774741ca8917718d78cfb36f Bind independent Sandbox grants to exact source authority
c6d6740e040c988dd9a2b91d321d67992605885b Fence source effects through typed Sandbox realization
81c5256f099bd839540e1d3dab8fcf243b76955f Compose source authority with the original Process launch
ad19dcd9f6db5e580d738b3de0c0e71b7f7744c2 Link source effect custody to original tool journals
eb1e6ffbf1d6c1c86273da037f1ca9ad0784b4f2 Route native source tools through pinned capabilities and original receipts
f9b97619634b1c15932584643cb52877d38e2264 Verify Git materializations and configure root-free native source tools
5fb362702bf93eeccee8308c0e57fdc45960bda4 Compose live native source approval into Actor host activation
6d93329733fa087a25e0e80c671dfed9b233240c Publish quiescent source binding changes under complete layout fences
cfa0517a09f836b309181e9bb1144330a31bbe60 Retire checkpoints and reconcile exact retained anchors
e3a63482607d40f048a1e298746e3970ccd6b289 Close source-bearing Sandboxes without releasing retained layouts
3bbe8c5a59e4dc9a75c04328f287121c400f88c9 Reconcile closed Sandbox materializations and release retained layouts
5f77c6347d4d8d015e8f81b3277d858416abaae3 Release exact managed Git restores using original applied provenance
cce7363f90059c2fa079c4c9301e22c87ad70f52 Restore exact checkpoint history into fresh destination baselines
75bda2087cab5890e90a2a6f527ade01fde36cdc Reconcile unpublished checkpoint capture and restore candidates
0dab4cc914de0374ac83a30e774b367e7854aa87 Share initial history ACL construction with checkpoint restoration
85028a4ed680760c9dbf3e198a1ec18d1c006421 Share initial task custody with captured-source checkpoint restore
0bf9bf6cbdce3d0929885198f93528572b85cf2b Restore baseline initial tasks through existing encrypted custody
```

### Latest recorded test evidence

At 0bf9bf6 (the recorded implementation validation, not a rerun during this
docs-only handoff):

| Gate | Result |
|---|---|
| Checkpoint/capture/restore | 4/4 |
| Construction receipt | 1/1 |
| Task-bearing ordinary Agent create | 1/1 |
| Sandbox owner | 4/4 |
| SecretStore | 6/6 |
| Encrypted queue restart / missing master | 2/2 |
| Strict Store all-target Clippy | green |
| Workspace all-target check | green |
| Formatting / scoped diff-check | green |

18 focused test executions at the latest cut. Prior real Git repository suite
32/32; Host 15/15 with Git and 15/15 without; native adapter 99/99; real
Process source integration and exact journal-linked custody, read/write,
symlink and Git admin exclusion, held gate and restart all qualified in their
original grouped regressions. Minimal and durable-only features were checked
at the relevant prior cuts; minimal dependency tree excluded SQLite/Git, with
two existing CLI dead-code warnings. Phase-0 published baseline: ordinary
all-target/all-feature check, shell 86/86, Graph 13/13, actual queued
Spawn-Lane cancellation/timeout and spawn refusal, strict Clippy/fmt/diff.
For exact historical results and limitations see §10 and prior handoffs.

For the next implementation, run affected real tests rather than repeatedly
requalifying everything. Example existing gates (choose the appropriate ones):

```sh
export AW_GIT="$(command -v git)" AW_BASH="$(command -v bash)"
cargo test -p aw-store --lib checkpoint_owner:: --locked
cargo test -p aw-store --lib sandbox_owner:: --locked
cargo test -p aw-store --lib construction_receipt::tests --locked
cargo test -p aw-store --lib source_barrier::tests --locked
cargo test -p aw-store --lib history_owner:: --locked
cargo test -p aw-store --lib task_bearing_create_publishes_pending_input_atomically_and_replays_exactly --locked
cargo test -p aw-store --test store aw_p6_9b2_queue_encrypted_pending_restarts_with_claim_barrier_and_writer_cas --locked
cargo test -p aw-git --features source-driver --test repository --locked
cargo test -p aw-host --features git-source-effects --lib source_process::tests --locked
cargo clippy -p aw-store --all-targets --locked -- -D warnings
cargo check --workspace --all-targets --locked
cargo fmt --all -- --check
git diff --check -- <exact owned paths>
```

Check actual test count and terminal result; zero-test filters are not evidence.
For changed shared/core/host/adapter feature boundaries run relevant affected
Clippy/all-target checks, feature variants and minimal dependency exclusion.
Workspace gates may compile concurrent work but cannot certify its correctness
or transfer its ownership. Report a blocker precisely, not a guessed green.
Do not run a production cleanup on user data merely to qualify a driver.

## 9. Safe checkpoint workflow and hand-back

1. Revalidate symbolic HEAD/main and exact last signed parent. Verify signature.
2. Inspect status/diff/staged **only** exact intended owner paths. Preserve
   concurrent work; never inspect reserved UX subtree/staging. No extra worktree.
3. Read relevant sealed sections and live owners; select one coherent slice.
4. Implement smallest actual shared primitive with real consumers. Keep
   held admission and unknown/replay semantics; do not promote incomplete routes.
5. Execute focused real regression(s), strict relevant check/Clippy/fmt and scoped
   diff. Capture honest counts and caveats.
6. Update `CURRENT_HANDOFF.md` + active plan TODO factual delta, not the shared
   global queue unless explicitly delegated. Keep earlier records as provenance.
7. Construct private index from expected parent; stage **only exact owned paths**.
   Inspect staged names/stat and diff. Verify normal index has no staged changes
   for those paths relative to parent. Commit tree with signing; check the exact
   key fingerprint. Recheck private index vs live owned files, branch, parent,
   then CAS `git update-ref refs/heads/main "$NEW" "$EXPECTED"`. Normalize only
   exact owned index paths against the new HEAD. Verify signature, HEAD and owned
   cleanliness. Never use an all-tree reset, checkout, stash or add.
8. Stop/pause and hand back commit, parent, actual tests and next boundary.
   Do not push/tag/release/deploy without separate request.

Do not make a new commit just to put that same commit's hash in a document.
The handoff beneath the new docs commit is an exact implementation checkpoint,
not the docs commit itself. If interrupted, first inspect the live repository,
recover prior owned jobs and verify actual outcomes. Do not rerun a durable
job merely because the response window ended. Do not claim completion based
only on a comment or intermediate output.

## 10. Detailed Phase-3 chronological implementation record

The following is the complete Phase-3 per-cut owner/decision/evidence record
from the signed CURRENT_HANDOFF at 0bf9bf6. It is kept as a fixed dated snapshot,
not a new normative spec. **Read in chronological order.** Each cut's “pending”
and “not yet” statements describe its then-current boundary; later entries and
§1/§6/§7 supersede them. This preserves the detailed why/how, regression results
and evolution rather than silently discarding interim decisions. The earlier
Phase-0/1/2 per-owner record remains at the same Git revision of CURRENT_HANDOFF,
and in the earlier comprehensive handoffs in §4. The sealed README/SPEC/
IMPLEMENTATION/TODO remain separate live documents.

**Phase 3 first prerequisite is now implemented locally.** `aw-agent-core::source_driver` owns the
first final SourceDriver registry leaf from day one and reuses the same private staged-registry publication
core as EventType/EventHandler. Durable `DriverKey` is only stable built-in/Extension namespace + local
driver ID; it contains no runtime generation or artifact version. `DriverSchemaPin` stores the artifact
kind (`binding|anchor|recovery`) and that artifact's adjacent schema version separately. Exact native
`SourceDriverHandle<T>` validity pins one registration revision/type/schema snapshot, while durable
`SourceDriverRegistryReader::resolve` looks up only stable DriverKey + artifact schema so unrelated leaf
publications cannot stale persisted source state.

The leaf is intentionally add-only in this first cut. SourceDriver removal/replacement is **not** exposed
until Store owns the complete set of live SourceBindings, live Checkpoint SourceAnchors and unresolved
Prepared/Unknown RecoveryDescriptors. That prevents an API caller from supplying an incomplete dependency
list and bypassing the durable pinning invariant. Later compatible replacement will mutate this exact leaf,
not migrate to another registry.

The shared source/recovery vocabulary also lands here without creating a recovery coordinator:
- `SourceName` enforces v1 `[a-z][a-z0-9_-]{0,63}`;
- `SourceAccess = read_only|read_write` and `CheckpointPolicy = required|ephemeral`;
- `SourceSetLimit` defaults to 128 and host configuration may only lower that effective limit;
- a separate absolute validator enforces the 1024-source corruption/safety ceiling;
- `RecoveryDescriptor` stores stable DriverKey + recovery schema + prepare ID + immutable prepared-source
  identity + at most 32 typed recovery fields (`id|digest|version|reference|opaque_handle`);
- `PreparedSourceOperation` shares that exact descriptor for Capture and Restore;
- `RecoveryInspection<T>` is exactly `NotStarted | Completed { result, evidence } | Unknown`;
- `ExternalEvidence` is either immutable/non-revocable identity evidence or a durable **opaque** provider
  fence handle. A held-fence handle is explicitly documented as not being publication authority by itself;
  the later lower constructor must still hold the live provider fence valid through the complete AW commit.
There is no generic credential/value blob, bearer-token field, driver-key database or second driver catalog.

All serde-backed durable values expose explicit `validate()` paths so Store-open/recovery can fail closed on
decoded state rather than trusting constructors. SourceDriver schema support is declared separately for
binding/anchor/recovery as `compatible_from..=current`; durable resolution rejects missing/incompatible
artifact schemas. The focused SourceDriver regression **1/1** proves staged publication, exact typed
implementation retrieval, stable durable schema resolution across an unrelated leaf publication,
unsupported recovery-schema rejection, the 128 effective / 1024 absolute SourceSet bounds, and the
opaque-fence/Completed recovery vocabulary. Repository fmt, full workspace all-target check, strict `aw-agent-core` Clippy and scoped diff-check are green.

Schema **v55** extends the existing logical Sandbox owner with
`sandbox_source_bindings_current`, keyed by exact Sandbox + validated SourceName. Each row retains
stable DriverKey + binding_schema, non-secret source identity/materialization locator, access,
checkpoint policy and binding revision. Existing Sandboxes migrate with an empty SourceSet, not an
inferred repository. The shared core `SourceBinding` validates names, driver/schema and bounded
opaque identities; checkpoint policy defaults to required. Project-authorized Sandbox snapshots expose
only bounded binding metadata, not protected source identities/locators.

The trusted Rust-only `sandbox_create_fresh_with_sources` uses the existing Sandbox constructor and
one transaction for the complete pre-existing materialized SourceSet. It checks the Store's effective
host bound, duplicate names, full binding shape and exact installed schemas before publication. It
does not call a driver, grant source authority or serve as an effectful capture/restore constructor.
The default bound is 128 and the host may only lower it through Store configuration; existing rows
remain readable if policy is lowered. Decoding rejects an overfilled set above the absolute 1024
ceiling. No general source-set replacement/removal or host RPC is promoted yet.

Exclusive Store open, trusted-local direct startup (currently empty installed registry), and staged
SQLite backup restore revalidate live bindings through the exact SourceDriver registry before
serving. Per-binding `compatible|recovery_required` is a durable diagnostic, not authority.
Missing/incompatible pins retain the exact binding, identity, schema and revisions; reinstalling
compatible definitions revalidates without rewriting binding truth. Only changed diagnostics are
written. No fallback driver, replay, cleanup or registry removal is introduced. Until source
realization and lifecycle support land, both explicit and legacy layout-only Agent selection refuse
source-bearing Sandboxes (no implicit empty replacement); source-bearing close also refuses.
Unexpected persisted Agent/source-bearing Sandbox associations fail closed on open. Existing Agent
Sandbox bindings remain immutable.

The grouped real Sandbox-owner regression **2/2** covers v40 layout/close compatibility and v54→v55
disk migration, atomic named-set admission, protected metadata, missing/incompatible restart,
compatible reinstall with different runtime registration revisions, direct-open degradation, staged
backup restore revalidation, lowered host limit, absolute corruption ceiling and no source-bearing
Agent admission/close. The existing SourceDriver leaf regression **1/1** remains green.
Full workspace all-target check, strict `aw-agent-core + aw-store` all-target Clippy, repository fmt
and scoped diff-check are green.

The first concrete built-in Git SourceDriver is available through `aw-git/source-driver`. The default
Git review library remains Agent-independent; trusted hosts explicitly register the concrete
`GitSourceDriver` in the existing add-only leaf with an absolute trusted Git executable. Stable key
`builtin/git` retains binding schema 1, anchor schemas 1–2 and recovery schemas 1–3
(capture 1–2, restore 3). No new registry or ambient driver installation. The driver's SourceRef identifies the local common/object directories; its materialization
pins the exact worktree/admin directory plus canonical path. The shared platform DirectoryIdentity
exposes its existing local filesystem identity, not a cross-machine resource ID or authority.
Unexpected directory replacement, alternate object databases, executable filters and partial clone
are refused. No network, ambient Git config, hooks or replacement objects enter capture.

Read-only capture planning emits the shared `PreparedSourceOperation` with a bounded canonical
recovery descriptor, candidate checkpoint/source identity, original binding and exact base commit.
Schema 2 additionally binds a Git-normalized flat tree fingerprint and real index/untracked observation.
The host must durably retain the descriptor and hold its live source barrier before execute.

The same bounded tree observation serves planning, capture and revalidation. It uses committed HEAD
plus tracked index paths, applies final working-tree contents/deletions and includes nonignored
untracked regular files. Tracked symlinks remain tree entries; untracked symlinks/reparse paths are
not traversed. Executable conversion filters, partial clones, hidden/conflicted index entries and
untracked nested repositories fail closed. Git performs normalized blob hashing in bounded batches.
Initialized submodules must themselves be clean and contribute only their exact HEAD gitlink;
uninitialized empty submodules preserve the indexed gitlink. Missing paths are deletions. No nested
object retention or fetch is implied. Unsupported native symlink representations fail closed.

Execute revalidates the Prepared observation before effects. A final tree equal to HEAD retains the
base commit. Dirty capture builds two independent private indexes seeded from HEAD, writes their exact
observed trees, and requires matching tree/status fingerprints. A deterministic synthetic commit binds
the exact prepared identity, captured tree and original base parent. The original HEAD, branch and
real index remain untouched. Both cases create one private
`refs/aw/checkpoints/<checkpoint>/<source>` ref by absent-old CAS, never overwriting an existing ref.
The typed GitSourceAnchor carries exact base/captured commit identity and source/access/checkpoint
metadata. Read-only retention validation checks snapshot-local object closure, including the dirty
base, without walking unrelated history.

Inspection reopens only the exact prepared binding and validates the operation's private ref,
commit closure, and (for schema 2) captured tree and synthetic preparation identity. Absent,
changed or damaged retention is Unknown, never automatic replay or cleanup. An interrupted capture
may leave unreachable objects or operation-private index residue for future reconciliation.
Completed returns immutable Git content evidence; the ref is not a live fence or authority. The
future Store constructor still owns durable retention and must hold AW source/cleanup barriers
through publication, including final source/HEAD revalidation. Schema-1 clean-only recovery remains
compatible and preserves its original admission and anchor semantics. No public checkpoint,
retirement or effectful Store constructor is introduced here.

Isolated Git restore now uses the same Prepared descriptor construction and existing SourceDriver.
Read-only planning binds the exact retained anchor, origin checkout, host-selected absent destination
and private canonical parent identity. A fresh linked worktree is registered at the original base HEAD,
with an operation-specific Git lock reason. Its real index is seeded from the base; dirty content is
materialized from the captured tree through a private index, without checkout of the synthetic commit.
Clean restore has no private-index overhead. Source HEAD, branch, index and contents are unchanged.
No ambient credentials, hooks, network, submodule initialization or fetch are invoked by the driver.

Restore reuses the bounded tree observation, checking the actual base index independently from the
captured tracked-path context. Captured symlinks and ignored-but-tracked additions are restored even
when untracked relative to the base. Unknown extra files are refused. Unchanged gitlinks remain empty
uninitialized directories; deleted gitlinks are absent. New/changed gitlinks and directory replacements
of base gitlinks require separate materialization and fail closed before the effect. Destination private
custody must be platform-supported. The driver does not grant access to shared Git metadata.

Read-only recovery checks exact registered detached base, operation lock reason, retained anchor,
destination identity, base index and final contents. Missing, partial or changed candidates remain
Unknown, never blindly retried or deleted. Completed mutable materialization returns a HeldFence
identity for the required live AW local source/destination barrier; neither that handle nor Git's
worktree lock supplies authority. The Store publication owner must acquire/retain the real barrier
through commit. Candidate retirement, cleanup, grant realization and the durable Store constructor
remain pending; no automatic destructive cleanup is introduced.

The grouped real Git regressions and full `aw-git --features source-driver` suite **32/32** prove
clean/dirty capture and retention/restart, schema-1 compatibility, effect-free planning, exact
tracked/untracked contents and gitlinks, clean/dirty restore at original base after source HEAD
moves, preserved indexes, explicit private candidate identity, partial checkout and changed
content/index/lock/parent refusal. Strict Git feature Clippy, default-feature-independent Git
all-target check, full workspace all-target check, repository fmt and scoped diff-check are green
on L5. No Windows-native runtime qualification is claimed.

Schema **v56** adds `checkpoints` and `checkpoint_sources`, starting empty on migration.
The core SourceAnchor carries DriverKey + anchor_schema, immutable version/retention identities,
name and captured access/checkpoint policy, and bounded typed non-secret identity fields. The
same identity-field validator serves recovery descriptors (existing RecoveryField aliases/wire
unchanged). SourceSet validates coverage, unique names and the shared effective/absolute source
bounds. Git converts its typed clean/dirty anchors to/from this common envelope without losing
base/captured OIDs or private retention identity. Store has no Git dependency.

Checkpoint rows retain exact sealed HistoryBase/head and immutable source Agent identity,
coverage, creator/note/time and indexed authenticated request provenance. A live checkpoint's
generated FK retains the existing HistoryBase; retirement can preserve original identity and
replay without that live root. Source rows retain only immutable driver anchor data and captured
layout metadata. SQL protects immutable anchors and live source retention; compatibility diagnostics
can change. The lower transactional publication helper admits the complete set and resolves exact
anchor schemas; it is private, with source-bearing use limited to storage qualification until the
effect-fenced constructor lands. No production caller can inject arbitrary source anchors.

Trusted Rust-only history-only construction uses the existing latest-settled-history seal and
current Agent view authority; the trusted host must admit checkpoint-create capability and pass its
stable scoped envelope request identity. Indexed replay lookup precedes target resolution; exact
replay preserves the original checkpoint even after newer history or retirement, changed request
semantics refuse, and lost view prevents payload disclosure. No Agent rev/ACL/history-pointer
mutation and no external effects. Metadata get rechecks current view and exposes source name,
driver/schema, captured maxima and schema diagnostic only; protected source/anchor/retention
identities remain withheld. No public checkpoint RPC or source-bearing create/restore is promoted.

Existing owner/direct startup and staged backup restore now share one read-side registry snapshot
and one compatibility transaction for bindings and retained checkpoint anchors. Missing or
incompatible artifact schemas retain exact durable truth and mark recovery_required; compatible
reinstallation clears diagnostics without rewriting identity or advancing Agent/Sandbox revs.
Retired anchor rows awaiting cleanup remain dependencies until removed. This is schema
compatibility, not evidence of actual retained objects. The registry remains add-only; v57 adds unresolved recovery descriptor custody/pinning to
the existing construction operation receipt, not a speculative replay catalog.

The grouped real disk checkpoint-owner regression **1/1** covers v55→v56 migration, settled
history during an active turn, replay/current ACL, revision neutrality, immutable source storage,
bounded complete-set admission, metadata redaction, incompatible restart, compatible reinstall,
staged restore/direct open and hard corruption ceiling. Existing Sandbox owner **2/2** and
SourceDriver leaf **1/1** remain green. Git feature suite **32/32** includes real typed anchor
roundtrip. Strict affected-crate all-target Clippy with the Git source-driver feature, full workspace
all-target check, repository fmt and scoped diff-check are green.

Schema **v57** extends the existing immutable construction audit receipt family, not a new
operation/replay table. One exact replay index spans the historical task-only action and the
current typed construction receipt. Existing task-bearing Agent creation uses the shared
claim/lookup/CAS transition functions; historical receipts decode without rewriting audit
facts and recover their original stable creator from the matching security audit fact.
The source-preparation index uniquely reserves the operation-owned prepare identity.
Current receipts carry semantic operation, authenticated scope/creator, origin Agent, original
request/fingerprint, typed candidate IDs, optional initial-task custody and an optional complete
PreparedSourceSet. Ordinary task-only creation preserves its established scope and fingerprint
semantics; full contextual replay/preflight ordering remains part of the shared constructor cut.

The core PreparedSourceSet validates the existing effective/absolute source bounds, unique names
and effect identities, common operation and owner-minted preparation identity; descriptors retain
exact DriverKey/recovery schema and bounded non-secret fields. Capture claims identify their
candidate checkpoint; restore claims identify the fresh Agent/Sandbox candidate. Complete schema
admission and claim happen transactionally before any future external effect. Exact request or
prepare-ID reuse cannot create another claim. History-only checkpoint creation and task-free Agent
creation cannot consume a previously Prepared effectful request under changed semantics.
Full descriptor data stays in the owner-private audit receipt; security audit metadata receives
only operation/state, avoiding disclosure and duplicate large descriptor payloads.

Owner/direct startup and staged backup restore now also stream the indexed initial source
claims, resolve their exact current receipts, and validate unresolved per-source recovery pins
against the same registry snapshot as bindings/anchors. The current receipt must preserve original
candidate and descriptor identity. Compatibility is a derived diagnostic, not a rewrite of the
immutable receipt or authority to execute. Missing/incompatible definitions preserve Prepared/
Unknown custody; compatible reinstall resolves the same durable schema regardless of runtime
registration order. A lower host limit cannot erase old dependencies. No driver is invoked on open.
Source-bearing claims cannot become Applied or Rejected through the task-only transition path;
that requires future proven publication/cleanup. No source-bearing production constructor or
candidate resource is exposed by this slice.

The grouped construction-owner disk regression **1/1** covers v56→v57 migration and legacy
receipt decoding, exact named capture/restore custody, duplicate/mismatched/unsupported/over-limit
claims, shared checkpoint replay namespace, no public candidates, CAS Unknown, schema degradation/
reinstall and staged backup restore. The actual task-bearing Agent create/replay regression **1/1**
passes through the new receipt owner. Checkpoint owner **1/1**, Sandbox owner **2/2**, SourceDriver
leaf **1/1**, strict affected-crate all-target Clippy, workspace all-target check, repository fmt and
scoped diff-check are green. The broader Agent-host test group is **1/2**: its unchanged
legacy nonempty Job Event subscription fixture is denied by the v54 cutover; this slice does not
change that Event owner or claim the broader group green.

The Store-local source mutation barrier reuses the existing local coordination execution-lease
owner. The validated SourceMutationSet derives one stable scope per DriverKey + SourceRef,
deduplicating names/worktrees/artifact schemas of the same source without weakening the original
named-set bound. The same scope derivation accepts binding or anchor sets. The exclusive persistent
Store owner acquires the complete set in one transaction, under its operation-owned preparation ID.
Source claims last for the owner incarnation (no timed renewal); acquisition refuses both active
claims and hazards. Unrelated source sets may proceed. No new lease table, registry or migration.
The ordinary timed coordination admission and source-set acquisition share the same lower lease
mechanism and source acquisition reaps/checks the owner once for the whole set.

The non-cloneable host-only barrier checks exact StoreScope/process/lease generation and preparation
identity. Transactional settlement can release all scopes in the final AW publication commit;
rollback retains all claims. Explicit Unknown preserves hazards; dropping an unsettled token
never releases it, and an owner restart converts abandoned active claims into hazards. The raw
generic write/execution lease and hazard-resolution APIs cannot acquire, renew, settle or release
reserved source scopes. The existing fresh Sandbox source metadata publication checks the same
scopes inside its transaction. No driver effects, source grants or model/RPC admission are added.

This is a Store-local coordination primitive, not proof that arbitrary filesystem writers or
another Store are fenced. Source-bearing Agent execution stays closed until its Process/effect
admission and structural Sandbox mutation share this barrier; cross-Store/external authority
needs the reviewed driver/provider fence. An unresolved barrier is intentionally not recoverable
by a generic lease release; the recovery owner must inspect the exact operation first.

Grouped source-barrier disk regression **1/1**, affected coordination regressions **3/3**,
Sandbox owner **2/2**, construction receipt **1/1**, strict Store all-target Clippy, full workspace
all-target check, repository fmt and scoped diff-check are green. Unchanged Git/Phase-2 suites
were not repeated.

Trusted checkpoint capture preparation now reads the exact current named Sandbox bindings, selects
all required or an explicit nonempty named set, resolves its binding schemas, checks current Agent
view and open same-Project Agent/Sandbox placement, and fences the settled HistoryBase plus legacy
active-run/hazard boundaries. The lower source-bearing seam refuses empty auto sets before planning;
the eventual public auto wrapper must use history-only construction for that case. The independently
authorized host supplies a read-only planner outside SQLite, receiving the bounded canonical binding
revisions and owner-minted checkpoint ID. Before any effect, one transaction repeats the exact
observation, resolves recovery schemas, acquires the complete mutation scopes and commits the
existing Prepared receipt. No candidate checkpoint is published. Replay checks precede target
resolution/planning; partial, mismatched and stale source plans refuse.

The receipt now retains exact selected binding revisions, Sandbox rev, history base, current
Agent access generation, selection and note. This additive optional capture boundary is decoded
alongside prior v57 receipts and checked for immutable equality and binding/recovery schema
dependencies during startup inspection. The live non-cloneable PreparedCheckpointCapture exposes
the exact trusted driver plans and revalidates receipt, leases, history, placement and ACL together.
Its Unknown transition and whole-set hazard settlement commit atomically even after view
revocation. No generic release or source-bearing terminal publication path is opened. Actual
source capability and external fence admission remain the trusted host/driver responsibility;
source-bearing Agent realization and writer integration are still closed.

The grouped capture-preparation disk regression **1/1** exercises a real active/settled Turn,
required and named selection, stale binding/history, missing/partial selection, replay and
revocation, durable exact receipt + active barrier and atomic Unknown hazards. Source-bearing
binding rows in this regression are a test-only fixture, not a production realization path.
Checkpoint owner **1/1**, source barrier **1/1**, construction receipt **1/1**, Sandbox owner
**2/2**, task-bearing Agent constructor **1/1**, strict Store all-target Clippy, full workspace
all-target check, repository fmt and scoped diff-check are green.

The same capture token now supports read-only inspection for fresh and recovered operations.
Trusted recovery locates the original unresolved receipt by authenticated caller/request identity,
checks current Agent view and exact compatible binding/recovery schemas, and atomically reclaims
only that receipt's complete source hazard set. Existing execution lease generations advance
under the exclusive owner; live, missing, released, foreign or prior-restore-generation claims
cannot be substituted. Recovery does not re-admit the old set under a lowered new-operation
limit. There is no new operation claim, replanning, automatic execution or generic hazard release.

The trusted inspector receives the complete original binding/descriptor set and compatible registry
reader outside SQLite under the live barrier. Only a complete ordered set of matching completed
anchors and validated evidence returns InspectedCheckpointCapture. Current receipt, disclosure
and barrier are rechecked after inspection. Partial, NotStarted, Unknown, mismatched or failed
inspection preserves whole-set Unknown custody. The non-cloneable inspected token exposes
anchors and evidence identities, not execution or publication authority. A stale current source/
history boundary may be inspected with independent source authority, but fails Store-side
revalidation; a saved external fence handle is never a live provider fence.

The grouped capture-owner regressions **2/2** exercise exact original descriptor inspection
after a real disk owner restart, missing drivers, live barrier exclusion, lost/partial fixture
effects, recovery without a second Prepared claim, stale placement, wrong anchor and NotStarted
refusal, and current view revocation. Test-only source-bearing Agent association is removed
before restart and reinstated only for boundary qualification; the production realization gate
is unchanged. No public checkpoint publication or automatic cleanup is introduced.
Source barrier **1/1**, construction receipt **1/1**, checkpoint owner **1/1**, affected
coordination **1/1**, strict Store all-target Clippy, full workspace all-target check,
repository fmt and scoped diff-check are green.

Inspected captures now publish through one shared source-bearing construction boundary.
Core SourcePublicationProof distinguishes revalidated immutable/non-revocable evidence from
an actual provider-owned SourcePublicationFence guard; the latter must guarantee the exact
evidence and authority until dropped. Immutable content alone is not non-revocable authority,
and a saved HeldFence handle cannot enter the immutable branch. The trusted verifier receives
the exact original named descriptors, bindings, anchors and inspected evidence outside SQLite.
It must revalidate actual driver results, source/capture authority and fingerprints. The proof
set is complete and identity-matched; guards are owned through commit or rollback (static
generic dispatch, no new registry). This seam relies on reviewed host/driver implementations,
not a claim that all concrete provider fences are implemented.

The private shared construction receipt publication function checks the original unresolved
receipt and exact preparation-owned barrier, invokes the existing resource owner inside one
immediate transaction, verifies the candidate result, records Applied and settles the whole
barrier atomically. The checkpoint owner revalidates current view/access generation, exact
Sandbox/source revisions, settled HistoryBase, quiescence and schemas in that transaction.
Checkpoint row, all anchors, local checkpoint.created commit-log provenance, Applied receipt
and released source claims have one commit boundary. Failed verification or rollback preserves
Unknown custody and source hazards; no partial public resource. The guards are dropped only
after the transaction and Store connection guard finish. No Agent revision fanout; optional
public Agent changed Events remain separate.

History-only and source-bearing replay share the indexed checkpoint result lookup and original
semantic fingerprint; the trusted source-bearing replay method returns the same disclosed
published snapshot without planning/inspection. Unresolved claims remain non-retryable and
changed semantics refuse. Source-bearing Agent realization and public host RPC remain closed.

Grouped capture-owner regressions **2/2** now also cover final-transaction view revocation
during external verification, saved-fence-handle refusal, actual fixture-provider read guard
held through commit/rollback, rollback after partial anchor insertion, exact Applied replay
and released complete-set leases. Checkpoint owner **1/1**, construction receipt **1/1**,
source barrier **1/1**, task-bearing Agent create **1/1**, SourceDriver **1/1**, strict
Core/Store all-target Clippy, workspace all-target check, fmt and scoped diff-check are green.

Restore now has a trusted lower preparation/inspection path for source-bearing checkpoints.
It resolves the original live checkpoint's exact retained HistoryBase (never the source
Agent's newer head), uses the existing Derive/Share/HistoryGrant audience checks and
destination Project writer authority, and requires independent host source/materialization
authorization. Private and current inherited access use the ordinary history-fork rules;
cross-Domain inherited ACLs fail closed. Custom initial ACLs, task custody and history-only
baseline restoration remain pending. The existing exact-history constructor accepts the
retention owner's resolved source; there is no restore-specific Agent constructor.

The bounded immutable checkpoint SourceSet is referenced by ID and set digest, not copied into
a second source owner. The same receipt retains the original request fingerprint, checkpoint
history and coverage, destination Project/writer epoch, title/access, host binding digest,
fresh Agent/root/branch/Sandbox IDs and complete per-source restore descriptors. The unresolved
receipt is a retention dependency; retirement/cleanup must preserve the retained anchor rows
until recovery completes. Additive receipt fields decode old v57 facts; startup resolves both
anchor and recovery schemas. No migration.

Read-only planning precedes one transaction rechecking current authority, exact checkpoint,
destination writer/layout and absence of private candidates. Capture and restore reuse
claim_sources_in, require_current_sources_in and mark_sources_unknown in the existing receipt/
coordination owner. No external effect precedes Prepared + whole-set barrier commit. Restart
inspection reclaims only the original hazards and inspects the original descriptors, never
replans. Complete ordered binding results must preserve captured names, driver/source
identities, access maxima and checkpoint policies; current compatible binding schema and
evidence are checked. Partial, mismatched, failed or unknown inspection keeps whole-set
Unknown custody. A retired checkpoint cannot pass final restore revalidation.

The grouped restore disk regression **1/1** covers retained history after source advance,
shared exact-history constructor in a rolled-back transaction, view-only denial, no empty
source fallback, private fresh IDs, replay, changed layout, missing drivers, partial fixture
effects, exact restart recovery and retirement refusal. No actual source-bearing Agent or
Sandbox is published by this slice; the existing source-bearing realization gate remains
closed. Concrete provider effect and fence integration is still separate.
History owner **4/4**, capture owner **2/2**, checkpoint owner **1/1**, construction receipt
**1/1**, source barrier **1/1**, task-bearing Agent create **1/1**, strict Store all-target
Clippy, full workspace all-target check, fmt and scoped diff-check are green.

Restore now consumes the shared source publication boundary. The trusted verifier receives
the exact inspected binding results and original anchors/descriptors, validates destination
isolation and source authority, and returns the complete live publication proof set. Final
Store transaction repeats retained history, current destination/writer and source compatibility
checks. Known existing same-Project layouts refuse fresh restore. Physical isolation, including
path aliases/external writers, still belongs to the reviewed host/driver verifier.

The existing Sandbox owner publishes the private candidate with all restored bindings, and
its constructor-owned FreshSandbox token enters the ordinary resource/Agent/exact-history
constructor. Initial selection never creates an intermediate auto Sandbox, never rebinds an
existing Agent, and checks the fresh same-Project layout. Agent, Sandbox, HistoryGrant,
commit-log provenance, Applied receipt and complete source lease settlement share one commit.
Failed publication rolls back every resource; the original receipt and hazards remain
recoverable. The replay lookup uses the original request fingerprint and current destination
Agent disclosure, even if the checkpoint is subsequently retired.

Source-bearing restored Agents are discoverable/readable with ordinary admission held. This
is not a second publication state or a source grant. The shared source-execution gate denies
legacy Session activation/model context, future Actor admission, queue Claim and attempts to
open admission; auto Sandbox selection does not fall back to a source-less alias of the same
layout. Startup accepts only held quiescent source-bearing associations with exact layout/
Project placement; incompatible driver schemas degrade binding metadata without erasing
the published Agent. No source-bearing runtime effects are enabled, and no prior runtime
authority, credentials, leases, staged inputs or Vault state are copied. The host/driver
realization and SandboxGrant admission cut is still required before these Agents may execute.

Grouped restore **1/1** now also exercises actual atomic publication of the fresh held Agent
and Sandbox, exact retained history, failed HistoryGrant insertion rollback, lease settlement,
current replay after checkpoint retirement, restart with missing/reinstalled driver and
direct/model/queue/open-admission refusal. Capture **2/2**, Checkpoint **1/1**, Sandbox **2/2**,
History **4/4**, receipt/barrier/task-bearing create **1/1 each**, Actor admission **2/2**,
Session view **6/6**, strict Store Clippy, workspace all-target check, fmt and scoped diff-check
are green.

V58 adds agent_sandbox_grants_current, one durable evidence row per exact Agent/Sandbox
binding. No grants are synthesized from existing Agents, source bindings, Project authority
or restored history. The trusted host supplies independently authorized exact Agent,
Sandbox, actor PrincipalId/stable user, policy ref/digest and authority ref/generation,
plus named source ceilings pinned to DriverKey, SourceRef and binding revision. Issuance
checks current Project writer authority, open placement, exact actor Submit/access
generation and all compatible current binding pins. Revoke retains a monotonically
increasing grant revision; stale CAS or source replacement cannot follow a name to
another binding. Unchanged issuance is idempotent. Grant changes do not fan out Agent
or Sandbox revisions and do not claim to revoke an existing Process.

The lower trusted preflight checks current Agent/Project placement, actor/access
generation, grant revision/state and independently supplied live host policy identity.
Exact current binding schemas/identities must match the recorded and host-authorized
pins. Effective source mode intersects binding maximum, persisted grant ceiling,
current host source ceiling and current tool ceiling. Missing names grant no access.
Core SourceAccess::intersect/permits and NamedSourceAccess are reusable; actual
physical mount/network/device policy remains in its existing owner. SourceGrant
preflight is a read-only observation, not a capability or durable authorization to
execute. Direct exact actor ceiling only in this cut (group-backed host policy may
authorize an exact actor; no group-wide grant expansion). Source-bearing Agents
remain held, including after successful grant issuance/preflight. Actual host policy
resolver, typed SandboxDriver realization and live effect/writer participation are
required before enabling source-bearing runtime execution.

Grouped grant regression **1/1** covers populated v57→v58 disk migration, absence
of implicit grants, independent authority on a shared Sandbox, current actor/ACL
generation, source/host/tool maxima, mismatched host subject and stale policy/source
pin refusal, revision-neutral grant issue, revoke/reissue and missing/reinstalled
driver restart. Test-only source insertion into held shared Sandboxes does not
enable production structural mutation. Existing Sandbox owner **2/2**, checkpoint
owner including capture/restore **4/4**, receipt/barrier/SourceDriver **1/1 each**,
strict Core/Store all-target Clippy, workspace all-target check, fmt and scoped
diff-check are green.

The final typed SandboxDriverRegistry leaf now uses the existing StagedRegistry
publication primitive and the shared NativeDefinition implementation custody also
used by SourceDriver. Add-only until live/unresolved execution dependencies can be
checked; typed handles bind the owning registry and exact registration. No physical
Sandbox policy or Process owner is introduced.

Trusted host source-effect handoff now resolves a live policy/tool guard outside
SQLite, observes the exact current grant and requested named modes, pins the
destination writer/layout/Store scope, and prepares an inert driver capability
outside the transaction. The one-shot start repeats that observation under the
existing complete-set mutation-barrier transaction before handing the capability
to the original effect owner. Read and write source exposures both participate.
The host guard is retained through the start future and dropped before the
existing effect owner waits/settles. The caller must own durable operation intent
and its exact effect ID; the handoff adds no independent execution/replay ledger.
No source-bearing Agent runtime route uses this seam yet, and held admission stays
closed. Reviewed concrete host resolver, driver containment and runtime effect
ownership remain required; the frozen physical Sandbox plan is unchanged.

The source-effect lease settles independently of current grant revocation.
NotStarted releases without mutation revision; factual Settled of a write-capable
effect advances Sandbox rev once with local source-change provenance and lease
release in the same transaction. Unknown retains the whole hazard set. Abandoned
claims stay fenced; exact effect-owner recovery and public Sandbox changed Event
projection remain pending. No synthetic Agent rev fanout.

Grouped effect regression **1/1** covers actual fixture file handles and writes,
typed registration/cross-registry refusal, stale grant at start, guard lifetime,
complete read/write exclusion, post-revocation settlement, no-start neutrality
and Unknown across restart. No production containment or Process launch is claimed.
All Sandbox owner tests **4/4**, checkpoint/capture/restore **4/4**, receipt/barrier/
SourceDriver **1/1 each**, strict Core/Store all-target Clippy, workspace all-target
check, fmt and scoped diff-check are green.

The SQLite-only aw-host source-effects feature now consumes the exact prepared source
effect through the original PipeProcess / aw-process owner. ProcessToolAuthority
composes the independent live source-policy guard with the original authenticated
ToolBindingAuthority evidence (exact Process effect, actor and Sandbox scope).
Both read-side preflight and the final source-claim commit use the existing synchronous
ToolBinding publication fence; no synchronous lock crosses await. Revocation after
that admission uses the original cancellation/authority-loss signal through the spawn
control and the original Process owner's wait. No new ToolPolicy grant is minted.

PreparedPipeLaunch carries the trusted driver's already-built command and typed
launch pins, not a layout-only PipeSpec fallback. All PipeSpec/driver commands share
the same existing output preparation and aw-process spawn path. start_with_authority
transfers the source-policy guard together with the capability into the existing
spawn control's lane-owned cancellation probe; caller drop during queued/actual spawn
cannot release those pins before the lane finishes. Ordinary start keeps its guard
in the owning start future for non-detached effects. No new queue, Process lifecycle,
physical Sandbox implementation or per-launch registry is introduced.

PipeLaunchFailure preserves definite NotStarted vs Unknown from the actual shared
runtime, without fabricating ProcessTerminal. The original effect owner receives the
actual PipeProcess plus source custody, or the launch failure plus source custody,
and must record its original effect fact before settling. SourceProcessLease uses
the actual known ProcessTerminal and reviewed driver proof of source quiescence;
a stop operation reaching a live scope or an empty Unix process-group observation
alone cannot prove descendants cannot still mutate sources. Unproven quiescence
preserves Unknown. Output completeness remains independent of physical settlement.
NotStarted releases without Sandbox rev; settled write-capable effects advance it
once through the existing Store settlement transaction. Terminal facts remain with
the original owner, including if settlement fails.

Grouped host regression **1/1** executes a real fixed trusted fixture Process with
current ToolBindingAuthority, driver pins and a real disk Store; revoked preparation,
pre-cancel and OS refusal, actual source mutation, original receipt, post-revocation
settlement and unproven quiescence across restart. It is not a proof of arbitrary
shell containment. No model/RPC launch path has been promoted. Existing physical
Bubblewrap BwrapPlan owns its own child; do not route it here by creating a second
Process owner or bypassing frozen physical work. Held source-bearing Agent admission
still refuses legacy Turn/tool/queue routes. Host **14/14**, existing PipeProcess
**27/27**, Sandbox owner **4/4**, strict Host/Adapter/Store all-target Clippy,
workspace all-target check, minimal build and fmt/diff-check are green. Minimal
normal dependency tree excludes aw-store/rusqlite; minimal check retains the
two existing CLI dead-code warnings (configured_agent_options/local_provider_account).

Direct journaled tool source effects now use SandboxEffectOrigin::Tool {turn_id,call_id}
and the original live ToolBindingSnapshot. Admission resolves the exact active original
ToolStarted on the authoritative Agent/Turn, verifies unique intent/no terminal,
initiating actor (original AuthoritySegment or ordinary run identity), source/effect/site,
current grant and admitted writer epoch. The source claim ID derives from that immutable
run_events sequence. ProcessToolAuthority supplies its exact original tool binding and
cannot use an unlinked Host origin. Generic trusted non-tool host operations retain
their existing seam; the journal-linked ID namespace is excluded from raw barriers.

The existing immutable commit_log owner records one local source custody fact atomically
with complete-set lease acquisition, keyed by the original ToolStarted. It retains
canonical named effective modes and one exact versioned-binding set digest, Sandbox/
Store-generation/driver identity and the original journal pointer. No argument/output
or materialization copy, new table, execution ledger, or runtime authority is introduced.
An already claimed original intent cannot launch again even after source scopes release.
The existing original ToolFinished is the logical receipt authority. Known source release
requires that unique matching terminal, independently proved physical quiescence/no-start,
and the live exact barrier; release provenance references the terminal sequence and commits
with lease release and the existing Sandbox rev advance. A missing/mismatched terminal
leaves Unknown, rather than creating a synthetic receipt.

Trusted terminal-linked recovery checks current View/Project writer, original immutable
claim/terminal and Store generation, exact complete binding set and compatible SourceDriver
schemas. It reclaims only the original hazards, inspects outside SQLite, and settles via
the same source owner. No current tool/SandboxGrant reissue is required for factual
settlement; the trusted inspector separately authorizes inspection and proves quiescence.
Unknown preserves hazards. Released custody returns the prior disposition without
reinspecting, rerunning or advancing Sandbox rev. Incomplete original tool receipts still
need their original journal/reconciliation owner; this does not fabricate a ProcessTerminal,
infer physical quiescence from ToolFinished, or yet support multiple Job nodes per tool.

The existing grouped host regression now uses real TurnJournal begin/Assistant/ToolStarted/
ToolFinished/finish and real fixture Processes. It covers missing intent, duplicate prepared
launch, reserved identity, no-start terminal, missing terminal refusal, actual receipt,
unproven physical settlement, missing driver restart and exact recovered release/replay.
Only the test associates an admitted source-free Turn with a held source-bearing Sandbox.
Host **14/14**, Sandbox owner **4/4**, checkpoint/capture/restore **4/4**, source barrier
and construction receipt **1/1 each**, strict Host/Store all-target Clippy, workspace
all-target check, fmt and scoped diff-check are green.

The direct native filesystem integration now uses PreparedNativeRequest, the original
canonical parser/limits/executor/result projection shared with the ordinary rooted tool
adapter. No second filesystem engine or compatibility fallback on source-aware calls.
NativeFilesystemDriver pins the independently authorized exact source directory via the
existing platform CapabilitySet/PreparedGrant owner, compares the physical root identity,
and restricts the capability to the effective read/write mode and original tool root.
Initial admission is one directory and exactly one source in the Sandbox; multiple or
overlapping source layouts need a reviewed complete physical policy. Source materialization
verification and independent physical authority remain with the trusted host resolver, not
caller-supplied model paths. No arbitrary shell/remote containment promotion.

SourceFilesystemRoute installs explicitly through the original OperationRouterSlot and
Agent ToolResult hook; exact Agent/Session/branch, direct host tool, original actor/source/
effect/site, ToolBindingAuthority and Actor Envelope checks are reused. It refuses Graph/
Process/control/legacy native arguments rather than falling back to the ambient executor.
Installation refuses another tool authority or an occupied router slot. A weak registry
authority reference breaks the route→registry→slot ownership cycle; invocation upgrades
only while the effect is live. Native and Process share SourceToolAuthority. Tool origin
now carries the full original ToolCall so the actual arguments are compared against the
immutable ToolStarted; they are not copied into durable source custody. The source barrier
and current authority commit precede the original blocking worker; its pinned directory
and live source-policy guard stay with the worker even when the awaiter is dropped.

Actual tool return precedes ToolFinished. defer_until_tool_finished persists one compact
positive source quiescence/no-start disposition in the existing local custody facts, keeping
the complete barrier hazardous until the matching original receipt commits. ToolResult
settles that exact claim using the original actor and no new effect authority. Restart
manager recovery consumes recorded proof without another inspection or launch. Reclaim,
release fact, optional Sandbox rev and complete lease release share one transaction for
recorded proof; failure rolls all back. Unknown still requires original inspection.
No pending in-memory completion map, additional table, duplicate tool receipt, or second
execution lifecycle. The callback does not infer quiescence from a logical result.

Grouped native host regression exercises real pinned read/write, symlink escape denial,
changed original arguments refusal, no fallback, original TurnJournal receipt and post-
receipt hook, and missed hook followed by disk restart/release without reinspection.
Dropping the Agent/route releases Store ownership. Held source-bearing Agent admission
is still closed; the route is an explicit reusable host composition, not yet installed
by the production Actor Service. The concrete Git resolver is covered by the next slice below.
Host **15/15**, native adapter **99/99**, router **3/3**, tool binding authority **6/6**,
Sandbox **4/4**, checkpoint/capture/restore **4/4**, source barrier/receipt **1/1 each**.
Strict Core/Host/Adapter/Store all-target Clippy, workspace all-target check, minimal
build and normal dependency SQLite exclusion, fmt and scoped diff-check are green
(minimal retains its two existing CLI dead-code warnings).

GitSourceDriver::verify_materialization now checks an independently approved absolute
checkout path and local directory identity before Git inspection. It reuses the original
binding_repository/source+checkout stamps and returns the Repository's retained identity
pins. A replaced directory or repointed Git administration mapping cannot inherit the
original binding. Dirty mutable content is not mistaken for a restore fingerprint.
The actual SourceDriver registered in SessionStore is looked up by typed handle; no
duplicate registry, durable registry revision or new source grant.

GitNativeSourceResolver composes the current independent host guard and physical approval
with exact Store SandboxGrant preflight, selects one authorized compatible Git binding,
and verifies its materialization. The original checkout pin stays live until the native
CapabilitySet pins that exact directory in the requested effective mode; no launch-time
pathname reopening. Native source realization excludes the checkout's .git pointer/
directory and actual Git admin/common directories within the root, using existing
capability exclusions. This is not a new Git process sandbox or a claim about arbitrary
external writers/aliases. Independent physical policy and private-artifact registry still
come from the trusted host, never from the persisted SandboxGrant alone. Source
verification runs on a blocking worker, before the existing effect claim; final admission
rechecks the exact selected binding/authority/layout as before.

SourceFilesystemRoute::configure now uses configure_routed_native through the existing
adapter registration/policy/parallelism owner and original router/hook. The selected
canonical native tools have no ambient backend, even if the router is removed. No root
open or private-artifact reconciliation during registration; no Jobs, shell, wake/control,
legacy native shapes or remove without separate durable private-artifact admission.
Explicit tool selection only, same builder authority and limits; no replacement of an
occupied router slot. This is the reusable host builder seam for source-aware activation.
The optional aw-host/git-source-effects feature is enabled by the product workspace
feature; generic source-effects-only hosts do not need Git.

Existing grouped native regression runs with the actual Git driver when enabled (and
with the original generic fixture otherwise), real source read/write, .git exclusion,
no ambient root during registration, original journal/receipt and restart settlement.
The existing Git restore regression checks native realization of the exact restored
binding and wrong root/physical identity/repointed .git refusal. No new production
Actor Service admission is enabled: HostAgentActivation still requires the independent
live host policy resolver and source-aware admission across activation/queue/journal/
context. Do not loosen those gates merely because a configured native route exists.
Git repository **32/32**, Host **15/15** with Git and **15/15** without Git,
native adapter **99/99**, router **3/3**, Sandbox and checkpoint owners **4/4 each**.
Strict Git/Host/Adapter/Store all-target Clippy, workspace all-target check, minimal
build with no Git/SQLite normal dependencies, fmt/scoped diff-check green. Minimal
retains the two existing CLI dead-code warnings.

NativeSourceApproval owns one explicit independently approved source directory and
exact Agent/actor/Store binding, retains its physical identity pin, and derives the
policy digest from the actual native approval. It is not a registry or a reconstructed
persisted grant. The trusted host supplies the source binding, directory access, original
private-artifact owner and stable authority identity/generation; it must not reuse a
withdrawn generation. Publication uses the original Project-writer-authorized
SandboxGrant CAS. Live withdrawal prevents new reads, waits for admitted guards and
releases the pin permanently; it does not fabricate a Process stop or automatically
mutate durable grant evidence. Current Store grant and tool/Actor authority remain
independent requirements. Trusted host lookup now has an exact authoritative
AgentExecutionBinding and live Store-owner identity check.

GitNativeSourceHost composes that approval, the actual registered Git SourceDriver,
and the existing typed native realization handle. Verify materialization against the
independently approved physical directory, revalidate live authority at resolution,
refuse non-native or beyond-ceiling selection, configure one root-free Agent builder
with a fresh per-build router slot, and preserve exact original receipt settlement.
No automatic grant or ambient path fallback. A persisted grant without live approval
does not authorize execution.

HostAgentActivation has an explicit workspace-feature Git native host selection;
the original Store activation gate runs before legacy tool/root realization or the
native builder. Native selection would install only the approved tools, no Job runtime,
wake/control capabilities or inherited ambient router. The production source-bearing
gate remains closed across activation, queue, model context and journal. This slice
provides the host composition seam but does not claim a live source-bearing Actor,
production config/RPC issuance, or physical Process containment. Other agent journal
work was preserved.

The existing native grouped regression exercises the concrete approval owner in both
Git and generic builds, separate grant publication, wrong Store/backing Agent refusal,
Git native builder selection, withdrawal with a held guard, denial despite an active
persisted grant, original file effects and restart settlement. The original source-free
task-bearing activation regression still passes. Recovered the interrupted working
tree at f9b9761 and reran its focused gates. Host **15/15** with Git and **15/15**
without Git, Sandbox/checkpoint **4/4 each**, router **3/3**, original task-bearing
Actor activation **1/1**. Strict Host/Store/Adapter all-target Clippy, workspace
all-target check, durable-only and minimal builds, minimal dependency exclusion
of Git/SQLite, fmt and scoped diff-check are green. Minimal retains its two existing
CLI dead-code warnings.

Quiescent metadata-only sandbox_sources_set now uses the original Sandbox owner and
Project-writer authority, exact Sandbox rev CAS, current driver compatibility and host
source-set limit. Trusted callers independently authorize old/new sources and the
complete resulting physical layout (including exclusion of removed/omitted sources),
verify already-materialized bindings and retain live authority through publication.
No materialization, deletion, remount or external driver effect is executed. Effectful
changes still need original Prepared custody. The existing complete old source set and
incoming source scopes must both be free of active/Unknown claims inside the same
immediate transaction; no extra persisted lease or receipt for a SQLite-only mutation.
The shared legacy layout quiescence check is reused by checkpoint capture and mutation:
exact tool-root run/hazard key plus the whole backing Session of every sharing Agent.

First attachment is limited to an unused Sandbox with no other known open exact tool
root. Existing source-bearing mutation requires held Agent admission. Final-source
removal remains refused until verified source-free layout/physical retirement support;
no ambient legacy fallback. Exact no-op changes preserve revisions and events; each
real change advances Sandbox rev once and uses that generation for the changed binding
(initial publication uses rev 1). Removal/reinsertion cannot resurrect an old grant,
even for identical SourceRef/materialization. Unchanged binding pins and Agent revs
stay untouched. Local sandbox.sources.changed provenance records only source name/rev,
not opaque locators. Public Event projection and effectful structural realization remain
pending. Existing grouped grant regression uses the production mutation owner: unused
attachment, legacy retrofit refusal, Project authority, complete old/incoming barrier,
limit, non-final detach, identical reattach, stale grant/CAS, idempotency and restart.
Sandbox owner **4/4**, checkpoint/capture/restore **4/4**, barrier and construction
receipt **1/1 each**, real Host source-effect **2/2**, strict Store all-target Clippy,
workspace all-target check, fmt and scoped diff-check green.

Checkpoint owner now provides bounded live metadata enumeration (exact Agent View,
immutable creation-order cursor, since-retired cursor accepted), current Agent Manage
logical delete, and trusted retired-source reconciliation. Retirement is idempotent,
removes the live history GC root, retains the original request/result tombstone and
does not advance Agent rev or disclose source locators. Unresolved restore receipts
retain the checkpoint's history/anchors; refuse retirement until original custody
is reconciled rather than orphaning their history dependency. No independent Project
retention-admin authorization path is introduced yet. Applied restore replay remains
valid after source retirement; fresh restore refuses it.

Retired checkpoint rows and their exact immutable anchor set are the cleanup intent.
Cleanup requires exclusive Store owner, current Manage and independent trusted source/
provider cleanup authority, compatible registered schemas, no live anchor sharing
the driver/source/retention identity, and the whole original source mutation set.
One reserved checkpoint-retention claim identity derives from the checkpoint ID;
reclaim exact original hazards, never active/absent claims as recovery. Retained
cleanup is not new source admission and can exceed a lowered host limit. The trusted
reconciler inspects exact original anchors outside SQLite and either proves absence
or conditionally releases them under its live external authority/fence. It does not
recapture/restore or reclaim materializations/shared backing. Unknown retains all
anchor rows and fences; final receipt-free local transaction revalidates custody and
dependencies, removes all retained rows and settles claims with local provenance.
Already-cleaned tombstones never reinspect. No extra durable cleanup table or ledger.

GitSourceDriver::release_retired_anchor verifies exact source repository identity,
inspects the private checkpoint ref, refuses symbolic/changed/unreadable refs, and
uses no-deref expected-OID conditional deletion; verifies absence, never touches
HEAD/index/other refs/objects. True absence is successful retirement observation,
not evidence of NotStarted for capture. Git without show-ref --exists fails closed.
Provider/cross-Store fencing and independently authorized repository resolution
remain the caller's responsibility. This is a trusted driver primitive, not a
production automated cleanup worker or public RPC. No real user checkpoint or
repository was retired during qualification; only isolated fixtures.

Existing checkpoint/restore regressions exercise actual logical retirement, current
Manage vs View, bounded list, replay, unresolved restore pin, live shared retention,
complete-set exclusion, partial cleanup/Unknown, missing-driver restart and exact
recovery with lowered limit. Existing Git regression exercises exact conditional
release, changed/symbolic refs, absent replay, preserved HEAD/index/other anchor and
substituted checkout refusal. Git repository **32/32**, checkpoint/capture/restore
**4/4**, Sandbox **4/4**, barrier/construction receipt **1/1 each**; strict Git/Store
all-target Clippy, workspace all-target check, fmt and scoped diff-check green.

Source-bearing Sandbox logical close now uses the existing Project-writer lifecycle
owner. Requires exclusive Store ownership, every bound Agent closed, no known live
Agent sharing either exact execution path, legacy layout quiescence and the complete
retained SourceBinding mutation set free of active/Unknown claims. Logical close
does not require a compatible installed driver or a new effect admission limit; it
increments Sandbox rev once and keeps every original binding, generation, materialization
and dependency. Closed retained binding compatibility is revalidated on restart.
A closing/open bound Agent cannot coexist with a closed source-bearing Sandbox.
No physical deletion, mount change, or grant resurrection.

Known closed retained tool-root/working-directory paths are fenced across the Store
(including different Project or cwd) at fresh layout validation, publication, automatic
and explicit selection, legacy execution admission, and real source binding mutation.
An older empty Sandbox cannot re-adopt that retained layout. Matching is exact known
path metadata, not a claim of physical alias/ancestor or cross-Store containment;
independent host physical authority still owns those boundaries. Source rows continue
to reserve the layout until a separately proved physical retirement releases them.
Existing source-bearing execution gates stay held. The original grouped restore
regression now covers actual Agent close, live same-root/different-cwd Agent refusal,
active/Unknown source claim refusal and recovery, missing-driver logical close,
unchanged bindings and physical markers, CAS/replay, legacy/explicit/fresh selection
and mutation refusal, and compatible-driver restart. A test-only connection guard
was dropped before reacquiring the Store for recovery settlement. Sandbox **4/4**,
checkpoint/capture/restore **4/4**, barrier and construction receipt **1/1 each**,
real Host source effects **2/2**; strict Store all-target Clippy, workspace all-target
check, fmt and scoped diff-check green.

Closed-Sandbox reconciliation now uses sandbox_reconcile_closed_sources with the
original exact versioned bindings (including ephemeral sources) and immutable closed
layout. Reuse the original Project-writer/Agent/run/hazard owners, compatible installed
drivers and complete-set coordination claims; refuse open materialization sharing,
open source-bearing exact-path sharing and open derived-child dependencies. Closed
retained bindings are the cleanup intent, not a new ledger. Checkpoint and Sandbox
cleanup share reconcile_retained_sources_in: claim or recover the original reserved
hazard, never steal a live owner, never apply a lowered new-admission bound to cleanup.

The trusted reconciler independently authorizes physical inspection/release and proves
the entire original layout safe for reuse (not merely absence of a locator); any returned
provider/physical guard is held through final Store commit. No default destructive driver
action, shared backing reclamation, or inference of physical authority from metadata.
Unknown or failed final publication retains the original complete set and claims.
Successful release atomically deletes retained bindings, advances Sandbox rev once,
records local release provenance and settles the barrier. A replay performs no inspection.
Retained-path reservation ends only at that commit. The original historical source-bearing
Session remains effect-inert via its indexed immutable Sandbox release fact, even after
a freshly authorized Agent uses a clean recreated root. No Agent revision fanout.
The grouped restore regression exercises open shared dependency refusal, complete-set
exclusion, partial physical release, missing-driver restart, recovery under a lowered
limit, idempotent settlement, and fresh clean layout vs historical Session admission.
Only isolated fixture files are removed; concrete managed Git worktree cleanup and
independent production physical/provider authorization remain to be integrated.
Sandbox **4/4**, checkpoint/capture/restore **4/4**, barrier/construction receipt
**1/1 each**, real Host source effects **2/2**; strict Store all-target Clippy,
workspace all-target check, fmt and scoped diff-check green.

GitSourceDriver::release_restored_materialization now reconciles exact original
managed restore ownership using the original Prepared descriptor and published
SourceBinding. Reuse the existing private parent stamp, source/checkout identities,
detached base, worktree registration/lock marker and exact restored tree/index verifier.
An unchanged clean or captured-dirty checkout may be released after independent
physical authorization. Later edits, ignored extras, changed index/HEAD/marker,
substituted directories, mismatched binding or unavailable original objects fail
closed. Only Git's exact worktree removal is invoked; its two force flags are required
for a locked checkout and captured dirty delta and are gated by these verification
and live host-fence requirements. No unlock/retry gap, broad prune, recursive
caller-side deletion or shared object-store deletion. Revalidate target, registration
and original admin absence; bounded original marker check also refuses a relocated
or partially registered original operation. Absence is a cleanup observation, never
restore NotStarted. No private retention ref is required after retirement; original
commit/tree content must remain available for exact present-worktree verification.

Store owner-private sandbox_restore_preparation retrieves the exact original Applied
restore SourceSet descriptors through the existing indexed immutable construction
receipt. Validate initial/current claim identity, exact candidate Sandbox and bound
Agent/result identity and compatible recovery schemas. No copying locator metadata
into another ledger. Current Project writer and independent source metadata authority
are required; closed retained Sandbox only. It works after checkpoint anchor release
and restart, including a lowered host admission limit. Shared current_source_claim_in
also serves unresolved receipt validation. Trusted host can compose this lookup and
Git driver release inside the existing closed-Sandbox reconciliation, independently
authorizing all source/destination physical fences and proving the entire layout
safe before settlement. No automatic production cleanup worker, arbitrary dirty
worktree discard, unpublished candidate cleanup or provider fence is introduced.
Real Git grouped restore regression covers unchanged clean/captured-dirty release
after checkpoint ref retirement, modified content/extra files/lock/binding refusal,
substituted or moved registration refusal, absence replay and preservation of
unrelated worktrees, original HEAD/index. Store grouped restore regression covers
exact original provenance after retirement/anchor release/restart, missing-driver,
wrong Sandbox/actor and released metadata refusal. Only isolated fixtures were
physically removed. Git repository **32/32**, checkpoint/capture/restore **4/4**,
Sandbox **4/4**, construction receipt/barrier **1/1 each**; strict Git/Store
all-target Clippy, workspace all-target check, fmt and scoped diff-check green.

History-only and empty Auto capture now share the existing immutable checkpoint
publication owner. checkpoint_create_empty_auto verifies the current effective open
Sandbox has no required bindings, anchors latest committed history even during an
unresolved Turn, preserves AutoAllRequired coverage and uses the same Auto request
fingerprint/replay as source-bearing capture. No source effects, no fallback if any
required binding exists. Explicit None keeps its original replay namespace semantics.

checkpoint_restore_baseline restores the exact retained immutable history with the
ordinary fresh Agent/Sandbox constructor and current Project/history Derive/Share/
audience checks. The host explicitly selects and independently authorizes a fresh
destination layout and any already-materialized current baseline sources, holding
physical/provider authority through publication. None may select current destination
baseline sources; empty Auto may include only current ephemeral sources. No captured
source set is ever silently replaced by baseline state. Complete incoming source
scopes must be unleased, source-bearing baseline remains held, source-free baseline
uses ordinary admission. Isolated restore checks exact known open cwd/tool-root
paths across the Store as well as closed retained paths; physical alias/ancestor and
cross-Store isolation stay host-owned. External materialization or other effectful
baseline creation still requires original Prepared custody; no host policy provisioning,
external driver calls, task or custom ACL is added.

Both restore paths reuse publish_restored_agent_in, the original exact-history lower
constructor and candidate result validation. The metadata-only case records one
Applied construction receipt atomically with its resources and no fictitious Prepared
or empty recovery descriptors. Original literal request fingerprint and replay lookup
are shared with captured-source restore; replay returns the current authorized
destination Agent and original coverage, even after checkpoint retirement, missing
drivers or invalid new layout selection. No new receipt ledger or migration.
Existing grouped checkpoint/restore regressions exercise empty Auto at the latest
committed prefix, actual capture/replay identity, captured-source fallback refusal,
View-only and existing layout refusal, current ephemeral baseline under held admission,
required baseline refusal for Auto, incoming barrier, exact history and retired/restart
replay. Required source capture never silently becomes empty Auto. Checkpoint/capture/
restore **4/4**, Sandbox **4/4**, receipt/barrier **1/1 each**, real Host source
effects **2/2**; strict Store all-target Clippy, workspace all-target check, fmt and
scoped diff-check green.

Prepared checkpoint capture and restore can now explicitly abandon their original
private candidates; checkpoint_abandon_capture/restore recover the exact original
hazards for the original authenticated request after restart, never steal a live
claim or replan. Reuse the initial indexed Prepared receipt/current immutable claim
validation, candidate absence checks and complete-set source barrier. Current
inspection disclosure and independent host cleanup authority are required before
effects; callback success proves all original operation-owned retention/materializations
released or absent, without affecting live backing dependencies. Returned external
guard survives final commit. Revalidate exact original receipt, candidate absence
(including checkpoint request publication) and barrier; one transaction appends
Rejected and releases the claims. Failed/partial cleanup remains Unknown with the
whole original recovery identity. Rejected replay does not reinspect or authorize
new construction; Applied resources cannot be abandoned. Existing receipt owner,
no new cleanup state/ledger. Recovery remains available under a lowered admission
limit. Shared candidate absence primitive also serves ordinary restore preparation.

GitSourceDriver::release_prepared_capture reuses exact descriptor/anchor verification,
checked retention absence and expected-OID conditional ref deletion, shared with
retired anchor cleanup. Never infer current HEAD or delete changed/symbolic refs.
Interrupted scratch directories without proven physical custody remain fenced, not
deleted by pathname. release_prepared_restore reuses original private parent,
origin/worktree/operation marker and exact restored tree/index verification and
the existing verified worktree removal/absence check. Exact completed candidates,
untouched registered --no-checkout candidates (no index or foreign content), and
verified absence can be reconciled. Modified or partially materialized content,
uncertain scratch/registration and missing required verification fail closed.
No broad prune, shared backing deletion, or automatic production cleanup worker.
The trusted host must independently verify and authorize original physical
ownership and preserve external fences. Only isolated Git/Store fixtures were
removed in validation. Grouped regressions cover live claim exclusion, partial
cleanup, missing-driver/restart/lowered-limit recovery, terminal replay, Applied
refusal, exact dirty retention release, changed ref and uncertain scratch refusal,
completed and untouched registered candidate release, foreign content refusal.
Git repository **32/32**, checkpoint/capture/restore **4/4**, Sandbox **4/4**,
construction receipt/barrier **1/1 each**; strict Git/Store all-target Clippy,
workspace all-target check, fmt and scoped diff-check green.

Custom initial ACL construction now uses the original AgentCreateAccessGrant,
bounded validation/resolution and Agent ACL initialization primitives. The shared
AgentHistoryForkAccess adds Custom(Vec<AgentCreateAccessGrant>) (Clone, no longer
Copy; existing private/inherit serialized forms unchanged). Ordinary history-derived
agent.create and both checkpoint restore paths resolve the destination-domain custom
principals and publish one complete initial ACL + HistoryGrant, rather than expanding
a private grant one viewer at a time. Explicit custom audience expansion retains
the current manifest/source-share checks (no historical legacy-coverage shortcut).
Destination owner remains implicit; reject duplicate/empty/foreign-domain/owner
entries. Custom requires authenticated destination owner. Before source preparation
and again at publication, restore revalidates the complete audience and current
history/source disclosure. No new ACL owner, source grant, provider authority, or
Agent revision fanout. Original request fingerprint/receipt preserves literal access
semantics; initial task custody remains separate.

Grouped restore regression exercises custom ACL on baseline and actual source-bearing
restore, invalid audience refusal before planning, exact viewer history and permissions,
one HistoryGrant, initial revs and no SandboxGrant. Existing history fork regression
also exercises ordinary agent.create(from=...) custom access. The initial test-only
SandboxGrant assertion used a wrong column and was corrected. Broad agent_host state-shell
test still fails at its unchanged legacy replace_agent_job_subscription call, which
is deliberately denied after Phase-2 cutover; not fixed by weakening that gate or
changing unrelated Event tests. Task-bearing Agent create regression passes.
Final focused gates: checkpoint/capture/restore **4/4**, history owner **4/4**,
task-bearing Agent create **1/1**, Sandbox **4/4**, construction receipt **1/1**;
strict Store all-target Clippy, workspace all-target check, fmt and scoped
diff-check green.

InitialTask now owns shared candidate-local queue identity minting, encrypted
payload custody identity and initial Pending publication for ordinary Agent create
and captured-source checkpoint restore. New task receipts pin the trusted
execution_site; historical task-only receipts decode with None and preserve their
original encoding. Original queue request ID derivation is unchanged. Restore
request adds optional task (omitted None preserves old serialization/fingerprint);
task-bearing fingerprint includes the task digest, never plaintext. The lower
checkpoint_prepare_restore takes an explicit trusted execution_site argument.
Task validation and inert custody minting precede driver planning; original
Prepared receipt and complete source barrier commit before encrypted payload write.
Failed write/verification retains Unknown, with the complete original descriptors
and task custody. No new candidate or re-encryption on replay/recovery.

Publish re-verifies exact encrypted task custody under the original source barrier
before the final transaction. Shared Agent/history/ACL/Sandbox constructor appends
one ordinary Pending queue fact (original candidate, site, current writer epoch)
in the same transaction as Applied and barrier settlement. Source-bearing admission
stays held; the queue cannot dispatch. Task text never enters receipt/audit metadata.
Unpublished source abandonment deletes the exact private task only after all
source cleanup succeeds, before atomic Rejected/settlement; partial cleanup
preserves the task and claims. Terminal replay does not repeat effects or delete
an Applied input. Normal queue owner handles deletion after publication.
No new queue, task lifecycle or receipt ledger. Source-free/empty-Auto baseline
with an initial task remains explicitly unsupported pending task-only Prepared
custody/recovery without a fictitious source barrier.

Grouped restore regression verifies separate encrypted input, exact site across
restart, failed final Applied publication rolls back Agent/Sandbox/history/ACL/
queue while retaining custody, exact single Pending input, changed-task replay
refusal, held queue claim refusal, partial abandonment preserves input and
successful abandonment deletes it, normal queue deletion before close. Existing
task-bearing Agent creation remains green. Final focused checkpoint/capture/restore
**4/4**, task create **1/1**, construction receipt **1/1**, Sandbox **4/4**, encrypted
queue restart integration **1/1**; strict Store all-target Clippy, workspace
all-target check, fmt and scoped diff-check green. The first queue unit filter
matched zero tests and is not counted.

Task-only baseline restore now admits optional initial input for None and empty Auto
using the original InitialTask, queue custody and construction receipt. The baseline
API takes an explicit trusted execution_site (same as captured-source preparation).
No-task restore still performs one metadata-only Applied transaction, with unchanged
receipt encoding and replay fingerprint. Task-bearing baseline pins the original
canonical source-set selection digest and destination binding/history boundary,
without copying opaque materialization locators into a second owner. Existing
compatible-driver/source-set/fresh-layout and unleased preflight checks run before
the local task write and again at publication. Baseline sources are independently
authorized, already materialized, and not operation-owned effects.

The existing exclusive Store owner and connection guard serialize the bounded local
encrypted write, final transaction and explicit recovery/abandonment. No new lock,
source lease, empty barrier, or long-lived external callback in that critical section.
Open the existing vault (initialization before claiming task custody); Prepared
commits before ciphertext write. Shared PreparedQueueCustody methods reuse the
original AEAD/identity/digest checks on an already-open SecretStore, avoiding
connection reentry. Missing master with original prepared baseline custody refuses
vault regeneration. Unresolved task-only restores retain their exact checkpoint
history root and block retirement even without SourceDriver dependencies.

checkpoint_resume_baseline inspects original task ciphertext, exact indexed
Prepared/current receipt and candidate, current authority and independently
authorized original layout/source selection. It never rewrites missing ciphertext
or performs external materialization. Publish the fresh Sandbox/Agent/history/ACL/
Pending queue/Applied atomically. Failed publication retains Unknown; ambiguous
commit never overwrites an Applied fact. Applied replay before current context
returns the existing result. checkpoint_abandon_baseline removes only the private
original task, never baseline files or an Applied input, and atomically rejects
the original receipt. Source-free baseline admits ordinary execution; any sources
keep admission held. Existing grouped restore regression covers rollback after
queue insertion, task-only retention, abandonment and unchanged baseline files,
source-free pending input, held empty Auto with restart resume and exact selection
refusal, terminal replay and no fictitious source claims. Final focused gates:
checkpoint/capture/restore **4/4**, construction receipt **1/1**, task create **1/1**,
Sandbox **4/4**, SecretStore **6/6**, queue restart/missing master **2/2**; strict
Store all-target Clippy, workspace all-target check, fmt and scoped diff-check green.

---

## 11. Copy-ready continuation prompt

See [AW-DL200-PHASE3-NEXT-AGENT-2026-11-24-LATE.md](AW-DL200-PHASE3-NEXT-AGENT-2026-11-24-LATE.md).
The prompt deliberately points to this single canonical handoff and live plan,
so the next agent gets the initial design, complete history, exact current
checkpoint and safe next cut rather than reconstructing an old chat.

This handoff refresh changed documentation only. It did not change code, run
new implementation qualification, modify dotlink/release, inspect reserved UX
content, or publish anything.
