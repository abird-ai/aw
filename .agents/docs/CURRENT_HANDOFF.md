# CURRENT HANDOFF — AW live state + DL-200 R6

Canonical comprehensive handoff (**read this first after AGENTS.md**):
[AW-DL200-PHASE3-FULL-COMPREHENSIVE-HANDOFF-2026-11-25-LATE.md](AW-DL200-PHASE3-FULL-COMPREHENSIVE-HANDOFF-2026-11-25-LATE.md).

Copy-ready next-agent prompt:
[AW-DL200-PHASE3-NEXT-AGENT-2026-11-25-LATE.md](AW-DL200-PHASE3-NEXT-AGENT-2026-11-25-LATE.md).

Previous Phase-3, Phase-1 and R6 comprehensive handoffs remain historical design/implementation provenance:
- [AW-DL200-PHASE3-FULL-COMPREHENSIVE-HANDOFF-2026-11-25.md](AW-DL200-PHASE3-FULL-COMPREHENSIVE-HANDOFF-2026-11-25.md)
- [AW-DL200-PHASE3-FULL-COMPREHENSIVE-HANDOFF-2026-11-24-LATE.md](AW-DL200-PHASE3-FULL-COMPREHENSIVE-HANDOFF-2026-11-24-LATE.md)
- [AW-DL200-PHASE3-FULL-COMPREHENSIVE-HANDOFF-2026-11-24.md](AW-DL200-PHASE3-FULL-COMPREHENSIVE-HANDOFF-2026-11-24.md)
- [AW-DL200-PHASE1-FULL-COMPREHENSIVE-HANDOFF-2026-11-23.md](AW-DL200-PHASE1-FULL-COMPREHENSIVE-HANDOFF-2026-11-23.md)
- [AW-DL200-R6-FULL-COMPREHENSIVE-HANDOFF-2026-11-22.md](AW-DL200-R6-FULL-COMPREHENSIVE-HANDOFF-2026-11-22.md)

Normative R6 design:
- [README](../plans/dl200-unified-jobs-agent-events-2026-11-20/README.md)
- [SPEC](../plans/dl200-unified-jobs-agent-events-2026-11-20/SPEC.md)
- [IMPLEMENTATION](../plans/dl200-unified-jobs-agent-events-2026-11-20/IMPLEMENTATION.md)
- [TODO](../plans/dl200-unified-jobs-agent-events-2026-11-20/TODO.md)

## Latest local implementation delta — 2026-11-26: exact whole-Run native recovery and Input/queue disposition

Parent `7b5dae500bcf19e1af98c92eedc1e31256eb12fd` (pre-tool restart
and native artifact recovery). Published AW main remains `7b92686`.
L5, clean at entry; Store v60, Git recovery v4 unchanged. No new
durable receipt, schema, effect owner or execution path.

- New original Store logical recovery consumer:
  `SessionStore::reconcile_native_turn(manager,canonical_agent,turn)`,
  actual offline `aw native-turn-reconcile --state-dir <existing-private-Store>
  <canonical-Agent> <original-Turn>`. No physical config selector
  or approval implied: each actual effect must already be released
  by its original owner (including explicit native-source-recover
  when necessary). Exact local OS principal/exclusive existing Store,
  current Agent Manage and Project writer/Session owner with original
  active writer epoch, exact bound open Git Sandbox/layout. Terminal
  attached original Run of a prior stopped/fenced owner-lock
  incarnation in the same Store scope, no live Run in the backing
  Session. Current live owner, unrelated/foreign Run, noncanonical
  Agent, attachment conflict or changed owner epoch refuses.
- Reuse the existing exact direct owner Run provenance from pre-tool
  startup compatibility: original first attempt, immutable prompt
  node and branch admission anchor, epoch, original Dispatching
  Input/prompt digest, original owner identity/process/Store scope
  and first owner authority segment (if recorded), no web/routing
  lineage. Preserve the existing pre-tool no-intent-only startup
  predicate. No generic no-start inference from missing claim.
- Scan the entire bounded original immutable Run journal. Every
  ToolStarted must be original direct local trusted-host non-Remove
  native Read/Write, unique exact original intent; original
  Sandbox/native driver claim, same complete named Git sources and
  original versioned binding digest, exact ToolFinished or original
  Interrupted Turn and actual immutable positive release. Missing/
  Unknown/unclaimed/mixed driver/unsupported intent, orphan terminal,
  revised tool or partial release refuses. ToolDenied is not a started
  effect. Compare actual write intents to original Run possible_effect
  and exact hazard root/session. Use the same claim binding validator
  as per-effect recovery, with a read-only durable binding selection;
  factual logical recovery does not require registered live drivers or
  checkout probes. Never release source barriers here or manufacture
  source settlement.
- In one original immediate Store transaction, settle only this Run's
  needs_inspection hazard with existing effects.inspected audit fact
  (or retain already-settled/no hazard), then use original Input
  and queue fact owners. Exact dispatching/Unknown Input becomes
  Inspected, never successful or retryable; existing terminal Input
  retained. Exact Accepted queue must match original Run process,
  Store generation, Input sequence/Turn/epoch/target/digest/priority/
  Session/branch and local site. Retire as Inspected, or Settled only
  if original Input was already factually Settled (missed queue ACK).
  Direct owner Input without queue supported; missing reserved qi_
  receipt refuses. Shared original claim validation/retire helper
  used by normal finalization too. No pending unrelated Input changed.
  One Agent revision advance for actual reconciliation, no write on
  exact terminal replay. Delete only original private encrypted
  queue payload after immutable terminal commit; replay can finish a
  missed unlink (original idempotent SecretStore delete). Return
  redacted `{turn,source_effects,hazard_cleared,input,queue,changed}`;
  hazard_cleared/changed indicate this invocation's mutation, not
  provider success or new authority.
- Do not rewrite original failed/interrupted/cancelled Turn,
  provider attempt/usage, retry policy or hints, source release,
  Sandbox revision, grant or other Run/import/coordination hazards.
  Generic Session journal acknowledgement now retains the original
  source-unaware layout gate; it cannot blanket-clear source-bearing
  hazards, even from a live source-aware journal. Existing legacy
  source-free inspection remains. No automatic replay, grant/open or
  source-bearing retry promotion. Explicit fresh live approval and
  grant still required for subsequent Input.
- Extend the existing actual native RPC fixture, not parallel mock
  owners: no-tool process kill + original unknown provider Run,
  production reconciliation/terminal replay instead of manual Input/
  queue inspection. Then real read (already released), native write
  with injected exact Stage reservation and unowned lookalike,
  actual process kill awaiting the following provider response.
  Original interrupted Run has both native intents, unknown last paid
  attempt, original Dispatching Input/Accepted queue and next pending
  Input. Partial-release whole-Run refusal preserves all custody;
  existing native-source-recover removes exact artifact only, releases
  original write and advances Sandbox rev once, but Run hazard
  remains. Exact whole-Run reconciliation clears only this hazard,
  retires original Input/queue, one Agent rev, no Turn/history payload
  rewrite; replay no changes. Fresh approved native root settles
  only the next pending Input. Actual Git HEAD/index/tracked contents
  preserved. Existing real Host lifecycle also checks generic
  acknowledgement and same-current-owner whole-Run refusal.
  Crash after real write before final Run settlement is qualified;
  actual OS loss inside native filesystem mutation remains separate.

Validation (L5 Rust 1.95.0, final source): actual executable native
RPC/crash/whole-Run recovery 1/1 (18.62s), including no-tool and
post-native-write process kills, partial proof refusal, original
Input/queue disposition, idempotent replay and subsequent pending
Input only. Agent lifecycle 3/3, Store Sandbox 4/4, Actor Input
admission 2/2, Store queue 8/8, original recovery 2/2, Host native/
Process 2/2, full Actor owner 46/46. Initial type-check corrections
used the original Transaction for release lookup and imported the
existing SecretStore trait; final check passed. Strict all-feature/
all-target Store/Host/CLI Clippy, durable-only and minimal all-target
checks (existing feature-specific dead-code warnings), full all-feature
CLI build (56.59s incremental), actual version/help smoke, scoped
rustfmt/diff and signed checkpoint. No full workspace suite, pinned
Nix or native-platform qualification repeated. No real user source
or provider effects.

Next: narrow unclaimed ToolStarted proof/recovery only if actual
original source route/no-start can be established, including
journal-boundary loss before claim. Also qualify linked checkout
artifact proof and independent Process quiescence/containment.
Current direct native lifecycle now has explicit startup, live
withdrawal, pre-tool/claimed-effect restart, per-effect physical
recovery and whole-Run/Input/queue reconciliation. Do not create
alternate recovery ledgers or broaden held shared/participant,
Graph/Job/Process/extension execution. Frozen dotlink unchanged.

## Previous local implementation delta — 2026-11-26: pre-tool restart and exact native artifact recovery

Parent `3e4132bf9b646e45e26b5b62e22f5699c09a911b` (live withdrawal);
published AW main `7b92686`. L5, clean at entry. Store v60, Git
recovery v4 unchanged; no new execution owner or receipt ledger.

- The exclusive owner startup compatibility gate previously refused
  every unresolved source-bearing Run lacking an original source
  claim, including process loss during its first provider request.
  `require_pre_tool_run_in` accepts only exact direct owner Input
  provenance with no ToolStarted/ToolFinished/ToolRevised, no possible
  mutation effect, no unresolved Run hazard, original first attempt
  and no routing/web dispatch. Require original Store/owner-lock
  incarnation, exact prompt/branch anchor/revision/writer epoch,
  original immutable Dispatching Input and owner identity; if an
  authority segment exists it must be the single exact first owner
  Input segment. No current grant, provider or SourceDriver needed
  merely to reconstruct the interrupted Run. Direct/staged modes
  remain quiescent-only; the existing claimed-effect recovery gate
  remains unchanged. Original Store recovery reconstructs a failed
  Turn with unknown provider usage/attempt, not a new attempt or
  a fabricated provider result. Original Input/queue uncertainty
  remains independently inspectable; no automatic replay or retire.
  **Unclaimed ToolStarted remains held**, even a native/read intent:
  absence of a source claim alone is not physical no-start proof.
- Add `SandboxToolRecoverySelection` and
  `sandbox_recover_tool_source_effect_exact` sharing the original
  recovery transaction with the generic SDK method. Exact original
  driver and optional Sandbox selection is checked before consuming
  any recorded quiescence or immutable release. No cross-driver
  replay. Existing `GitNativeSourceResolver::recover` and configured
  recovery share the same physical inspection/custody implementation.
  Registered typed native/Git driver selection is lazy for missing
  physical proof; released replay needs no checkout or artifact
  namespace probe. Recorded quiescence retains its original Store
  binding revalidation/release path. No second cleanup mechanism.
- New actual offline command:
  `aw native-source-recover <private-recovery-approval.json>`.
  The strict v1 file is in the **original private artifact state
  directory**, whose identity is independently approved. Fields:
  `{version,store_directory,store_id,store_generation,manager,agent,
  sandbox,turn,call,base,base_identity,artifact_state_identity,
  repositories:[{location:GitSourceLocation,binding_rev,access}]}`.
  GitSourceLocation is `{name,repository,repository_identity}`;
  directory identities are volume/object pairs. Exact local OS
  principal/exclusive existing Store, current Agent View and Project
  writer authority, original tool journal/claim and original driver
  required. Private advisory approval lease across operation; match
  exact original complete named source set, current binding revisions
  and access ceilings, original base and physical identities. First
  physical recovery consumer deliberately supports only private
  ordinary checkouts (one base or named children), with contained
  Git administration and disjoint Store/artifact roots. Linked
  worktrees/other drivers not promoted. Independently trusted host
  must ensure permanent quiescence of original native worker and
  actual original artifact namespace custody, excluding surviving/
  external writers, aliases, mounts, other Stores and backing users
  throughout. Exclusive Store/pins are not kernel containment or
  sufficient evidence of arbitrary Process death. No creation of a
  missing artifact namespace; original driver verifies exact backing/
  materialization, protected Git paths and actual existing registry.
  Cleanup only exact owned artifacts using original lower capability;
  preserve unowned lookalikes. Guard and pins through original
  release/Unknown settlement. Terminal replay matches original
  Agent/Turn/call/Sandbox/driver without re-running physical proof
  or requiring old checkout. Outcome is explicitly source_effect_only;
  no whole-Run hazard acknowledgement, queue disposition, new grant,
  activation, tool retry or modification of the original Turn.
- Extended existing executable fixture (one private Store, Git
  checkout, loopback provider): actual process kill during first
  provider request of a new Turn, no tool intent; reopen preserves
  interrupted_unknown/unverified provider attempt, exact Dispatching
  Input/Accepted queue, later pending item. Original explicit Input
  and queue inspection (not provider/no-effect proof) lets a fresh
  independent approval dispatch the next Input. At real private
  Stage fault fixture boundary (reserved/finalized record and actual
  owned stage file), actual native write leaves source custody
  unresolved. Restart and offline recovery wrong artifact identity
  refusal, exact cleanup, unowned lookalike retained, original
  source barrier released, one Sandbox rev advance, history
  unchanged; wrong Sandbox/driver replay denied, exact released
  replay with checkout temporarily absent succeeds. No fixture DB
  row fabrication, real provider credentials or user source effects.

Validation (L5 Rust 1.95.0, final source): expanded actual executable
native-source RPC/recovery 1/1 (14.78s), including original no-tool
Run process loss and separate real native write with fault-injected
original artifact record. Initial fixture comparison used a HistoryState
without PartialEq; compare the same serialized history instead. Initial
queue-priority fixture accidentally used negative (urgent) priority;
corrected to positive later priority. Both corrections are test-only.
Host native/Process 2/2 (7.71s), Store Sandbox 4/4, source barrier 1/1,
Actor 46/46 (46.71s), Agent lifecycle 3/3 passed. Strict all-feature/
all-target Store/Host/CLI Clippy, durable-only/minimal all-target checks
(existing feature-specific dead-code warnings), full all-feature CLI
build (48.13s incremental), actual version/help smoke, scoped fmt/diff
and signed checkpoint. No full workspace, pinned Nix or native-platform
qualification claimed. Actual process loss during a native mutation
was not simulated in the executable test; the existing Host interrupted
custody suite also passed. No broad Run-hazard or containment promotion.

Next: original whole-Run hazard and custody reconciliation using
complete original native effects/proofs; handle unclaimed ToolStarted
only with actual original route/no-start evidence, never merely
absence of a claim. Production explicit original Input/queue
disposition and physical recovery of linked layouts/Process sources
remain separate; no blanket inspection acknowledgement, Process
containment or source-aware shared/Graph/Job execution promotion.
Frozen physical Sandbox unchanged.

## Previous local implementation delta — 2026-11-26: live native source withdrawal and cooperative Turn cancellation

Parent `c963f43d4143b5a663cf9940f2e51269f3914b9f` (production
configured native source-root stdio). Published AW main `7b92686`.
L5 only, clean at entry; no new durable schema or execution owner.

- `NativeSourceApproval::request_withdrawal` is monotonic, nonblocking
  withdrawal of this one independently selected live policy. The shared
  atomic fence is checked after acquisition of the original read guard:
  new admission fails immediately, already admitted guards retain the
  actual decision/pins through the original factual worker settlement.
  Existing async `revoke` first withdraws then drains those guards via
  the original write lock. Neither operation claims physical Process
  stop, and neither mutates durable SandboxGrant evidence. The revoked
  in-process owner can never be reactivated. Nonblocking withdrawal may
  retain the decision/pins until explicit drain or owner destruction.
- The explicitly configured native root returns the same Git host
  approval as a narrow `LiveSourceWithdrawalPort`. Only that bound
  local stdio mode advertises `native_source_withdraw`; strict empty
  object params, no target/ref/path/provider/revision/recovery selection.
  Reader thread invokes the one live decision and signals the existing
  active cancellation token, if any. Response states
  `{admission:"withdrawn",newly_withdrawn:bool,cancel_requested:bool,
  durable_grant:"unchanged",note:...}`. No "stopped" or "drained"
  acknowledgement; no new persisted withdrawal receipt or grant/Agent
  mutation. Repeated distinct-ID withdrawal is monotonic/idempotent;
  duplicate frame identity rejected before the port. Existing queued
  items are not deleted or requeued, no auto resume/renew. New custody
  crossing during withdrawal can remain uncertain at its original
  boundary; never assert such a race is a no-effect disposition.
  Editing/removing private config alone remains insufficient to withdraw
  an existing live decision; new process startup still needs fresh
  independent approval and explicit grant CAS. This host control is
  not available to model tools, shared participants or remote peers.
- Move the original bounded connection-local request ID/budget check
  from the response loop to the original stdio input reader, before all
  concurrent live operations including abort. Single bounded identity
  set; ordered Rejected response through the same channel. Previously
  an invalid duplicate identity could reach a concurrent port even
  though the response loop rejected its result. Existing strict
  parameter validation and original durable operation receipts remain.
  No new transport queue or durable request ledger. Live operations
  still use the bounded reader lane; no claim of hard real-time stop or
  bypass of reader backpressure.
- `AgentSessionController::run_once_verified_with_cancel` and
  `Service::run_once_with_cancel` pass the existing trusted host token
  into the original default-provider/verified-Input Turn path. Existing
  selected-provider/participant restrictions retained. Replace stdio
  Actor step's outer select (which dropped the active future) with this
  cooperative path. A signal is not a provider-side cancellation proof;
  attempted paid work records its actual failed/cancelled attempt and
  unverified usage. Already-started native blocking work retains its
  original lease/guard until factual completion, or Unknown requiring
  recovery. A cancelled Turn may still have original independent
  hazards; protected-output disclosure validation is not weakened.
- Extend actual executable/loopback fixture: rejected duplicate and
  malformed withdraw, actual dispatched provider request, withdraw while
  active, distinct-ID repeated withdrawal, factual cancelled Turn and
  unverified original attempt, no more model dispatch, next Input
  remains pending, durable grant still active at original revision,
  unchanged original Git state. Extend existing detached native worker
  test: immediate withdrawal denies new admission before drain,
  existing worker still owns exact source mutation barrier/guard
  through factual worker settlement; no synthetic ToolFinished.
  Existing concurrent constructor test also verifies duplicate ID
  cannot reach its actual port.

Validation (L5, Rust 1.95.0): actual executable native source RPC
1/1 (9.09s), including live withdrawal during actual seventh provider
request, duplicate/malformed refusal, repeated withdrawal, cancelled
original Turn with unverified provider usage, one pending Input left,
no more dispatch and unchanged grant/Git state. Headless 8/8 (duplicate
concurrent constructor blocked), Actor owner 46/46, Agent lifecycle
3/3, Host native/Process 2/2 (immediate withdrawal + detached worker
drain), prior stdio integration 3/3, Core journal 14/14 passed.
All-feature/all-target type-check and strict affected Core/Host/CLI
Clippy, durable-only/minimal all-target checks (existing feature-specific
dead-code warnings), full all-feature binary build (51.06s incremental)
and actual version smoke aw 0.15.0, scoped fmt/diff and signature.
No complete workspace suite, pinned Nix or native-platform qualification
claimed. No real provider or user source effects.

Next: original incomplete-source Turn restart reconciliation and
production independent native artifact/Process recovery proof
acquisition. Live withdrawal is separate from durable grant CAS and
does not give physical stop or recovery proof. Broader source-aware
Process/Graph/Job, shared Agent/participant access-generation cutover
and OS containment remain held. Store v60, Git recovery v4 unchanged.
Frozen physical Sandbox unchanged.

## Previous local implementation delta — 2026-11-26: production configured native source-root stdio

Parent `1215c2e3c7c9e63c475a271099f266f34714c59a` (queue/history
correction); published AW main remains `7b92686`. L5, clean at entry.
Original source-aware Actor, journal and Git native route from e34da44
now consumed by the actual CLI, not a second owner or scheduler.

- Workspace-feature startup:
  ```text
  aw rpc --agent-stdio --agent-actor-owner --state-dir <existing-private-Store>
    --session-id <exact-ID> --session-branch <ID-or-name>
    --agent-native-source-root <existing-private-approval-directory>
    --agent-native-source-grant-rev <expected-grant-revision>
    [--agent-native-source-open-rev <expected-Agent-revision>]
    --tools-mode read-only|rw --tools-allow <explicit-native-tool-names>
  ```
  The explicit grant CAS is required (0 only for first grant); new
  process authority is fresh, not reconstructed from persisted grant.
  Held Agent additionally needs explicit matching Manage-authorized open
  revision. Already-open intent alone gives no live authority; open
  flag optional there, still CAS if specified. Startup does not create
  a Store/Agent/Session, infer a grant, resume an ambient root, or
  dispatch an Input. No context/layout transfer, implicit model setup
  wizard, shared provider, routing, project-resource/skill/system
  overrides, subagents, MCP/extensions, shell, Remove, Graph or Process
  route. No other optional checkpoint/Sandbox/Vault admin capability in
  this initial narrow execution mode. Exact saved local Session ID
  required; branch selector resolves to the exact canonical Agent
  pinned by the independent config. Other RPC modes remain unchanged
  except raw prompt now truthfully disabled at the transport gate for
  all bound Actor owners, including templates.
- `native_source_root::Selection::activate` selects the existing
  Session/Agent/active writer through the authenticated local OS owner.
  Checks exact saved cwd/tool root/system/provider destination (no
  transfer), explicit native tool mode/unique allowlist, original
  open/held CAS, current grant revision, read-only original history/
  reconciliation state, private canonical state/approval dirs and
  physical/lexical separation. Reuse the existing strict v1
  `git-native.json` approval format and
  `GitNativeSourceResolver::provision_configured_approval_with`;
  callback checks the exact selected Agent, Sandbox, full source set,
  immutable layout, actual same stable owner identity (configured
  actor is local authenticated principal or original Session owner),
  writer and access ceiling before Git checkout/artifact probes.
  Registered actual Git and native filesystem drivers only. No
  persisted policy minting or new source schema. Host independently
  excludes other writers/users, aliases, mounts, backing/private state
  access through execution and interruption, as with existing native
  approval; no kernel containment claim.
- Existing configured provider selection (including separately
  authorized local/OAuth/server-account provider selection), current
  live host decision, explicit grant CAS, `build_execution`, optional
  explicit Agent-open CAS, `activate_with_source`, original history/
  SessionHandle and `Service::attach_for_agent_with_source`. Existing
  Host receives that actual Agent and Actor owner; bypass only ambient
  tool construction and duplicate tool-free RPC Runtime attachment.
  Same `aw::headless::serve_stdio`, original encrypted queue, step,
  query, inspect/disposition, cancellation, state/messages operations.
  No event router/automatic wake, new resource admin ports or model
  tools registered. RPC cannot choose policy paths, versions, provider,
  source set, Actor, grant or recovery selectors. Explicit grant and
  Agent open are original separate durable mutations: a subsequent
  startup failure can leave a factual grant/open intent, but no live
  policy or executable route survives failed startup/process exit.
  Do not silently roll back these facts or retry effects. Observe
  current revisions using original Store/SDK owners before startup.
- Existing native approval is a live in-process decision from a
  protected configuration snapshot. Editing/removing that file alone
  does not withdraw an already-live decision. Restart requires fresh
  configuration including actual current Sandbox rev after writes and
  a fresh explicit grant CAS. No new live withdrawal endpoint or
  incomplete-Run proof; separately qualify next. Normal source-unaware
  activate still denies. Original native tool fences and factual
  settlement unchanged.

Validation (L5 Rust 1.95.0, final source): actual executable
native-source RPC 1/1 (7.35s) with private Git/SQLite fixture and
loopback SSE provider, no real credentials. Ordinary resume, missing
grant CAS, held/stale open, shell selection, unrelated admin flag,
wrong physical root identity refuse before first grant/model; valid
startup runs real read/write and leaves second Input queued; stale
Sandbox/grant approval refuses; fresh approval/regrant on restart
settles original queued Input once, then idle. Exact two native tools
at provider, raw prompt disabled, no extra model dispatch, unchanged
Git HEAD/index/original tracked content; durable grant revisions 1/2.
Headless 8/8, prior stdio integration 3/3, Agent lifecycle 3/3 passed.
Strict all-feature/all-target CLI Clippy, durable-only/minimal
all-target checks (existing dead-code warnings), full all-feature
executable build (47.08s incremental), actual version/help and
Actor-owner required flag smoke, scoped rustfmt/diff passed.
Initial routing-option type-check correction was completed before
final validation (role/lineage are defaulted strings, not optional
route selectors). No full workspace test-suite, pinned Nix or
native-platform qualification repeated.

Next: explicit live withdrawal/stop and original incomplete-source
Turn recovery, including independent physical native artifact proof.
Do not promote broader source-bearing Process/Job/Graph/extension
dispatch or general shared/participant/forked source execution.
Legacy participant Agent ACL/access_generation cutover remains a
separate qualification item. Store v60, Git recovery v4 unchanged.
Frozen dotlink physical Sandbox untouched.

## Previous local implementation delta — 2026-11-26: reconcile committed queue revision and local history projection

Parent `e34da446d08157a87cac72a65175511fb34c5905` (live native
source Actor/Turn admission); published AW main remains `7b92686`.
L5, clean at entry. No new durable schema, receipt, owner or mode.

- Reproduced both earlier Actor qualification failures with retained
  pre-native-change test executable `aw-0e4d37c92bc526ba` (mtime
  2026-11-25 12:13 UTC; lacks the new native Actor test), with the same
  assertion/permission failure sites. Exact parent source comparison
  confirms the queue acceptance and Agent ACL binding paths were
  unchanged by e34da44. The old binary is historical reproduction,
  not a rebuilt or cryptographically attributed exact parent.
- Ordinary encrypted queue publication in the original immediate
  transaction now requires exact branch and open lifecycle (held
  admission still permits staging/queueing), checks optional original
  Agent revision, appends the Pending fact and advances the same
  branch's authoritative Agent rev once. No schema, new index or
  receipt. `actor_queue_enqueue_minted_at_revision` shares the original
  owner with existing unconditional/direct enqueue APIs. Self and
  delegated-parent jobs_send compatibility CAS now reaches the
  actual transaction; normal additive model-facing input has no CAS
  requirement. Original prior exact request replay still occurs
  before fresh CAS; rejected or repeated custody does not advance
  rev. Delegated capacity reservation/cleanup unchanged. Prepared
  construction and staged input Send retain their original publication
  owner and rev behavior, so no double increment or per-item rev fanout.
- Shared Session/Agent regression now demonstrates Session
  participation alone cannot read the exact Agent binding; explicitly
  grant View+Submit via the existing Agent ACL/HistoryGrant owner.
  No automatic synchronization, grant broadening or public Session
  authority resurrection. This uncovered actual duplicated history
  after same-Agent audience expansion: the current HistoryBase may
  seal the Agent's own local span for disclosure/retention, but it is
  not a second inherited data prefix. Reuse the existing self-span
  skipping selector (renamed `inherited_base_for_branch`, bounded)
  for effective history and factual Turn provenance as well as sealing.
  Verify actual inherited spans as before; append the original local
  projection once. Existing exact settled-Turn lookup, local immutable
  nodes, HistoryGrant pointer, retained seals and manifest authorization
  are preserved. No new persistence or lazy writes. Extend existing
  real fork lifecycle with inherited seed, local Turn, new audience,
  exact original provenance and refork; both Turns appear once.
- The older explicit participant dispatch still uses its legacy
  Session participation/audience evidence. This slice corrects the
  fixture's independent Agent binding grant and actual history owner,
  not a full legacy participant execution/access_generation cutover.
  Keep that separate qualification item; do not infer source-bearing
  or shared execution from the corrected test. Native source execution
  remains deliberately exact owner-only.

Validation (final source, L5 Rust 1.95.0): full Actor owner suite
46/46 (44.06s; both previous failures now pass); History owner 4/4
(2.38s), Store queue 8/8 (13.01s), staged input plan 1/1,
checkpoint owner 4/4, Agent lifecycle 3/3 (actual native source Turn/
restart included), stable child lifecycle 2/2. Strict affected
Store/Host/CLI all-feature/all-target Clippy, durable-only and minimal
all-target checks, full all-feature CLI build (35.94s incremental)
and actual version smoke `aw 0.15.0` passed; prior link timeout resolved.
Existing feature-specific dead-code warnings remain. Scoped rustfmt/
diff and exact signed checkpoint. No full workspace test-suite,
pinned Nix or cross-platform qualification claimed. No parent rebuild;
historical earlier-binary reproduction and source comparison as above.

Next: explicit production configured Git source-root activation
through the existing CLI/stdio owner, without ambient root realization
or implicit grant/admission. Reuse `GitNativeSourceHost::build_execution`,
`SandboxExecution`, original SessionHandle and Actor service. Keep
live withdrawal and incomplete-Turn recovery, legacy participant
Agent ACL/evidence cutover, broader Job/Process/Graph/extension
containment and independent physical recovery held until individually
qualified. No frozen dotlink changes.

## Previous local implementation delta — 2026-11-26: live native source Actor/Turn admission

Parent `7b926864bd0aa1a733323933f56eacdf68b88e72` (formatting)
and `fdcdb549` (EventInput forwarding) are signed, published to AW main;
the worktree was clean at entry. L5 only. No new schema (Store v60),
receipt ledger, Actor, queue, execution or tool registry owner.

- Add the owner-local `SandboxExecution` scope: explicit trusted-host
  acquisition of the live independent policy guard on every admission,
  exact original Agent/actor/StoreScope and writer epoch, current
  registered native tool authority (weak identity, exact snapshots),
  isolated bound Sandbox, original named binding/grant/ACL ceilings,
  and exact immutable Session layout. Require all approved bindings,
  matching execution site, no shared Session/Sandbox/layout, no
  unresolved layout reservation, and unleased complete source set
  before new Input/Turn. No source-unaware execution gate is widened.
- `agent_open_with_source` is explicit Manage/CAS under live policy,
  not approval provisioning or implicit admission on activation.
  `activate_with_source` returns the original SessionHandle, carrying
  that scoped acquisition. Persisted open intent survives restart
  but is not live authority; a fresh configured host decision and
  durable grant publication are needed. Original held admission,
  source-free activation, factual close and retirement remain.
- Existing Actor queue claim, Input prepare/accept/dispatch and
  original journal begin recheck current scope; begin requires the
  exact original Dispatching Input (branch anchor, Turn, epoch and
  prompt digest), never an ambient prompt. Original context and
  disclosure gates are retained; journal authority/model/tool starts
  acquire the live policy through their Store transaction. Existing
  actual native effects keep their own live ToolBinding admission
  fence and physical realization guard through effect custody.
  Factual terminal records are not conditional on current approval.
  The source scope is direct native-only; participant authority,
  steer/interrupt/freeze/resume, argument revision, retry/routing/web
  submission and non-native tools do not acquire new source effects.
  Cancellation remains supported by the original owner.
- Correct original tool intent observation for the first owner Input
  segment without participant evidence: corroborate the Run identity
  with the immutable historical Dispatching Input, produced-from
  branch anchor, prompt node/digest, writer epoch and exact Input
  segment identity. No ACL inference or new durable evidence.
  Preserve the original recovery path for journal-linked source
  custody even if persisted admission was open. Broader unresolved
  Run recovery without an original source claim is still held.
- Existing `GitNativeSourceHost` builds the native route and exact
  execution scope together. Acquire actual live approval, verify
  original registered driver and Git materialization pins plus base
  identity before Store admission. This is an independently controlled
  layout, not OS-level containment; no physical proof promotion.
  The original `HostAgentActivation` now consumes the scope, either
  using already-open intent or explicit Manage/CAS opening. No new
  worker or alternate inference loop. No CLI flag activates a
  source-bearing root yet.
- Real configured Git-backed Actor test: deny before grant and while
  held, deny source-unaware activation and journal begin without an
  original Input; first direct Actor Turn executes read/write against
  actual checkout (Git index unchanged), next original queue item
  stays pending on hold and live revocation; reopen Store preserving
  open intent, observe the actual new Sandbox rev after write, create
  fresh independent approval, reject stale persisted grant, publish
  new grant by CAS, activate existing host worker and settle original
  queued Turn once; factual close. No fixture-only authority shim.

Validation: focused real native Actor lifecycle 1/1 (8.13s), grouped
Agent lifecycle 3/3, Store Sandbox 4/4, Host source Process/native 2/2
passed. Broader original Actor suite 44/46: two failures in unchanged
legacy paths, `jobs_send_agent_queue_is_replay_safe_and_revision_fenced`
(expected stale Agent revision Conflict) and
`shared_session_submit_keeps_one_agent_branch_and_uses_participant_provider_authority`
(Session-only permission setup cannot read the Agent binding). The
failure sites are outside new native scope; baseline was not independently
re-run, so neither failure is certified pre-existing or dismissed.
No weakening of those checks or unrelated fixture changes. Strict
all-feature/all-target Store/Host/CLI Clippy and durable-only/minimal
all-target checks passed (existing feature-specific dead-code warnings).
Full all-feature CLI binary link exceeded the 180s time budget; stopped
without a new executable/version claim. All-target Clippy/type checks
passed. Pinned Nix/native-platform and cross-repo qualification not repeated.

Next: investigate the two legacy Actor qualification failures before
broader release. Explicit production configured source-root activation
through the original CLI/stdio owner is the next implementation slice,
followed by live withdrawal/stop and original restart reconciliation of
incomplete source-bearing Turns.
General shared/forked/multi-source-policy dispatch, source-aware
Job/Process/Graph/extension execution and physical recovery proof
remain independently held. Do not treat this direct native scope as
a generic containment or provider-fence implementation. Git physical
verification is intentionally conservative per admission; reuse of
verified live pins without redundant full Git probes remains a hot
path optimization, subject to preserving exact current mapping proof.

## Previous local implementation delta — 2026-11-26: reconcile EventInput journal bridge and formatting

Parent `b8564e3047e4cc180e6bc935dbbf046584236739` (authenticated
Sandbox source setter) was independently verified published to main.
User explicitly authorized reconciliation of the whole previously dirty
worktree. L5 only.

- Read-only audit identified 41 modified tracked files: 38 exact
  rustfmt-only outputs from existing committed source, three actual
  EventInput journal adapter changes. Comparison normalized both HEAD
  and worktree versions with the same Rust 1.95.0 rustfmt (1.9.0);
  output equality, not whitespace-diff heuristics. Earlier dirty-path
  ownership inference was wrong: no separate unfinished UX, resource
  CLI or networking implementation was present in those changes.
- Phase 2 cutover `e0c6bb31` already calls
  `Agent::bind_event_input/clear_event_input` from the original Actor
  queue; `bd18dcd` already owns durable EventInput scope/one-time
  binding in SessionHandle. The three forwarding edits were omitted
  from signed history. Earlier full CLI checks consumed these working
  copies, so clean published source lacked the required methods.
  Reconcile them together: trusted-host Agent delegates to its existing
  TurnJournal; default bind fails closed for unsupported backends,
  matching clear is idempotent; SQLite SessionHandle delegates to
  its original scope validation and pending hint consumed in begin.
  No new Event, queue, journal, grant, schema or execution owner.
- Complete stable child Agent's BudgetJournal adapter in
  `src/subagents.rs`: forward both methods directly to the original
  journal, without altering delegation reservation or settlement.
  Extend the existing real stable-child lifecycle fixture, not a new
  scheduler: use actual future-only Event subscription, source
  publication and accepted EventInput, the existing trusted Actor
  queue/wake bridge, original budgeted child Turn and result Event.
  Same delivery enqueue replay deduplicates; original source Event ID
  becomes descendant causal root, hop increments once, actual
  delegation attempts account for the one extra Turn. Next ordinary
  continuation has no stale causal root/hop. Existing legacy coalesced
  wake, ordinary continuation and stable child close still exercised.
  Generic Event scheduling beyond that trusted bridge remains out of
  scope. First test observation incorrectly opened an active child
  Session; use the original branch-settlement observation, and the
  source Event's actual ID (fresh root is implicit until delivery).
- Correct the known durable-only non-workspace binary compilation
  failure in the same child adapter file: the workspace-only private
  artifact state directory assignment now has its existing workspace
  feature guard. Service::new still denies tool-enabled children
  without workspace support. No runtime permission change.
- Separate exact 38-file formatting commit from functional correction.
  No additional formatter churn, reserved UX plan changes, reset,
  stash, destructive cleanup, or unrelated feature changes.

Validation: Agent core journal integration 14/14; Store Event causal/
cutover 2/2; original Actor queue 1/1; Agent Jobs wake/disclosure 1/1;
stable child lifecycle 2/2 (including actual accepted EventInput and
ordinary subsequent Turn). Strict all-feature/all-target Core/Store/CLI
Clippy, durable library, minimal all-target, full binary build and
scoped rustfmt/diff passed. Reproduced durable-only missing workspace
field compile error, restored feature guard, then durable-only
all-target check passed (existing feature-specific dead-code warnings);
final strict Clippy, child lifecycle 2/2 and full binary build passed.
No whole-workspace full test suite, pinned Nix/native-platform
qualification or unrelated inventory-test rework claimed.

Next: the earlier journal/resource CLI dirty-path blocker is resolved
by this reconciliation. Recheck current ownership at the next turn
and prioritize original live source-aware Actor/Turn admission under
independent current host authority. Do not infer active concurrent work
from historical dirty-path lists. Phase 3 remains underway; Store v60,
Git recovery v4 unchanged. No new physical containment proof.

## Previous local implementation delta — 2026-11-26: authenticated Sandbox source setter

Parent: `6cdfd88d9fc65792abc4f6216d0016ab34b7bc60` (source
release), signature verified. Original Agent/Turn journal and resource
CLI owners remain occupied. Independent production host surface,
L5 only. No source-bearing runtime activation.

- Complete the narrow Git-backed unused-Sandbox portion of sealed
  SPEC §25.4 `sandbox.sources.set` through the existing authenticated
  local stdio transport (`sandbox_sources_set`). Explicit workspace-
  feature startup `--agent-sandbox-source-approval <file>` requires
  `--agent-actor-owner` and the existing exact local SQLite Session,
  tools-off/no MCP/subagents/extensions guard. Default disabled.
  Real Host verifies original Actor/Agent and writer epoch at actual
  dispatch; the original Store checks current Project authority and
  quiescent/unused isolated Sandbox. Dispatch serialized **after**
  transport request-ID/budget/parameter gates, not speculative
  concurrent input reader execution. Hello advertises only the
  explicitly enabled setter; no new model tool or resource owner.
- Exact wire params `{ref,name,binding,rev}`. Explicit null is named
  removal; omission refuses. Non-null
  `binding:{source:{driver:DriverKey,source_ref},checkpoint:"required",
  access:"ro"|"rw"}` is only the opaque typed source identity and
  requested maximum policy. Nested and top-level unknown fields
  refuse; no version, path, retention locator, configuration selector,
  destination Project, physical target, attach/replace override or
  recovery/continuation mode from RPC. Independent host selects
  exact incoming retained version and attach/replace mode; match
  every logical field and original positive revision. No implied
  SourceRef disclosure or mount/runtime grant.
- Reuse the existing private v1 attachment/replacement or named-release
  approval file, no new policy schema. Read exactly one strict typed
  variant under its existing bounded exclusive advisory file lease
  at activation and anew per request; retain actual lease through
  the original Host/Store effect, publication/Unknown or terminal
  replay. Current approved file may be renewed by trusted cooperating
  host writer; invalid/missing/busy or mismatching policy fails closed.
  Original canonical current Project and Store scope/manager matched.
  Approval, destination and origins must not overlap bound Agent
  execution paths, in addition to existing Store/approval exclusions.
  Actual driver/pins verify original source/materialization identities
  and whole layout, with extra bound layout exclusions. Host still
  independently excludes other users, physical aliases/mounts,
  ancestor/external writers and backing/retention/private-state users
  through operation and interruption; no new kernel containment proof.
  Closed-retention approval is not a setter approval. Same shared
  loaded approvals and existing materialize/release owners used by
  offline commands; do not duplicate physical implementation.
- Both incoming effect modes share original semantic receipt namespace
  `rpc:stdio:sandbox-sources-set:<request-id>`. Exact existing fingerprint
  includes Sandbox, revision, complete host-selected anchor and attach/
  replace mode; replay cannot substitute a new version, mode or target.
  Only fresh or exact Applied replay in RPC; unresolved or Rejected
  refuses. Existing offline `sandbox-source-attach` / `sandbox-source-replace`
  accept explicit `--agent-rpc-invocation` only with recover, abandon or
  replacement continuation, using the original raw RPC ID and approval.
  No replan or fresh effect under this namespace. Removal retains
  original name/revision/descriptor release claim and immutable fact,
  including safe exact reconciliation and settled replay; no extra
  request ledger. Current redacted bounded Sandbox snapshot response
  only. Original Agent/admission/queue, grant and history unchanged.
  No schema or driver recovery format change.

Validation: real stdio integration 3/3 (including approved attach,
replacement, final release, wrong/nested/duplicate-ID refusal, exact
terminal replay and original-ID interrupted attachment recovery after
Store reopen); existing offline Git lifecycle 1/1; headless protocol
8/8. All-feature/all-target strict CLI Clippy, durable library check,
minimal all-target check (two pre-existing dead-code warnings), full CLI
build, actual approval/Actor dependency and explicit recovery-only flag
smoke, scoped fmt/diff passed. Initial fixture retained an external
shared tool-router reference and prevented exclusive Store reopen; use
fresh tool options per test host, then restart passed. Strict Clippy
required boxing both cold approval variants; final grouped stdio run
passed. Unchanged Store/Host suites and pinned Nix/native-platform
qualification not rerun; existing durable-only binary blocker remains.
No physical containment promotion.

Next: exact live source-aware Actor/Turn admission through original
journal owners when available. Otherwise narrow actual host
`sandbox.create` production consumer or original independently
authorized native artifact/Process recovery proof acquisition.
The narrow Git attachment/replacement/release setter and offline
lifecycle are now actual host consumers; do not duplicate them or
enable recovery from untrusted request-selected paths. General mixed/
unversioned baselines, linked-checkout policy, bound-Agent structural
mutation, source-bearing Actor/Process admission and remaining
checkpoint/Sandbox host surfaces remain held.

## Previous local implementation delta — 2026-11-26: exact offline Git source release

Parent: `0110a2c09d5a4fcaaa1c8c2bc8ded4acd8baf0be` (source
replacement), signature verified. Original Agent/Turn journal and resource
CLI owners remain occupied. Independent unused-Sandbox named release,
including final-source removal, L5 only; no runtime activation.

- Production workspace-feature `aw sandbox-source-release --state-dir
  <existing private Store> <approval.json> <canonical-Sandbox> <source-name>`
  consumes the existing structural release owner. Same original
  Sandbox/name/CAS revision identifies the operation; no invented
  invocation ID, extra receipt or ledger. Current Project writer and
  exclusive Store owner must observe an exact open unused isolated
  Sandbox; original Applied construction/attachment/replacement descriptor
  and complete retained set must remain available. Existing source
  mutation barrier claims the full old set before the driver effect;
  exact layout/source and writer observations revalidated through commit.
  Same original release claim may reconcile after interruption; no
  general retry or source adoption. Failed effects or failed final
  publication retain the old binding set/revision and Unknown hazard.
  No separate recover flag: re-submit the exact original approved
  removal; original coordinator/driver distinguishes first claim and
  exact reconciliation. Changed, ambiguous or shared materialization
  stays held; no force delete, prune, broad backing GC or arbitrary
  physical locator. Final release proves named parent empty, retains
  it and exact layout identity, and removes just the binding at one
  Sandbox revision advance. No Agent creation/activation.
- Share existing offline closed-Sandbox maintenance private approval:
  v1 `{version,store_directory,store_id,store_generation,manager,sandbox,
  sandbox_rev,destination_root,destination_identity,repositories,source}`.
  Named removal requires `source` exactly equal to the command source
  name; closed retention requires absent/null source, never consumes
  named-removal approval. Exact canonical target and OS manager,
  current Store scope, full original named source locations and actual
  registered Git driver. Reuse the existing lease-backed pinned named
  layout and independent host exclusion of all users/writers, backing/
  retention users, aliases, mounts, other Stores through operation and
  interruption. Only ordinary private origin checkouts and existing
  private named-child layouts with cwd=tool_root; all siblings' exact
  identity verified without discarding their edits. No implicit
  disclosure, capture/restore, retention or execution grant.
- `SessionStore::sandbox_replay_source_release` returns None for the
  original current CAS (normal admission avoids history lookup).
  For a later revision, current Project View and exact canonical
  Sandbox required; the existing entity-indexed commit history must
  contain the exact original local `sandbox.sources.changed` release
  fact (`sbx-release:` identity, selected source_name, expected_rev+1,
  released=true). Metadata-only changes, replacement provenance,
  later names/revisions, or missing source alone cannot acknowledge
  release. Successful replay returns current redacted snapshot, before
  selecting the Git driver or probing the physical layout. It still
  requires the independent original semantic approval and current Store
  scope. A subsequent attachment at the same name is never touched.
  Original strict-CAS SDK release remains compatible; no schema,
  provenance index, operation ledger, recovery format or new resource
  owner. New command only invokes original Host and Store owners.

Validation (L5, Rust 1.95.0): production all-feature binary type-check;
grouped real offline Git/SQLite lifecycle 1/1 (81.56s), Store Sandbox
owner 4/4, Host source-checkpoint lifecycle 1/1 (167.01s). Existing
grouped fixture proves separate named/closed cleanup approvals, wrong
original origin and foreign layout refusal, modified worktree preserved
under unresolved release, exact repair/restart reconciliation, final
empty parent preserved, one revision advance, wrong name/revision and
replacement-fact replay refusal, exact replay after same-name
reattachment without touching the new materialization, and terminal
replay without installed Git or physical origins. Original retention
refs, other Sandbox content, Agent and public checkpoint lists preserved.
Existing Host multi-source interrupted physical removal, sibling
preservation and exact release-fact replay passed. Strict affected
all-feature/all-target Clippy, durable library/minimal all-target checks,
full binary build, actual source-release/retention help and required
args/unsupported continuation smoke passed. Scoped fmt/diff and signed
exact commit checked. Prior durable-only non-workspace binary blocker,
feature-specific warnings and broad CLI inventory discrepancy remain;
unchanged Process suites and pinned Nix/native-platform qualification
not rerun. Only isolated fixture effects; no real user source mutation.

Next: exact source-aware Actor/Turn admission through original journal
owners when available. All narrow offline standalone Git creation/
derive, unused-Sandbox attach/replace/continue/release and closed
retention consumers are now landed. Do not proliferate alternative offline
commands. If original journal ownership remains blocked, prioritize
remaining genuine production authority/containment or other sealed
Phase 3 owner boundaries. Broader mixed/unversioned assets, linked
checkout policy, task-bearing/cross-Project effects, protected host
RPCs, bound-Agent structural mutation and source-bearing Actor/Process
admission remain held.

## Previous local implementation delta — 2026-11-26: exact offline Git source replacement

Parent: `6974a4b35ad5281326eb1237bb846c0c7ce74029` (source
attachment), signature verified. Original Agent/Turn journal and
resource CLI owners remain occupied; independent unused-Sandbox
replacement consumer, L5 only. No source-bearing runtime activation.

- Production workspace-feature `aw sandbox-source-replace --state-dir
  <existing private Store> <approval.json> <canonical-Sandbox> <request-id>`.
  Shares attachment's original Project SandboxSourceAttach Prepared/
  barrier/inspection/publication owner and existing replacement
  fingerprint (Sandbox, original expected rev, exact incoming anchor,
  separate replace domain). Stable namespace
  `cli:sandbox-source-replace:<request-id>`.
  Mutually exclusive `--agent-recover` (inspect only),
  `--agent-abandon` (reject only an intact exact original predecessor
  with no incoming registration), and `--agent-continue`
  (explicit driver-proven post-release continuation of the original
  operation). No fresh preparation or rematerialization in normal
  recovery. After a genuine post-release gap the old logical binding
  and original revision remain fenced; only exact continuation may
  repair the gap, never abandonment of that old row. Fresh retries
  of unresolved/Rejected claims refuse. Terminal Applied or Rejected
  replay requires original semantic approval, not physical layout
  or installed Git, and never touches later resources.
- Reuse attachment private v1 approval fields; replacement additionally
  **requires** `replaced_repository: GitSourceLocation` naming the
  selected incoming anchor's name but the exact original origin, and
  optionally `allow_continuation: true` (default false). The final
  `repositories` set contains all unchanged sibling origins plus
  incoming origin. The old origin decision is explicit disposal
  permission, not inferred from a binding or incoming anchor. The
  original host authenticates current OS manager, exclusive Store
  generation, Project writer, exact open unused Sandbox revision and
  complete protected named-child layout. Current independent authority
  must cover both origins, retained versions, administration,
  materializations and other backing dependencies; all existing
  ordinary private-checkout/disjoint-layout/external-writer exclusions
  apply. Attachment rejects replacement-only approval fields (a null
  old origin and false continuation retain legacy semantics).
  `--agent-continue` requires a renewed approval explicitly enabling
  continuation; already Applied replay does not need this additional
  effect permission. No extra request scope or persistent continuation
  authority, no generic retry or prune. Replaced location is pinned
  in the same lease-backed `PinnedGitLayout` at the already selected
  destination and parent identity; the actual driver verifies original
  Applied descriptor and unchanged predecessor, incoming version,
  exact sibling identities and final predecessor release. No new
  retention acquisition, deletion of baseline refs, or inferred grant.
- `sandbox_recover_attachment_exact` shares the original receipt
  recovery before any reclaim; exact request checks revision, anchor
  and operation fingerprint. The existing exact inspection wrapper
  delegates to it. `continue_sandbox_replacement_exact` shares
  the existing driver proof/continuation, rejects attach mode before
  reclaim and holds the actual renewed fence through revalidation,
  driver effect/inspection, final publication or Unknown. Untouched
  predecessor, modified or ambiguous incoming, unaccounted backing,
  changed descriptor or missing independent authority refuses. Actual
  driver reuses original candidate and existing exact partial cleanup;
  completed incoming is preserved. Successful publication advances
  the original Sandbox and selected binding once, preserves siblings
  and layout, and records existing release provenance. No new schema,
  driver recovery format, receipt, Agent, checkpoint, grant, tool,
  resource owner or RPC.

Validation (L5, Rust 1.95.0): production all-feature binary type-check;
grouped real offline Git/SQLite lifecycle 1/1 (79.81s), Store Sandbox
owner 4/4, Host source-checkpoint lifecycle 1/1 (197.79s). Existing
grouped fixture proves real independent different-backing replacement,
modified predecessor and wrong old-origin refusal, attach/replace
approval separation, untouched original candidate continuation refusal
and exact abandonment, fresh replacement with one revision advance,
post-release gap preserves original logical binding/revision and rejects
inspect-only recovery/abandonment, default-deny continuation, changed
request refusal before reclaim, explicit renewed continuation after
restart, terminal replay without Git/physical origins and old Rejected
replay after later publication. Original and incoming retained refs,
original HEAD/index/content, layout, Agent and public checkpoint lists
preserved. Existing Host multi-source/partial replacement recovery
regression passed. Strict affected all-feature/all-target Clippy,
durable library/minimal all-target checks, full binary build, actual
replacement and attachment help, required args, continuation conflict
and attach continuation rejection smoke passed. Scoped fmt/diff and
signed exact commit checked. Prior durable-only non-workspace binary
blocker, feature-specific warnings and broad CLI inventory discrepancy
remain; unchanged Process suites and pinned Nix/native-platform
qualification not rerun. Only isolated fixture effects; no real user
source mutation.

Next: exact source-aware Actor/Turn admission when original journal
owners are available. Otherwise production exact source release
(including final-source removal) using the original release claim
and factual provenance, including replay after settlement without
physical reinspection. Standalone creation/derive, attachment and
replacement are now actual offline consumers; do not repeat them.
Broader linked checkout policy, mixed providers, ephemeral assets,
bound-Agent structural mutation, source-bearing Actor/Process
admission and Sandbox host RPCs remain held.

## Previous local implementation delta — 2026-11-26: exact offline Git source attachment

Parent: `8532f5e12a02477e22c4cb9ab5938c076935d39a` (standalone
retained-version Sandbox baseline), signature verified. Original Agent/
Turn journal and resource CLI owners remain occupied. Independent
unused-Sandbox attachment consumer, L5 only; no Agent/runtime activation.

- Production workspace-feature `aw sandbox-source-attach --state-dir
  <existing private Store> <approval.json> <canonical-Sandbox> <request-id>`
  uses the original SandboxSourceAttach Project receipt and existing
  named-source publication. Mutually exclusive `--agent-recover`
  (inspect/publish original effects) and `--agent-abandon` (reconcile
  only the original unpublished incoming worktree). Namespace
  `cli:sandbox-source-attach:<request-id>`. No replacement or removal
  is admitted by this command; occupied name refuses.
  Unlike standalone create/derive, the original attachment fingerprint
  includes exact incoming anchor and expected Sandbox revision, so
  terminal replay still requires the independent private approval
  containing those semantic inputs. Applied replay returns the current
  redacted Sandbox snapshot, no driver/layout access. Rejected replay
  verifies the original indexed Prepared/current Rejected fact, not a
  later current layout; a later successful attachment may legitimately
  advance the old Sandbox revision. No cleanup can target that later
  source. Fresh unresolved/Rejected retry and Applied abandonment refuse.
- Private v1 JSON:
  `{version,store_directory,store_id,store_generation,manager,sandbox,
  sandbox_rev,anchor,destination_root,destination_identity,repositories}`.
  Manager authenticated local OS principal, active exclusive Store scope,
  original current Project writer, exact open unused Sandbox/rev
  required. `anchor` is exact existing SourceAnchor for a retained
  required Git version; no newly acquired retention, branch/HEAD
  selection or implicit source permission. `repositories` covers
  complete sorted final named layout (all old origins and incoming
  origin); exact original Applied descriptors and retained siblings
  verified by Git host. Shared private approval lease and pinned Git
  layout: cwd=tool_root=destination_root, ordinary private origin
  checkouts, disjoint existing private named-child layout. Fresh
  incoming target must be absent, foreign files/links refused; recovery
  uses original descriptors, not new path discovery. Only unused
  Sandboxes, including the empty first-source layout or originally
  managed siblings, qualify. Original Store rejects bound/shared/
  active/unresolved layouts and preserves first-source layout
  reservation plus source barrier. Independent host excludes all
  outside users/writers, aliases, mounts, other Stores, backing and
  retention users through operation and interruption; no physical
  containment claim. Approval is separate from any native execution,
  checkpoint history, retention cleanup or metadata grant.
- Reusable `SandboxMaterializationRequest {expected_rev,anchor,replace}`
  checks original semantic fingerprint before recovery reclaim;
  `sandbox_inspect_attachment_exact` and
  `sandbox_abandon_attachment_exact` share existing original receipt
  and barrier owner. Exact Rejected replay checks immutable indexed
  claim; the original transactional Rejected fact is authoritative
  even after unrelated later revisions. Existing SDK methods remain
  available. Shared Git attachment inspection/abandonment now retain
  the actual independently authorized physical guard outside the
  inspector/reconciler callback through Unknown settlement on error
  as well as successful publication/cleanup (including existing SDK
  replacement). No reexecution, adoption of a new candidate or
  physical cleanup of any Applied resource. First attachment increments
  original Sandbox rev and binding rev once, retains exact layout,
  other binding identities and source retention. No new schema,
  receipt, resource owner, Agent, checkpoint, grant, model tool or RPC.

Validation (L5, Rust 1.95.0): production all-feature binary type-check;
grouped real offline Git/SQLite lifecycle 1/1 (57.11s), Store Sandbox
owner 4/4, Host source-checkpoint lifecycle 1/1 (164.82s). Existing
grouped fixture proves exact first-source attachment, stale revision/
busy approval/foreign destination refusal, bound-Sandbox refusal before
planning, original unresolved layout reservation, edited unpublished
candidate preserved through failed cleanup, exact repair/restart
abandonment, subsequent fresh attach and old Rejected replay without
touching the new binding, changed anchor/revision/replacement request
refusal before inspection, original interrupted candidate publication
after restart, terminal replay with original approval but no Git or
physical origin. Parent content/HEAD/index, retained refs, Agent and
public checkpoint lists unchanged. Existing Host multi-source,
replacement and partial cleanup regression passed. Strict affected
all-feature/all-target Clippy, durable library/minimal all-target
checks, full binary build, actual CLI help/required args and mutually
exclusive mode smoke passed. Scoped fmt/diff and signed exact commit
checked. Prior durable-only non-workspace binary blocker, feature-specific
warnings and broad CLI inventory discrepancy remain; unchanged Process
suites and pinned Nix/native-platform qualification not rerun. Only
isolated fixture effects; no real user source mutation.

Next: exact source-aware Actor/Turn admission when original journal
owners are available. Otherwise production unused-Sandbox replacement
(including separately authorized post-release-gap continuation and
original old+incoming origin fences), or exact final-source release
through original release claim and factual event. Attachment is
now a real offline consumer; do not duplicate it. Broader linked
checkouts, mixed providers, ephemeral assets, bound-Agent structural
mutation, Sandbox host RPCs and source-bearing Actor/Process admission
remain held.

## Previous local implementation delta — 2026-11-26: offline retained-version Project Sandbox creation

Parent: `bab8987c86260e906a8266f5c4763c0615698a77` (standalone
Git derive), signature verified. Original Agent/Turn journal and resource
CLI owners remain occupied; independent versioned baseline consumer,
L5 only. No source-bearing runtime activation.

- Production workspace-feature `aw sandbox-create --state-dir <existing
  private Store> <approval.json> <canonical-Project> <request-id>`
  uses the original Project-scoped SandboxCreate Prepared/inspection/
  publication owner, with from=None. Explicit mutually exclusive
  `--agent-recover` inspects/publishes original effects and
  `--agent-abandon` reconciles only unpublished original materializations.
  Stable namespace `cli:sandbox-create:<request-id>`. Applied replay
  returns the original current redacted Sandbox before config/driver
  selection; Rejected abandonment verifies original claim/candidate
  absence. Fresh unresolved/Rejected retry, Applied abandonment,
  substituted Project/request and absent original recovery refuse.
  Neither host-selected layout nor baseline is a public request selector.
- Strict private approval JSON v1:
  `{version,store_directory,store_id,store_generation,manager,project,
  baseline,binding,destination_identity,repositories}`.
  Manager is authenticated OS principal; current Project writer and
  active exclusive Store scope required. `baseline` is a canonical
  `SourceSet {coverage:"selected", anchors:[SourceAnchor...]}`
  selecting nonempty complete required retained Git versions, not
  mutable HEAD, branch names or unversioned/ephemeral assets. Each
  anchor is the existing typed driver/schema/name/source_ref/access/
  checkpoint_policy/identity/retention/fields descriptor, checked by
  GitSourceAnchor conversion and actual driver retention/content
  verification. The original host independently approves exact metadata,
  backing objects, retention and materialization for this destination
  Project; no history grant or existing checkpoint ID confers this
  authority. Retained versions may be independently selected from
  existing retention, but no new retention acquisition/ownership is
  fabricated. Binding has cwd=tool_root; ordinary private origin
  checkouts and existing disjoint private named-child destination only.
  Same bounded config lease, pins and exact destination identity; host
  excludes all other writers/users, aliases, mounts, other Stores and
  retention users through execution, inspection, publication and
  interruption. No global physical containment or autonomous policy.
- `SandboxRecoverySelection::{Baseline(&SourceSet),Parent{reference,rev}}`
  shares exact baseline/parent + layout check in original Store recovery
  before claiming the original barrier or running driver inspection.
  Original exact derive SDK wrappers delegate; generic SDK APIs remain
  compatible. Baseline anchors sorted by name at approval loading as
  at original preparation; compare complete original set including
  immutable retention identity, access/policy/schema/fields. Same
  `resume_sandbox_create_exact` and
  `abandon_sandbox_create_exact` Git host paths, holding approval
  through failed inspection/Unknown as well as final publication or
  cleanup. No additional receipt format or recovery ledger. Baseline
  abandonment does not release the independently retained anchor.
- Consolidate duplicate offline Git layout guard structs into one
  `PinnedGitLayout` borrowing the original file lease and owning the
  actual directory pins, with shared destination lexical preflight.
  Existing checkpoint restore, Sandbox derive and closed-Sandbox
  retention use the same guard. No new physical authority from this
  refactor. Original Prepared, source barrier and Git materialization
  ownership unchanged. No new Agent, history, checkpoint, grant, model
  tool, RPC or schema.

Validation (L5, Rust 1.95.0): production all-feature binary type-check;
grouped real offline Git/SQLite lifecycle 1/1 (46.89s), Store Sandbox
owner 4/4, Host source-checkpoint lifecycle 1/1 (160.41s). Existing
grouped fixture proves missing/mismatched retained version refusal,
held approval refusal, exact fresh baseline despite changed mutable
origin, Applied replay, changed original baseline/layout recovery
refusal before inspector, original child publication after restart,
exact unpublished abandonment/rejection and no retry, unchanged
baseline retention refs/parent/Agent/checkpoint lists and original
HEAD/index/content, terminal replay without config/driver/repo.
Existing Host multi-source construction and partial cleanup passed.
Strict all-feature/all-target Store/Host/CLI Clippy, durable library
and minimal all-target checks, full binary build, actual help/required
args and mode-conflict smoke plus derive help, scoped fmt/diff passed.
Prior durable-only non-workspace binary blocker, feature-specific
warnings and broad CLI inventory discrepancy remain; unchanged Process
suites and pinned Nix/native-platform qualification not rerun.
Only isolated fixture effects; no real user Sandbox creation or cleanup.

Next: exact live source-aware Actor/Turn admission when original journal
owners are available. Otherwise independent unused-Sandbox attachment/
replacement and final-source release with original Prepared descriptors
and current exact layout authorization. Standalone retained-version
creation and mutable parent derivation are now actual offline consumers;
do not duplicate them. Empty/unversioned/mixed baseline effects, linked
checkout policy, Sandbox host RPCs, bound-Agent structural mutation
and source-bearing Process/Actor admission remain held.

## Previous local implementation delta — 2026-11-26: exact offline standalone Git Sandbox derivation

Parent: `8e0c950332afd210ffe9cfe7b5ee7c656f9737b7` (source identity
metadata), signature verified. Original Agent/Turn journal and resource
CLI owners remain occupied. Independent standalone Sandbox constructor
consumer, L5 only; no source-bearing runtime activation.

- New workspace-feature offline `aw sandbox-derive --state-dir <existing
  private Store> <approval.json> <canonical-parent-Sandbox> <request-id>`.
  `--agent-recover` inspects/publishes the original candidate;
  `--agent-abandon` reconciles only unpublished effects. Modes are
  mutually exclusive. Parent supplies exact canonical Project; no redirect.
  Stable invocation namespace `cli:sandbox-derive:<request-id>` uses
  original SandboxCreate fingerprint (Project/from/request), no second
  constructor, ledger, receipt or candidate. Layout and parent revision
  are independently selected host inputs, not client request parameters.
  Applied replay before approval/driver selection returns the original
  current redacted Sandbox snapshot. Rejected cleanup replay verifies
  original source claim and candidate absence, without config/driver.
  Fresh retry of unresolved/Rejected, and abandonment of Applied, refused.
- Private v1 JSON: `{version,store_directory,store_id,store_generation,
  manager,parent,parent_rev,binding,destination_identity,repositories}`.
  Manager is authenticated local OS principal; original Project writer,
  exclusive Store owner and exact Store scope required. `binding` is
  existing SessionBinding (only cwd/tool_root are Sandbox layout facts),
  cwd=tool_root; destination identity is [volume,object]. Each repository
  is existing GitSourceLocation {name,repository,repository_identity}.
  Exact current open parent revision pins complete observed required
  bindings, sorted exact names and actual registered Git implementation.
  First CLI cut admits only ordinary private origin checkouts at parent
  root (one source) or named children, and fresh existing private
  destination parent with named child targets. Parent cwd=tool_root;
  layouts and approval/Store directories disjoint. Reuse bounded private
  approval lease, ordinary checkout and named-layout pins; reject
  unrelated files and mismatched origins/identity. Host must independently
  exclude all other users/writers, mounts, aliases, Git backing/retention
  users, Store and approval access through operation and interruption.
  Advisory leases/observations do not establish physical containment.
  Linked parent worktrees and other layouts remain outside this CLI cut.
- Original Store `sandbox_prepare_derive_exact` checks approved parent
  revision in the initial transaction before the planner; original
  immutable parent observation and layout/writer fences are rechecked
  before Prepared and publication. Exact inspection/abandonment check
  original stored parent revision and Sandbox layout digest before
  reclaiming the original all-source claim. The same original
  SandboxCreateBoundary, candidate, descriptors, source mutation barrier,
  inspection, held publication and cleanup owners remain. Host shares
  the existing derive/capture+restore implementation; recovery holds
  independent approval through failed inspection/Unknown as well as
  success. No reexecution in recovery, no public checkpoint, Agent,
  queue or grant creation. Derived child uses original private retention
  and materialization ownership. Parent content/HEAD/index unchanged.
  Ambiguous/modified worktree or uncertain scratch remains held.
- No new schema, driver recovery format, Sandbox namespace, metadata
  disclosure, RPC method, model tool or execution authority. Existing
  generic trusted SDK construction and cleanup methods remain available
  through the same owner. Exact terminal cleanup additionally checks
  the original reconciled claim rather than trusting a Rejected enum.

Validation (L5, Rust 1.95.0): production all-feature binary type-check;
grouped real offline Git/SQLite lifecycle 1/1 (46.99s), Store Sandbox
owner 4/4, Host source-checkpoint lifecycle 1/1 (164.74s). Existing
grouped fixture proves stale parent rejection before planning, held
approval lease/foreign destination refusal, real isolated dirty child,
Applied replay, exact wrong parent revision/layout recovery refusal
before inspection, original completed child publication after restart,
modified unpublished checkout preserved with hazard, exact repair and
restart cleanup, terminal rejection/no retry, published children intact,
unchanged parent/Agent/checkpoint lists and source HEAD/index/worktree,
terminal replay without config/driver/repo. Existing Host multi-source
and partial cleanup regression passed. Strict all-feature/all-target
Store/Host/CLI Clippy, durable library/minimal all-target checks, full
binary build, actual help/required args and mutually exclusive mode
smoke, scoped fmt/diff passed. Prior durable-only non-workspace binary
blocker, feature-specific warnings and broad CLI inventory discrepancy
remain; unchanged Process suites and pinned Nix/native-platform
qualification not rerun. Only isolated fixture effects; no real user
Sandbox creation/cleanup or publication.

Next: exact live source-aware Actor/Turn admission if the original journal
owners are free. Otherwise independent standalone versioned baseline
construction or original unused-Sandbox source attachment/replacement
under exact approval and Prepared custody; never bypass bound-Agent
quiescence or original effects. Do not repeat standalone derivation,
offline checkpoint/retention or metadata wiring. Broader linked-layout,
ephemeral/mixed baselines, original Sandbox host RPCs and source-bearing
runtime/Process containment remain held.

## Previous local implementation delta — 2026-11-25: independent source identity metadata disclosure

Parent: `bcd18323c83f33a7d609d5d11273f1031bc64216` (Vault entry metadata),
signature verified. Original Agent/Turn journal and resource CLI owners
still occupied; independent source metadata consumer, L5 only.

- Close the protected identity portion of SPEC §§18.5 and 25.4 on
  existing local stdio `sandbox_get`, `sandbox_sources_list`,
  `checkpoint_get`, `checkpoint_list`. No new RPC method/selector,
  model tool, source execution, driver call, grant, schema or resource
  owner. `sandbox_list` remains its compact redacted summary page.
  Default unconfigured responses remain unchanged; other resources/
  named sources stay redacted. No extension Views or provider-specific
  recovery fields disclosed.
- Explicit local startup `--agent-source-metadata-approval <file>`
  requires the original exact SQLite Actor-owner/stdio/tools-off binding.
  Private bounded JSON v1: `{version:1,store_directory,policy:{store_id,
  store_generation,actor,actor_user_id,project,sources:[...]}}`.
  Each selection is either
  `{kind:"binding",sandbox,sandbox_rev,binding_rev,source:{name,driver,
  schema,source_ref}}` or `{kind:"anchor",checkpoint,source:{name,
  driver,schema,source_ref,version}}`. The `version` is the exact
  immutable SourceAnchor.identity, not a locator or caller assertion
  of availability. Store generation is the original restore generation,
  actor_user_id the original stable user. No wildcard, duplicate,
  arbitrary provider fields, materialization/retention paths, or
  new grant. Selection count bounded by existing hard source limit,
  file 1 MiB. Host independently approves these exact metadata identities
  for this subject/current Project, not merely because it can manage
  a Sandbox or read history. Policy stays outside Store and execution
  layout; host excludes other access, aliases, ancestor and external
  writers. Existing private approval loader/lease extracted from Git
  maintenance and reused, no second file-policy mechanism. File is
  reread under its actual exclusive advisory lease per disclosure read
  and held through complete Store snapshot, JSON projection and size
  check. Cooperating policy writers honor same inode lease; no replacement
  while leased. Policy edits/empty selection govern future reads without
  a Store write. Invalid/missing/busy/stale-scope configured approval
  fails closed; no previously approved metadata cached or replayed.
- Reusable Store `SourceMetadataApproval` validates the exact typed
  selection and bounded scope; `SourceMetadataView<T>` is deliberately
  nonserializable and returns the original snapshot plus exact approved
  identities. The Store checks current Project View or original
  checkpoint source Agent View, stable principal/user and Store scope
  inside the same read transaction; checkpoint pagination/cursor
  semantics remain unchanged. Binding match includes exact open
  Sandbox and binding revision, name/driver/schema/source identity;
  anchor match includes live checkpoint identity and exact immutable
  version. No extra provider lookup, no backing availability assertion.
  Host output adds optional `source` (typed name/driver/schema/source_ref
  and anchor version only) to the matching redacted source summary.
  Closed Sandbox/retired checkpoints do not disclose retained private
  custody. Reads remain meaningful with an unavailable driver/repository
  when the independent metadata decision still covers the immutable
  identity. No SourceRef use/materialize permission, raw locators or
  secret bytes returned. No retroactive recall of prior disclosure.

Validation (L5, Rust 1.95.0): grouped real Git/SQLite offline
checkpoint/Sandbox lifecycle 1/1, actual configured stdio tests 2/2,
Store checkpoint 4/4, Store Sandbox 4/4. Exact binding/version projection,
default and stale-selection redaction, wrong stable-user scope denial,
policy lease contention, no inherited checkpoint permission, explicit
additional checkpoint selection and retired-anchor redaction, live
immutable anchor disclosure after the driver/repository disappear,
policy withdrawal and unchanged source grants/Agent revision are covered
in existing grouped fixtures. Actual bound Host accepts explicit policy
selection and refuses duplicate activation. Initial compile caught
SourceName's intentionally unordered type as a BTreeMap key; corrected
to existing name strings without changing core type semantics.
Strict all-feature/all-target Store/Host/CLI Clippy, durable library
and minimal all-target checks, full binary build, actual startup
dependency smoke and scoped fmt/diff passed. Previous durable-only
non-workspace binary blocker, feature-specific warnings and broad
CLI inventory discrepancy remain. Unchanged Host physical/Process
suites and pinned Nix/native-platform qualification not rerun.
Only isolated fixture identities disclosed; no real user source
disclosure, mutation or publication.

Next: exact live source-aware Actor/Turn admission through original
journal owners when available. Otherwise take another substantial
independent boundary (actual physical recovery/containment or original
Sandbox construction/attachment host composition). Both Vault and
source identity metadata now have actual independent host consumers;
do not repeat metadata or offline lifecycle wiring. Remaining
provider-specific extension Views and broader metadata authority,
production Process projection/containment and source-bearing admission
remain held. Prior L5 strict Linux containment restriction unchanged.

## Previous local implementation delta — 2026-11-25: exact provider-authorized Vault entry metadata

Parent: `7f9a2f6116ef3eb0472412ad7e648c3b8d1fad69` (live signing broker),
signature verified. Original Agent/Turn journal and resource CLI owners
still occupied; independent protected Vault metadata consumer, L5 only.

- Production Process dispatch was not promoted: existing source Process
  consumes original journaled ToolStarted and source custody; no live
  source-aware Actor admission without the original journal owners. Actual
  L5 execution environment has cgroup v2 mounted read-only, no effective
  capabilities, NoNewPrivs=1; no independently qualified closed-descendant
  Linux scope. Existing direct Unix scope remains best-effort; strict
  containment continues to refuse. Frozen dotlink Sandbox is untouched.
- Close the corresponding VaultProvider metadata portion of SPEC §28.10
  on the existing authenticated `agent_vault_list` stdio surface. No new
  RPC method, selector, capability registry or flag. Existing configured
  signing.json v1 entry gains optional boolean `disclose_entry_ref`
  (default false; managed provisioning also defaults false). This is a
  separate provider-owned disclosure decision for that entry's configured
  exact manager, not inferred from the attach/use grant or broker startup
  capability. Existing configs and unapproved entries retain the original
  redacted output. The existing explicit local provider activation grants
  redacted binding metadata for its exact current Agent; the provider
  must independently approve each additional entry reference.
- Reuse the original bounded Store list, exact canonical Agent and current
  Manage check, same SQLite snapshot, actor/Store scope and access-generation
  revalidation. `VaultGrantView` contains the original flattened summary
  plus optional typed `entry: VaultEntryRef` (provider ID and opaque entry
  ID), omitted when unauthorized. `agent_vault_list_signing_metadata`
  obtains the real registered provider, holds its read guard through the
  Store snapshot and projection, and matches exact live policy, manager
  principal AND stable user, current grant, lifecycle and supported
  projection. Revoked/rotated, stale ACL, wrong manager/provider and
  old-provider-after-restart grants stay redacted; no key read or entry
  enumeration. Base summary's revalidation_required still describes the
  Store observation, not live provider-use authority. Reattach is still
  an explicit independent mutation. No scopes, authority/secret/backend
  locators, key bytes, history/ACL inheritance, or new persistence.
  Redacted generic SDK `agent_vault_list` remains compatible.
- Metadata disclosure does not authorize execution, and the host's provider
  configuration exclusion contract still applies. Changing configuration
  requires the existing live withdrawal and explicit activation; already
  disclosed metadata cannot be recalled.

Validation (L5, Rust 1.95.0): Store Vault signing 2/2 and actual
configured Host/stdio Vault lifecycle 1/1. Existing grouped fixtures
prove default-redacted legacy config, opt-in exact typed entry projection,
separate host metadata denial, ACL broadening/regrant and Manage-only
peer redaction, provider withdrawal, restart without regrant, and
current exact reattachment; no protected scope/key/authority locator or
revision fanout. Initial fixture moved a borrowed provider key; corrected
with a clone. Strict Store/CLI all-feature/all-target Clippy, durable
library and minimal all-target checks, full binary build, real CLI help
and scoped fmt/diff passed. Unchanged Process and headless suites not
rerun this slice. Prior durable-only non-workspace binary field blocker,
feature-specific warnings and broad inventory discrepancy remain;
pinned Nix/native-platform qualification not rerun. Only private test
provider entries used; no real user Vault disclosure or publication.

Next: exact live source-aware Actor/Turn admission if the original journal
owners are free. Otherwise protected SourceBinding/checkpoint anchor
metadata under an independent exact disclosure decision, remaining
physical recovery or actual Process projection/containment only when
its authority can be established. The configured signing broker and
provider-authorized Vault binding metadata are now actual consumers;
do not repeat their wiring or enable unsupported Process execution.
Socket/file/env production dispatch remains held.

## Previous local implementation delta — 2026-11-25: explicitly selected live signing broker

Parent: `29e4d4d9c60ac8d762607305a68f5968d62b3e4e` (closed Git Sandbox
retention), signature verified. Original Agent/Turn journal and resource CLI
owners still occupied; independent provider-specific broker consumer, L5 only.

- The already configured signing Vault now has a real synchronous production
  broker consumer for the authenticated local stdio Actor host. Explicit
  startup selection `--agent-signing-broker-entry <entry>` requires the
  existing `--agent-signing-vault-root`, exact SQLite Session/Actor-owner
  and tools-off constraints. The Vault binding/metadata port alone does
  not enable the service. No new provider, key configuration, model tool,
  generic Vault invoke, remote transport or Agent activation.
- The reusable `aw_store::SigningBroker<S>` pins the exact selected provider
  registration and policy identity (entry, name, scope, actor, Agent,
  authority generation, Store scope) and owning writer epoch. The cloned
  registry is the same original owner; each synchronous operation creates
  its own non-Sync reader, without an extra registry or mutex. Selection
  checks the actual configured provider, exact actor/Agent, open admission,
  Submit, Store scope, local Session owner/writer epoch and current key
  version, but issues no grant. Every operation reacquires the actual
  provider guard, rejects any different decision, and calls the existing
  shared HMAC broker with the same immediate Store transaction fencing
  Agent admission, current exact grant revision/access generation, actor,
  Store scope and original writer epoch through key use and settlement.
  The original generic SDK method still delegates to that same operation
  with its existing current-writer semantics. Replacement of the selected
  provider policy requires explicit reselection, not just attaching a new
  grant. Provider restart invalidates prior grants; no automatic regrant.
- The stdio hello advertises `signing_broker_sign` only when explicitly
  selected. After the original request ID/budget and strict parameter gate,
  the service accepts only `{grant_rev, message}` (positive exact revision,
  1..65536 bytes of non-NUL UTF-8 text) and returns `{"tag":"<64 lowercase
  hex characters>"}`. The bound host rechecks its original Actor state
  and Agent binding before invoking the selected broker; scope, entry,
  key/provider identity, target, paths and projection cannot be supplied
  by the client. No key bytes, protected locators, generic decrypt or
  arbitrary output destination. HMAC encoding is unchanged:
  `aw-vault-signing-broker-v1\0 || len(scope)_BE64 || scope ||
  len(message)_BE64 || message`. This is a pure synchronous computation,
  not a durable external mutation, so no new receipt/replay owner. Same
  connection duplicate IDs remain rejected; a fresh request revalidates
  live authority every time, including after loss. The host independently
  authorizes disclosure of the derived tag to its local stdio consumer;
  relying-service freshness/replay and post-disclosure revocation remain
  outside this cut.
- No source-bearing Actor or Process admission change, additional
  schema, credential inheritance or model registry expansion. The original
  configured provider root and ancestor/alias/writer exclusion contract
  still applies. The source Process proof projection remains a lower host
  primitive; no claim that a source-bearing runtime now dispatches it.

Validation (L5, Rust 1.95.0): configured real local Host/stdio Vault
broker lifecycle 1/1, Store Vault signing 2/2, headless protocol 8/8,
Host source-Process 2/2. The existing grouped fixture now exercises the
actual opt-in broker and verifies no implicit grant, wrong selected entry
and writer epoch refusal, exact HMAC equality with the original SDK,
strict client params/size, duplicate IDs, held admission, withdrawal,
fresh provider restart requiring regrant, removal, and no Agent revision/
queue/model tool changes from signing. Strict all-feature/all-target
Store/Host/CLI Clippy, durable lib, minimal all-target checks, full CLI
build, actual broker startup flag dependency smoke and scoped fmt/diff
passed. Initial compile rejected retaining the non-Sync registry reader;
corrected to share the registry owner and acquire a reader per call.
Corrected fixture admission argument order and feature-specific trait
import. Additional durable-only all-target check exposed an existing
non-workspace `src/subagents.rs:2744` reference to absent
`Inner.state_dir` (same source on parent; parent build not rerun);
this independent owner was not modified. Durable library/minimal/full
supported gates passed; durable-only binary gate remains unqualified.
Existing feature-specific warnings remain. Pinned Nix/native-platform
qualification and broad CLI inventory discrepancy not rerun. No real
user key or provider service invoked; only fixture-owned keys used.

Next: exact live source-aware Actor/Turn admission if original journal
owners are free. Otherwise prioritize independent production Process
projection authority/containment or protected source metadata. The narrow
configured signing broker is now reachable through the trusted local
stdio host; do not duplicate its dispatch or conflate it with model or
Process authority. Socket/file projections, production Env/Process
dispatch, broader physical containment and remaining Phase-3 gates held.

## Previous local implementation delta — 2026-11-25: exact closed Git Sandbox retention reconciliation

Parent: `2291ac301201c00d09852ff3edf4637cfdcb8eff` (offline original
capture/restore abandonment), signature verified. Original Agent/Turn journal
and resource CLI owners remain occupied; independent closed-Sandbox physical
reconciliation slice, L5 only.

- Production workspace-feature command `aw sandbox-retention <approval.json>`
  operates only on an already-closed Sandbox with exact original managed
  Git materialization provenance. It never requests Agent or Sandbox closure,
  activates an Agent, discards queue custody, changes a live binding, releases
  checkpoint retention or prunes shared Git objects. Strict v1 private JSON
  fields: `version`, `store_directory`, `store_id`, `store_generation`,
  `manager`, `sandbox` (exact canonical ref), `sandbox_rev` (original
  closed revision), `destination_root`, `destination_identity:[volume,object]`,
  `repositories` (each `{name,repository,repository_identity:[volume,object]}`).
  Existing private Store owner, local OS principal and Project writer
  authority required. The first CLI cut accepts only a private named-child
  layout with cwd equal to tool_root, ordinary private origin checkouts,
  and independent approval of every retained source. It preserves the
  containing destination directory. Whole-root and other host layouts
  remain SDK-only or unsupported. Approval is distinct from native tool
  access, restore and retired-checkpoint permission.
- The original closed-source owner now exposes fenced read-only preflight
  before claiming cleanup. Recheck exact closed snapshot, retained set,
  quiescence, dependencies, registered binding schemas and current Project
  authority in the claim transaction. Denial before claim creates no new
  Unknown hazard. Existing generic trusted method delegates to the same
  owner, preserving its returned-guard semantics. Git host verifies original
  Applied per-binding descriptors, approved origins, exact named targets
  and whole layout before claim; actual conditional release uses the existing
  driver methods. Retain independent approval through failure and final
  settlement. Failed/partial physical release keeps the complete original
  closed set and reservation until exact reconciliation. No additional
  retention owner, schema, registry, receipt or event.
- Exact original closed revision is optional at the SDK boundary and
  mandatory in the CLI approval. Fresh cleanup advances revision once
  with the existing local sandbox.retention.released fact; terminal replay
  requires that fact at the immediately following revision. It skips
  Git lookup and physical layout inspection (the private approval and
  current Store/Project authority are still required). An arbitrary closed
  empty Sandbox or wrong revision is not a successful exact cleanup replay.
- Share canonical named-location validation and private named-layout pinning
  between restore and closed-Sandbox maintenance, reuse the bounded
  approval loader/lease, exact Store scope and private checkout observations.
  Independently exclude other layout/Git backing users, aliases, mounts,
  other Stores and external writers for the entire operation; neither
  advisory lease nor directory pins alone are physical isolation. Modified
  or unrelated files, missing/incompatible driver, unproved provenance,
  retained dependent children or ambiguous layout remain held.

Validation (L5, Rust 1.95.0): grouped real offline Git/SQLite
checkpoint/Sandbox lifecycle 1/1; Store checkpoint owner 4/4; Store
Sandbox owner 4/4; Host source-checkpoint lifecycle 1/1. The grouped
regression proves open/stale Sandbox refusal, foreign-file preservation
and no new preflight hazard, modified managed checkout retention and
Unknown custody, exact repair/restart release, preserved closed Agent
identity and unrelated live sibling/retention, and terminal replay after
the original layout and driver disappear. Strict all-feature/all-target
Store/Host/CLI Clippy, durable lib and minimal all-target checks, full
binary build, required-approval CLI smoke and scoped fmt/diff passed.
Existing feature-specific warnings remain. Unchanged Git/Process suites,
broad CLI inventory discrepancy and pinned Nix/native-platform
qualification were not rerun. Only private fixture layouts reconciled;
no real user cleanup or publication.

Next: exact live source-aware Actor/Turn admission through original
journal/queue/context owners if free. Otherwise proceed with independent
protected source metadata or Vault/Process dispatch authority. Exact closed
Sandbox managed Git maintenance is now an actual narrow host consumer;
broader Sandbox structure mutation/cleanup, physical containment and
source-bearing runtime admission remain held. Do not repeat offline
lifecycle wiring or weaken authority to finish Phase 3.

## Previous local implementation delta — 2026-11-25: exact offline capture/restore abandonment

Parent: `5cfc6f632dce2f41a365fd131eb1dfc98bb1f59f` (captured Git
restore), signature verified. Original Agent/Turn journal and resource CLI
owners remain occupied; independent original-custody cleanup slice, L5 only.

- Both workspace-feature offline commands `checkpoint-capture` and
  `checkpoint-restore` accept explicit `--agent-abandon`, mutually
  exclusive with `--agent-recover`. Supply the original request ID,
  semantic arguments and existing private approval. Cleanup authorization
  is separate from native tool access, capture/restore creation and logical
  checkpoint retirement. Successful reconciliation emits only
  `{"status":"rejected"}`. Never abandons an Applied checkpoint/Agent,
  creates a candidate, recaptures, rematerializes or broad-prunes Git.
- Capture uses the exact original request fingerprint before reclaiming
  existing custody, with the registered Git driver resolved lazily. Restore
  additionally checks the exact original SessionBinding digest and excludes
  independently selected baselines. Terminal Rejected replay checks the
  original receipt and candidate absence without reloading approval, driver
  or physical layout; request mismatch still fails. Restore's read-only
  replay lookup shares the original semantic/receipt validation, not a new
  ledger. Existing general trusted SDK cleanup methods remain compatible.
- The original source claim and all-source mutation barrier own cleanup.
  Check every original descriptor/location before Git release, then use the
  existing exact retention/scratch and worktree/partial-tree cleanup methods.
  Changed refs, modified/ambiguous worktrees and unproven scratch stay
  unresolved. Rejected and barrier settlement commit atomically only after
  every original resource is absent or released. The shared Store
  reconciliation keeps returned guards through final failure's Unknown
  settlement as well as success. Git capture, restore and standalone
  Sandbox cleanup hold the acquired physical fence outside the callback
  so it also survives provider error. No new schema, config format, driver
  descriptor or cleanup owner.
- The offline commands retain their existing narrow authority contract:
  exact current local manager/actor, private approval lease, original
  named roots and explicit operator Git administration/exclusion. Capture
  still requires a current matching configured native approval; restore
  remains same-Project/private/taskless with exact protected destination.
  The operator must have continuously protected original private candidate
  and scratch namespaces, Git backing and other retention users. Advisory
  leases and directory observations do not prove physical containment.
  Unmarked/ambiguous scratch or non-exact partial materializations remain
  held, as do cleanup under a changed capture binding and broader layouts.

Validation (L5, Rust 1.95.0): grouped real offline Git/SQLite
capture/restore/abandonment/retention lifecycle 1/1; Store checkpoint
owner 4/4; Host source-checkpoint lifecycle 1/1. Grouped regression
reuses the existing interruption fixtures and proves wrong original
request/layout refusal, changed retention/modified worktree preservation
and retained hazard, exact repair/restart reconciliation, absent private
candidates, Applied cleanup refusal, never-retry Rejected receipts,
terminal replay without config/driver, unchanged published resources
and source HEAD/index/worktree. Strict Store/Host/CLI all-target/all-feature
Clippy, durable lib and minimal all-target checks, full CLI binary build,
actual capture/restore help and conflicting recover/abandon rejection,
retention help and scoped fmt/diff passed. Existing feature-specific
warnings remain. Unchanged Git/Process suites, broad CLI inventory
discrepancy and pinned Nix/native-platform qualification not rerun.
Only isolated fixture effects reconciled; no real user cleanup or
publication.

Next: live source-aware Actor/Turn admission if original owners are free;
otherwise independent protected metadata or Vault/Process authority,
or exact closed Sandbox/materialization maintenance. Offline capture,
restore, original abandonment and retired checkpoint retention now have
actual narrow host consumers. General physical/provider exclusion and
remaining authority/activation gates remain held; do not repeat offline
lifecycle wiring or weaken admission to finish Phase 3.

## Previous local implementation delta — 2026-11-25: independently approved captured Git restore and exact recovery

Parent: `c99ff5478d657a56f61cebe50fdef01dfff90ac7` (configured offline
Git checkpoint capture), signature verified. Original Agent/Turn journal
and resource CLI owners remain occupied; independent offline restore slice,
L5 only.

- Production workspace-feature `aw checkpoint-restore --state-dir <state>
  <approval.json> <checkpoint> <request-id>` with optional `--session-title`
  and `--agent-recover`. Taskless, same source Project, private history access,
  explicit local OS principal, existing exclusive Store owner. Uses original
  captured-source Git Prepared/execute/inspect/publish and original history,
  fresh Sandbox and Agent owners. Destination Agent remains held; no task,
  activation, inherited grant or credentials, or inferred destination profile.
- The restore approval is a separate independent physical/administrative
  decision, not native filesystem access or retained-source cleanup authority.
  Strict v1 private JSON (max 1 MiB) fields: `version`, `store_directory`,
  `store_id`, `store_generation`, `manager`, `checkpoint`, `binding` (the
  existing full non-secret SessionBinding, explicitly supplied by the host),
  `destination_identity:[volume,object]`, `repositories` (each `{name,
  repository,repository_identity:[volume,object]}`). The binding's tool_root
  and working_directory must equal the exact existing canonical private
  destination parent, disjoint from approval/Store/origin state. Every
  captured named Git anchor must match the exact selected repository and
  checkpoint; targets are direct children named by the SourceName, never
  arbitrary caller/descriptor locators. Fresh parent must be empty; recovery
  admits only original named directory entries, with exact driver marker,
  content and parent identity verification. Independent ordinary private
  origin checkout roots only, not linked/separate Git administration.
- Reuse the original offline Git approval lease, typed bounded private file
  loader, Store scope validation and checkout pins across restore/retention.
  The live host must independently exclude external writers, aliases,
  mounts, other Stores and retention users through settlement. Cooperating
  configuration writers must honor the held advisory lease on the same
  inode. This is not kernel isolation or automatic physical authority.
- Applied restore replay precedes configuration/driver/layout inspection;
  stable `cli:checkpoint-restore:<request-id>` namespace. Explicit recovery
  checks original semantic request fingerprint AND SessionBinding digest
  (no alternate baseline) before reclaiming the existing receipt/hazard set
  or probing a physical location. It inspects and publishes only the
  original completed restore; incomplete or ambiguous effects remain
  Unknown. Original generic restore recovery remains compatible. Corrected
  host recovery to keep the acquired fence alive through failed inspection's
  Unknown settlement as well as successful publication. No new schema,
  recovery descriptor, replay ledger or source registry.

Validation (L5, Rust 1.95.0): grouped real offline Git/SQLite
capture/restore/retention lifecycle 1/1, Store checkpoint owner 4/4,
Host source-checkpoint lifecycle 1/1. Grouped regression verifies exact
fresh and recovered worktree content, same retained history with fresh
local roots, held admission and empty queue, no new grants, refusal of
foreign destination entries and occupied approval, original candidate
identity across restart, wrong request/layout refused before inspection,
terminal replay without the approval/driver, unchanged source HEAD/index/
worktree and unrelated retention. Initial fixture expected a parent-Agent
link instead of a fresh history root; corrected to the existing contract.
Strict Store/Host/CLI all-target/all-feature Clippy, durable lib and minimal
all-target checks, full binary build, restore/capture/retention help and
required restore arguments smoke passed. Existing feature-specific warnings
remain. Unchanged driver/Process groups, broad CLI inventory discrepancy
and pinned Nix/native-platform qualification not rerun. Only isolated
fixture worktrees materialized; no real user restore or publication.

Next: exact live source-aware Actor/Turn admission if original owners are
free; otherwise original incomplete-capture/restore reconciliation and
remaining independent authority paths. Offline restore does not expose
source-bearing Actor admission, task-bearing/cross-Project restore, native
policy/grant publication, general physical containment, protected source
metadata or abandonment/cleanup CLI. Retain original source/driver custody
until exact proof; no blind retry or broad prune.

## Previous local implementation delta — 2026-11-25: configured offline Git checkpoint capture and exact recovery

Parent: `0c9eab3e8566fb9fc3fddc6a7a708f9b2dbd05ed` (offline Git retention
reconciliation), signature verified. Agent/Turn journal and resource CLI
owners still occupied; independent offline host slice, L5 only.

- Actual production consumer of the existing configured `git-native.json`
  approval: `aw checkpoint-capture --state-dir <private-store>
  <approval-root> <canonical-agent> <request-id>` with optional
  `--agent-source <name>` (repeat for exact named selection; default Auto),
  `--agent-note <note>` and `--agent-recover`. No new config schema; the
  existing config independently selects the exact Store generation, manager,
  actor, Agent, Sandbox revision, policy, and named binding/root identities.
  Offline capture requires manager = actor = authenticated local OS principal,
  existing exclusive Store owner, and the exact selected source set.
- The configured approval loader now composes a second independently
  authorized host-operation fence after current Store/source selection and
  before probing Git or provisioning private artifact namespaces. Existing
  configured/directed native callers share the same implementation. The
  offline command is explicit authorization of the bounded Git retention
  administration effect, separate from native filesystem read/write ceilings.
  It retains the live `NativeSourceApproval` read guard, approval file's
  exclusive advisory lease, and checked ordinary checkout/admin/object
  directory pins through the original Prepared/execute/inspect/publish owner.
  No SandboxGrant publication, Agent activation or inference. The host must
  protect all config, artifact, Store and Git backing, aliases and external
  writers/other retention users; this is not kernel or cross-Store isolation.
  Separate Git common directories and non-private checkout roots are refused.
- The capture owner retains its original request fingerprint (canonical Agent
  selector, canonical selection and note). Exact Applied replay skips approval
  loading and driver lookup, including retired results. Unresolved requests
  cannot be fresh capture; explicit `--agent-recover` checks the original
  complete semantic fingerprint before the original receipt/barrier is
  reclaimed. It can only inspect/publish already completed physical capture;
  incomplete/ambiguous retention remains unresolved and is never recaptured.
  Original generic trusted recovery APIs remain compatible. No second
  receipt, replay owner, schema or driver registry. CLI request namespace is
  `cli:checkpoint-capture:<request-id>`.
- Shared private Git maintenance helpers now serve both offline capture and
  retired retention reconciliation; no duplicate lease or checkout pin logic.
  No new cleanup/disclosure authority or runtime registration is inferred
  from this metadata/maintenance path.

Validation (L5, Rust 1.95.0): Store checkpoint owner 4/4, Host
source-checkpoint lifecycle 1/1, native source-Process integration 2/2,
grouped configured offline capture/recovery/retention regression 1/1.
The grouped real Git/SQLite fixture verifies stale approval and locked
config refusal, fresh capture, exact Applied replay without the config,
interrupted Prepared physical capture/restart, wrong request refusal
(including at the lower inspection boundary), exact recovery without
recapture, unchanged Agent/SandboxGrant/HEAD/index/worktree, and retained
unrelated live refs during retirement. Strict all-feature/all-target
Store/Host/CLI Clippy, durable lib, minimal all-target checks, full CLI
build, actual capture/retention help and required-argument smoke, scoped
fmt/diff passed. Existing feature-specific dead-code warnings remain.
The preceding Git repository implementation was unchanged; its 33/33
suite, broad CLI inventory discrepancy and pinned Nix/native-platform
qualification were not rerun. Physical effects tested only in private
fixtures; no actual user checkout captured, no publication.

Next: exact live source-aware Actor/Turn admission through the original
journal/queue/context owners if free. Otherwise independent substantive
physical source restore/reconciliation or protected metadata/Vault host
integration. Configured Git approval has an actual offline capture/recovery
consumer; native Actor tool activation, complete independent physical
containment, abandoned/incomplete capture cleanup CLI and captured-source
RPC/restore remain held. Do not confuse this offline host path with live
source-bearing execution.

## Previous local implementation delta — 2026-11-25: offline Git checkpoint retention reconciliation

Parent: `0e68089d9f5983b27325548fb667aa1940e45bb1` (configured signing
Vault activation), signature verified. Agent/Turn journal and resource CLI
owners still occupied; independent maintenance slice, L5 only.

- The original retained-source reconciler now has a fenced variant. Read
  the exact retired SourceSet under current Manage, unresolved restore and
  shared retention checks; acquire independent read-only approval outside
  SQLite; recheck the same set and authority under the immediate transaction
  before the existing cleanup claim. A denied preflight creates no new
  Unknown custody. The original compatibility method delegates to it.
  The live guard survives exact external effects and factual settlement;
  failure after claiming retains the original set and Unknown hazard.
- Git capture/retirement host and offline registered-driver path share one
  implementation. Verify and retain the original Repository objects for
  the complete selected set, then reuse exact no-deref expected-OID release
  on those objects. No rediscovery from stored locators after authorization.
  No new replay ledger, schema or driver registry. Already-cleaned retirement
  skips authority callback and Git lookup, even if the original root or driver
  has disappeared.
- Production workspace-feature CLI: `aw checkpoint-retention <absolute-private-approval.json>`.
  Explicit trusted local maintenance, **not** an Agent/model RPC, and never
  automatic. Requires an already existing private Store, exclusive owner,
  current source-Agent Manage, and an exact independent administrative
  approval. Reuses `sessions::open_sqlite_owner` and the installed Git driver
  (`AW_GIT` / build-pinned Git). The operation only releases exact retired
  Git refs and reconciles the existing Store; no live-checkpoint retirement,
  branch/index/HEAD/worktree deletion, GC, activation or inference.
- Approval format v1 (strict bounded JSON, max 1 MiB), in an existing
  owner-private canonical directory outside the Store and checkout roots:
  `version`, `store_directory`, `store_id`, `store_generation`, `manager`
  (authenticated OS PrincipalId), `checkpoint`, `repositories` (each
  `{name, repository, repository_identity:[volume, object]}`). Complete
  selected set in canonical name order; original anchor must belong to
  that checkpoint. The host chooses identities independently, never from
  a persisted grant or client path. Private ordinary checkout roots only;
  linked-worktree pointers/separate common-directory mappings unsupported.
  A host approval file is read under a real exclusive advisory lease held
  through settlement. Cooperating host writers must lock the same inode
  before editing/withdrawing; no replacement while live. The operator MUST
  independently protect the Store, approval, Git administration, objects,
  ref namespace, ancestors and aliases and exclude external writers/other
  retention users throughout. Directory pins and the lease alone are not
  kernel containment or cross-Store physical proof. No automatic general
  Git-native approval activation or broad cleanup permission.

Validation (L5, Rust 1.95.0): Store checkpoint suite 4/4; real Git
repository suite 33/33; Host source-checkpoint lifecycle 1/1; new offline
reconciliation 1/1 (live/incorrect approval denial, no preflight hazard,
changed-ref Unknown, exact restart cleanup, unchanged HEAD/index/worktree,
terminal replay without checkout/driver). Strict all-feature/all-target
Clippy for Store/Git/Host/CLI, durable lib and minimal all-target checks,
full binary build and actual required-approval CLI smoke all passed.
Initial empty Git filter corrected with the real repository suite; initial
fixture compile error (missing SessionBackend import) corrected. Existing
durable/minimal dead-code warnings remain. Pre-existing CLI inventory
fixed flag count differs from the preceding built binary; broad CLI
inventory suite and pinned Nix/native cross-platform qualification not
rerun. No deployment or publication.

Next: exact live source-aware Actor/Turn admission if the original owners
are free. Otherwise configured Git native authority activation, or the next
independent substantive source/Sandbox/Vault authority path. Offline Git
checkpoint retention now has a real narrow host consumer; general physical
containment, cross-Store/provider exclusion and Project retention-admin
capability remain outstanding. Source-bearing execution, capture/restore
RPCs and other projections remain held. Do not treat logical retirement or
this exact maintenance approval as wider execution or materialization access.

## Previous local implementation delta — 2026-11-25: configured signing Vault activation and binding RPCs

Parent: `78a4c54c1bd33e93b395550df7c61433d4c3edf1` (replay-safe logical
checkpoint retirement), signature verified. Original Agent/Actor/Turn
journal and resource CLI owners remain concurrently occupied; source-bearing
admission stays held. L5 only.

- Explicit local Actor-owner stdio `--agent-signing-vault-root` activates the
  existing configured signing provider in the typed VaultProvider registry,
  retaining its live handle for the exact bound Agent. Requires the original
  tool-free durable Session/Actor host. The provider directory must already
  be canonical/private and contain the existing validated configuration and
  encrypted keys. Observed execution/Store layout overlap is refused;
  the trusted host must independently protect aliases, ancestors, mounts
  and other writers for the provider lifetime. No physical containment
  claim, implicit grant, model-supplied path, key, actor or scope.
- Optional `AgentVaultPort`: `agent_vault_list(ref=.)` requires the explicit
  host metadata capability and current Manage; only current exact-Agent
  redacted binding summaries (name/projection/grant revision/status), never
  provider/entry/authority/key metadata. `revalidation_required` is not
  authorization. `agent_vault_set(ref=., name, binding, rev)` accepts the
  exact typed entry/projection or explicit null to remove. Only configured
  broker/env decisions are supported. No broker invocation or Process
  admission RPC. Startup does not grant any Vault entry.
- Both original grant mutations and authenticated host mutations share
  the original Store transaction body. Host replay uses the existing
  indexed immutable commit-log identity (active user + namespaced request
  ID), original typed request fingerprint including literal contextual
  selector, and exact settled Agent/revision. Replay lookup precedes
  contextual resolution/provider admission; fresh mutation rechecks
  Manage, writer, CAS and the actual live provider guard through the same
  commit. Even an accepted no-op consumes the identity. Exact replay does
  not regrant or append a fact; mismatched replay denied; lost disclosure
  returns opaque `prior_invocation`. No parallel ledger/schema, secret
  receipt or new mutation owner. Removing a binding never removes keys.
- Fixed a preexisting tool-free startup mismatch: local durable hosts
  no longer register model-facing Event tools in tools-off mode, so the
  real Actor-owner attachment can enforce its existing no-tools guard.
  Tool-enabled Event registration and the Store Event owner are unchanged.

Validation: on L5 (Rust 1.95.0): existing signing Vault owner group **2/2**,
headless protocol **8/8**, source-Process integration **2/2**; actual
configured Host + Actor attachment + stdio dispatch regression **1/1**
(after the tools-off startup correction), including live broker proof,
replay after withdrawal/restart, wrong authority/selector/projection,
no-op identity consumption, removal preserving the encrypted key and
redacted metadata. Strict Store/Host/CLI all-target/all-feature Clippy,
durable-only lib and minimal all-target checks, full CLI build and actual
flag/required Actor-owner smoke passed. Initial lint caught an unnecessary
unit expression; corrected before the final strict run. Existing
durable/minimal dead-code warnings remain. No live model request.
Pinned Nix/platform qualification and broad unrelated suites not rerun.
Store v60, Git recovery v4; no publication.

Next: exact live source-aware Actor/Turn admission when original journal,
context and queue owners are free; otherwise independent configured Git
authority/physical recovery or another remaining substantive Sandbox/Vault
integration. Configured signing activation and local redacted list/set
are landed; production broker/Process dispatch, independent provider and
protected metadata authority, other Vault projections and source-bearing
admission remain held. Do not infer physical isolation or provider access
from the management port.

## Previous local implementation delta — 2026-11-25: replay-safe logical checkpoint retirement

Parent: `94838c0caf1da105a7cf001892d630eaf66c99ec` (comprehensive handoff
refresh; preceding implementation `ffa8fc940fd82468925be3505adf7aa6082399b4`).
Both signatures verified. Original Agent, Actor/Turn journal and resource CLI
owners still concurrently occupied; this independent resource slice leaves
source-bearing admission held. L5 only.

- New optional `CheckpointDeletePort` on the existing authenticated
  `StdioAgentStatePort`. Explicit local Actor-owner stdio
  `--agent-checkpoint-delete`, independent of create, read and restore.
  The existing exact durable Session/tools-off restrictions remain.
  Request `checkpoint_delete` accepts only `id`; ordinary connection
  duplicate-ID/budget/parameter gate precedes dispatch. No client-supplied
  cleanup flag, authority, source selector or driver/provider locator.
- `SessionStore::checkpoint_delete_host` composes the original retirement
  owner (`retire_in` also used by `checkpoint_delete`). One immediate Store
  transaction checks current source Agent Manage and exact Project scope,
  rejects unresolved restores, and retires the row without rewriting
  history, advancing Agent rev or touching retained anchors. No implicit
  Project-admin capability or physical deletion.
- Existing immutable indexed `commit_log.event_id` owns replay, derived
  from active user + namespaced request ID (`rpc:stdio:checkpoint-delete:{id}`
  on stdio); exact checkpoint ID in the event entity is the complete request
  semantics. Replay precedes fresh admission; differing target denied.
  `checkpoint.retired` is the one actual transition. A new authorized
  request on an already-retired row receives its own local-only
  `checkpoint.retirement.acknowledged` fact so that its ID cannot later be
  reused for another target. Exact replay appends no event and returns
  `retired`, or opaque `prior_invocation` after loss of current Manage or
  Project scope. No checkpoint metadata in either response. No new ledger,
  schema, event delivery route or driver owner.
- Retired anchor rows remain the existing cleanup intent; physical release
  requires separately authorized exact external/provider fences and the
  original retained-source reconciler. The RPC never invokes cleanup,
  probes drivers, materializes sources or implies GC safety. Complete
  physical retirement/recovery integration and Project retention/admin
  authorization remain held.

Validation on L5 (Rust 1.95.0): checkpoint owner group **4/4** (capture,
restore, original retention and host retirement; retained anchors, single
actual retirement, current Manage loss, replay with missing driver), headless
protocol **8/8**, actual Store-backed metadata/construct/restore/delete
port **1/1** (wrong caller/Project, exact vs mismatched replay, new no-op
request identity, opaque prior invocation, reopen/rebind, unchanged Agent
rev). Strict Store/CLI all-feature/all-target Clippy, durable-only lib and
minimal all-target checks, full CLI build, real flag help and required Actor
owner rejection smoke, scoped fmt/diff passed. Initial command used the
binary name rather than Cargo package `aw-cli`; corrected and reran the
remaining gates successfully. Existing durable/minimal dead-code warnings
remain. CLI smoke tests opt-in parsing, not a live provider Session. Shared
builds include concurrently owned dependencies; commit contains only owned
paths. Pinned Nix 1.98.1, native platform/Bubblewrap and unrelated broad
qualification not rerun. Store v60, Git recovery v4; no publication.

Next: exact live source-aware Actor/Turn admission when original journal,
context and queue owners are free. Otherwise take the next independent
production authority/containment or physical recovery integration from the
comprehensive handoff. Configured Git/signing approval activation and
remaining source-bearing capture/restore, task-bearing restore, protected
metadata disclosure, physical retention and other Sandbox/Vault authority
remain held. Do not repeat the completed logical checkpoint RPC slice.

## Previous local implementation delta — 2026-11-25: replay-safe empty-baseline checkpoint restore

Parent: `077ac07803d9fa19db9743393cc4e68f2a6f4676` (replay-safe history
checkpoint construction). Parent signature verified. Original Agent/Turn
journal owners and resource CLI remain concurrently occupied; source-bearing
admission remains held.

- Reusable `aw_host::checkpoint_baseline::EmptyCheckpointRestore` composes
  the original `checkpoint_replay_restore` / `checkpoint_restore_baseline`
  owners. Explicit trusted Project, caller, current host execution-binding
  template and independently selected destination root; not another history
  constructor, layout registry, grant or recovery ledger. The helper uses
  the existing platform dependency in the `source-effects` feature, with
  no new locked dependency versions. No Git driver needed for this slice.
- Fresh restore requires current checkpoint view, same source/destination
  Project, a live checkpoint with no captured anchors (history-only or
  empty Auto), and a canonical existing private empty directory. Refuses
  task-bearing or captured-source restore; never substitutes empty history
  for captured sources. Uses original Store history derive/share, Project
  create/writer, fresh layout and atomic Agent/Sandbox publication checks.
  Explicit `private` (default), `inherit`, custom ACLs use original parser
  and lower authority. Returns redacted Agent snapshot, Sandbox ref and
  source coverage. No activation, queue, provider invocation, directory
  creation, deletion or materialization on this host path.
- `checkpoint_restore` optional stdio port enabled only by explicit
  `--agent-checkpoint-restore-root <absolute-canonical-private-directory>`
  with the existing `--agent-actor-owner` / `--agent-stdio` local tool-free
  exact durable Session mode. No model/client root, provider, authority or
  layout selector. Request fields `id`, `project?`, `title?`, `task?`,
  `access?`; task is unsupported on fresh restore. Namespace the original
  authenticated request as `rpc:stdio:checkpoint-restore:{id}`. Execute
  after the ordinary connection duplicate-ID/budget/parameter gate, not
  on the speculative live reader. Creation capability stays independent.
- Exact replay precedes any destination inspection and fresh admission;
  original semantic mismatch denied and result Project/disclosure rechecked.
  Original receipt remains replayable even after checkpoint retirement,
  Store reopen or missing/consumed destination. On fresh admission retain
  the original platform directory identity through publication; reject
  lexical overlap and exact identity alias with Store state/current host
  execution layout. Store checks exact known occupied layout. The trusted
  host MUST independently protect the selected baseline and ancestors
  from other layouts, physical aliases, mounts and external writers; path
  checks, permissions and pins are observations, not tenant isolation.
  No global physical containment or complete all-layout ancestor proof.
  No new persistence schema, source authority or frozen Sandbox changes.

Validation on L5 (Rust 1.95.0): full headless **8/8** (restore dispatch,
duplicate IDs, no client root); grouped actual Store-backed metadata/
construction/restore **1/1** (missing/nonempty/protected baseline refusal,
no implicit creation, task/Project refusal, fresh Agent/Sandbox publication,
occupied root rejection, mismatch denial and replay on durable reopen after
retirement/missing layout); original restore-owner exact history/captured
sources/recovery **1/1**. Strict Host/full-product all-target/all-feature
Clippy, durable-only lib and minimal all-target checks, actual full binary
build and CLI opt-in dependency smoke, scoped fmt/diff passed. Initial
ACL-parser argument mismatch and test assumption that restored local head
must equal retained source head corrected; the original restore owner
creates a new local root over the exact immutable history base. Existing
minimal/durable warnings remain. CLI smoke only validated opt-in parsing;
actual publication covered by the real Store port regression. Shared builds
include concurrent dependencies; signed tree contains only owned paths.
Pinned Nix 1.98.1, Windows/macOS/Bubblewrap and unchanged broad suites not
rerun. Store v60, Git recovery v4; no publication.

Next: exact live source-aware Actor/Turn admission when original journal
owners are available; resource CLI startup composition remains occupied.
Otherwise choose a substantive independent actual host authority consumer
or remaining sealed resource surface. Task-bearing/captured-source restore,
checkpoint retirement RPC, independent physical retention cleanup and
protected metadata disclosure remain held; do not infer their authority
from this empty baseline restore.

## Previous local implementation delta — 2026-11-25: replay-safe history checkpoint construction

Parent: `1518684de8c74fe9c5df1f65765b9bbc71a921ec` (protected source metadata
and checkpoint reads). Parent signature verified. Original Agent/Turn journal
owners and resource CLI remain concurrently occupied; source admission held.

- Explicit local stdio `--agent-checkpoint-create` (requires the existing
  `--agent-actor-owner` and `--agent-stdio` exact durable Session binding;
  tools off and original Actor-owner restrictions unchanged) enables the
  optional `CheckpointCreatePort`. Default off, read capability independent.
  Reuses the authenticated `StdioAgentStatePort`; no new runtime owner.
  The sealed RPC accepts `ref` (default `.`), `sources` (`none` or `auto`,
  default `auto`) and optional `note`. `none` captures settled immutable
  history; Auto succeeds only when the effective Sandbox has no required
  SourceBindings (ephemeral bindings omitted). Required or explicitly
  selected source capture remains held; no fallback to `none`.
- `CheckpointCreateScope` and `checkpoint_create_history_host` compose the
  existing history/empty-Auto Store publication and original replay owner.
  The literal ref and complete semantics are fingerprinted before replay;
  resolve `.` only on fresh admission, require same Project and current view.
  A matching replay returns the original checkpoint identity, with current
  disclosure/Project checks, even after current Agent rebinding or retirement.
  Existing lower constructors continue to use their original fingerprints.
  Authenticated stdio request IDs use `rpc:stdio:checkpoint:{id}` in the
  existing caller-scoped receipt; no new ledger, synthetic evidence, or
  schema. Agent rev remains unchanged. No driver probes or physical effects.
- The new constructor executes on the ordinary RPC request path after the
  connection's duplicate-ID/budget/parameter validation, rather than in the
  speculative live input reader. It therefore queues behind an active RPC
  operation. The lower owner still supports settled-history capture during
  a live Turn; no new concurrency guarantee claimed. The previous read port
  and existing live routes are unchanged. `checkpoint_restore` and
  `checkpoint_delete` remain unavailable as host RPCs; no physical retention
  cleanup or source-bearing admission promotion.

Validation on L5 (Rust 1.95.0): original checkpoint owner **1/1**, grouped
Store-backed real metadata/construction port **1/1** (history/empty-Auto,
current ACL and Project scope, literal-ref and coverage mismatch, changed
current Agent on durable reopen, retired replay tombstone, unchanged Agent
rev), headless **8/8** (optional capability, strict params, duplicate-ID
constructor rejection), full product all-target/all-feature check, strict
Store/full-product Clippy, minimal all-target check, actual full binary build
and CLI opt-in dependency smoke, scoped fmt/diff passed. Existing minimal
dead-code warnings remain. CLI smoke checked parsing/opt-in, not a live
provider session; actual Store-backed effects covered by the port regression.
The original required-source Auto guard remains in the Store; full physical
capture qualification was not rerun. Shared builds include concurrently
owned dependencies; signed tree contains only owned paths. Pinned Nix
1.98.1 and native cross-platform qualification not rerun. Store v60, Git
recovery v4; no publication.

Next: exact live source-aware Actor/Turn admission when original journal
owners become available. Resource CLI startup composition remains occupied.
Otherwise take the next independent actual host authority consumer or sealed
resource surface. For checkpoint restore/retirement preserve replay-first
publication, existing constructor and physical retention owners; do not
infer capture or disclosure authority from history checkpoint capability.

## Previous local implementation delta — 2026-11-25: protected source metadata and checkpoint reads

Parent: `44d92bc0cfe0eff269942e49c8b9832dbb76aa62` (configured Git native
source authority). Parent signature verified. Original Agent/Turn journal
owners and resource CLI remain concurrently occupied; source admission held.

- Existing `StdioSandboxPort` now projects the actual named source summaries
  instead of the previous empty placeholder. `sandbox_get` includes them by
  default; the existing `include:["sources"]` remains supported. `sandbox_list`
  returns source counts; `sandbox_sources_list(ref)` returns exact ref, rev
  and the redacted summaries. Existing current Project checks remain in force.
- Optional authenticated `CheckpointReadPort` uses the original stdio reader
  and live-result transport. The product implements it on the same bound
  `StdioAgentStatePort` (no extra Store or host state). `checkpoint_list` takes
  current/canonical same-Project Agent `ref` (default `.`), `limit` 1..64
  (default 16) and the original opaque checkpoint-ID cursor. Returns `ref`,
  `items`, `next_cursor`. `checkpoint_get` takes `id` and optional empty
  `include`; no extension views available. Both use original Store metadata
  reads with current Agent view authorization, same-Project host scope, no
  driver probes or materialization. List includes live checkpoints only; get
  can show a retired replay tombstone. No construction/restore/retire RPC.
- Explicit allowlists project history base/head, coverage, lifecycle, note,
  created time and source names, public driver kinds, access (`ro`/`rw`),
  checkpoint policy and compatibility; Sandbox bindings also report binding
  rev. Extension kinds are generic `extension`, not provider/extension IDs.
  Raw paths, source/retention/anchor payloads and layout digest withheld;
  no independent metadata-read disclosure route yet. Oversized metadata
  responses fail before emission, preserving the connection for smaller pages.
- Existing capability handshake advertises checkpoint reads only for a
  supplied optional port and keeps unsupported mutation operations false.
  No new model tool, schema, source authority, recovery route or actor gate.
  Reuses the original request validation and current Project/ACL owners.

Validation on L5 (Rust 1.95.0): real Store-backed grouped metadata port
regression **1/1** (actual source-bearing Sandbox, history-only checkpoints,
redaction, ACL/Project scoping, cursor pagination and retirement read), full
headless protocol **8/8** (bound/unbound negotiation and strict parameters),
full product all-target/all-feature check, strict full-product Clippy,
minimal all-target check and scoped fmt/diff passed. Initial JSON capability
macro recursion limit fixed by factoring the handshake object; no crate-wide
recursion-limit increase. Minimal existing dead-code warnings remain. The
new test does not capture a source-bearing checkpoint end-to-end; its lower
capture/metadata owner remains unchanged. Shared builds include concurrently
owned dependencies; signed tree contains only owned paths. Pinned Nix 1.98.1
and platform-specific qualification not rerun. Store v60, Git recovery v4;
no publication.

Next: exact live source-aware Actor/Turn admission when original journal
owners become available. Resource CLI remains occupied. Otherwise take a
substantive independent host authority/containment consumer or the remaining
sealed host resource surface, preserving replay-first publication and real
independent disclosure. No automatic grant/admission from metadata.

## Previous local implementation delta — 2026-11-25: configured Git native source authority

Parent: `3d2fd314cbaf8cccb6bdd0c69ab7d39cd6c9b559` (source-aware single-link
native capabilities). Parent signature verified. The original Agent/Turn
journal owners and resource CLI remain concurrently occupied; source-bearing
admission remains held.

- `GitNativeSourceResolver::provision_configured_approval(manager, root)`
  consumes explicit trusted-host configuration from `root/git-native.json`.
  The canonical existing owner-private root also owns the original durable
  artifact namespaces; the existing platform private-file open rejects aliases
  and unsafe file permissions. Bounded v1 JSON (1 MiB; existing source-set
  limit; unknown fields denied). The configuration is never an Agent input,
  automatically discovered file, or serialization of a durable grant.
- Config fields: `version` (1), `store_directory` (canonical),
  `store_generation` (current `local_process_info().store_generation`),
  `manager`, `actor`, `agent_ref`, `sandbox_ref`, `sandbox_rev`, `policy_ref`,
  `base`, `directories`. Each directory specifies `name`, `repository`
  (canonical), `repository_identity` (volume/object pair), `binding_rev`,
  `access` (existing SourceAccess encoding). These are independently selected
  host facts; no ambient repository discovery. Requires the actual active
  exclusive Store owner and exact manager. The original Store authorization
  and exact Sandbox selection occur before Git probes. Config and artifact
  state must lie outside the entire layout, including unselected siblings.
  Existing complete-set materialization and artifact-state checks still apply.
- Reuses the original approval provisioning, typed registered Git driver,
  private artifact owner, and `NativeSourceApprovalSpec` validation. A fresh
  random authority ref with generation 1 on each activation prevents reuse of
  previous persisted grant evidence; publication is the existing explicit
  revision CAS. The returned approval composes directly with
  `GitNativeSourceHost::new` and its original native route. No second registry,
  Store schema, recovery route or model-facing policy knob. Revoke the old
  owner before changing policy/reactivating; editing the file is not a live
  revocation. The caller must independently exclude external writers and
  mount/ancestor aliases and protect the config/state root. File permissions,
  observed identities and configured policy are not tenant isolation.
- The existing grouped real Git native-route regression now uses configured
  approval for actual effects. Rejects missing/aliased/unknown-field config,
  wrong manager, Store generation or Sandbox before namespace provisioning;
  confirms explicit grant publication, fresh authority on reactivation and
  unchanged original receipt, read/write/alias/recovery/revocation semantics.
  No automatic product startup selection or source-aware admission promotion.

Validation on L5, installed Rust 1.95.0: focused configured native regression
**1/1**, source Process/Git native **2/2**, non-Git native **1/1**, strict Host
all-target/all-feature Clippy, full product all-target/all-feature check,
durable-only lib check (existing dead-code warning), scoped fmt/diff passed.
Feature-scoped direct dependencies reuse the existing locked platform,
serde and getrandom versions. Shared builds include concurrently owned
changes; the signed commit contains only owned paths. Pinned Nix 1.98.1,
Windows/macOS/Bubblewrap and unchanged broad suites not rerun. Store v60,
Git recovery v4; no publication.

Next: exact live source-aware Actor/Turn admission when original journal
owners are available; resource CLI composition remains occupied. Otherwise
choose a substantial independent production authority consumer, not another
configuration wrapper or repeated provisioning check. Physical containment
and remaining source Process/Job/Vault/public surface remain held pending
actual authority and original owner participation.

## Previous local implementation delta — 2026-11-25: source-aware single-link native capabilities

Parent: `9121572780b24e3c4fd382016e6914d818b5d183` (trusted native-shell
startup containment). Parent signature verified. Original Agent/Turn journal
owners and resource CLI remain concurrently occupied; admission remains held.

- Existing Unix `CapabilitySet` supplies `open_prepared_single_link_with_exclusions`:
  opt-in single-link requirement for nondirectory objects on the original
  descriptor-backed Root, using a shared check over existing link metadata.
  Ordinary capability and Root constructors retain their existing hardlink
  semantics. No new backend, recursive startup scan or separate effect owner.
- Explicit final-component resolution, regular-file reads/streamed updates,
  metadata and staged publication use the same policy. Aliased entries are
  unaddressable/unsupported in listing; no payload or size/time metadata is
  returned for them. Bounded recursive lifecycle preflight rejects aliases
  before publication; direct transfer/rename/removal and staged target checks
  use current link evidence. Internal artifact cleanup retains its original
  exact custody contract, not an external capability.
- Native source approval and the actual approved native layout (including Git
  and non-Git execution and recovery) opt into the policy. Real host regression
  refuses hardlinks to unapproved files and protected Git administration.
  Grouped lower regression covers read/metadata, indirect symlink resolution,
  staged write, recursive copy/move/remove, late target link before commit,
  ordinary hardlink compatibility and normal single-link publication.
- This is conservative: even multiple links entirely within one source are
  denied. Observed link counts do not exclude racing outside writers, mount
  aliases or arbitrary Process access. The host's independent physical
  authority remains required. Windows source single-link constructor refuses
  unsupported use rather than downgrading; Windows and other Unix runtime
  qualification remain pending. Physical mount/config-v13 unchanged.

Validation on L5, installed Rust 1.95.0: full Unix filesystem **42/42**,
source Process/Git native route **2/2**, non-Git native route **1/1**, strict
Unix filesystem/adapter/Host all-target/all-feature Clippy, full product
all-target/all-feature check and scoped fmt/diff passed. Initial compile
helper qualification and redundant platform casts corrected before final
all-green rerun. Shared builds include concurrent dependencies; commit tree
contains only owned changes. Pinned Nix 1.98.1, Windows/macOS/Bubblewrap and
unchanged broad suites not rerun. Store v60, Git recovery v4; no publication.

Next: original source-aware Actor/Turn admission when its journal owners are
available, otherwise the next independent actual production authority consumer.
Resource CLI startup composition remains occupied. Do not treat single-link
checks, directory pins or installed definitions as independent physical authority.

## Previous local implementation delta — 2026-11-25: trusted native-shell startup containment

Parent: `9566d10ef3578b0834d30d74dfc8a944fe9f60b3` (exact SourceDriver
ownership and complete artifact-state preflight). Parent signature verified.
Original Agent/Turn journal owners and resource CLI remain occupied.

- Explicit host startup environment `AW_LOCAL_SHELL_CONTAINMENT` selects the
  existing typed `ContainmentRequirement`: unset or `best-available` preserves
  legacy behavior; `closed-descendants` requires the stronger guarantee.
  Invalid/empty/non-Unicode values reject startup, never fall back. Shared
  `aw::context::local_shell_containment_from_env` captures it once before
  normal Agent state/executor setup in full/minimal/stateless entry points.
  The scheduled owner validates before claiming work and passes the captured
  minimum into its ordinary Agent execution. Web-owner construction and
  existing runtime/session rebuilds carry the same cloned `LocalToolOptions`.
- The existing native shell Process/finite Job/Graph owner enforces the
  minimum and runtime binding. No new model/profile/session selector or
  additional executor. No tool authority conferred, no filesystem containment
  claim. Current Unix DirectScope refuses strict shell launches before spawn;
  native Windows Job support remains unqualified on L5. This applies only
  to the configured local native shell, not independent source routes, MCP,
  external tools/extensions, remote execution or human terminal sessions.
  README documents the operator setting and boundaries.
- SDK defaults stay unchanged; embedders can still supply typed minima
  explicitly. No frozen Sandbox/backend, schema or admission changes.

Validation on L5 (Rust 1.95.0): shared config regression **1/1** with full
features and **1/1** stateless; original direct/finite/Graph strict shell
runtime **1/1**; full and minimal all-target checks, strict full-product
all-target/all-feature Clippy, full binary build and actual CLI smoke (invalid
setting exit 2; valid strict setting accepted for metadata-only profile
listing), scoped fmt/diff passed. Minimal/stateless retain existing dead-code
warnings; existing minimal formatting preserved. The CLI smoke did not invoke
a provider or execute a shell; actual refusal covered by the existing runtime
regression. Shared builds include concurrent dependencies, signed tree contains
only owned paths. Pinned Nix 1.98.1 and platform-native qualification not
rerun. Store v60, Git recovery v4; no publication.

Next: exact live source-aware Actor/Turn admission remains the priority once
original journal owners become available; resource CLI still needs its owner
to adopt driver startup composition. Otherwise select an independent real
physical-authority/containment consumer. Do not infer global containment or
source authority from the local native shell setting.

## Previous local implementation delta — 2026-11-25: exact SourceDriver ownership and complete artifact-state preflight

Parent: `80f1ca09ae6129266410c099d103be94794c54f1` (persistent signing Vault
provisioning). Parent signature and clean owned paths verified. The original
Agent/Turn journal owners and resource CLI remain concurrently occupied;
source-bearing admission remains held.

- `SourceDriverHandle<T>` now binds the exact live registry owner, matching
  the existing SandboxDriver and VaultProvider handle semantics. A single
  process-local Arc identity is shared across registry clones/readers/handles;
  equality and typed retrieval reject a foreign registry even when stable
  key, schemas, concrete type and registration revision all match. Durable
  `DriverKey`/schema pins and restart compatibility are unchanged. Consumers
  resolve a fresh typed handle from the current Store after registry restart.
  No persisted registry identity, additional lookup owner or authority grant.
- `validate_source_artifact_state` is the shared read-only preflight for
  existing owner-private canonical state, disjoint protected directories and
  observed exact directory aliases. Existing single-root namespace open/reopen
  uses it; Git provision/recovery validates the complete approved checkout,
  Git directory and common-directory set before opening any namespace.
  The existing Git layout exclusions also reject duplicate physical checkout
  identities. Verify each actual Git materialization against its independently
  supplied location before preflight; reuse the verified repositories in
  recovery's actual layout pin rather than repeating Git discovery. The
  original recovery authorization/receipt and artifact owners remain intact.
- Real grouped native regression rejects state under the later selected source
  and its Git admin before creating any namespace. Registry regression and
  product startup restart regression distinguish durable compatibility from
  a foreign/previous runtime handle. No new artificial evidence fixtures.
- Scope: validation of exact observed directories and lexical separation is
  not physical mount/ancestor-alias containment, nor a lock excluding external
  writers. The independent host still owns that guarantee. Provisioning is
  not transactional across namespace I/O failures; this change prevents known
  complete-set layout conflicts before namespace mutation. No automatic source
  activation, recovery by persisted locator, or grant widening.

Validation on L5, installed Rust 1.95.0: original source Process/native route
**2/2**, SourceDriver registry **1/1**, private artifact owner **8/8**, grouped
Git checkpoint/restore/construction recovery **1/1** (159.49 s), full product
startup/restart **1/1**; strict AgentCore/adapter/Host all-target/all-feature
Clippy, full product all-target/all-feature check, scoped fmt/diff passed.
Shared checkout builds include concurrently owned dependencies; only exact
owned paths enter the signed commit. Pinned Nix 1.98.1, Windows/Bubblewrap,
and unchanged broad suites were not rerun. Store v60, Git recovery v4; no
publication.

Next: coordinate original live source-aware Actor/Turn admission and resource
CLI composition when their owners become available. Otherwise choose the next
independent production host physical authority/containment consumer, without
repeating completed provisioning/structural work or claiming that directory
observations establish live external authority.

## Previous local implementation delta — 2026-11-25: persistent signing Vault provisioning

Parent: `e2f3e667fd7438bb317f0fccb19e8e0dfcf48288` (production Git SourceDriver
owner-open registration). Parent signature and clean owned paths verified.
The original Agent/Turn journal owners and resource CLI remain occupied; this
cut consumes the existing VaultProvider, VaultGrant and Process projection
primitives independently without promoting source-bearing admission.

- `LocalSecretStore::open_existing` is a reusable no-initialization open of an
  existing private encrypted backend. Missing directories/master never cause
  creation or replacement. Normal `open` retains its existing initialization
  contract; both paths share the same cipher construction.
- `SigningVaultProvider::open_configured(store, root)` explicitly loads an
  existing owner-private `root/signing.json` and dedicated encrypted backend.
  Requires an exclusive durable Store owner, canonical private provider root
  separate from the Store's canonical directory, bounded v1 config (1 MiB,
  1024 entries), distinct entry IDs/key refs, and exact registered subjects,
  scopes, supported projections and valid signing keys. No partial provider is
  returned on failure. The host registers the result in the existing
  `VaultProviderRegistry`; no implicit discovery, Agent grant, secret copy or
  new database. No provider config or raw key bytes belong in Agent state.
- Host configuration shape: `version: 1`, `provider: DriverKey`, `entries`;
  each entry contains `entry`, `key_ref` (existing opaque SecretStore version),
  `agent_ref`, `name`, `projection` (broker/env), `scope`, `actor`, `manager`.
  `provider` uses the existing DriverKey JSON representation. The config owner
  provisions the dedicated key via the existing SecretStore before activation.
  The provider root and its ancestors must be independently protected from
  Agent access and other writers; the lexical Store-root check is not physical
  mount/alias containment. No automatic product/CLI activation or file watcher.
- Configured mode retains external key-version custody; dynamic `provision`
  is refused, live `revoke` removes the decision without deleting configured
  key material. Withdraw live authority before changing persistent config;
  durable withdrawal requires the host to remove the entry from config as
  well. Explicit reactivation mints fresh provider authority; old Agent grants
  remain stale until current manager explicitly attaches again. There is no
  automatic config reload or regrant. This is not retroactive erasure of
  already materialized Process proofs.
- Both managed and configured signing use the same entry policy construction,
  provider-held live guard and key retrieval. In-memory key digest rejects
  delete/reuse of a key ref with changed bytes at attachment and actual
  broker/Process proof generation. No digest persisted or publicly projected.
  Rotation/revocation stays with the original provider owner.

Validation on L5, installed Rust 1.95.0: signing Vault **2/2** (new real
configured backend/restart plus existing managed-provider regression),
SecretStore **6/6**, original source Process host **2/2**, strict Store/Host
all-target/all-feature Clippy, full product all-target/all-feature check,
scoped fmt/diff passed. No pinned Nix 1.98.1 or Windows/Bubblewrap qualification
claimed; unchanged broad suites not rerun. Shared checkout builds include
concurrent dependency changes; commit tree includes exact owned paths only.
Store v60, Git recovery v4; no publication.

Next: exact live source-aware Actor/Turn admission when original journal
ownership is available. Resource CLI still needs startup composition with its
owner. Otherwise choose one actual independent physical-authority/provisioning
or containment consumer; do not treat configured provider or installed driver
as source execution authority. Remaining Vault projections/host integration
and public surface remain held pending actual authority.

## Previous local implementation delta — 2026-11-25: install production Git SourceDriver at owner open

Parent: `def7bba2549567dce1c06877fd1d59857f7ea213` (trusted local shell
containment). Verified the previous signed checkpoint and clean owned paths.
The interrupted provisioning turn had made no source changes. The original
Agent/Turn journal paths and `src/resources_cli.rs` remain concurrently owned;
this cut does not modify them or enable source-bearing Agent admission.

- `sessions::open_sqlite_owner` registers the real built-in Git driver in the
  existing SourceDriver leaf before the exclusive Store's compatibility pass.
  Workspace-enabled builds use the trusted `AW_GIT` / build-time
  `AW_DEFAULT_GIT` executable selection, never ambient PATH lookup. Missing
  selection means an empty registry and ordinary degraded diagnostics; an
  explicitly selected invalid executable errors before opening the Store.
  No Git invocation, source discovery, layout approval, grant or activation
  occurs at startup. No new registry, configuration schema or execution path.
- Eager/lazy main product owner open, OAuth, scheduled execution and settlement,
  and the existing realm-checked team owner open share this composition.
  `SessionStore::open_with_sources` remains the original reusable Store entry.
  Durable-only and minimal/stateless feature boundaries stay independent of
  Git source effects. Trusted-local direct mode remains unchanged.
- `SourceDriverRegistryReader::handle<T>` and `SessionStore::source_driver_handle`
  resolve a current native definition from the actual installed leaf by stable
  key and concrete type. They reuse existing schema/registration identity and
  `get`, not a parallel catalog or a source authorization token.
- One real Git + SQLite restart regression exercises invalid configured path
  refusal, source-free creation, live Sandbox source metadata, unavailable
  driver degradation, compatible reinstallation even while the checkout is
  absent, type mismatch refusal, held source-bearing construction and ordinary
  source-free activation. Startup schema compatibility remains distinct from
  physical identity, live policy and external-effect authority.

Validation on L5 (installed Rust 1.95.0, not the unavailable pinned Nix
1.98.1 environment): product startup regression **1/1**, typed registry **1/1**,
existing durable SourceBinding migration/restart **1/1**, strict AgentCore/Store
all-target/all-feature Clippy, full product all-target/all-feature check,
durable-only library and minimal all-target checks, scoped fmt/diff all passed.
Durable-only has one existing dead-code warning; minimal has two. Builds include
concurrently owned dependencies in the shared checkout; the signed tree contains
only exact owned changes. Broad unchanged suites and platform-native
qualification not rerun. Store v60, Git recovery v4; no publication.

Next: coordinate the resource CLI owner to adopt the same startup composition;
original live source-aware Actor/Turn admission remains the priority when the
journal owners become available. Otherwise select an independent actual physical
authority/provisioning consumer. Installed Git is not authority to inspect an
arbitrary persisted source or automatically recover an external effect.

## Previous local implementation delta — 2026-11-25: trusted local shell containment

This checkpoint follows the signed comprehensive documentation parent
`cf0b131e3f6e7beb681060f4cbdaf430ff0df2b1` (implementation predecessor
`2b4fac33d8345f284ecfd3e0585f332c72037dc6`). Phase 3 remains underway. The
original Agent/Turn journal paths (`aw-agent-core/src/agent.rs`,
`aw-agent-core/src/journal.rs`, `aw-store/src/journal.rs`) remain concurrently
occupied; source-bearing production admission stays held. The dated comprehensive
handoff remains the historical design and implementation index, with this live
delta superseding its local checkpoint position.

- The existing trusted local `LocalToolOptions` supplies a typed
  `local_process_containment` minimum for the configured native shell only.
  Both Bash and PowerShell construction feed that minimum into the original
  shared shell registry. Default/CLI behavior stays `BestAvailable`; there is
  no new CLI/model selector, delegated authority, or filesystem isolation claim.
- `PipeSpec::prepare` validates host input and produces the same owned
  `PreparedPipeLaunch` as the existing driver path. The prepared Job-output
  launch shares the original spawn-control/custody and Process owner with
  ordinary prepared launch. Both local direct execution and the original
  finite Job/Graph ShellNode consume that capability. The source-fenced launch
  and its original independent authority/Prepared custody are unchanged.
- Exact shell runtime bindings include the stronger minimum without changing
  historical `BestAvailable` identities; subsequent weakening cannot satisfy a
  sealed strict plan. The original `JobHost` settles typed `NotStarted` versus
  `Unknown`, without inventing Process terminals. Foreground known no-launch
  failures project their actual retained diagnostic instead of an exit label.
  Strict closed-descendant containment refuses current Unix DirectScope before
  spawn; the Windows native Job path remains unqualified on this host.
- One grouped regression exercises standalone direct, foreground/background
  finite and multi-node Graph shell dispatch through the configured runtime:
  no markers, no fabricated ProcessTerminal, finished empty output streams,
  factual retained denial, exact strict runtime bindings. Existing default
  behavior and monotonic binding tests remain covered. The shell fixture now
  retains the actual configured JobRuntime instead of constructing a duplicate.

Validation on L5: grouped regression **1/1**, complete adapter all-feature
library suite **259/259**, strict adapter all-target/all-feature Clippy, original
source Process host regressions **2/2** with `git-source-effects`, full and
minimal `aw-cli` all-target checks, scoped Rust formatting and owned-path diff
checks passed. Minimal retains two existing dead-code warnings. The pinned
1.98.1 Nix shell could not evaluate offline (missing flake input/SSL CA setup);
validation used the existing installed/cached Rust 1.95.0 toolchain. Literal-only
minimal CLI and web-test changes preserve their pre-existing formatting. Builds
ran in the shared live checkout including concurrently owned dependencies; the
signed checkpoint tree is constructed from the exact expected parent plus only
these owned paths. Broad Store/workspace runtime and Windows/Bubblewrap native
qualification were not rerun. No schema changes (Store v60, Git recovery v4),
release/mount changes, UX changes, or publication.

Next: exact live source-aware Actor/Turn admission across the original Agent,
Actor, TurnJournal, context and queue owners once ownership is available;
otherwise continue one independent production physical authority/provisioning or
containment consumer. Do not repeat the completed structural mutations or enable
unsupported routes.

## Current authoritative snapshot — 2026-11-25

**R6 + the shared Process/release-correction amendment are SEALED and published. Phase 0B is
complete and published. Phase 1 is COMPLETE and published. Phase 2 is COMPLETE and published: v44 Event projection,
one shared private EventScan, v45 source-local Event authorship, v46 durable Event read/wait, v47
exact-Project wildcard placement, v48 durable subscription state, v49 replay-safe typed EventInput delivery,
v50 disclosure/causal execution closure, v51 EventTypeRegistry + custom Event publication, the model-facing
`events_publish` / `events_subscribe` adapter, v52 EventHandler definition/activation/receipt ownership,
v53 same-store cross-Domain Event-universe completeness, and v54 serialized v36 delivery cutover +
compatibility provenance are implemented locally. Phase 3 is now UNDERWAY with the first final
SourceDriver leaf plus durable DriverKey/Prepared/ExternalEvidence vocabulary and v55 named
SourceBinding persistence/restart compatibility, built-in Git clean/dirty capture/private retention,
explicit gitlink semantics, isolated Prepared restore, v56 immutable Checkpoint/SourceSet
persistence with trusted history-only replay, v57 indexed construction receipt/source
recovery custody, Store-local complete-set source mutation barriers, receipt-bound
checkpoint capture preparation, exact restart inspection/recovery and evidence-fenced atomic
checkpoint publication, receipt-bound non-destructive restore preparation/restart inspection
and atomic held Agent + fresh source-bearing Sandbox restore publication, plus v58 independent
SandboxGrant evidence/source-access preflight, typed SandboxDriver/fenced source-effect
handoff, original PipeProcess source-aware host composition and direct journal-linked
source-effect custody/terminal recovery, and pinned native single-source filesystem route
with deferred post-journal settlement, concrete registered Git materialization resolution,
root-free native-only tool registration, live native source approval, explicit Actor host
activation selection, quiescent metadata-only source binding mutation, checkpoint
retirement/retained-anchor reconciliation, source-bearing Sandbox logical close and
exact closed-Sandbox materialization reconciliation with proven retained-layout release,
managed Git restore-materialization release with original Applied restore provenance,
metadata-only fresh baseline restore with explicit None/empty Auto coverage, and
unpublished capture/restore candidate reconciliation, shared custom initial history
ACL construction, captured-source restore initial-task custody/publication,
task-only baseline restore/recovery, concrete fenced Git checkpoint capture/restore/
retention reconciliation, managed closed-Sandbox retirement, effectful versioned
baseline restore host consumers, and v59 Agent VaultGrant references with the final
staged VaultProvider leaf, concrete local signing broker, fenced Process proof
projection, interrupted source-effect custody recovery and worker-owned native
and finite Process quiescence, marked Git private-index scratch reconciliation,
exact-subset unpublished Git restore abandonment and verified metadata-only final
source detachment, independently approved multi-source native filesystem
realization, durable native artifact custody/recovery and exact Store-backed Git
native approval provisioning, shared finite Vault Process settlement, v60
Project-scoped standalone versioned Sandbox construction, same-Project
required-source Sandbox derivation, exact managed-source structural removal,
versioned attachment (including first source into an empty layout) and replacement
with per-binding materialization provenance, exact partial replacement recovery,
and enforced minimum Process containment are implemented locally.
Materialized/provider audience expansion remains Unknown until its provider-held principal fence exists, but
it no longer blocks Phase-2 completion. Real-Bubblewrap worker-retirement and Windows-native Process
qualification remain explicitly deferred evidence and are not claimed green.**

Verified publication truth:

```text
AW remote main                  4ddeed8d0023cd0222633fc496f0d252e3dda54c = Share finite source Process settlement with Vault projection
AW R6/Phase-0 publication       20a63a4d4547cc20b2a154787823125b82f5e385
dotlink remote/main             85ceccc1088e201c98c994a46a68834440eeab32
release/0.17                    576d0a1a3fe0a5522a02d51f1d0598c3ddc160c2 = v0.17.9
dotlink AW pin                  20a63a4d4547cc20b2a154787823125b82f5e385
```

The Phase-1/2 and Phase-3 line through 4ddeed8 was pushed and independently
verified on 2026-11-25. The eight subsequent implementation checkpoints through
`2b4fac33d8345f284ecfd3e0585f332c72037dc6` (Enforce trusted minimum Process
containment before launch) remain local. The interrupted validation job completed
successfully; exact parent, signature and clean owned paths were independently
revalidated at the comprehensive handoff. Store schema v60, Git recovery v4.
Phase-1 completion is
`7f4724f942e4d90a6eaaca77af6c528b37a2262d` (`Expose Agent state RPCs`). Phase 2 began at
`14b53872236d2af262d24a2ad9529c4b652c2756` (`Start Event projection foundation`) and closes at
`e0c6bb3184bf87fc1bb2186ddc7f5ffb97a39c96` (`Cut over Event delivery ownership`). Phase 3 starts at
`184ea6b05643c49d34a7f3bfdbfed86f26753162` (`Establish SourceDriver recovery contracts`). The
intermediate private EventScan/source/read/wake/Project/subscription/EventInput/disclosure/custom-publication/
model-tool/Handler/universe checkpoints remain in Git history. The latest verified
remote main and local tracking ref agreed at 4ddeed8 before the v60 work.

Pinned qualification is green: ordinary all-target/all-feature build, shell **86/86**, Graph
**13/13**, real queued Spawn-Lane cancellation, pre-spawn timeout/no-Process projection, canonical
spawn refusal, strict Clippy, fmt and diff-check. Real Bubblewrap worker-retirement and
Windows-native qualification remain explicitly deferred evidence, not claimed green.

Phase 1 has advanced well beyond the initial StoreScope cut. One private `StoreScope` still reuses
existing `store_id + restore_generation`, and transparent `StorePos(u64)` carries store-local
`commit_log.seq` ordering without becoming a global clock. Schema v38 makes
`agent_state_current + agent_acl_current` authoritative for Agent envelope/ACL state; schema v39 adds
the Domain principal/group owner; schema v40 adds immutable HistorySpan/Base/Grant + the persistent
DisclosureManifest DAG. Agent-facing Jobs/wake/history authority now uses the full Agent ACL owner,
not Session participation.

`agent_state_current` owns frozen canonical handles/backing Session+branch, owner, title, Agent-local
`rev`/`acl_rev`, lifecycle/admission, immutable delegation parent lineage and history/Sandbox
pointers. Migration seeded the initial rev from the exact legacy `MAX(audit.seq)+1` bridge; after
activation only Agent-owned mutations advance it. Turn admission/result/attachment, accepted Input,
Applied Actor Control, direct child/close/recovery and exact title/ACL mutations use that owner.
Reads, Process output, cursor reads and group-membership churn do not manufacture Agent rev.

Agent access is now the full seven-permission allow-only model
`view/contribute/submit/control/derive/share/manage`, with owner rights UNION direct user grant UNION
matching active group grants. Group/security changes fence the opaque `access_generation` without
Agent-rev fanout. Wake receipts and history cursors bind to that generation; mixed exposure remains a
candidate projection followed by current authoritative access recheck. Legacy Session title and
participation are backing compatibility only, not Agent revision/access truth.

Schema **v39** now owns the Domain principal namespace and v1 groups. `domain_principals` enforces
one `(domain, handle)` namespace across user/group kinds; existing stable `usr_*` handles are
backfilled from Domain membership, Agent ownership and direct Agent ACLs without renaming canonical
Agent refs. Domain-membership inserts register user principals at the database boundary, while group
membership/direct Agent grants can register authenticated non-member users on demand. Authentication
status/security epochs remain owned by `users`; principal registration never becomes a second login
or security owner.

`principal_groups` + `principal_group_members` implement the sealed simple group model: one owning
user, no nested groups, server-minted immutable handle, mutable presentation name, complete-set
membership replacement under one monotonic group rev, bounded management reads/listing and tombstone
deletion. Delete refuses current Agent ownership/ACL references; historical identity is retained and
handles are never reused. Group create/edit/delete publish immutable principal facts, but membership
changes deliberately do **not** fan out Agent writes or revisions. The minimal `domain_principal(s)`
snapshot exposes principal kind/status/display identity without leaking a group roster.

Group-aware Agent access now uses the **full seven-permission** owner model without another schema
migration. `AgentPermissions` covers `view/contribute/submit/control/derive/share/manage`; owner
rights UNION direct user grant UNION every active matching group grant are resolved by one bounded
`agent_owner::authorize_permission` path. Existing Jobs five-action callers are a zero-policy adapter
over that same function. Group-owned Agents remain authorized through exact live membership. The hot
query streams at most 512 ordered ACL dependencies rather than building a heap-side grant map.

Every successful authorization returns the caller's full effective permission set plus one opaque
32-byte `access_generation` derived from the exact Agent + actor, Agent `acl_rev`, active user security
epoch, owner kind/handle, direct/group grant revisions, exact group handles/revs and all seven
effective permission bits. The generation digest moved to v2 for the expanded permission vocabulary;
it remains an observation/security fence, never an Agent mutation fence. Group membership/name edits
advance only group rev and never Agent rev/acl_rev.

`agent_access_list` is the bounded management snapshot: canonical owner principal, direct user/group
ACL rows, Agent `rev`, `acl_rev`, caller effective permissions and `access_generation`, without group
roster expansion. `agent_access_set` resolves one canonical `/d:/i:` principal through the v39
registry, replaces/removes its complete seven-permission grant under Agent-rev CAS, and advances
`acl_rev + rev` exactly once. Stored non-view permissions still require explicit `view`; owner ACL
rows remain forbidden. The previous direct-user/group five-permission setters are now thin identity
adapters into this canonical path; their duplicate ACL SQL/policy path is gone.

Agent history cursors and wake v3 authority continue to bind to `access_generation`; because the
seven-bit set participates in that generation, derive/share or group/security changes fence old
security observations without manufacturing Agent revisions. Mixed Agent/finite-Job candidate SQL
remains group-aware before exact row-level authorization.

Schema **v40** now installs the immutable history/disclosure owner lazily, with no eager Session
rewrite. `history_spans` retain exact backing Session/branch/root/head boundaries plus raw immutable
node-content/provenance digests; `history_bases` structurally compose inherited foreign prefix + one
complete local branch tail and retain total/next Turn coordinates. Re-sealing a branch now skips any
same-branch base layers before choosing the inherited parent, so repeated seals/compactions replace
rather than duplicate that local tail. `history_grants` + normalized disclosed-principal rows are now
actively used by ACL audience expansion and exact-history Agent/settled-Turn fork construction.

`DisclosureManifest` is represented as a persistent content-addressed bounded-fanout DAG rather
than flattened JSON: immutable authority roots, leaf entries and branch child links use SHA-256
content identities; 64-way nodes structurally share prior roots. Current legacy provenance extraction
conservatively captures transitive `node_dependencies` as exact source-node facts + stable legacy
Session authority roots, and materialized facts as exact fact hashes + materialization-policy roots.
The per-seal extraction bound is 16,384 inputs and fails safely rather than flattening an unbounded
closure. Parent/local manifest composition reuses the existing parent root and adds the local root;
future authorization traversal can deduplicate repeated authority roots without rewriting history.

Lazy sealing is a production path: successful exact-branch `checkpoint` sealing happens in the same
SQLite transaction after the new checkpoint head is established. Seal keys memoize exact boundaries;
sealing emits no Agent change and never advances Agent `rev`. HistoryBase content identity includes
local fact content, provenance and manifest identity, so identical bytes under different protected
provenance cannot alias.

A genuinely new ACL viewer on an Agent with committed/inherited history now goes through v40
HistoryGrant expansion in the **same ACL transaction**. The caller must hold current Agent `share`;
the exact current history boundary is sealed; the manifest's deduplicated authority roots are walked
under bounded work; already-disclosed user/group principal identities are preserved; and a new
immutable superseding HistoryGrant is minted before the single ACL rev bump. Re-adding a principal
already present in the current immutable grant does not require `share`, because that history was
already factually disclosed. Narrow/removal never rewrites old grants. Legacy-Session authority roots
allow expansion only when the target already has source visibility or the acting source owner may
share non-private content. Materialized external-policy roots still fail closed because no reviewed
principal-level source-share contract exists yet.

Exact-history **Agent-ref** destination construction is now live as one lower Store primitive:
`create_agent_from_agent_history(..., access=private|inherit)`. It requires current source `derive`,
seals the source Agent's exact latest committed boundary, validates the v40 disclosure closure, and
creates the destination Session/Agent + HistoryGrant + ACL + Turn cursor in one transaction. The new
Agent stays in the source Project with its Project owner in this cut, while the caller supplies the
new execution binding explicitly; history ancestry therefore does not copy Sandbox/source state.

The destination is a physically fresh Session containing only its own root node. Its Agent points at
the shared immutable HistoryBase/HistoryGrant, `sessions.next_turn` starts at the inherited base's
`next_turn`, and ordinary `history_in`/activation now render `flatten(inherited HistoryBase spans) +
destination-local projected tail` through the existing context renderer. Every inherited backing node
is re-hashed against the immutable digest recorded in its span before projection. `inherit` copies
source direct user/group ACL rows as fresh destination-owned rows with grant rev 1; `private` carries
only the canonical owner. The source and destination ACLs then evolve independently while shared
group principals remain live references.

An active source Agent is handled exactly: because Turn admission moves the branch head to an
unresolved prompt, the constructor reads the active run's durable `produced_from_head` + `turn_id` and
seals that prior committed boundary. Thus a fork created during Turn N inherits only through N-1 and
starts its first divergent Turn at N. No active run, prompt, staged input, Process/Graph object, watch,
or source Agent revision is inherited. Re-sealing uses the same content-addressed HistoryBase rows.

Fork disclosure validation preserves immutable prior disclosure: for a principal already covered by
the source's current HistoryGrant, previously granted legacy authority roots are not reauthorized;
new roots are. Materialized/external roots remain fail-closed on every new destination because their
revocable source-equivalent share contract is still not implemented. Destination HistoryGrants start
a fresh lineage (`supersedes=NULL`); source grants are evidence only, never the destination's mutable
security owner.

Exact-history selector resolution and the model constructor adapter are now live over the same lower
owner. The canonical Store-side host adapter is `create_agent_from_history_ref`: it accepts only an
absolute canonical Agent ref or settled Turn ref, reuses the shared destination publication helper,
and therefore keeps one disclosure/HistoryGrant/ACL/publication path. A settled Turn selector resolves
an exact attached local result when present, otherwise walks the structurally shared HistoryBase chain
to the inherited immutable Turn fact. Canonical decimal Turn handles are enforced; active/unsettled
Turns fail closed rather than falling back to the mutable Agent head.

Model `agent(task,...,from?)` now accepts contextual Agent/Turn refs through the existing
`ExecutionRef` grammar. The caller surface resolves `.`/relative/parent context to one absolute ref,
requires the model-selected source to remain in the current Project, and passes only that canonical
ref into the durable child-allocation transaction. Source `derive`, exact boundary selection and
HistoryGrant disclosure are rechecked by the Store before child publication. The fresh child Session
is created first inside that transaction and history is attached before the existing subagent/replay
facts commit, so a disclosure failure leaves no half-published child.

Constructor replay identity is semantic: canonical `from` is part of the fingerprint, so `from="."`
and the equivalent absolute ref replay the same committed child. Requests with no `from` deliberately
retain the pre-feature fingerprint JSON shape byte-for-byte, preserving previously committed Agent
constructor replay across upgrade. Policy/operation delegates continue to pass no history source.

The first external host Agent **state-shell** cut is now live locally over the existing owners; it does
not create another Agent/Session/Project/Sandbox owner or publication path. `agent_host` provides
project-scoped row-authorized `agent.list`, authoritative `agent.get`, title-only `agent.edit`,
`agent.admission`, canonical-ref `agent.access.list/set` adapters, and the lower close-request seam.
The host create request keeps trusted context separate from caller fields: the host supplies the
current Project/current Agent and already-resolved execution `SessionBinding`; `from` resolves to the
existing canonical Agent/settled-Turn history owner; Project defaults to source/current; and
`private|inherit|custom` access is installed inside the **same immediate construction transaction**.
The history constructor was factored only into an in-transaction helper so both its existing public
adapter and the host shell use the same source-resolution, disclosure, destination creation and
history-attachment path. Custom initial ACL rows use the existing disclosure-expansion owner before
publication rather than publishing private and widening afterward.

The Agent admission state is now a real execution gate, not presentation-only state:
`actor_input_prepare` checks authoritative Agent lifecycle/admission under the same writer transaction
before allocating a future Input fact, then retains the existing model-pause gate. Holding admission
therefore lets the current Turn settle while refusing future Turn admission. Close-request atomically
moves lifecycle to `closing`/admission to `held`; `agent.admission` cannot reopen a closing/closed
Agent. The currently exposed `agent_request_close` is deliberately only the **lower host seam**.
The existing stable-Agent runtime close primitive is now stronger and reusable: one shared
`LiveAgentControl` owns the close bit plus the currently active `CancellationToken`; every stable Turn
registers that token only for its execution interval, and close cancels it before queue/worker
settlement. Background construction arms cancellation before the constructor handoff can return, so a
close racing the first paid Turn cannot fall into the old wait-for-provider-completion gap. Repeated
close uses the same level-triggered close state and does not enqueue duplicate close commands. There is
still no second runtime/control owner or background cancellation task.

Recipient subscription retirement is now also owned by the close transaction without introducing the
Phase-2 Event engine. `SourceEventBook::retire_recipient` removes one receiver without needing the old
opaque admission token; the Store close seam applies it across the existing legacy source books, drops
already-pending deliveries to that receiver, deletes its durable wake admission, and advances each
affected source Agent revision **once** even when both root and Turn sources changed. This is deliberately
a cold close-path scan over the transitional JSON books rather than a new schema/index that Phase 2
would immediately replace. Delegated/model close performs the same retirement (including replay repair)
and evicts matching in-memory wake admissions before signalling the live worker.

Finite Job ownership now has the missing reusable lifecycle seam too, without moving Job facts out of
`JobManager`. `JobRuntime::close_owner` fences new finite-work admission for one exact Agent Session,
requests ordinary factual cancellation through the existing `JobHost`/`JobManager` for every live
runner, signals those exact runner tokens, waits for runner quiescence, and returns success only when
`JobManager::has_nonterminal_jobs()` proves there is no journal ambiguity, in-flight transition or live
writer. Unresolved work returns conflict rather than manufacturing terminality. Tool configuration now
has a host-facing `configure_with_runtime` variant that returns the **same Arc<JobRuntime>** installed
into the Agent builder, so root/stable lifecycle composition can retain the real finite-work owner
without constructing a parallel runtime. The old `configure`/`configure_with_toolset` APIs and the
ToolMode::Off zero-runtime fast path are unchanged for existing callers.

Stable-Agent close now composes that exact finite-work owner before terminal settlement. Child tool
configuration uses `configure_with_runtime`; the resulting `Prepared` retains the same `Arc<JobRuntime>`
already installed into the Agent builder. Once the active Turn has been cancelled/settled and queued
future Inputs are deleted, stable close awaits `JobRuntime::close_owner`; only a proven-quiescent result
may proceed to the existing atomic `settle_agent_job` / Agent `closed` transition and live-owner removal.
If finite work remains unresolved, the Agent stays `closing`, its worker/router owner remains available
for inspection/reconciliation, and no terminal fact is fabricated. Close signalling now has one tiny
`close_queued` bit solely to serialize close attempts: duplicate requests while one attempt is in flight
do not enqueue duplicates, while a failed attempt clears that bit so a later authenticated close can
retry after reconciliation. This does not add lifecycle authority; `closing` remains durable Store state.

Full SPEC `agent.close` therefore now has complete stable-child terminal composition. Root/default
close now also has its two missing lower factual seams. `actor_owner::Service::cancel_active_turn_for_agent_close`
is a trusted-host lifecycle operation, not a model Tool: it binds replay identity to the durable Agent
close revision plus exact admitted Turn/Input, reuses the existing Actor Control Prepared ->
Applied/Unknown owner, and returns `None` only when both the live owner and durable branch prove no
active Turn. `SessionStore::agent_finish_close` is the matching terminal Store seam: it accepts only a
previously requested `closing` Agent, rechecks under one writer transaction that no journaled Turn is
running, advances once to `closed`, and is idempotent thereafter. It intentionally knows nothing about
finite Jobs; the outer host must prove `JobRuntime::close_owner` first. Root/native Host now retains the
exact configured `Arc<JobRuntime>` through the opt-in runtime-returning construction path and one
transport-neutral `RootAgentClose` composes the existing owners in order: durable close request + pending
queue retirement, exact current-Turn cancel/withdrawal, finite-work close, factual Turn quiescence,
terminal Store close, then live-router retirement. It owns no second lifecycle state.

Taskless host-created Agents now have the missing reconstructible live-runtime activation path without
changing publication semantics. `HostAgentActivation` accepts only an already-published canonical Agent
root, derives the authoritative child Session owner/binding/history, rebuilds the exact trusted provider
selection + tool envelope, registers the normal Actor owner/Router wake, and starts one long-lived worker
for future queued Turns. Activation returns a bounded `ActivatedHostAgent` handle and is post-publication
cache/runtime state only; it cannot construct an Agent or bypass Store authority. The wake/close/current-
Turn cancellation primitive is now shared with stable model-created Agents through one reusable
`LiveAgentControl` / `LiveAgentWakeHandle` / `live_agent_channel` implementation rather than parallel
copies.

Task-bearing host construction now satisfies the sealed publication invariant rather than queueing after
publication. The host constructor pre-mints exact child Session/root/branch IDs, durably claims a compact
`agent.create.preparation` receipt on the bound current Agent, then writes the initial task into the
existing encrypted SecretStore queue custody under an opaque operation-owned ref. The receipt contains
only normalized semantic fingerprint, candidate IDs, task digest, queue ID and opaque custody ref—never
task plaintext. The final single `BEGIN IMMEDIATE` publication transaction revalidates Project/history/
ACL authority, creates/forks the Agent using those candidate IDs, appends the ordinary Pending Actor queue
fact, emits `agent.created`, and transitions the same preparation receipt `Prepared -> Applied` with the
canonical Agent ref. That commit is the only public linearization point. Definitive precommit failure
cleans the unique custody blob and consumes the receipt as `Rejected`; uncertain custody/publication keeps
exact recovery identity as `Unknown` and never automatically retries. Exact Applied replay returns the
same Agent; changed semantics under the same request ID are denied. No constructor table, second replay
ledger or public `preparing` Agent lifecycle was added.

`HostAgentActivation` now performs one ordinary wake only after its worker reports ready. Therefore an
atomically published initial Pending task (or another restart-recoverable Pending input) is claimed by the
normal queue/Actor owner after publication; Claimed/Accepted ambiguity is still non-replayable. A focused
end-to-end regression proves task-bearing Store create -> activation -> exactly one provider Turn ->
authoritative history advance -> normal close.

Native/public `agent.create` composition is now complete for the current Phase-1 destination policy.
`configured_provider_selection_for_actor` rebuilds the trusted current profile under the destination
Project's canonical stable-user principal instead of rewriting the caller's selected identity. Local and
OAuth profiles therefore create a fresh owner-only personal `AccountDescriptor` whose actor/owner/payer is
that stable principal; OAuth connector lookup reuses the Store's same-stable-user ownership proof. An
explicit persisted server account still passes its existing Store grant/version/provider-kind checks and
is selected for the stable child actor. The RPC cannot supply provider/model/account/execution fields.

The Agent Router now supports exact-actor facades over one structurally shared live-owner/wake/revision
registry through `Router::for_actor`. The local root alias and canonical stable child principal therefore
address the same Agent graph without copying live state or weakening exact ToolContext actor checks.
`HostAgentActivation` first performs an authorized binding lookup plus a cheap `has_live_owner` replay
fast path, then uses atomic register-if-absent as the race fence; exact create replay never replaces or
spawns a second worker. `HostAgentCreator` owns only composition: Store create/replay with the stable host
request ID, then idempotent activation.

Native Host retains one lazy `StdioAgentCreatePort` alongside the existing close owner. Stdio now exposes
`agent_create` concurrently with a running root Turn and accepts exactly the sealed fields `from`,
`project`, `title`, `task`, `access`, and `sandbox`; the authenticated RPC frame ID is the durable create
request identity. Provider/model/credential/execution authority is captured from trusted Host state only.
Explicit `--account` has the same precedence as root prompt dispatch; otherwise current local/OAuth
profile policy is used. A frame containing an authority-bearing extra field such as `provider` is rejected
before the create port executes.

Logical Sandbox ownership and Agent binding are now real Phase-1 state rather than an implied execution
binding. Schema v41 adds one minimal `sandboxes_current` owner whose `sandbox_id` is the canonical
`/d:/i:/p:/s:` ref itself. It owns only Project identity, immutable logical execution layout
(`working_directory` + `tool_root` + digest), optional parent ref, `rev`, and `open|closed` lifecycle.
It does **not** prebuild SourceBindings, SandboxGrant policy, mount/network/device realization, Vault
rights or Process lifetime. Those remain separate later owners, and frozen dotlink 017 sandbox/mount work
is untouched.

The v40 -> v41 migration transactionally binds every existing authoritative Agent. Exact Project+layout
peers share one Sandbox, the first Project layout gets `/s:0`, and distinct mutable layouts never collapse.
The generic Agent constructor auto-binds only when the v41 table exists, so older v38-v40 migrations remain
valid; v41 then seeds the historical rows. New constructors after v41 always get a logical Sandbox. Store
now exposes reusable `sandbox_get`, bounded `sandbox_list`, trusted-layout `sandbox_create_fresh`, and
idempotent `sandbox_close`; close refuses while any `open|closing` Agent is still bound. Derived
`sandbox.create(from=...)` remains intentionally unimplemented until the SourceDriver/checkpoint owner can
preserve required source state correctly.

Agent Sandbox selection is immutable at creation. `HostAgentCreator` captures the current root Agent's
already-authoritative Sandbox ref as trusted `sandbox=auto` policy. `agent.create` may instead name an
explicit Sandbox ref, but Store revalidates that it is open, belongs to the **exact destination Project**,
and has the exact trusted execution layout. The choice is written inside the Agent publication transaction
without an Agent rev bump and there is still no `agent.sandbox.set`/rebind path. History ancestry never
selects or grants a Sandbox. Binding the Sandbox grants no new tool/source/network/device authority.

Native/stdio Sandbox observation/lifecycle is now exposed through one current-Project-bound
`SandboxPort`. `sandbox_list(project?,limit?,cursor?)`, `sandbox_get(ref,include?)` and
`sandbox_close(ref,rev?)` run on the concurrent reader lane like Agent lifecycle operations. The port
accepts no layout/mount/network/device/provider authority fields, defaults list to the trusted current
Project, refuses cross-Project refs, and projects only public logical Sandbox identity/rev/lifecycle/
parent data. `include=["sources"]` truthfully returns an empty bounded source list until Phase 3 installs
SourceBindings. Pagination remains a bounded non-authority continuation over the Store's exact Project-
bound list order; no Phase-4 cursor framework was pulled forward merely for this surface.

`sandbox_create` is intentionally advertised unavailable. A tempting resource-row-local replay receipt
was reviewed and rejected before commit: real fresh/derived Sandbox creation requires filesystem/source
materialization, so the sealed Prepared claim must exist **before** that external effect and cannot live
only on a not-yet-public Sandbox row. Exposing the current lower `sandbox_create_fresh` would also falsely
promise an isolated mutable environment while SandboxDriver/SourceDriver realization does not exist.
Public create therefore remains Phase-3 work using the shared Prepared + SourceDriver/checkpoint path;
we did not add temporary v42 schema/replay machinery that would immediately become wrong.

Host `agent.close` now routes non-current live Agents as well as the current root without adding another
lifecycle owner. One `HostAgentClose` keeps the retained `RootAgentClose` for `.`/the exact current root;
other canonical Agent roots first pass a read-only Store `control` + optional-revision preflight (so live
runtime topology is never disclosed to a view-only caller), then require an existing shared-Router live
close controller. The final Store `agent_request_close` transaction revalidates authority/revision and
atomically publishes `closing/held` + recipient retirement before the existing live worker is signalled.
Host-created workers replay through `RootAgentClose`; delegated workers use their existing factual
`settle_agent_job` path. The host router does not classify or duplicate those runtimes. A controller race
after the durable request surfaces explicit conflict and leaves inspectable `closing` state rather than
fabricating terminal success. Already-closed replay still reauthorizes `control` and is idempotent.

Cross-Project host `agent.create` redirect is now implemented for the current native/public trust model.
The destination Project is reauthorized before preparation claim, at preparation commit, and at final
publication. Its `i:` principal must be the authenticated caller's canonical stable user (raw `project=`
can never impersonate another principal), that user must have active destination Domain membership, and
the destination Workspace must have the current writer. Fresh and exact-history construction use the same
existing Agent publication owners. Redirected history requires source `share` in addition to `derive`;
the lower destination-aware history core rechecks that invariant in the final transaction and validates
the destination owner plus inherited/custom audience against the existing disclosure manifest closure.
Same-Project model/history constructors retain their old source-Project default unchanged.

Because truthful Sandbox materialization is intentionally Phase 3, any destination Project different from
the current host Project requires an **explicit open Sandbox in that destination Project**. `sandbox=auto`
redirect fails with conflict rather than pretending the current Sandbox is portable or creating another
logical ID over the same mutable environment. The existing task-bearing Prepared receipt already
fingerprints Project + Sandbox, so redirected task create/replay needs no second replay owner. Native host
activation continues under the same stable user principal and therefore needs no identity rewrite.

Schema **v42** now installs the mutable host-staged Agent input-plan owner without reusing runtime
`ActorQueue` facts as drafts. Every authoritative Agent has one `agent_input_plans` row with independent
`inputs_rev`; staged items use opaque `q:` IDs, bounded sparse integer ordering, immutable creator/source
provenance, mutable last-editor metadata, exact followup/steer/interrupt Turn intent and encrypted
SecretStore payload custody. There is no per-item revision or second history/journal owner. New Agents are
seeded through the generic Agent constructor only when v42 exists; the v41->v42 migration seeds all
existing Agents at `inputs_rev=1`.

The lower staged owner currently implements `list/add/edit/delete/reorder`. `view` reads the plan;
followups require `contribute`; steer/interrupt drafts require `control`; editing/deleting another stable
user's draft additionally requires `manage`. Blind append may omit `inputs_rev`, while relative insertion,
edit and delete use the one plan CAS. Steer/interrupt intent must name the exact current active Turn;
followup `after` must name an already-known Turn. Successful mutations advance `inputs_rev` exactly once,
emit immutable `agent.changed {aspect:"inputs", inputs_rev}` in the same transaction, and deliberately do
**not** advance Agent `rev`. Once Agent lifecycle leaves `open`, list remains readable while draft mutation
freezes. Payload text is never written into audit/commit-log rows.

Schema **v43** adds one compact durable replay owner for staged-input mutations, keyed by authenticated
stable user + host request ID with operation/fingerprint/target/state/result revision and bounded non-secret
result metadata only. It is deliberately one table for add/edit/delete/send, not per-method ledgers.
`add/edit/delete/send` now all consume that same owner: exact request replay is checked before new
SecretStore custody, same identity with changed semantics conflicts, and fresh pure draft mutations commit
the mutation + Applied replay receipt in one transaction. Only Send uses Prepared/Rejected/Unknown because
it crosses the live Runtime boundary.

Atomic staged **Send is now complete internally** through one transport-neutral `HostAgentInputs`
coordinator. Store first authorizes and fingerprints the complete selected set before any Router topology
lookup. Followups require `submit`; steer/interrupt require `control`; other users' drafts additionally
require `manage`; lifecycle/input-plan revision and exact Turn targets are rechecked. The existing Runtime
allows one pending same-Turn authority update, so one Send may contain any number of followups plus at most
one steer/interrupt; a selection with multiple controls conflicts atomically and leaves every draft staged.

For a control item, the v43 mutation receipt enters `Prepared` before the existing `AuthoritySteer`
preparation; the already-live `actor_owner::Service` stages the exact pending segment but owns no policy or
new durable state. The final SQLite transaction revalidates the whole plan, accepts that exact control
segment through the existing authority owner, publishes every selected followup through existing
`ActorQueue` prepared custody, removes all selected drafts, advances `inputs_rev` once, advances Agent
`rev` once, emits `agent.changed {aspect:"inputs"}`, and transitions the replay receipt to Applied. A
precommit failure withdraws the exact live segment and rejects the Prepared control; uncertain withdrawal
marks both owners Unknown and forbids blind retry. Followup-only Send needs no external Prepared phase and
publishes its Applied replay receipt in the same final transaction. Existing staged SecretStore custody is
transferred directly to ActorQueue for followups; only a control draft's original staged blob is retired
after commit because AuthoritySteer owns its own prepared encrypted copy.

The public/native host now exposes all five staged-input methods together through one concurrent
`AgentInputsPort`: `agent_inputs_list`, `agent_inputs_add`, `agent_inputs_edit`, `agent_inputs_delete`, and
`agent_inputs_send`. The RPC frame ID is the sole durable mutation identity for add/edit/delete/send; there
is no request-id field in method JSON. Strict top-level field whitelists reject provider/model/Sandbox/tool
or other authority-bearing extras before dispatch. Item/intent bodies reuse the typed Store structs with
unknown-field denial, refs default to `.`, non-current targets require canonical Agent refs, edit/delete/send
require a positive observed `inputs_rev`, and omitted Send IDs mean all staged items. The operations run on
the concurrent reader lane so exact active-Turn steer/interrupt can be submitted while the provider Turn is
in flight; only Send consults Router/live Runtime.

The host-created Agent regression proves staged followup -> atomic Send -> existing queue wake -> one
second Turn, Agent-rev advancement, and exact same-request Send replay with no duplicate execution. Existing
same-Turn steer and interrupt custody/AuthoritySegment regressions remain green after factoring the
crate-private transactional accept helper. The consolidated v43 Store regression now also proves durable
add/edit/delete replay is a no-op and changed semantics under the same host request identity conflict.
The strict headless regression advertises all five capabilities together, dispatches a valid list once and
rejects an authority-bearing extra field before the port.

Committed history provenance now has one authoritative projection rather than copied fields on every hot
record. `TurnFactProvenance {original_agent, original_turn, actor, created_at}` is reconstructed in batches
from the existing v40 structural history owner: inherited spans reuse their immutable `source_agent_ref` +
verified backing nodes, while current local continuation uses the authoritative current Agent ref + branch
ancestry. The inherited-span verifier is shared with effective-history projection, so provenance reads do
not create a second history walker, table or lazy-seal write. Conflicting duplicate Turn attribution fails
closed.

Explicit `jobs_read` history records now attach that same provenance envelope to message, assistant result
and redacted tool-call records. Natural/latest result reads return before the provenance lookup and preserve
the old plain-content path, so model/default ergonomics and prefix/cache semantics do not change. Tool and
model invocation structs themselves remain untouched: their factual lineage stays inside the immutable
TurnJournal and inherits the Turn envelope. Exact source/fork/refork tests prove inherited Turn provenance
remains byte-for-byte original rather than being re-authored under the destination Agent.

New model-settlement commit evidence (`llm.attempt_settled`) now also carries canonical `agent_ref` + exact
`turn` alongside retained internal Session/branch/run locators. The commit actor remains the real initiating
`InvocationIdentity.actor`. Session IDs stay valid private audit locators but are no longer the only public
provenance referent.

Finite Process/Job provenance now follows the same owner-local rule rather than introducing a generic
provenance service. Finite Job admission already owns immutable `TaskIdentity`; the adapter projects
`FiniteJobProvenance {canonical_job_ref, original_agent_ref, original_turn, initiating_actor}` directly from
that identity plus the bound Agent. The Store's immutable `job.exposure.reserved` commit now persists the
same canonical provenance. Publish/release re-read that original reservation and refuse to rewrite the
initiating actor. Older reservation events remain readable because missing provenance is conservatively
derived from the typed canonical Agent/Turn/Job ref plus the original commit actor. The hot
`job_exposure_current` projection gains no columns or scans.

Finite public status/control receipts (`jobs_list`, `jobs_get`, `jobs_ack`, successful finite `jobs_send`
and `jobs_cancel`) append one bounded canonical provenance envelope only after the typed factual receipt has
been rendered. If that envelope cannot fit the existing result budget, the receipt fails rather than
silently dropping attribution. Raw `jobs_read` stdout/stderr/result bytes remain raw bytes. `ProcessTerminal`
and `aw-process` remain Agent-agnostic exactly as sealed; no lifecycle/runtime ownership moved upward or
downward.

The existing model-requested Process control owner now freezes
`ModelProcessProvenance {canonical_agent_ref, original_turn, initiating_actor}` at Prepared after its exact
active-Turn + original-Input checks. Applied/Rejected/Unknown/Inspected facts structurally retain it. The
reduced model-facing close-stdin/terminate receipts expose only that canonical Agent/Turn/actor attribution
alongside their existing safe fact/handle/state fields; PID/program/argv/env/output/private Session IDs stay
out of the receipt.

The Phase-1 Agent exposure/index boundary is now normalized to the authoritative owner. `agent_state_current`
already has the exact project/list indexes needed for `agent.list`; `job_exposure_current` remains the
separate rebuildable mixed Agent/finite-Job lookup/list projection. Owner-locked `SessionStore::open()` now
performs a cheap exact comparison after schema activation. When Agent exposure rows are missing/stale it
rebuilds **only** those rows from `agent_state_current`, deriving each original publication position from
its immutable `session.created`/`branch.created` commit evidence. It emits no synthetic event/audit, never
changes Agent rev/lifecycle/title/lineage, and leaves finite Job rows untouched. Trusted-local direct mode
still never migrates/repairs. The old `repair_missing_agent_exposures` path is explicitly pre-v38 migration
compatibility only.

The final concrete Phase-1 host gap is now closed. One concurrent **`AgentStatePort`** exposes the
already-authoritative Store operations `agent.list`, `agent.get`, `agent.edit`, `agent.admission`,
`agent.access.list` and `agent.access.set`. The headless transport rejects unknown/authority-bearing
fields before port dispatch; the port accepts only current `.` or canonical Agent roots, keeps list/get/
access-list as pure Store reads, and delegates title/admission/ACL mutations directly to their existing
owners. No Agent state table, ACL owner, replay ledger, constructor or authority field was added.

A fresh SPEC §28 audit after this cut finds no remaining **Phase-1** owner/surface gap. Current/non-current
`agent.close`, native/public same-user cross-Project `agent.create`, all five `agent.inputs.*` methods,
logical Sandbox identity/binding plus native list/get/close, immutable history/provenance, finite Job/Process
provenance and authoritative Agent exposure normalization are all present. The still-later §28 pieces remain
in their sealed owning phases rather than being pulled forward: Vault bindings and physical/fresh Sandbox
materialization are Phase 3; generic Event provenance/cutover is Phase 2; View/Extension enrichment and
later Jobs traversal/high-water work remain in their planned phases. Trusted delegation into another
principal's Project remains a separate future host-policy capability and is not smuggled through raw
Project fields. Phase 1 is therefore **COMPLETE locally**.

Phase 2 has now started with the minimum trustworthy scan foundation rather than a temporary direct
`commit_log` reader. Store schema **v44** adds one rebuildable `event_projection_index` plus one
StoreScope-bound `event_projection_state` safe watermark. `commit_log` remains the sole immutable
Event/fact history owner. Pre-v44 rows remain explicit non-DL-200 Events; newly authored rows are Events only
when their v4 commit payload carries the reserved DL-200 Event marker. The existing commit writer projects
that marker in the same transaction and advances the contiguous watermark for every commit row, including
non-Events. Event-marked writes are refused from autocommit so Event truth cannot publish without its
projection boundary.

Canonical concrete Event-subject syntax now lives once in `aw-tools::event_subject`: typed hierarchy is
strict, literal handle `.` round-trips through uppercase `%2E`, malformed/noncanonical escapes fail
closed, and custom `#name` grammar is bounded. Parsing avoids a token-vector allocation. The Store
projection retains canonical subject/source plus parsed source columns for later source-first authorization.
Healthy Store open checks projection scope/watermark/dirty state in O(1); projection-local SQLite triggers
mark any out-of-band insert/update/delete dirty, and owner open rebuilds from immutable `commit_log`,
advancing only the projection generation and never manufacturing Event/source/resource revisions.

The one private **EventScan** boundary is now implemented over that gap-free projection. Absolute
`EventPattern` syntax lives with `EventSubject` in `aw-tools` and implements the sealed typed NATS
cardinality: typed `*` matches exactly one token of the same kind, terminal `>` matches one-or-more
trailing tokens, literal handle encoding stays canonical, and contextual expansion remains a host-edge
concern. Matching allocates nothing after pattern parse.

EventScan captures only StoreScope + projection generation. Selector-placement identity remains owned by
the caller's existing placement owner: the scanner asks a zero-cost `Current | Stale | Unknown` callback
before traversal and again before returning any candidate/progress, rather than inventing a placement-token
type. Each bounded scan examines at most 4096 projected Event rows. Proven pattern nonmatch/source denial/
deeper denial may advance private progress; any source/deeper/placement Unknown returns
`BlockedUnknown` with no progress. Dirty/rebuilt/scope-mismatched projection or changed placement returns
`Stale`. The scanner loads the immutable commit payload **only after** a matching concrete source has been
allowed, so hidden sources cannot trigger deeper payload/manifest/provider work.

Authorization callbacks are statically dispatched and return `Allow(evidence) | Deny | Unknown`.
Source evidence is passed into the deeper check, and both source + deeper preliminary evidence are retained
with an allowed candidate for the consumer's mandatory final-owner reauthorization. This permits a future
consumer to carry `()` for purely local proofs or an owned provider fence/receipt without a trait object,
global authority service or allocation on skip/blocked/end outcomes. Only the already-large visible
candidate is boxed so the result enum stays small.

Schema **v45** now splits source-local Event revision ownership out of the legacy v36 Job-only
book without cutting delivery over yet. `event_sources_current` owns exactly the concrete `source_ref`,
the first unissued `next_event_rev`, and a nullable future `delivery_cutover_rev`. The v44→v45 migration
parses every legacy `SourceEventBook`, validates its canonical source ref and seeds the generic next
revision; it does not rewrite commit-log/Event history or claim a cutover. New legacy subscriptions created
after v45 initialize their book from that same generic next revision, so coexistence cannot silently fork
the revision sequence.

The first authoritative source family now emits real DL-200 system Events. Attached Turns emit Agent-root
`result` plus exact-Turn `result + settled`; Agent terminal settlement emits root `settled`. One source
transition reserves exactly one revision in the existing authoritative transaction, and multiple names from
that transition share it. If a legacy source book exists, its committed revision is passed as an exact
expected value; any divergence aborts the transaction. Sources with no legacy subscription still emit DL-200
Events. The immutable `commit_log` row remains Event truth/id; its bounded marker carries canonical
subject, source-local revision, factual actor/Turn provenance and causal_hop=0, and v44 projects it in the
same transaction. System emission rejects custom `#` names, duplicate names, autocommit use, oversized/
deep/non-object Event data, and revision exhaustion. Metadata-only result/settled Events deliberately attach
no DisclosureManifest: section 26.5 permits source-policy-only system facts, and the existing history
manifest code has no generic current-viewer authorizer that should be repurposed into a second policy owner.

Schema **v46** adds the first durable Event read consumer. `event_read_cursors_current` stores only
opaque random cursor identity plus authenticated principal, normalized selector digest, exact Agent
placement binding, StoreScope, private position and created/last-used time. Public pages never expose
StorePos, hidden-row counts, source evidence or disclosure-manifest identity. Cursors expire after 24 hours
idle and are capped at 64 live rows per principal after opportunistic cleanup. The logical replay floor is
the sealed union of all Events from the last seven days and at least the newest 65,536 Event rows; stores
below 65,536 Events short-circuit that check without a time-floor scan.

`SessionStore::events_read_page` is the first real EventScan consumer and deliberately implements only
the placement scope that is complete today: an absolute selector wholly rooted at one **exact Agent**.
`EventPattern::exact_agent_ref` extracts that root syntactically; wildcard Domain/Principal/Project/Agent
selectors fail closed rather than pretending this Store is globally complete. `after=Now` requires current
Agent `view`, captures the projection's current safe watermark, and returns an empty page with an opaque
cursor. Cursor reads bind principal + selector + StoreId/restore generation + exact Agent placement, scan
at most 4096 underlying Event rows, and return at most 256 visible Events. The current Agent owner is the
placement proof: missing/moved ownership is stale; ACL changes themselves do not stale a cursor.

For one read transaction, exact-Agent `view` evidence is resolved once and reused across the bounded
scanner because the existing `BEGIN IMMEDIATE` Store transaction makes that authority snapshot immutable;
a visible page performs one final evidence recheck before cursor progress commits. This avoids per-Event
ACL point lookups without weakening the final-owner boundary. Proven pattern nonmatch/source denial advances
only the private position. A caller whose `view` is revoked therefore skips unread matching Events without
seeing their identities; restoring `view` later does not reveal rows already skipped. Candidate payload/
manifest work still happens only after source allow. Current metadata-only result/settled Events have no
manifest and pass source policy; any future envelope carrying `disclosure_manifest` returns the
candidate-oblivious `AuthorizationUnavailable` with unchanged progress until the real manifest/provider
authorizer exists. StoreScope/projection/placement changes return `CursorStale`; idle/missing/retention
loss returns `CursorExpired`. Public Event projection revalidates immutable marker subject/revision/source
agreement and actor/Turn/causal bounds.

This removes the scanner's temporary non-test dead-code allowance: EventScan and its projection snapshot
are production-reachable through a real consumer. Exact-Agent section-26.2 waiting is now also implemented:
`SessionStore::events_read` wraps the single-attempt `events_read_page` with bounded `wait_ms=0..30000`
using one process-local `tokio::watch` generation. The receiver registers **before** the first durable read,
then follows register→recheck→await-change until one absolute deadline; every wake re-enters the same
authorized cursor read, so there is no polling loop or Event worker. Current Agent/Turn Event producers
signal only **after** their authoritative transaction commits. A timeout performs one final durable read,
covering deadline races and trusted-local commits from another process/Store instance that cannot share the
ephemeral watch. Thus cross-process wake latency may reach the caller's requested deadline, but correctness
and no-loss semantics remain entirely projection/cursor-owned rather than requiring new IPC.

The wake is deliberately store-local and coarse: any committed local Event may wake readers, which then
cheaply prove pattern/source relevance through EventScan. This avoids a per-Agent waiter registry/worker
until profiling shows a real need. The Tokio `time` feature is the only runtime addition; no background
task is created. Subagent terminal Agent Events use the same post-commit wake path.

Schema **v47** generalizes only the cursor's private placement binding: existing `agent_ref` becomes
`placement_ref` plus `placement_kind=agent|project`. Migrated exact-Agent cursors retain their existing
Agent placement-token bytes; no cursor reset is required. The first broader selector shape is deliberately
narrow and composable: exact `d:<domain>.i:<principal>.p:<workspace>.a:*` with any valid trailing Event
pattern. `EventPattern::wildcard_agent_project_ref` recognizes only that shape, so Project-level Events,
wildcard Project, wildcard Principal and wildcard Domain selectors remain fail-closed.

Project admission reuses the existing resource owners rather than adding topology state. The `p:` handle is
the Workspace ID; the Store must prove that exact Workspace is active, hosted by this Store's local Machine,
and still belongs to the selector Domain. Broad cursor creation additionally requires current active Domain
membership as a conservative discovery gate. That membership is **not** frozen into later source
authorization: every matching concrete Agent/Turn/Job/Node Event still resolves its canonical Agent root and
uses current Agent `view` authority. An ACL-only collaborator therefore keeps exact-Agent observation but
cannot broaden into Project discovery merely from one grant. Per-page source checks cache one Agent evidence
per canonical Agent ref and all visible Agent evidences are revalidated before cursor progress commits.
Missing current Agent ownership is indeterminate/AuthorizationUnavailable, never silently classified as a
proven deny. A Project cursor becomes stale if its Workspace is archived/moved away from local hosting or its
Domain is no longer active; ordinary Agent ACL changes remain visibility changes, not placement staleness.

The second real selector consumer justified one small extraction: `event_placement` now owns the shared
exact-Agent/exact-Project same-Store placement parse, admission, snapshot, stale/unknown classification and
placement token. `events.read` still owns its cursor/privacy/source authorization mechanics; the new
subscription owner reuses only placement affinity rather than duplicating it.

Schema **v48** adds `event_subscriptions_current`. The durable key is exact recipient Agent + normalized
absolute subject. Each row stores StoreScope + placement affinity, monotonic generation, private
baseline/scan positions, creating principal + host authority provenance, recipient control/manage evidence,
and state `active|lagged|disabled`. The shared immutable Event log remains the only backlog: there is no
per-subscriber Event/message table. `disabled` is a private tombstone so disable→re-enable must mint a
strictly newer generation and old in-flight admission evidence can never become valid again.

`SessionStore::events_subscribe` implements the sealed idempotent state transitions. Current recipient
`control` or `manage` is required; enable additionally requires recipient lifecycle `open` and current
selector admission through the shared same-Store placement owner. First enable commits baseline=scan at the
current safe Event projection tail in the same `BEGIN IMMEDIATE` transaction. Repeating `enabled=true`
for a current active row is a zero-write no-op preserving generation, baseline, scan and timestamps.
Disabled/lagged rows—or an active row whose stored StoreScope/placement affinity is stale—re-enable with
generation+1 and a fresh future-only baseline. Disable changes active/lagged to the disabled tombstone while
preserving generation; absent/already-disabled disable is unchanged. Generation exhaustion fails closed.

Recipient close is transactionally fenced across every current Agent close path: explicit host close,
delegated close request, direct subagent terminal close and restart recovery all disable live Event
subscriptions in the same Store transaction that changes recipient lifecycle. Management-only
`events_subscriptions_list` follows the existing canonical-key `after` paging style and exposes only
subject/generation/live state/creator/authority metadata; baseline, scan, StoreScope and placement tokens
never leave the owner. Disabled tombstones are omitted. The `lagged` owner state and explicit rebaseline
semantics exist now, but active→lagged is intentionally **not** manufactured by list/open-time mutation; the
future subscription scanner will set it when it actually proves retention or placement failure.

The third shared prerequisite extraction is similarly narrow. `event_authority` now owns the current
Agent-source visibility + immutable envelope validation used by read and subscription delivery; source
visibility remains first, and the deeper candidate step validates subject/source/marker agreement,
actor/Turn/causal bounds and the same object/4096-byte/depth-8 Event-data contract used by publication.
A retained Event carrying `disclosure_manifest` remains Unknown before any payload disclosure.
`event_retention` likewise owns only the already-sealed seven-day-or-newest-65,536 logical retention
floor, with read choosing CursorExpired and subscription choosing lagged.

Schema **v49** adds append-only `agent_event_inputs` plus the small
`event_causal_roots_current` safety projection. EventInput is a typed immutable Agent-input fact, **not**
a second pending Event queue; subscription scan position remains the only Event backlog. A deterministic
delivery identity is SHA-256 over recipient + normalized subscription subject + generation + immutable
Event.id. The admission transaction reloads the exact active subscription generation/StoreScope/placement,
revalidates the creating principal's current recipient control-or-manage ownership plus current recipient
`submit`, proves source Agent `view` before deeper Event authority, suppresses source-Agent==recipient,
revalidates the preliminary source/envelope evidence, deduplicates before budget charging, appends one
EventInput, charges the causal root exactly once and advances recipient Agent rev for the newly accepted
input. A fresh Event with no inherited root uses its existing opaque immutable Event.id as the causal root,
so no second random identity owner was introduced. The projection allows at most 256 accepted EventInputs
per root and records max observed Event hop through 16.

`SessionStore::event_subscription_deliver` is the explicit lower trusted Rust host-scheduler seam; it
creates no worker and performs no polling. It runs one bounded EventScan window (<=4096 examined,
<=256 terminal candidates) under the persisted creator/placement. Proven pattern/source denial advances
privately without EventInput work. A visible candidate enters the EventInput transaction; accepted,
deduplicated or proven-denied results may advance scan, while AuthorizationUnavailable holds at the
candidate. Proven StoreScope/placement mismatch or retention overtake marks the exact generation `lagged`
and stops retention pinning. Scan advancement and all accepted EventInputs for the sweep commit in the same
`BEGIN IMMEDIATE` transaction. The scheduler status exposes accepted/deduplicated/skipped/blocked/lagged
to trusted Rust callers; its Store position remains crate-private and is not an Events RPC field.

Schema **v50** closes the AW-owned protected-disclosure and causal-execution path without adding another
grant service or ambient causal context. `agent_event_inputs` now stores the retained disclosure manifest
and a normalized immutable audience side table. The retained manifest is the existing v40 content-addressed
DAG: each EventInput unions an `agent_source` authority root for the concrete source Agent with the Event's
transitive DisclosureManifest. The history sealer walks EventInput-originated prompt ancestry and unions
those same manifests, so cross-Agent delivery cannot discard source/share dependencies before later
history/fork audience expansion.

Reader and delivery authorization are intentionally distinct but share the same manifest graph. A reader
reauthorizes its own current principal against every root. EventInput admission snapshots the recipient
Agent's current owner + direct view ACL audience; if that audience is broader than the source Agent's current
audience the creating principal must currently hold source `share`. `agent_source` and
`legacy_session` roots are fully reauthorized with existing Agent/context owners. A current exact
materialized-policy root can authorize an individual reader through the existing federation/materialization
authority, but delivery to an entire recipient audience remains **Unknown** while AW lacks a principal-level
provider/materialized share fence; revoked/missing policy is proven denial. Unknown keeps subscription scan
at the candidate. No Event-specific grant database was added.

Causal scope also remains adjacent to existing owners. `event_subscription_deliver` now returns stable
ready EventInput IDs. `SessionHandle::bind_event_input` is a host-only one-shot next-admission hint, parallel
to the existing web/retry/route bindings. The normal `begin_sync` transaction verifies that exact immutable
EventInput belongs to the same Agent and is unbound, then records its delivery ID, causal root and parent
Event hop directly on the factual run. There is no TurnStart-wide API churn and no task-local/global causal
state. System Event emission derives descendant scope from the exact run: fresh work stays hop 0/rootless;
EventInput-derived work retains the root and increments Event hop exactly once, updating the causal
projection when that descendant Event is authored. Same-context provider retries copy root/parent-hop from
the prior run of the same logical operation while leaving the EventInput delivery ID unique to its original
admission. A parent Event at hop 16 reserves the source-local revision but suppresses a would-be hop-17
DL-200 Event, preserving source revision alignment while stopping the causal chain.

The causal owner now records exactly one non-deliverable host-visible `event.causal_exhausted` commit fact
per root, keyed by the existing causal root. It fires when the 256th EventInput is accepted or when a
hop-16 Turn attempts another Event generation. The attention fact is deliberately **not** a DL-200 Event,
so it cannot reset or recursively escape the exhausted budget. Accepted EventInputs/audience rows and causal
attention facts are database-immutable; the small causal-root row remains the replay-safe current safety
projection.

Current locally emitted result/settled Events remain metadata-only, but the protected Event path is now
real for retained/future envelopes carrying DisclosureManifest. The existing combined Event regression
proves a delivered EventInput has a 64-byte effective retained manifest and immutable audience, source owner
authorization succeeds, and after source-view revocation the same retained source manifest denies the
observer. The same accepted EventInput is bound to the recipient's actual next journaled Turn; the run
records the exact input/root/parent-hop, and its three real descendant Agent/Turn Events all retain the root
with hop **0→1**. All prior read/wait/Project/privacy/subscription checks remain green, as do the existing
history disclosure-grant and manifest-fanout regressions. Migration now exercises
v44→v45→v46→v47→v48→v49→v50. Current-schema `aw-tools + aw-store` all-target check, strict Clippy, fmt and
focused diff-check are green. The prior broad `aw-store --lib` result remains
**226 green / 4 historical stale-downgrade reds** and was not repeated.

Schema **v51** adds the final one-leaf EventTypeRegistry and custom Event publication without inventing a
temporary root registry. `aw-agent-core::registry` is one private staged mutation primitive layered directly
over the existing left-right immutable `publication` owner: stage captures one base generation, builds and
validates one complete successor exactly once, then publication performs a same-writer-lock generation CAS.
An ordinary validation/conflict error leaves the published snapshot unchanged; reader handles remain
lock-free immutable snapshots. The new publication CAS primitive is crate-private rather than a second
public registry API.

`EventTypeRegistry` is the final EventType definition owner from its first implementation. Durable identity
is `{extension local-id schema-version}`; in-process `TypeId` only binds the Rust payload type. A typed
`EventKey<T>` additionally pins the exact registration revision so removing/re-registering one definition
stales only that exact handle; unrelated EventType publications do not. Runtime
`EventTypeRegistryReader` revalidates handles against lock-free snapshots before typed publication or
decode. Qualified wire custom names are deterministically/collision-safely derived inside the ordinary
`#...` Event namespace. A raw Event may intentionally use the same subject; absence of the matching
immutable `schema` tag keeps it generic and typed decoding fails closed. No separate typed Event instance,
history or grant namespace exists.

Custom Event envelopes now carry optional `EventSchemaRef`, which is validated on retained reads, exposed
as optional public read metadata and copied into immutable EventInput facts for later typed consumers.
`SessionStore::events_publish` is the trusted Rust host form: one exact canonical open Agent, current
`control`, bounded custom `#name`/data, and a disclosure ceiling derived from the Agent's current local
head plus inherited HistoryBase/source authority. `events_publish_typed` adds exact current registry-key
revalidation and the schema tag.

`SessionHandle::publish_model_event` / `publish_typed_event` are the lower trusted-runtime model forms.
Source Agent and actor are not caller-selectable: both derive from the exact currently-running original Turn.
The v50 Turn disclosure manifest and causal run scope are reused, so model custom Events preserve protected
dependencies and inherited root/hop exactly like system Events. Successful model custom publications are
durably capped at **32 per Turn** by `event_custom_turn_counts`; causal exhaustion does not consume that
success budget. These Store methods intentionally do **not** manufacture or bypass the model
`events_publish` capability: actual model/tool exposure remains with the existing tools-adapter capability
owner and is the next thin surface.

Until explicit v36 cutover, custom publication also keeps the legacy Job-only source revision book aligned
in the same transaction through one pure transitional `advance_revision_without_delivery` primitive.
Custom Events never enter the legacy pending-delivery queue. Any revision disagreement aborts the whole
custom publication, so the next legacy/system transition cannot silently fork the source-local sequence.

Focused v51 evidence stays compact. The one-leaf registry regression proves complete staged publication,
invalid-stage revision neutrality, unrelated-definition handle stability, exact removal staleness and
remove/re-register identity renewal. The existing combined Event lifecycle regression now additionally
publishes one generic and one typed model custom Event from an EventInput-derived Turn, verifies both retain
the same causal root/hop, then publishes a raw host Event on the **same typed subject** and proves only the
schema-tagged Event decodes as `T`. It also creates a real legacy bookkeeping row first: two custom
revisions create zero legacy custom deliveries and the following system result lands at revision 3,
demonstrating pre-cutover sequence alignment. Migration now exercises
v44→v45→v46→v47→v48→v49→v50→v51. EventType **1/1**, Job Event **4/4**, Event subject **2/2**,
EventScan **1/1**, Event projection **1/1**, combined Event lifecycle **1/1**, all-target check and strict
Clippy are green. The prior broad `aw-store --lib` result remains
**226 green / 4 historical stale-downgrade reds** and was not repeated.

The model/tool surface is now a thin composition over those v48/v51 owners rather than another Event API.
`aw-tools-adapter::events` defines only the two model-visible Tools plus a host-injected `EventRouterSlot`.
The slot stores no Event state and grants nothing. `events_publish` and `events_subscribe` are ordinary
Host/Control Tool definitions, so the existing ToolPolicy + Actor Envelope machinery remains the exact
capability owner. They are intentionally not forced through the generic filesystem/Jobs
`OperationRouter`: Events have no canonical `OperationId`, and inventing one merely to satisfy adapter
plumbing would create the wrong semantic owner. A selected Event tool with no installed host router fails
closed, including ToolMode::Off.

The durable root composition reuses the existing `agent_jobs::Router` instance as the EventRouter because
that object already owns the exact Store + authenticated principal + Agent-binding resolver; it gains no
Event state. Before either model operation it proves ToolContext actor and immutable invocation actor match
the durable router principal, requires a journaled scope, and checks execution-owner canonical
Agent/session/branch **and** scope session/branch against the authoritative Store binding. Host-created
root and stable delegated Agents install the same router object into both the Jobs and Events adapter slots;
legacy one-shot children remain unchanged. The pre-existing durable top-level CLI/web Agent is composed in a
different order (Agent first, Store router after persistence load), so it uses the existing trusted
`Agent::register_host_tool` path to add those exact two definitions only when the Store-local router is
actually bound. A truly remote MCP execution-site owner leaves the Event slot empty and does not advertise
these unrouted Store-owner controls rather than falsely claiming a remote execution-site grant.

Model `events_publish` accepts only `event` + optional object `data`; the custom `#name` grammar is
validated at the adapter edge and the model cannot name another source. The router calls the Store's exact
running-Turn primitive, so actor/source/disclosure/causal provenance remain v51 facts. This pass also fixed a
v51 lower-seam bug exposed by the real integration test: obtaining a fresh `SessionHandle` by
`SessionStore::activate` during an already-running Turn correctly conflicted with the existing owner.
`SessionStore::events_publish_turn(session,branch,turn,...)` now publishes directly against that factual
active run; `SessionHandle::publish_model_event` is only a convenience delegate. No second run/Agent owner
is created.

Model `events_subscribe` has implicit recipient=current Agent and no scheduling-mode argument.
Relative `e:/t:/>` patterns resolve from the current Agent; `a:` patterns resolve from the current
Project. Absolute patterns are accepted only if the already-implemented placement can prove the selector is
an exact Agent under the current Project or current-Project `a:*`; cross-Project/Principal/Domain model
selectors fail closed for now. The normalized absolute selector is handed unchanged to the durable v48
subscription owner, which still reauthorizes concrete Event sources at delivery.

Focused adapter evidence is deliberately small: one adapter regression proves ToolMode::Off exposes exactly
the two explicitly selected Events tools only when an EventRouter is installed, both bindings are exact
trusted-Host/Control grants at the configured execution site, and the router receives the unchanged
authenticated ToolContext/current Agent rather than model-selected source/recipient data. One real
Store-backed root regression proves relative `e:#ready` becomes the current Agent's canonical selector,
cross-Project subscription is denied, a real active Turn publishes `#ready`, and the existing Event cursor
reads that exact Turn/data back; disabling the same relative subscription reaches the same durable row.
The existing v51 combined lifecycle regression remains green after the Store running-Turn refactor.
Repository fmt, full workspace all-target compilation, strict root+adapter+Store Clippy and scoped diff-check
are green. The attempted historical subagent long-test filters execute zero tests in the current harness
and are **not** counted as evidence; their changed visible-tool expectations are compile-checked instead.

Schema **v52** adds EventHandler as EventScan's third consumer without turning the Handler registry into a
runtime queue. `EventHandlerRegistry` is the second real leaf on the same private staged-registry core used
by EventType. It owns only inert typed native definitions: stable Extension/local ID, bounded declared
pattern, schema version, exact registration revision and the concrete in-process implementation type.
Registration creates no Store cursor, pins no retention and grants no Event authority. The lock-free
`EventHandlerRegistryReader` revalidates the exact registration before returning a descriptor or typed
implementation; unrelated Handler definitions do not stale a handle, while exact removal/re-registration
does. The common Extension/local-ID validation moved into the existing private registry core rather than
being duplicated between leaves. Canonical EventPattern syntax remains Store-owned and is validated when a
trusted host actually activates the inert definition.

The durable owner is deliberately smaller than an extension runtime. `event_handler_cursors_current` owns
only one StoreScope/placement-bound activation per `(extension,handler)`: schema/registration identity,
normalized pattern hash, activation generation, `active|suspended|lagged`, explicit service principal +
opaque authenticated authority ref/generation, private baseline/scan position and lag reason.
`event_handler_receipts_current` owns **one** deterministic in-flight receipt for that ordered consumer:
activation generation + Event.id + Event Store position. It never copies Event payload/data, Handler output
or product state. Handler/product read models and external-effect idempotency remain outside AW.

Activation is explicit and separate from definition registration. `start=now` baselines at the current safe
Event tail; `earliest_available` starts immediately before the oldest Event still promised by the existing
seven-day-or-newest-65,536 logical retention floor. A compatible suspended reinstall resumes the retained
cursor/receipt and keeps activation generation; an updated compatible executable registration may advance
only its registration revision. A lagged Handler never silently resumes through ordinary activate:
`events_handler_rebaseline` is the explicit recovery operation and mints a fresh activation generation/
baseline. StoreScope or structural placement mismatch likewise transitions active/suspended state to
`lagged`; service-principal authority loss is instead retryable blocking, so authority restoration can
resume without pretending the resource moved.

`events_handler_poll` is the lower bounded host-scheduler seam and creates no worker/backoff loop. With no
receipt, EventScan may privately advance proven nonmatches/denials; Unknown holds progress. On a visible
authorized candidate AW persists one deterministic `ehr_...` receipt and returns the existing public-safe
Event projection. Re-poll before ACK reloads immutable Event truth and returns the **same receipt/Event**.
No cursor advances past that candidate. `events_handler_ack` revalidates the exact current definition,
activation, service binding, structural placement, source visibility and DisclosureManifest closure by
running the same EventScan candidate again; only then does one transaction delete the receipt and advance
the cursor to that Event position. Repeating the immediately committed ACK is idempotent through the
persisted last-acked receipt. A stale placement lags instead of ACKing; an unavailable/revoked service or
deeper authority leaves progress unchanged.

This is intentionally not EventInput delivery: no recipient Agent, no Agent rev, no causal-delivery budget
and no copied pending Event queue. It also does not fabricate the future `ExtensionContext` callback inside
Store. A trusted native/product host obtains the typed implementation from EventHandlerRegistry, polls one
authorized delivery, performs its own idempotent/product work using immutable `Event.id` as provenance, and
ACKs only an accepted result. That preserves at-least-once semantics and keeps uncertain external-effect
receipts where they belong.

The same cut narrows two shared third-consumer mechanics instead of duplicating them: Event selector hashing
now lives in EventScan, the public-safe Event projection is reused from Event read, and
`event_retention::earliest_available_after` derives from the same logical retention-floor calculation used
for expiration. No generic cursor manager or Jobs+Events traversal was added.

Focused v52 evidence remains compact: Handler staged-registry **1/1** proves typed implementation retrieval,
unrelated-definition stability, invalid-stage revision neutrality, exact removal staleness and
re-registration renewal. The real Store Handler lifecycle regression **1/1** publishes retained Events,
activates from earliest available, proves the same deterministic receipt/Event replays until ACK, proves ACK
and immediate ACK replay idempotency, delivers a second Event, suspends without losing its unacked receipt,
resumes the compatible activation at the same generation, and proves an explicitly lagged cursor stays
lagged until rebaseline increments generation. EventScan **1/1**, projection **1/1**, EventType **1/1** and
the existing combined Event lifecycle/migration regression **1/1** remain green; migration now exercises
v44→...→v52. Repository fmt, full workspace all-target check, strict affected all-target Clippy and
scoped diff-check are green. The historical broad Store result remains
**226 green / 4 stale downgrade-fixture reds** and was not rerun.

Schema **v53** closes sealed Phase-2 step 12 without adding a topology service or merged cursor. Event
placement now has a third private shape, `universe`, for the reviewed broad Agent-subtree grammar:
`d:<exact|*>.i:<exact|*>.p:*.a:*...`. Exact Agent and exact-Project/`a:*` placement are unchanged.
The syntax helper only recognizes that shape; Store placement remains authoritative.

The Store completeness proof is authority-scoped. For a broad universe, the existing resource owner resolves
the caller to its current user and enumerates only **active Domains with active membership for that caller**,
optionally filtered by an exact Domain. For every included Domain it proves (a) the Domain's existing
`authority_store_id` is this StoreId and (b) every active Workspace is the existing `hosted` kind on this
Store's local Machine. If any currently-authorized matching Domain/Workspace is remote, unknown or otherwise
not wholly local, admission fails closed; an already-live cursor becomes stale and subscription/Handler
becomes lagged. Hidden Domains never enter this query, so their creation or remote placement cannot perturb
another principal's affinity or reveal topology.

No global/per-user topology generation was introduced. The durable placement token still binds
`StoreScope + normalized selector`; every use recomputes the caller's authorized completeness before scan
and before progress. This is intentional under the **live resource-owner invariant**: current production
owners create/project Domain/Workspace placement but do not retarget `domains.authority_store_id` or
`workspaces.host_machine_id` in place. Workspace writer failover has its own writer epoch/revision but does
not move Event-store placement and therefore must not spuriously stale Event cursors. A future reviewed
owner that adds in-place Event-store movement must expose a monotonic placement fence before such moves are
accepted by this proof; v53 does not invent that dormant state today.

Concrete source visibility remains separate from placement completeness. A broad Event source must still
match the selector's exact/wildcard Domain+Principal coordinates, the scanning principal must still hold
current Domain membership for that concrete source, and the exact source Agent still requires current
`view`; DisclosureManifest authorization remains after source visibility. Revoking membership therefore
privately denies future concrete matches rather than revealing or retroactively replaying them. Gaining
same-store authority can expose future matching Events without needlessly invalidating the cursor; gaining
authority to a remote Domain makes completeness stale instead of silently returning a partial world.

All three durable consumers use the same actor-aware placement proof:
- `events.read` returns `cursor_stale` on incomplete authorized universe;
- Agent subscriptions lag and stop retention pinning/delivery;
- EventHandlers lag and require explicit rebaseline.
The final EventInput transaction also recomputes structural completeness **and** re-runs the same
placement-aware `source_visibility` evidence before admission, closing scan→commit races where Domain
membership or one-store completeness changes after the advisory EventScan.

Persistence changes are mechanical only. v53 rebuilds the three existing durable placement tables to extend
their CHECK from `agent|project` to `agent|project|universe`, preserving cursor/subscription/Handler
identity, private progress and Handler receipts. No placement-index/topology table was added. The existing
v44→... migration regression now reaches v53.

The model contextualizer is widened only to shapes the Store can now prove: exact Agent, exact
Project/`a:*`, and `p:*.a:*`, `i:*.p:*.a:*`, or `d:*.i:*.p:*.a:*` Agent universes. Source and recipient
remain implicit/current; unsupported mixed wildcards still fail before Store dispatch. Actual authorization
and one-store completeness remain Store-owned rather than being inferred from the model string.

Focused v53 evidence uses one grouped three-consumer regression rather than parallel fixtures. Two **real
Agents in two distinct authorized local Domains** publish matching `#ready` Events. The broad cursor reads
both; the Agent subscription self-suppresses its own Event and accepts the other Domain's EventInput; the
Handler receives/ACKs both. A hidden principal's remote Domain leaves all three consumers current. Changing
one Domain actually authorized for the caller to a remote authority-store fixture makes read stale and
subscription/Handler lagged, proving no partial world. EventSubject **2/2**, the cross-Domain grouped
regression **1/1**, Handler lifecycle **1/1**, combined Event migration/lifecycle **1/1**, model selector
contextualization **1/1**, real model router **1/1**, repository fmt, full workspace all-target check,
strict affected Clippy and scoped diff-check are green.

Schema **v54** closes sealed Phase-2 step 13 and the Event phase. The schema-owner migration runs one
SQLite-serialized compatibility transaction over every remaining v36 `agent_job_event_sources` row. For
each source it first proves the legacy `SourceEventBook.next_event_revision` equals the generic
`event_sources_current.next_event_rev`, then atomically writes
`delivery_cutover_rev = current next_event_rev`. That revision is the first DL-200 delivery-owned revision:
all names below it remain legacy-only; the whole revision at/above it is DL-200-only. Because the write,
subscription translation, pending bridge and legacy-pending drain share one transaction, crash either leaves
the old v53 owner intact or publishes the complete cutover. Restart reads the durable cutover; it never
guesses ownership.

Legacy source rules translate to **exact Event subjects**, not another broad event bus. Their tiny
`event_legacy_subscription_compat` sidecar retains source/name, legacy generation, effective scheduling mode,
cutover revision and the original wake creator/authority. A collision with an already-active ordinary DL-200
subscription does not overwrite that subscription: `shadow_existing=1` preserves its creator, generation,
baseline and progress, while one candidate performs the normal admission and then an explicit legacy-shadow
admission under the old wake authority. A non-shadow translated row is just the normal subscription owner
with compatibility identity attached. New non-empty legacy `jobs_subscribe` admission is fenced after
cutover; empty cleanup remains harmless.

Only genuinely pending pre-cutover deliveries bridge. Existing dual-written Event envelopes are reused when
present; otherwise v54 writes one deterministic immutable compatibility Event fact for the exact
source/revision/name. The EventInput owner remains the only admission linearization point. Legacy delivery
identity is deterministic over source + revision + recipient + legacy generation + effective mode, so
same-revision result/settled rules with the same effective queue mode coalesce exactly once independent of
legacy pending JSON order. `event_legacy_input_compat` stores only the scheduling/provenance sidecar; the
typed EventInput remains the factual Agent input and the old pending JSON is cleared after successful bridge.

Already-accepted legacy `qi_...` Inputs are never re-minted. Migration identifies the existing factual
ActorQueue/Input pair and adds only `legacy_event_input_compat` disclosure provenance keyed by that exact
Input ID. The frozen historical ceiling is deliberately conservative: only the factual recipient actor can
be proven from the accepted Input without replaying a second ACL policy engine. If the legacy queue's
`job-event:<sha256(source_ref)>` sender can be matched uniquely to a canonical source, the manifest also
retains the normal `agent_source` authority root. A later reader/fork outside the frozen actor is therefore
allowed only when that reconstructed source root independently reauthorizes current `view/share`; without
source reconstruction the old Input remains hard-frozen. A viewer merely present in the current ACL at
cutover is explicitly **not** inferred as historical disclosure. The sidecar is pulled into descendant
history manifests through the existing run/ancestry owner; no accepted Input identity or history fact is
rewritten.

Post-cutover source lifecycle keeps the old JSON only as frozen compatibility evidence. `commit_source`
no longer calls `SourceEventBook::commit`, custom publication no longer advances its revision, and no new
legacy pending row is produced. The generic source counter alone advances. A source first created on schema
v54 is born with `delivery_cutover_rev=1`, so a resource created after migration cannot reopen legacy
delivery accidentally.

The runtime bridge remains worker-free. The existing factual source-transition `deliver_source_events`
wake point drives only translated compatibility subjects through normal `event_subscription_deliver`.
Accepted compatibility EventInputs are then projected into existing ActorQueue custody under their exact
`edi_...` delivery ID. `run_once` binds that immutable EventInput into the next Turn before the ordinary
queue ACK, so causal/provenance ownership stays with EventInput. Live-owner registration and later source
wakes reconcile any compatibility EventInput that has no factual `runs.event_input_delivery_id`; existing
queue custody is idempotent, making a crash between EventInput commit and queue projection recoverable without
a new worker or queue.

Focused v54 evidence stays grouped: `v54_cutover_bridges_pending_once_and_fences_legacy_delivery_owner`
**1/1** proves exact cutover rev, pending result+settled coalescing, pending-order independence, existing
generic-subscription shadowing, unchanged accepted `qi_...` identity, conservative non-declassification of
a later ACL viewer, frozen legacy book, post-cutover system/custom revision ownership, legacy subscribe
fencing and fresh-source cutover-at-1. The combined attached-Turn/Event migration regression **1/1** now runs
its legacy phase explicitly under epoch 53 and migrates v44→...→v54. Existing Agent Jobs group-authority
regression **1/1** remains green. Repository fmt, full workspace all-target check, strict
`aw-tools + aw-store + aw-cli` Clippy and scoped diff-check are green. Zero-test generic name filters are
not counted as evidence.

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

**Next Phase-3 slice:** integrate exact live source-aware admission with the original
Actor/Turn journal and context/queue gates once the concurrent journal owner is ready.
While blocked, prefer independent production authority/containment or remaining
physical recovery proof acquisition through existing owners; do not open held routes. Independent
production host policy/physical authority configuration, other effectful baseline materialization, broader Run-hazard
reconciliation, general multi-source Process policy, unmarked/ambiguous scratch and non-exact partial
checkout cleanup, remaining production physical authority/cleanup, effectful
bound-Agent structural mutation and other
removal cases, remaining Vault broker/socket/file/other env integrations, persistent provider
configuration, provider-specific protected metadata and checkpoint/Sandbox/Vault host
RPCs remain pending. Store schema v60.
Do not contract v36 Event compatibility tables; Phase 7 owns removal after cross-version drain/restart
evidence.

Current focused evidence for the final Phase-1 cut: the existing Store host state-shell regression now
also exercises a real group ACL grant through `agent_access_set_ref`, and the existing strict headless
frame/capability regression exercises all six Agent-state methods plus provider/Sandbox/owner injection
rejection before port dispatch. Both are green; repository all-target strict Clippy is green. The broader
Phase-1 evidence remains as recorded below.

Current focused evidence: populated **v37 -> v38** Agent-owner seed and **v38 -> v39** principal
backfill remain green; group access/wake/history fencing remains green. The focused **v40 -> v41**
Sandbox-owner regression is green: same-layout Agents share one migrated Sandbox, distinct layouts remain
distinct, `/s:0` is seeded for the first Project layout, bound Sandbox close conflicts, and an unbound
fresh Sandbox closes/idempotently replays. The exact-history owner regression remains green after the
generic constructor started auto-binding Sandboxes, and Actor admission remains green. The consolidated
host shell regression now additionally proves trusted `auto` shares the current Sandbox, an explicit
same-Project/same-layout Sandbox binds exactly, an incompatible layout is rejected before Agent
publication, cross-Project `auto` fails closed, and fresh + exact-history redirects land in a second real
Workspace only with an explicit destination Sandbox. The existing host-activation regression is now itself
task-bearing cross-Project end-to-end, proving Prepared custody/publication, stable-principal provider
selection, live activation, exact replay and later non-current close across that boundary. The unchanged
model exact-history regression remains source-Project-local. The focused **v41 -> v42 -> v43** staged-plan
regression is green over a real persistent Store: migration seeding, encrypted blind append, CAS relative
insert, stale rejection, exact-Turn control-intent rejection without an active Turn, edit+reorder, delete,
separate `inputs_rev`, unchanged Agent `rev`, close freeze, no plaintext audit leakage, and exactly one
input-change fact per successful transaction. The host-activation regression now additionally sends a
staged followup through `HostAgentInputs`, observes exactly one second provider Turn, proves Send advances
Agent rev, and replays the same host request without another runtime Input. Existing
`jobs_send_same_turn_steer_is_custodied_segmented_and_applied_once` and interrupt counterpart remain green
after the transactional AuthoritySteer accept refactor. The exact-history fork regression now additionally
proves source -> fork -> refork Turn provenance preserves the original canonical Agent, Turn, actor and
timestamp without copied nodes, and verifies `llm.attempt_settled` stores canonical Agent/Turn attribution.
The paged Agent-history regression proves explicit `jobs_read` message records expose that provenance while
the default/latest result path remains unchanged. Focused finite-provenance evidence is also green: the
mixed exposure regression proves the immutable reservation event stores canonical Job/Agent/Turn/actor;
the existing contextless explicit-owner Job regression proves adapter provenance derives from the exact
bound Agent + immutable TaskIdentity; the bounded receipt regression proves canonical attribution is never
silently truncated; and the existing Process close-stdin + terminate regressions prove durable
`ModelProcessFact` and reduced model receipts agree on canonical Agent/Turn/actor while retaining the
existing no-PID/program/env disclosure boundary. The authoritative Agent-exposure rebuild regression now
also deletes every Agent projection row after real title/admission mutations and a published finite Job,
rebuilds from `agent_state_current`, proves both Agent snapshots/revisions and original publication positions
are unchanged, proves the finite row survives, proves commit-log cardinality is unchanged, and proves a
second normalization pass is a no-op. `cargo check -p aw-store --tests`, strict aw-store all-target Clippy
and repository fmt are green for this cut. A broader historical
Store sweep is still not claimed here; the previously documented old remote-start/remote-view downgrade
fixtures remain outside this focused Agent/Sandbox cut.
The host-created Agent activation regression now also closes that non-current child through
`HostAgentClose`: the call returns authoritative `closing/held`, the existing worker reaches factual
`closed`, and a later close replay returns the same terminal revision. Root close remains independently
green, and the stable delegated-Agent blocking-provider close regression still proves preemptive active
Turn cancellation/factual child settlement, so both live-owner implementations remain covered without a
second test harness. Existing event-source
coverage now proves receiver retirement clears both root+exact-Turn rules and pending deliveries while
bumping the source Agent once; the host-shell regression proves close invokes retirement atomically.
A real finite shell-Job regression proves `JobRuntime::close_owner` cancels through JobManager, waits for
terminal/quiescent output ownership, then rejects new finite-work admission. The existing stable-Agent
blocking-provider close and create/continue/close regressions remain green after stable close was wired to
that exact runtime handle, proving the composition did not regress active-Turn preemption or normal
terminal settlement. The existing scope-replacement runner-cancellation regression remains green. A new
Actor-owner regression proves trusted host close both withdraws an accepted pre-claim Input as durable
`Rejected` and cancels a genuinely running provider Turn without any model `jobs_cancel` grant, settling
it as Cancelled. The host-shell regression proves terminal close is rejected before a close request,
advances `closing -> closed` once, and replays idempotently. A single root lifecycle regression now drives
a blocked tool-capable root Turn through `RootAgentClose`, including retained JobRuntime closure and
revision-stable replay. The existing headless strict-frame regression remains green with the new live
`agent_close` capability. The host-activation regression now drives the reusable `HostAgentCreator` over
a root Router carrying the local alias and a child `Router::for_actor` facade carrying the canonical
stable principal. It publishes encrypted initial Pending custody atomically with the Agent, executes
exactly one provider Turn, observes authoritative history advance, exact-replays the same create request
without a second provider call/worker, then closes through the shared live registry. The Store-side
regression separately proves no plaintext in queue audit, exact Applied create replay returns the same
Agent, and changed semantics under the same request identity are rejected. A focused provider regression
proves child selection keeps `InvocationIdentity.actor`, personal account owner and payer on the canonical
stable principal. The strict headless frame regression now advertises `agent_create`, invokes the port once
for a valid sealed frame, and rejects an extra `provider` field before the port. The exact-history fork
regression remains green after adding pre-minted candidate IDs, and the older in-memory host-shell
regression still fails task-bearing create before any half-publication because it has no persistent
SecretStore. The strict headless regression now also advertises Sandbox list/get/close, explicitly reports
Sandbox create unavailable, dispatches one valid bounded Sandbox list request, and rejects an extra raw
layout field before the port. The focused v40->v41 Sandbox lifecycle regression remains green alongside
host task create and root close. Root-close and both stable-Agent close regressions remain green.

The user authorized this publication/repin/proceed sequence. Do not tag/release/deploy without a
separate explicit request. Use dotlink---l5 only; never touch `.agents/plans/ux/**`; 017
sandbox/mount/config-v13 remains frozen.

> The dated sections below are preserved implementation chronology. When their old status wording
> conflicts with the snapshot above or the canonical comprehensive handoff, the current snapshot +
> live source + sealed SPEC win.

## LIVE DL-200 R6 shared-Process cross-repo checkpoint — published source, 2026-11-21

The Phase-0 Process ownership cut is now published across both repos:

- AW runtime pin `7c16708...`: `Occupancy`, minimal `aw-process`, weak-Unix Unknown semantics,
  sole async Scope/Child/stdin/pipe/wait/stop/reap/drain ownership, semantic `stop_with`, one
  global drain bound, cancellation/deadline-preemptible interactive writes, and bounded reader
  quiescence that flushes captured partial sink batches before reporting incomplete output;
- AW main `59d67ba...` adds only the focused shared failure-boundary test; `aw-process` is **3/3**
  including `reader_stop_flushes_partial_sink_batch_before_incomplete`, strict Clippy green;
- dotlink source `16475e4` consumes `aw-process`, deletes `src/shell_process.rs` and all
  production `ManagedProcessGuard`/local Child/ChildStdin/stop-tree/drain ownership;
- dotlink `JobCompletion` stores the canonical optional `ProcessTerminal`; definite pre-spawn
  failure has no synthetic terminal. Occupancy projects compatibly as Empty=>no `tree`,
  Live=>`tree="leftovers"`, Unknown=>`tree="unknown"`;
- Live leftovers clear dotlink product `success` without rewriting factual exit/status into a
  generic tool error; weak-Unix escaped descendants remain Occupancy Unknown;
- direct + Bubblewrap Bash converge on non-login/non-profile `-c`; current dotlink main still has
  the historical pre-v0.17.8 30/35 bounded-turn projection. The target v0.17.8 convergence below
  supersedes it with one dotlink-owned 35m horizon / 10m report reserve / 25m command cap policy;
  AW remains horizon-free;
- dotlink product stdin authority/revision fencing remains local, but actual ChildStdin custody
  never leaves `aw-process`; initial stdin is chunked through the shared 64-KiB primitive.

Qualification stayed grouped: final dotlink all-target/all-feature source graph type-checks;
strict all-target/all-feature Clippy `-D warnings`, rustfmt and diff-check are green. The final
v0.17.6-era L5 executable rerun was green: shell filter **134/134** plus child-env **1/1**; Process
filter **18/18** plus its two process-related integration tests; Remote Start/replay **16/16**.
That remains valid evidence for unchanged semantics, but later SIGKILL attribution is amended by
the v0.17.7 audit below.

### Shared Process/release correction — AW + dotlink consumer source implemented locally, publication/platform gate pending

The current audited release tag is **v0.17.9** (`576d0a1`). Its exact `81ef891..v0.17.9` diff has
SHA-256 `1aa632e35d9c…`, identical to the staged candidate already imported into this plan. Its
underlying v0.17.7 Process fix (`64e3bd8`) is *stop a retiring Tokio worker from killing the
sandbox*. Bubblewrap `--die-with-parent` uses Linux
`PR_SET_PDEATHSIG`, whose parent is the **forking thread**, not merely the process. A command
spawned on a Tokio worker can therefore receive SIGKILL when that worker retires even though
dotlink remains alive. This matches the long-running L5 symptom pattern closely enough that prior
"external host SIGKILL" wording is withdrawn: those interrupted compile/link runs are
**inconclusive infrastructure failures**, not evidence of an outside sender and not AW test
failures.

Current uncommitted AW source now implements the sealed correction without sacrificing Tokio's
lightweight scheduling. `aw-process::Process::spawn` is async; Linux non-PTY launch crosses one
private bounded process-lifetime Spawn Facility with one stable lane initially. `Process: Send`
remains an O(n) movable async lifetime; only spawn + exact `DirectScope` attachment runs on the
stable lane. `block_in_place`/`spawn_blocking` are not used as lifetime anchors.

The implementation transfers private RAII `Spawned { DirectScope, Child, pid }`; a vanished reply
cleans the exact scope/child, lane panic poisons future launch admission without exiting the anchor,
and the lane never retires. The runner's absolute execution deadline is reused across Spawn-Lane
queueing + Process wait; a level-triggered cancellation probe is rechecked on the lane immediately
before fork, so stale pre-spawn cancellation creates no ProcessTerminal/StopRecord. Linux
`DirectScope::ready` now uses strict parent-owned non-reaping `waitid` with no ECHILD→pidfd-poll
cleanup fallback. Stop evidence is reconciled against physical exit before causal projection.
Windows source uses `CREATE_SUSPENDED | CREATE_NO_WINDOW` and a private Job termination marker;
`aw-process` contains the async interruptible occupancy-settle path. Native Windows execution is
not qualified on L5 because this toolchain has only the Linux target installed.

The dotlink-specific pieces remain at its edge: caller-vs-connector cancel origin, lifecycle prose,
`by=external` wording and wire projection. An unclaimed signal means only "no recorded shared stop",
never proof of an outside sender. The frozen 017 Sandbox/mount work and future PID-1 backend remain
unpromoted.

### v0.17.8 + post-release dotlink convergence — planned after AW Process repin

The currently audited release checkpoint is exact **v0.17.9 (`576d0a1`)**. Its committed
`81ef891..v0.17.9` diff SHA-256 is `1aa632e35d9c…`, exactly matching the staged candidate previously
audited. The release branch may continue moving, so the future dotlink-main port must still re-diff
from this exact tag before editing. At the actual dotlink-main convergence step, first re-diff
the then-current read-only `release/0.17` from this checkpoint and classify every new delta by
owner: shared Process/Job factual semantics go into AW first; transport/product policy and wire/
operator projection stay in dotlink over AW facts; frozen sandbox/config changes remain deferred
unless separately promoted; relevant tests/docs are reused. Never blind-cherry-pick a release
runtime implementation into main.

The v0.17.8/v0.17.9 dotlink-only bounded-turn refinement is now **implemented locally on dotlink
main against the existing published AW pin**, still as projection over shared Job facts rather than
another Jobs/Process owner:

- one local `RuntimeSurfacePolicy` derived from `ActiveTransports.constraints()`;
- bounded turn = 35m, with 10m reserved for reporting/hand-back, so new command cap = 25m/1500s;
- `sync_hold.is_some()` enables the bounded surface; local-only stdio/HTTP stays uncapped unless a
  transport itself declares a bound;
- one policy source derives server instructions, effective shell ceiling/schema/validation, startup
  warning and near-deadline `jobs_get.next`;
- effective new-command ceiling/transport constraint participates in execution-authority
  fingerprinting; current advisory capability is rebound on every machine construction and never
  rewrites an existing Job's admitted timeout;
- v0.17.9 fixes the advisory clock: `created` remains Job-age/`elapsed_ms` (including product
  queueing), but the runner publishes one absolute `execution_deadline` when it enters Running;
  Queued has no deadline advice and `jobs_get` counts down from `execution_deadline - now`, never
  `timeout_secs - elapsed_ms`. Advice still appears only inside `min(timeout/5, 5m)` on the bounded
  surface and is absent from list/read/ack/cancel and terminal/unknown/cancelling states;
- AW `ReceiptProfile` supplies factual `ts` only for dotlink (`checkpoint_minutes=None`) so generic
  AW checkpoint prose cannot conflict with exact 25/35/10 product guidance;
- if the future shared Jobs adapter blocks this composition, factor only a narrow typed factual
  Job-status projection seam. No rendered receipt parsing, arbitrary callback/plugin, AW clock/turn
  policy, or duplicate dotlink lifecycle state.

The post-release `81ef891` delta plus v0.17.9 refinement is **shared Occupancy correctness**,
not dotlink presentation. Windows `CREATE_NO_WINDOW` can leave transient console/job infrastructure
counted briefly after the main process exits. AW therefore performs bounded post-terminal Job-
accounting settling before publishing pre-cleanup Occupancy: zero at any sample => Empty; query
failure => Unknown; nonzero for the full **uninterrupted** reference bound (<=2s, ~10ms cadence) =>
Live. Cancellation or execution deadline preempts the grace and sweeps immediately; unless Empty
was already proven, the interrupted observation is Unknown and the main physical exit remains
unchanged. `aw-platform-win` owns only the cheap synchronous exact accounting query; `aw-process`
owns the async settle/cancel/deadline race. No conhost/image-name exception and no blocking Tokio
sleep. Dotlink later consumes that shared fact and retains only its `tree`/`success` compatibility
projection.

v0.17.9 (`576d0a1`) also tightens **shared Spawn Facility admission**: when a runner enters
Running it establishes one absolute execution deadline before Spawn Facility admission. A request
queued behind another lane may become stale; cancellation/expiry is rechecked on the stable lane
immediately before fork through a private linearizable launch gate. Stale-before-`Launching` means
no child and therefore no ProcessTerminal/StopRecord/kill claim. Once `Launching` wins, any real
child follows ordinary RAII/Process settlement. Spawn-Lane queueing consumes the same deadline;
pre-Running product queue age does not.

**Local qualification:** `aw-process` 7/7; `aw-platform-unix` 18 executed + 1 explicit cross-UID
ignore; `aw-tools` 126/126; adapter Process 27/27 using the real Nix Bash; focused foreground shell,
shell cancellation, Graph ShellNode cancellation and Graph nonzero-exit smokes all green; root
`aw-cli --tests` type-check green; strict affected all-target/all-feature Clippy, rustfmt and
diff-check green. No Windows target is installed on L5, so no native/cross Windows pass is claimed.

**Dotlink product-policy source checkpoint:** against the current old AW pin, Jobs 121/121,
shared-native 15/15 and Graph 13/13 are green; focused shell queue/timeout/schema tests and bounded-
turn/deadline tests are green; full test graph type-checks; strict all-target/all-feature Clippy,
fmt and diff-check are green. The exact Running/Start checkpoint now establishes the local absolute
execution deadline; local surfaces remain uncapped/quiet and hand-back surfaces derive one
25m/35m/10m policy source.

### Phase-0B compact replay/lifetime checkpoint — complete locally

AW's indexed `policy_admission_current` remains the sole hot replay owner: the immutable replay key,
actor, canonical target, original request digest and revision stay in indexed columns; after a
consumed effect crosses a durable recovery boundary, the full current-row detail compacts best-
effort to a versioned <256-byte never-retry tombstone. Immutable audit retains the full evidence.
A failed compaction leaves the full row and never changes the effect result. Legacy
Consumed-without-Start remains full until Remote Start commits, preserving exact recovery.

Grouped qualification extends the existing policy-admission owner test: the first consume returns
full evidence, explicit post-boundary compaction retains exact identity/target/route, replay cannot
become executable under changed policy/authority, and a fresh admission still succeeds after 8,205
unrelated Session audit rows. AW Remote Start owner tests are 5/5; policy-runtime replay tests 4/4;
root strict all-target/all-feature Clippy, test type-check, fmt and diff-check are green.

Dotlink's existing in-memory `remote_starts` owner now follows the same rule without a new ledger:
full `Mcp`/`JobMcp` receipts retain ordinary TTL, then compact to `RemoteStartReceipt::Compacted`
instead of deleting the authorization-id claim; Unknown is already a payload-free tombstone. Claim
lookup remains HashMap O(1), full-receipt retention compaction is amortized through a FIFO, and
payload-free tombstones do not consume the 4,096 active/full receipt budget. Current disclosure
still gates replay projection: filesystem/shell may return a generic compacted acknowledgement only
under current source/tool disclosure; Job/Graph controls without a retained exact Job ref return the
opaque withheld acknowledgement. Consolidated dotlink `remote_start` tests are 16/16 and strict
all-target/all-feature Clippy/test-graph/fmt/diff are green.

R6 Event/Job capacities remaining in source are live-resource bounds, not lifetime-history limits:
Agent Job Events caps current subscription count; finite Job Exposure counts only current
reserved/published rows in one generation/project; policy/Remote Start limits are per active Turn.
They are intentionally retained.

### Dotlink shared-Process consumer cut — implemented locally against local AW

Dotlink's sole shared Process launch now awaits `aw_process::Process::spawn` and passes the exact
Running/Start deadline plus a level-triggered cloned `JobCancel` through `SpawnControl`. Lane-side
`CancelledBeforeSpawn` and `TimedOutBeforeSpawn` project directly to product cancelled/timed-out
results with `process_not_started=true`; they carry no `ProcessTerminal`, stop evidence, signal or
synthetic Process fact. Definitive OS spawn refusal remains no-Process Failed. Other post-launch/
setup uncertainty remains Unknown as before. The internal old `spawn_failed` boolean is generalized
to `process_not_started`, and shared settlement uses ordinary status settlement when no Process
existed instead of calling `settle_process`.

Focused consumer qualification uses the real process-global Spawn Lane: one test briefly holds the
lane, lets a second Job reach Running and queue, cancels it, and proves no marker command executes,
product status is Cancelled, caller origin is preserved and no ProcessTerminal exists. The timeout
sibling uses the same projection helper without monopolizing the global lane. Against the local AW
checkout, full shell tests are **86/86**, Graph **13/13**, canonical spawn-refusal settlement is
green, full test-graph check is green, and strict all-target/all-feature Clippy/fmt/diff are green.
Validation temporarily substituted local AW paths and restored Cargo.toml/Cargo.lock exactly; the
manifest still pins published `7c16708`, so the current combined source intentionally requires the
future AW publication/repin before an ordinary dotlink build can consume this API.

A real Bubblewrap smoke was explicitly attempted (`DOTLINK_TEST_BWRAP=1`) and reached the installed
`bwrap`, but L5's outer tool sandbox rejects mounting `/proc` with `Operation not permitted`. This is
an unavailable host capability, not a passing/failing Bubblewrap product qualification. AW's generic
`PDEATHSIG` parent-thread regression is green; real-Bubblewrap worker-retirement remains to run on a
host that permits Bubblewrap mounts. Windows-native qualification likewise remains unavailable on
L5.

**Next coherent sequence:** when explicitly authorized, publish AW and repin dotlink to this exact
shared source; re-diff release/0.17 from exact v0.17.9; rerun the already-green consumer gate under
the real pin, plus real Bubblewrap worker-retirement on a suitable host and Windows-native
qualification. After that review, Phase 1 is the next implementation phase.

Author: coordinating design/implementation agent
Owner: @pvl
Updated: 2026-11-21
Source authority host: **L5 only**
Canonical checkout: `/home/pvl/spaces/abird/src/aw`

> Authority order: live source/tests/executed results > direct user instruction >
> active plan/TODO/spec > dated comprehensive handoffs.
>
> The November 15 comprehensive handoff is historical context for the Workspace
> continuation. It is not the current Git-state record.

## LIVE DL-200 checkpoint — original policy ToolCall replay, 2026-11-21

Read `IMPLEMENTATION.md` Phase 0B for the owning contract and unresolved gates.
The existing indexed `policy_admission_current` owner now finds accepted ToolCalls by
immutable `(session, branch, Turn, call ID)` **before** resolving contextual refs or
selecting current policy. The original operation+arguments fingerprint detects changed
request identity, while mutable writer/policy/participation epochs remain admitted
factual evidence. Exact replays retain the original canonical target after reparenting.
A current participant `view` check gates replay lookup; fresh calls still require
`submit` plus the real writer/tool/provider/payer authority. Reusing the same call ID
for ambiguous legacy targets fails closed, and no additional replay ledger was added.

Validation on L5: `aw-store --test store policy_admission` **2/2**; the filtered
production `aw-cli` policy-operation router integration **1/1**; `cargo check`
for AW Store/CLI green; strict affected all-target Clippy **green** after two
behavior-preserving `needless_return` cleanups in the in-flight Unix Process
observation code; policy-owner rustfmt and `git diff --check` green.

### Phase-0B replay payload disclosure checkpoint — cross-host owners composed, uncommitted

Replay identity and payload disclosure are now separate at the correct owners. AW revalidates
current **Session View audience** plus exact **ToolBinding + Actor Envelope source/effect/site**
before asking for a retained remote receipt. Dotlink then independently revalidates the concrete
target-side disclosure before releasing cached content: current issuer trust, current prepared
filesystem mutation capability for canonical native FS requests, current shell ToolPolicy, or
current Job-control ToolPolicy plus exact retained Job availability. Lease expiry and old runtime
generation still block only new effects; they do not erase a factual replay. If either host loses
current disclosure, the caller receives only one generic non-retryable prior-invocation
acknowledgement—no target/ref, Job ID, output size, timestamp, provider/site metadata or protected
payload—and the replay claim remains consumed.

No cross-host preflight protocol, Tool trait hook, replay service or table was added. AW adds only
zero-cost `SourceKind::as_str()` reuse plus its existing source/audience projector. Dotlink reuses
its existing `ValidatedRemoteStart`, prepared `CapabilitySet`, canonical request `access_paths()`,
`SecuredFilesystem::preflight_mutation`, `has_tool_for_policy`, and `JobManager::standalone_status`.
The filesystem replay path performs only authority resolution/preflight: no payload read, lock,
stage or commit. This keeps concrete authority on the machine that actually owns the resource.

Validation on L5: AW production bin check is green; prior policy owner **2/2** and AW Remote Start
owner **5/5** remain green. Dotlink compiles, grouped Remote Start/replay is **16/16**, strict
all-target/all-feature Clippy `-D warnings` is green, and rustfmt/`git diff --check` are green. The
existing routed AW integration binary remains subject to the unresolved L5 SIGKILL during linking;
after the v0.17.7 audit this is treated as an unattributed/infrastructure interruption, not a
proven external-host kill, and the test was not fragmented into replacement fixtures.

### Phase-0B participant Provider/payer checkpoint — complete, uncommitted

Fresh policy admission now reselects the authenticated participant through one shared
`ProviderAccounts::select_exact(actor, identity)` primitive. Full `InvocationIdentity` equality
covers account owner, **payer**, custody/version, executor, model and grant revision; there is no
Session-owner account/payer fallback. Replay lookup still happens first, so a factual consumed
operation never becomes fresh merely because provider authority changed. Guardian/delegate paths
reuse the same exact selection, and the service-less remote router receives the same
`ProviderAccounts` owner directly.

Policy delegation now separates the two principals that were previously conflated: the parent
Session owner owns the delegation root/child Session/journal/lifecycle, while the initiating
participant owns provider/payer/model/tool identity. The participant path is deliberately narrow:
current `Submit` is rechecked transactionally, depth is exactly 1, child routing/constructor replay
are forbidden, and recursive widening remains unchanged/denied. Store model-attempt budgeting
binds the paid identity to the child relation's stored `InvocationIdentity` rather than assuming it
matches the Session owner. Lifecycle mutations authorize the resource owner only. The child Actor
attach path independently rechecks participant `Submit` and the exact ProviderSelection.

Focused evidence without new fixtures: account publication/revoke/regrant **1/1**; Store
participant child creation → running → Bob-identity model-attempt → settlement + revoke denial
**1/1**; policy admission replay **2/2**; `cargo check --offline --locked -j1 -p aw-cli --tests`
green; strict affected all-target Clippy `-D warnings` green; fmt/diff green. The existing
end-to-end policy-router test includes Bob's own payer/provider, revoke/no-fallback and paid
delegate assertions and type-checks, but its root test executable still receives an unattributed
SIGKILL while linking on L5 before assertions. The v0.17.7 parent-thread bug is now a plausible
infrastructure cause, so this is not classified as an outside-host kill and the test was not split
into replacement fixtures.

**Remaining Phase 0B:** compact replay tombstones + removal/qualification of former lifetime audit
ceilings. No commit/push/release.

## LIVE DL-200 implementation checkpoint — Phase 0A, 2026-11-21

Current AW source checkpoint: `c7cc948` (`Checkpoint DL-200 Phase 0 owner hardening`).
It preserves both the pre-R6 Phase-0A Process slice and the independently developed
Phase-0B owner-hardening slice. Phase-0A Process state:

- added shared `Tree::{Empty,Live,Unknown}`;
- `ProcessOutputMeta` carries per-stream completeness; aggregate completeness is derived;
- `ProcessTerminal` now retains `exit + Option<StopRecord> + tree + output`;
- `ProcessReceipt` now stores one ProcessTerminal directly; Outcome/completeness/status/stop
  helpers are derived projections;
- `PipeProcess` owns one private `Scope`; it no longer owns Child + PID + guard separately;
- direct-host fallback is explicitly `aw_platform::DirectScope`: Linux prefers pidfd and falls back safely to the still-unreaped child PID plus process group; Windows owns duplicated leader HANDLE + Job Object; macOS owns child/group identity;
- no public/free readiness API, no higher-layer disarm/release, and the bounded final retry is owned by DirectScope; Darwin EPERM is accepted as NoTarget only when the leader is terminal and remaining scope is proven empty;
- Linux direct observation remains a fallback proof path (same PID namespace + unfiltered
  procfs, zombies excluded, live threads counted), not the target architecture;
- strong Linux target is a tiny AW PID-1 supervisor in a per-command PID namespace with
  Bash/main command as PID 2, instantiated by a real host/Sandbox launcher when promoted;
- Sandbox and Scope are explicitly orthogonal: Sandbox is the durable logical execution
  environment; a fresh private Scope owns one Process lifetime. Unsandboxed/direct remains
  first-class; a Sandbox may enable stronger containment without changing ProcessTerminal;
- sandboxed does not imply PID namespace, Scope backend choice is private/host-owned, and
  launch prefers prepare environment -> prepare Scope -> launch into Scope -> publish;
- `Tree` is immutable pre-cleanup occupancy; cleanup never rewrites it. `Tree::Empty` is
  backend-independent and means the full semantic Process scope is proven empty; direct
  Unix negative group/procfs observation becomes `Unknown` whenever escaped descendants
  cannot be excluded;
- tree facts are not yet projected into compact result strings;
- direct fallback settlement remains non-reaping where needed and settles scope before drain;
- redirected background children are factually `Tree::Live` and are killed before drain;
  inherited-pipe output now quiesces instead of timing out incomplete;
- a launch requirement for closed descendant containment must reject before effects if no
  closed backend is available; never silently downgrade to DirectScope;
- PID-1 supervisor/report loss before the main-command exit fact yields
  `ProcessExit::Unknown`; namespace cleanup never masquerades as the main exit or rewrites
  Tree;
- dotlink Bash/runtime-horizon/result projection is **next and not started**; dotlink must
  consume the same shared Process lifetime owner/settlement semantics, not keep a parallel
  guard/wait/reap runtime and merely copy fields.

Focused green evidence below predates the latest stronger Tree qualification and is **not**
Phase-0 acceptance. Two explicit implementation gates remain: current direct-Unix
`Ok(false) -> Tree::Empty` semantic mapping must become Unknown for weak negative evidence,
and the real dotlink consumer must compile against the shared Process owner without pulling
the full Agent adapter solely for lifetime logic.

Focused evidence:

- affected shared crates compile, including `aw-platform-win`;
- Unix managed-process tests 4/4;
- aw-tools runner tests 4/4 with `full`;
- strict affected all-target Clippy is green;
- DirectScope tests 4/4 and adapter regressions green for redirected-background cleanup, inherited-pipe cleanup, external exit vs connector stop, external signal identity, cancellation, deadline and terminate-once/SIGTERM;
- `git diff --check` clean.

Workspace check note: low-parallel `cargo check --workspace` reached root `aw-cli` and hit
two pre-existing Rust-2024 let-chain expressions in `src/subagents.rs`; those unrelated
files were restored/left untouched and are outside this Phase-0A diff.

Concurrent-work guard: `crates/aw-store/src/job_exposure.rs` became independently dirty
during verification with substantial Job-exposure changes unrelated to Phase 0. It was
not edited/reverted by this work and must be preserved.

## LATEST DL-200 dependency/owner review refinement — 2026-11-21

The external review was accepted with one stronger Process rule:

- Phase 3 now lands the one private replay-safe prepare/publish/reconcile construction
  primitive before `checkpoint.restore`; Phase 4 exposes/reuses it for host/model Agent
  constructors rather than creating another path;
- `Tree::Empty` retains its strong semantic Process-scope meaning. Weak direct-Unix negative
  membership evidence is `Unknown` when escaped descendants cannot be excluded;
- final typed leaf registries are introduced at first semantic use (`EventType` in Phase 2;
  SourceDriver/SandboxDriver/VaultProvider in Phase 3) and Phase 5 composes those same
  owners into ExtensionRegistry without migration/shadow registries;
- dotlink must reuse the same shared Process lifetime owner and is behaviorally qualified,
  not merely field-compatible;
- nested Agents remain allowed within existing bounds, but direct/Agent-mediated/descendant
  nested `batch` stays denied transitively while active Graph-node ownership exists. A later
  independently admitted Turn after release regains ordinary top-level batch eligibility;
- `agent_state_current` is authoritative durable Agent state after one-time **exclusive
  store migration**: complete Agent+ACL seed and new owner/schema epoch activate atomically
  under write exclusion; v1 has no online dual-write/catch-up. SessionStore remains
  Turn/history owner and only Agent lookup/exposure indexes rebuild;
- AW transaction commit is the sole constructor visibility point; post-commit wakes/caches
  are reconstructible accelerators only;
- replay owners compact full detail to minimal never-retry tombstones; replay without current
  disclosure returns only minimal/opaque non-retry acknowledgement and never fresh effects;
- Event cursors/subscriptions/Handlers bind StoreId + **authority-scoped** placement
  generation: hidden Domains cannot perturb observable staleness; authorized cross-store
  scope stales/lags explicitly and progress is revalidated at commit;
- Event projection progress is fenced by one contiguous indexed-through commit position;
  consumers never advance across a missing committed Event projection;
- EventInput acceptance is the one delivery linearization point: subscription generation,
  store affinity, current source/recipient/disclosure authority, dedupe and causal charge are
  checked with input admission; unsubscribe/close/revocation races use transaction-winner
  semantics. V1 Agent delivery requires Event/subscription/causal owner + recipient Input
  owner in one transactional store; cross-store Agent delivery is rejected. Authorization is
  source-first: proven hidden source skips privately without deeper manifest/provider work.
  For an already-visible source, externally mutable disclosure authority must be immutable or
  provider-fenced through acceptance; otherwise `events.read` returns candidate-oblivious
  retryable `authorization_unavailable` with unchanged cursor and subscription/Handler holds
  position with bounded backoff;
- repeated `events.subscribe(..., enabled=true)` on an active same-subject row preserves
  generation/baseline/backlog; lagged re-enable explicitly creates a fresh baseline;
- v36 delivery migration sets `delivery_cutover_rev = current next_event_rev` inside the
  serialized source-owner cutover; entire revisions are owned by one path. Already accepted
  legacy Inputs retain identity and gain immutable compatibility disclosure/provenance
  sidecars; incomplete provenance is conservative `legacy_protected`, while only pending
  pre-cutover deliveries bridge once;
- early leaf registries use the same private staged mutation primitive from first use, so
  Phase 5 composes them atomically without wrappers/migration;
- multi-resource constructors durably claim the existing operation receipt as Prepared
  before the first external/materializing effect; **capture and restore share the same
  prepared SourceDriver operation invariant**, with descriptor pinned to exact driver
  registration + recovery schema. Named sources are bounded (default 128, hard 1024) and
  durable descriptors are non-secret opaque IDs/digests/proofs/handles only. Crash/replay
  inspects/reconciles that candidate instead of blindly repeating effects; live bindings/
  checkpoints/unresolved descriptors block incompatible driver removal/reload, and store-open
  validates persisted dependencies before source effects;
- final publication revalidates all AW-store-owned source/destination/grant/ACL/lifecycle
  authority **inside** the publication transaction and holds AW-owned source fences through
  commit. External evidence must be immutable/non-revocable for the commit interval or a
  provider-enforced fence/lease held valid through the entire AW commit; a pre-commit
  `valid()` check is insufficient. Otherwise the operation stays Prepared/Unknown.
  Revoke/replace-first denies; publish-first remains factual;
- replay guarantees stable effect/created-resource identity rather than indefinite payload;
  ToolCall tombstones follow retained Agent/history lifetime and host-RPC tombstones require
  irreversible request-namespace retirement before deletion;
- `jobs_list` is exposure-store-local and gap-safe. Page one captures publication high-water;
  exact order/cursor key is `(publication_position DESC, canonical_ref ASC)`. A private
  rebuild captures H and, under the final write/swap fence, must verify current head == H or
  catch up `(H,current_head]` before activation; a stale replacement cannot erase newer
  rows. A resumed tree
  frontier is navigation only: every Agent ancestor gate is reauthorized and a now-hidden
  parent discards its entire queued subtree even if a descendant is directly viewable;
- DisclosureManifest is a persistent bounded-fanout content-addressed set/DAG: exact
  provenance facts remain authoritative, while ordinary disclosure checks deduplicate over
  structurally shared stable authority roots with bounded per-request work. An authority
  root is dependency/maximum-ceiling identity only; it identifies the live policy/ACL/group/
  provider owners to recheck and never caches current permission except for an explicitly
  immutable export grant contract;
- the future PID-1 report channel is unidirectional supervisor→host, command descendants
  inherit no writable endpoint, and the host validates framing/launch identity/exactly one
  terminal report so user code cannot forge ProcessTerminal facts;
- Event read privacy promises no direct hidden identity/subject/exact-count/private-position
  disclosure, not constant-time/constant-volume behavior; bounded scans may still permit
  coarse aggregate traffic/timing inference.

## LATEST DL-200 design refinement — 2026-11-20

This section supersedes older DL-200 design summaries below where they conflict.

- one public host constructor: `agent.create(from=Agent|settled Turn)`; no `agent.fork`;
  trusted in-process library code may use typed HistoryBaseRef as a lower primitive;
- model ancestry selector is `agent(..., from=...)`;
- `checkpoint.restore` remains first-class because it restores exact history **plus**
  exact SourceSet into a new Agent + new Sandbox;
- checkpoints capture a plural named typed `SourceSet`; Git is the mandatory first
  SourceDriver and versioned/content-addressed object/bucket manifests fit the same
  fail-closed driver contract;
- Sandbox is canonical at `/d:/i:/p:/s:<sandbox>` as a Project-level sibling of the
  Agent execution branch; it never enters Jobs/control `ExecutionRef` grammar;
- Sandbox is logical mutable execution state. An Agent chooses one Sandbox at creation and
  does not live-rebind in v1; Agents can share one, branch one, or start fresh by creating
  a new Agent; one fresh private Process Scope remains per dispatch;
- named source attachment is only `sandbox.sources.list/set`; SourceRef never carries
  credentials, which remain Vault authority;
- same Sandbox does not imply same authority;
- Agent-scoped VaultGrants project selected provider-backed secrets/services as
  broker/socket/ephemeral-file/env capabilities; Vault authority is never inherited from
  history/Sandbox/checkpoint/ACL state;
- system Event names are bare; custom Events are structurally `#name`; Event routing
  reversibly encodes literal `.` in canonical handles as `%2E`, so resource refs need no
  grammar weakening;
- Events expose stable opaque `Event.id`, durable causal root/hop budgets and internal
  immutable DisclosureManifest refs; read, Agent delivery and durable Handlers all enforce
  exact source + disclosure closure; host also has bounded `events.subscriptions.list`;
- one plain `Extension` registers through one typed `ExtensionRegistry` composition root;
- typed subregistries include Tool/Hook/Action/Resource/View/EventType/EventHandler/
  Provider/ProviderHook/ProcessHook/UI/Setting/State/Compactor plus native SourceDriver/
  SandboxDriver/VaultProvider registries;
- native Rust registration preserves concrete types and may return typed handles; stable
  Extension/local IDs + schema versions own durable/wire identity, never Rust TypeId;
- all contributions from one Extension registration publish atomically across registries,
  but registered/installed definitions are inert until explicitly activated/bound by host
  authority; typed handles stale on their own registration/schema, not unrelated reloads;
- runtime callbacks still use scoped `ctx.tools`, `ctx.state`, `ctx.ui`, `ctx.models`, etc.;
- Context customization is Hook(Context), dynamic Tool visibility/calls use `ctx.tools`,
  Views are pull/read facets, EventHandlers are durable push/background reactions;
- Histate can register fully typed domain Actions/Views/Handlers/UI/Tools/Hooks without
  converting its Rust domain model to arbitrary JSON;
- dynamic Extensions reuse/generalize the existing `aw-extensions` child-process path: one bounded full-duplex stdio protocol projects the same registries and scoped `ctx.*` capabilities;
- Node/Bun/Deno/Python/Rust/etc. participate as ordinary executables; AW owns no npm/pip/cargo marketplace/resolver in v1;
- do not embed a JS runtime or Wasmtime for v1. A WASI Extension may already run through an external `wasmtime run ...` process speaking the same protocol; embedded WASM remains optional later;
- Histate may own a product Session referencing many AW Agents + zero/many Sandboxes and a
  product checkpoint referencing one/more AW Checkpoints; cross-store writes use a
  product-owned idempotent saga/outbox, not AW distributed transactions;
- shared Process settlement is sealed as one small AW delta: `Tree::{Empty,Live,Unknown}`,
  per-stream output completeness and one `ProcessTerminal { exit, stop, tree, output }`
  truth; platform observation stays `io::Result<bool>` and settlement happens before final
  reap/drain;
- dotlink alone owns result/log projection, non-login Bash and the runtime horizon:
  ~30-minute coherent-checkpoint guidance + 35-minute hard conversational command bound;
- one authenticated mutation replay identity/fingerprint protects constructors/additive
  mutations without per-method request-id clutter; multi-resource construction uses private
  prepare -> atomic AW publication -> reconciled cleanup, so half-created resources are
  never public;
- clean and dirty Git checkpoint anchors are both privately retained; submodule contents
  require their own SourceBinding when they must be retained/reproducible;
- Vault `file/env/socket` projections are provider/host-defined safe profiles, never
  arbitrary caller mount/env injection; grant removal never pretends to erase already-
  materialized capability from a running Process;
- the frozen 017 sandbox/mount/config-v13 implementation plan remains frozen.

The canonical detailed authority is the DL-200 SPEC/TODO, not older summaries in this
handoff. The fresh full pre-seal audit and earlier naming/typed-registry/dynamic-host
reviews are clean: no duplicate owner/catalog/database/replay ledger, partial registry
publication, authority/disclosure leak, Event-log duplication, stringly native Histate
path, second transport/runtime stack or Histate/AW ownership conflict remains.

## LIVE published AW + dotlink repin/trust checkpoint — 2026-11-20

This section supersedes older current-state/pending-order text below where it
conflicts.

Current published boundary:

- AW shared source implementation is signed/published at
  `b8a46a942a56a5ae5cb7e2a8fad47010fde4d65b`
  (`Complete v36 runtime admission and process semantics`);
- dotlink consumes that exact AW source commit in all seven direct/transitive AW
  Git packages;
- dotlink signed source checkpoint is
  `648519add3d174b9e13de81aeca62431591dac6d`
  (`Repin AW and complete process-control hardening`);
- dotlink `main` is published through
  `bbd29c13904b7255eb28e2ed6c776c3590ac122a`
  (`Record AW repin and Remote Start trust cutover`);
- the read-only release worktree is currently local
  `release/0.17 = v0.17.6 = 6cd056672d9390b17ac591786253af5f04bfeef1`,
  3 commits ahead of the still-published
  `origin/release/0.17 = v0.17.5 = 3995448d7bf33794ef84c7bb51150d77cc260ada`;
- D4-D7/v0.17.5 semantic equivalence remains closed; the later v0.17.6 forensic delta is
  now captured by sealed DL-200 section 25.11 / implementation Phase 0;
- sandbox/mount/config-v13 remains frozen.

AW v36 completion published in `b8a46a94...` includes:

- explicit v35 -> v36 migration/open coverage and projection backfill;
- exact replay of historical issuer-less v35 Remote Start facts without
  manufacturing v36 issuer evidence;
- `RemoteStartIssuer { client_id, trust_epoch }` and
  `RemoteStartLease { target, issuer }`;
- issuer-bound v2 authorization digest for new Starts, with issuer-less v1 facts
  retained only for exact historical replay;
- issuer/lease-independent recovery of an exact committed Start before new
  mutable authority is consulted;
- indexed canonical Agent lookup with current Session authorization recheck;
- query-only `job_exposure_current` plus bounded keyset pagination for mixed
  Agent/finite Job listing;
- durable Agent/Turn source events, wake admissions and restart recovery;
- shared `ProcessTerminal::status()` projection where physical/semantic process
  outcome and output completeness are orthogonal:
  `Primary` derives from exit/stop evidence and
  `Status.incomplete = !output.complete`.

The dotlink consumer cutover is also complete:

- ordinary child exit immediately performs reasonless managed-descendant
  housekeeping;
- inherited stdout/stderr use a separate 250 ms post-exit drain grace;
- an escaped `setsid` writer cannot erase a known command exit merely by
  retaining an inherited FD;
- the temporary dotlink `shared_process_status` compatibility shim is gone;
  dotlink consumes AW `ProcessTerminal::status()` directly;
- revision-fenced public cancel requires canonical shared Job ownership;
- stdin authority and revision acceptance are linearized before bytes are
  enqueued;
- duplicate static MCP instruction ownership is removed.

Trusted AW Remote Start issuer enforcement is complete target-side:

- ordinary OAuth authentication remains distinct from AW-bridge issuer trust;
- private dotlink OAuth state persists one monotonic trust epoch per approved
  DCR client;
- owner controls are `oauth trust-aw-bridge` and `oauth untrust-aw-bridge`;
- lease issuance is 401 when unauthenticated, 403 for authenticated but
  untrusted clients, and returns issuer-bearing `RemoteStartLease` only for a
  currently trusted bridge client;
- the authorization issuer client ID must match the authenticated OAuth client;
- exact committed replay resolves before current issuer trust;
- revocation blocks a genuinely new Start but does not erase the original
  committed replay;
- re-trust advances the epoch, so an older lease cannot become current again.

Final repinned dotlink qualification on L5:

- main binary: **706 passed / 0 failed / 1 intentional ignored**;
- integrations: **42/42**;
- strict all-target/all-feature Clippy `-D warnings`: green;
- fmt/diff, Alejandra and Markdown (**0 issues / 86 files**): green.

Two combined all-feature test invocations received unattributed SIGKILL before
producing test output or Rust diagnostics. The complete main binary and all six
integration targets were then run serially and are green. At the time these were
treated as infrastructure noise; dotlink v0.17.7 later identified the retireable-
Tokio-worker parent-death bug as a plausible mechanism. They remain non-source
failures, but are no longer attributed to an external host.

Remaining boundaries are external/deferred only:

- native Windows Job Object runtime validation;
- native macOS runtime/deployment validation;
- trusted-selector Bubblewrap runtime smoke on a suitable host;
- frozen sandbox/config-v13 unless explicitly promoted.

No release/tag/deploy action was performed by this source publication.

## LIVE 0.17 fresh-review convergence checkpoint — 2026-11-19

This section supersedes older state summaries below where they conflict.

Current Git boundary:

- authoritative fresh-review AW implementation pin:
  `1a5d0f65208e4a05b737c804a55a3aaad3e2fa59`
  (`Relax shared MCP protocol dependency ranges`), signed and selected as
  `AW_IMPL_SHA`; it follows source checkpoint `7c7de6c9...` and prevents the
  protocol crate from downgrading compatible downstream Tokio/serde_json;
- previous source checkpoint `eb325b31...` is historical and must not be used
  for the dotlink repin;
- AW `HEAD == origin/main == 4234447612421069a746c925c76a25e3ba27774a`
  (`Record final shared 0.17 implementation pin`); final reusable implementation
  remains `AW_IMPL_SHA = 1a5d0f65208e4a05b737c804a55a3aaad3e2fa59`;
- dotlink `HEAD == origin/main == 4c3b96fd5ba9e6772bc159f10a3ad893c9729887`
  (`Record repinned D1-D3 convergence checkpoint`);
- dotlink D0-D3 are published/complete; D4-D7 are the active consumer queue;
- AW and dotlink working trees are clean at this handoff refresh;
- the read-only release reference is now fully published at
  `release/0.17 = origin/release/0.17 = v0.17.5 =`
  `3995448d7bf33794ef84c7bb51150d77cc260ada`. This worktree was not modified
  by this agent.

Fresh-review shared corrections:

- `StopReason::as_str()` is the single shared
  `timeout|cancelled|runtime_error` spelling;
- zero-cost `StopTracker` owns Unknown -> retry reason preservation;
- `ProcessReceipt::stop_record()` exposes the final factual logical stop while
  `ExecutionLifecycleEvent::ProcessStop` remains attempt-level diagnostics;
- v0.17.5 outside-signal projection remains product-only: physical
  `ProcessExit::Signal(n)` plus no reasoned final stop record may become one
  `LOG job exit ... from=outside`; no new AW outside/provenance enum is needed;
- `ProcessTerminal` remains minimal `{exit, stop, output}` terminal truth;
- typed Job/Node receipts now preserve `ProcessTerminal.stop`;
- Job receipt lifecycle distinguishes `revoking`, terminal `revoked`,
  `cancelling`, terminal `cancelled`, and ordinary `settled`;
- physical exit remains
  `ProcessExit::{Code(i32),Signal(i32),Unknown}`, with shared
  `aw_platform::exit_signal(&ExitStatus)` and `signal_name(...)`;
- `JobRef` remains flat/bounded with private fields and typed accessors;
- `RawPageBytes` and `SerializedResultBytes` remain orthogonal;
- bounded diagnostics and typed Job receipt grammar remain shared.

MCP projection is now split cleanly:

- **`aw-mcp-protocol`** owns generic MCP model shaping:
  result validation, 16 KiB result/block metadata bounds, replay-stable receipt
  timestamps, text-only serialized fitting, receipt sidecar reservation,
  heartbeat/progress projection and observer summaries;
- it depends on `aw-tools` with **no aw-tools features**, rmcp model types,
  `serde_json` and Tokio timing/macros — no Agent runtime, reqwest, jsonschema,
  clap or MCP transport stack; receipt decoration takes a feature-neutral
  copyable `ReceiptProfile` and no longer clones whole results just to reserve
  a visible marker;
- the full **`aw-mcp`** client adapter depends on and re-exports that surface;
  it no longer carries a second implementation;
- fitting never silently truncates: the generic marker is mandatory, and
  immutable typed/structured facts fail explicitly when they cannot fit;
- decoration first fits against the full envelope, so a visible stored `ts=`
  that survives fitting never pays an unnecessary sidecar reserve;
- receipt timestamp mutation preserves existing fields under
  `io.abird/receipt` and fails transactionally if the reserved value is not an
  object;
- at the 32-block MCP limit, an existing text block can carry the timestamp
  marker; an all-typed full block set fails explicitly.

Fresh AW validation:

- `aw-tools --all-features --lib`: **357/357**;
- `aw-tools-adapter --all-features --lib`: **255/255**;
- `aw-mcp-protocol`: **12/12**;
- `aw-mcp --all-features --lib`: **20/20**;
- `aw-platform-unix`: **13/13**;
- Unix live PTY integrations: **4/4**;
- cross-UID fixture: **1 intentionally ignored**;
- `aw-terminal --lib`: **20/20**;
- all relevant no-default/minimal checks: green;
- host-side `aw-platform-win` check: green;
- downstream `aw-host` / `aw-cli` and full workspace all-feature check:
  green;
- strict Clippy for core/adapter/protocol/client/platform/terminal: green;
- rustfmt and `git diff --check`: green.

The native Windows Job Object / exit-signal assertions remain authored but
unexecuted on L5 because its Windows Rust std target is unavailable.

Publish and atomic repin are complete. `AW_IMPL_SHA` is fixed at
`1a5d0f65208e4a05b737c804a55a3aaad3e2fa59`. The direct dotlink AW set now
pinned atomically is:

- `aw-tools`;
- `aw-tools-fs-unix`;
- `aw-tools-fs-win`;
- `aw-platform`;
- **`aw-mcp-protocol`** (not the full `aw-mcp` client adapter).

All direct/transitive AW Git packages in dotlink resolve to that exact SHA,
with no registry package/version churn and no path override. Frozen
sandbox/config-v13 work remains untouched and deferred.

## Latest AW-tools published checkpoint — 2026-11-18

- Shared-runtime implementation commit:
  `be6d48db552fd44b9dd85364cd8734454109ce68`
  — `Harden shared Job output and journal uncertainty`; strict-ACK follow-up
  `26a8c788d6dc7e0a7fe16995f8057fb332440978` remains the D1 ACK-compaction
  checkpoint. Writer-quiescence landed in
  `beef1bc2d2f37b5660ebe928d368bca37368e608`; the current shared metadata pin
  is signed/published `7e6fe20a95640e32610819ce94b61a121e165dea`
  — `Align shared Job list metadata`.
- Local `main` contains signed typed-media implementation
  `d6009deaea31abe0cdfbd87b4de48b0eaf9a0ba0` plus its owning-plan follow-up;
  `origin/main` remains published `7e6fe20a95640e32610819ce94b61a121e165dea`
  until a separately authorized push.
  Dotlink `main` is published at
  `5381729cd469eda54691e540deb3fee13e9f3e3e`
  (`Complete shared Toolset metadata adoption`) and consumes `7e6fe20` in all
  three shared dependencies/lock records.
- Git reports **Good ED25519 signatures** for the published AW commits using key
  fingerprint `SHA256:4aB+NvRb1shCx4xjtZDr2Y7FzfHGph80eFtpF+pHuCI`; L5 has
  no `allowed_signers` principal file, so local trust status is `U` rather than
  a principal-mapped `G`.
- Current implementation qualification: `aw-tools` **341/341**,
  `aw-tools-adapter` **251/251**, read-only shared **110/110**, strict
  all-target/all-feature Clippy, formatting/diff checks, minimal shared build,
  MCP, minimal CLI and full CLI all green.

## Typed MCP wire/media checkpoint — 2026-11-18

- Signed local AW checkpoint `d6009de` adds bounded `aw_mcp::McpToolResult` as
  the raw typed MCP result boundary. It preserves current RMCP Text/Image/Audio/
  Resource/ResourceLink blocks plus structured content, with 32-block and
  existing 128 KiB semantic-result bounds plus bounded metadata overhead.
- Existing `ToolOutput` and the Agent/provider conversation path stay text-only.
  The ordinary MCP `RemoteTool` path still rejects non-text content; this work
  does not implicitly grant provider media delivery, device/media consent or a
  new durable media transcript format.
- Real cross-repo qualification uses exact published dotlink `5381729` over
  stdio with a private temporary profile: `bread(format=mcp)` preserves a PNG
  image block (`image/png`), WAV audio block (`audio/wav`) and PDF embedded
  resource (`application/pdf`) through AW's MCP client. Synthetic regressions
  also prove non-text text-path denial and block/payload bounds.
- Current gates: `aw-mcp` **18/18**, strict all-target Clippy `-D warnings`,
  downstream `cargo check -p aw-cli --lib`, rustfmt and diff checks green.
- Remaining media work is explicitly **provider/model delivery**, durable media
  receipt/history semantics and consent—not MCP wire interoperability.

## Current dotlink D1–D4 security checkpoint — 2026-11-17

- Signed implementation commit `26a8c788d6dc7e0a7fe16995f8057fb332440978`
  — `Preserve strict output ACK replay across compaction` — contains the shared
  fix discovered by dotlink D1.
- Dotlink's first D1 delegation removed its duplicate shared delivery watermark
  and exposed one shared bug: strict `acknowledge_delivered_output` rejected a
  lost-response replay of an already-ACKed owner watermark after another owner
  legitimately compacted the physical retained prefix below it.
- The fix validates exact owner + delivery history first, accepts only an
  already-committed `through <= owner.through` replay, and still requires current
  retained bytes for every **new** ACK advance. Foreign owners and new ACKs into
  compacted-away bytes remain rejected.
- The owner-aware compaction regression exercises the strict API for both the
  replaying Job owner and independent Task owner. Full shared **339/339** and
  minimal Job-runtime **200/200** tests pass; strict all-target/all-feature
  `aw-tools` Clippy, format and diff checks pass.
- `beef1bc` is published and the current uncommitted dotlink D1–D4 worktree
  pins all three AW dependencies/lock records to it. D1 remains complete there: shared
  delivery proof + Through-ACK + omitted-offset Finish + aggregate
  `jobs_get.tail` all delegate to AW. Tail uses the shared 512-default / 1024-max
  combined stdout+stderr split/redistribution and remains a pure peek with no
  delivery/ACK credit.
- D2 is also complete without a production grep fork. Dotlink direct MCP and
  Graph-native grep already use AW's exact schema, `request::parse`,
  `direct::execute`, and `search::grep`. A real stdio regression qualifies exact
  files/directories, runtime context 0/8/128, rejection of 129 and unknown
  fields, asymmetric side overrides, pagination/resume, compact incomplete
  projection, and native `batch` parity.
- Dotlink's first D3 owner-policy review found one concrete host/shared mismatch:
  standalone shell Jobs could become native MCP Tasks while the immutable shared
  plan said `task_result_eligible=false`; dotlink then attached a private
  hash-derived Task owner outside AW's plan policy. The current dotlink tree now
  declares standalone Process Task-result eligibility in the sealed plan and
  derives the exact lease with shared `task_result_owner(id, 1)`. Native and
  shell Graph plans remain Task-ineligible; non-Task dispositions release the
  prospective lease only after terminal metadata fixes the retained prefix.
- Hosted physical-delivery semantics still require positive POST delivery plus
  replay commit; 404/expiry/ambiguous delivery does not ACK the inner request
  key. A second D3 review found a real old-owner leak on namespace/execution-
  authority rotation: outer replay state could become retired/uncertain while a
  hidden durable shell request key still pinned the inner Job/spool. Dotlink now
  records bounded shell-owner dependencies before dispatch and routes revoked
  generations through its existing `RetireShell → retire_job_request_delivery`
  queue. A real replacement-client regression proves the same execution is
  exposed/released without rerun, while ordinary mutations remain `Uncertain`.
- Optional `consumer_output_owner` leases remain intentionally unadopted because
  dotlink currently has no distinct authenticated output-consumer audience beyond
  canonical Job/model and native Task owners.
- Final D3 audit adds direct proof that Busy old-owner retirement stays queued
  until the active response attempt releases, then retires/exposes the same Job;
  positive `Delivered` tombstones remain final under later authority revocation
  even with a registered shell-owner dependency. Runtime replacement sequencing
  was re-read end-to-end: old transports are cancelled/drained before a new
  runtime/client is constructed, and cleanup failure blocks restart.
- D4 independent review found one additional dotlink-host bug outside AW shared
  semantics: a cached shell response whose live sandbox guard was revoked could
  become generic uncertainty while hidden discard was Busy, allowing later
  expiry to lose cleanup while the revoked request key remained. Dotlink now
  carries guard revocation through the same bounded shell cleanup queue in an
  explicit **discard** mode; Busy cleanup stays queued, revoked hidden work is
  cancelled/discarded rather than exposed, and ordinary old-owner retirement
  still exposes the same execution. A red regression fails before and passes
  after the fix; no AW source change was required.
- D4 all-feature host qualification is **684 passed / 1 ignored** in the binary
  plus **36/36 integrations**; strict all-feature Clippy/fmt/diff/Alejandra pass.
  Repo-wide dotlink Markdown is **0 issues across 76 files**. All Nix check
  derivations are green using command-local verified SSH + an immutable Nix-store
  CA bundle. The hermetic Nix test fixture now supplies pinned Bash explicitly and
  passes **684/0/1 + 36/36 integrations** with a verified store output. Native full
  Bubblewrap qualification is also **684/0/1 + 36/36**, and all seven targeted
  GPU/KVM/device-security cases pass on a capable host. D1–D4 are complete.
  Fresh post-repin Rust qualification on `beef1bc` is **684/0/1 + 36/36**,
  strict all-feature Clippy/fmt/diff green.

## Repository state before this review

- Branch: `main`
- HEAD / `origin/main`:
  `7d09d5f3c882894cd46b427a89fa75280223115a`
  — `Advance workspace views, remote resources and handoff`
- Previous AW-tools merge:
  `0d2b4e3`
  — `Build configurable toolset and harden shared Job control`
- The worktree was clean, nothing staged, with one worktree.
- `.agents/plans/ux/**` had no worktree or staged diff.
- Use `dotlink---l5` only. Do not use A1.
- Do not create another worktree.
- Do not commit/push/tag/release/deploy without explicit authorization.
- Keep implementation turns near 30 minutes and checkpoint coherently.

## AW tools merge review — 2026-11-16

The merged boundary remains sound:

- `aw-tools` owns canonical transport-neutral filesystem, Graph, finite-Job,
  Toolset metadata/limits and typed receipt semantics.
- `aw-tools-adapter` remains AW projection/compatibility glue. It does not own a
  second filesystem engine, JobManager, scheduler or authority source.
- `Toolset::only` constrains both direct filesystem registration and Graph
  native leaves.
- Tool metadata overrides are presentation-only: the thin described-tool wrapper
  delegates availability, validation, parallelism and execution to the original
  Tool and cannot change name/effect/schema shape/grants.
- AW `ToolMode`, selected tool names, ToolPolicy, execution site and live Actor
  Envelope remain independent positive authority gates.
- finite `jobs_get` output disclosure independently rechecks live
  `jobs_read` authority before retained bytes are appended.
- finite `jobs_ack` composes current `ToolBindingAuthority` and live
  `AgentStepGate` commit guards around exactly one synchronous JobManager
  ACK/Finish mutation. The established lock order remains
  ToolBindingAuthority → Actor runtime → JobManager.
- shared delivered-output proof credits only raw bytes actually represented by a
  successfully validated finite projection.
- Graph `unknown` remains dominant over mixed known outcomes; explicit Finish
  refuses unresolved effects.
- canonical grep parser/schema/engine remain shared by direct and Graph
  execution.

### Review fixes currently uncommitted

Only narrow merge-quality corrections were required:

- update four hex/digest parsing loops to the Rust 1.98 strict-Clippy-preferred
  `slice::as_chunks::<2>()`; behavior is unchanged and the repository MSRV
  remains Rust 1.88, where this API is available;
- replace stale hard-coded `jobs_get tail 0..1024` / `wait_ms 30 seconds`
  validation messages with truthful configured-ceiling messages. Enforcement
  already used the configured `JobProfile`; only the diagnostics were stale;
- reconcile the toolset-builder and Job-output/grep TODOs with the actual merged
  commit `0d2b4e3`;
- clarify in the dotlink-tool-port plan that its November 9 AW revisions are
  historical consumer pins, not evidence that dotlink already consumes the
  newer Toolset/Job-control merge.

### Still open — do not claim complete

The approved Job-output contract still has two intentionally unfinished
observation semantics:

1. finite `jobs_get.wait_ms` must become lost-wakeup-safe and event-driven,
   waiting **only for settlement or deadline**. Current code still polls at
   10 ms and can return on any revision change.
2. finite `jobs_read.wait_ms > 0` must wait for new bytes or EOF/settlement at
   a nonnegative absolute stream offset. Current finite-Job path still rejects a
   nonzero wait; negative offsets remain nonblocking snapshots.

These belong to
`.agents/plans/ai-native-tool-output-grep-2026-11-15/` and are the next shared
Job semantics work. Do not hide them by marking the whole plan complete.

Before dotlink adopts configurable grep work profiles above AW defaults, B6
should also decide/document explicit product-neutral shared grep safety maxima
rather than inventing host-independent caps ad hoc. AW itself already prevents
widening beyond its configured/default product ceilings.

## Current uncommitted review paths

Preserve exactly these review changes unless the user asks otherwise:

- `.agents/docs/CURRENT_HANDOFF.md`
- `.agents/plans/ai-native-toolset-builder-2026-11-15/TODO.md`
- `.agents/plans/ai-native-tool-output-grep-2026-11-15/TODO.md`
- `.agents/plans/dotlink-tool-port/TODO.md`
- `crates/aw-tools/src/filesystem/direct.rs`
- `crates/aw-tools/src/filesystem/request.rs`
- `crates/aw-tools-adapter/src/binary.rs`
- `crates/aw-tools-adapter/src/jobs_tools.rs`

Nothing should be staged or committed without explicit authorization.

## Validation at this checkpoint

Fresh review gate after the code fixes:

- `aw-tools --all-features --lib`: **327/327**;
- `aw-tools-adapter --all-features --lib`: **242/242**;
- `aw-agent-core --lib`: **90 passed, 1 ignored**;
- read-only Toolset tests: **9/9**;
- `aw-tools --no-default-features`: green;
- strict affected-crate all-target/all-feature Clippy with `-D warnings`: green;
- formatting: green.
- downstream `aw-mcp`, `aw-host`, and `aw-cli --lib` checks: green.
- `git verify-commit 0d2b4e3` reported a good ED25519 signature, but L5 lacks
  the configured `allowed_signers` file, so principal trust resolution is
  environment-incomplete rather than fully verified here.

Run the final stable-tree diff/staging/UX audit before handing off.

## Other active work

Workspace/remote-view state moved in `7d09d5f`; use the live
`.agents/plans/workspace-views/TODO.md`,
`REMOTE-DEVICE-TRUST.md`, and `NEXT-AGENT.md` for that continuation. Do not
infer its current gate from the older November 15 snapshot prose.

Release-readiness and platform-native external gates remain separate from this
AW-tools review.

## 2026-11-17 AW–dotlink tools handoff update

The newer, comprehensive **shared-toolset/Job-output** handoff is [AW-DOTLINK-TOOLS-FULL-COMPREHENSIVE-HANDOFF-2026-11-17.md](AW-DOTLINK-TOOLS-FULL-COMPREHENSIVE-HANDOFF-2026-11-17.md). It supersedes this dated file for the post-merge Job wait, owner/ACK, journal and dotlink migration state. Keep this file for its original AW merge review and broader Workspace context; live code/tests and the active owning plan take precedence.

## 2026-11-18 current AW-tools revalidation

- Revalidated current `HEAD == origin/main == beef1bc` and reviewed the
  post-`0d2b4e3` shared changes through `be6d48d` and `26a8c78`, plus the
  current dotlink D1–D4 consumer paths. The older 2026-11-16 “Still open” wait
  bullets above are historical: settle-only `jobs_get` and byte/EOF-aware
  finite `jobs_read` are implemented and qualified in the active Job-output
  plan.
- Two adjacent shared removal defects were found red/green: a locally terminal
  Job with a still-live retained `OutputWriter` could be erased by
  `discard_unexposed_terminal`, and automatic exposed-capacity pruning could
  erase the same state when that live stream retained zero bytes. The published
  `beef1bc` fix applies the same writer-quiescence invariant already used by
  `expire_terminal` to both paths; after `OutputWriter::finish`, ordinary
  hidden retirement/capacity reclamation proceeds.
- `has_nonterminal_jobs()` now treats unfinished retained writers as live
  runtime work because AW uses it to fence parallel-policy changes and Session
  scope replacement. Execution terminality remains a separate factual state;
  this does not alter `Snapshot::is_known_terminal()`.
- Source change is signed/published in
  `beef1bc2d2f37b5660ebe928d368bca37368e608`
  (`Harden Job writer quiescence`). Preserve the existing dirty documentation
  changes in this file and the Job-output TODO; nothing else is staged.
- Final post-fix gate: `aw-tools` **341/341**, adapter **251/251**, Agent core
  **90 passed / 1 ignored**, read-only shared **110/110**, minimal shared build,
  strict affected-crate Clippy, `aw-mcp`, `aw-host`, `aw-cli --lib`, and
  rustfmt all green.
- Dotlink repin qualification on the exact Git dependency is also green:
  all-feature binary **684 passed / 1 ignored**, integrations **36/36**,
  locked all-target/all-feature check, strict Clippy, fmt and diff checks.

## 2026-11-18 B6 Toolset cross-product checkpoint

- Published dotlink `main` is now
  `4f343c3183d427e036a6cb1a9cc42c8a4f2c2cb4`
  (`Complete AW shared-tools convergence`). The current uncommitted B6 slice
  is repinned to published AW
  `7e6fe20a95640e32610819ce94b61a121e165dea`.
- The current dotlink B6 slice consumes one shared immutable `Toolset` for
  canonical filesystem, shell, batch and finite Job MCP descriptors.
  Runtime Job defaults now come from one `DOTLINK_JOB_PROFILE` matching the
  shared contract: tail 512/1024, waits 30 s, list max 256. Toolset does not
  replace dotlink policy/sandbox/replay/transport authority.
- Dotlink deliberately leaves `ReceiptProfile` disabled: no `ts` or
  checkpoint metadata is added until a typed shared receipt can carry factual
  status. Existing free-form MCP strings are not reparsed for presentation.
- B6 review found one shared descriptor gap: `JobsList.limit` had the
  configured maximum but not the runtime-universal `minimum=1`. Published
  AW `7e6fe20` now projects that lower bound while preserving stricter host
  minima; focused Toolset **12/12** and full
  `aw-tools` **341/341** plus strict Clippy/fmt/diff are green.
- Dotlink is repinned to `7e6fe20`; the local lower-bound shim is removed.
  Shell/PowerShell/Batch and finite Job provider metadata use one shared
  descriptor projection after host dynamic limits. Inspection, `list_tools`
  and `get_tool` report the same shared canonical titles while product
  descriptions/schema shapes remain unchanged.
- Current dotlink validation is **685 passed / 1 ignored + 36/36 integrations**,
  all-target/all-feature check, strict Clippy, fmt and diff green.
- B6 is complete for the current product surfaces. Release/version/tag/deploy
  remains separate.
