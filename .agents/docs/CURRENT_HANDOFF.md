# CURRENT HANDOFF — AW live state + DL-200 R6

Canonical comprehensive handoff (**read this first after AGENTS.md**):
[AW-DL200-PHASE3-FULL-COMPREHENSIVE-HANDOFF-2026-11-24.md](AW-DL200-PHASE3-FULL-COMPREHENSIVE-HANDOFF-2026-11-24.md).

Copy-ready next-agent prompt:
[AW-DL200-PHASE3-NEXT-AGENT-2026-11-24.md](AW-DL200-PHASE3-NEXT-AGENT-2026-11-24.md).

Previous Phase-1/R6 comprehensive handoffs remain historical design/implementation provenance:
- [AW-DL200-PHASE1-FULL-COMPREHENSIVE-HANDOFF-2026-11-23.md](AW-DL200-PHASE1-FULL-COMPREHENSIVE-HANDOFF-2026-11-23.md)
- [AW-DL200-R6-FULL-COMPREHENSIVE-HANDOFF-2026-11-22.md](AW-DL200-R6-FULL-COMPREHENSIVE-HANDOFF-2026-11-22.md)

Normative R6 design:
- [README](../plans/dl200-unified-jobs-agent-events-2026-11-20/README.md)
- [SPEC](../plans/dl200-unified-jobs-agent-events-2026-11-20/SPEC.md)
- [IMPLEMENTATION](../plans/dl200-unified-jobs-agent-events-2026-11-20/IMPLEMENTATION.md)
- [TODO](../plans/dl200-unified-jobs-agent-events-2026-11-20/TODO.md)

## Current authoritative snapshot — 2026-11-24

**R6 + the shared Process/release-correction amendment are SEALED and published. Phase 0B is
complete and published. Phase 1 is COMPLETE locally. Phase 2 is COMPLETE locally: v44 Event projection,
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
with deferred post-journal settlement, concrete registered Git materialization resolution
root-free native-only tool registration, live native source approval and explicit
Actor host activation selection are implemented locally.
Materialized/provider audience expansion remains Unknown until its provider-held principal fence exists, but
it no longer blocks Phase-2 completion. Real-Bubblewrap worker-retirement and Windows-native Process
qualification remain explicitly deferred evidence and are not claimed green.**

Verified publication truth:

```text
AW remote main                  78339410710472ec51d5ff46a5c9e586b67ff30d = Own logical Sandboxes
AW R6/Phase-0 publication       20a63a4d4547cc20b2a154787823125b82f5e385
dotlink remote/main             85ceccc1088e201c98c994a46a68834440eeab32
release/0.17                    576d0a1a3fe0a5522a02d51f1d0598c3ddc160c2 = v0.17.9
dotlink AW pin                  20a63a4d4547cc20b2a154787823125b82f5e385
```

The local Phase-1/2/3 implementation line remains unpublished. Phase-1 completion is
`7f4724f942e4d90a6eaaca77af6c528b37a2262d` (`Expose Agent state RPCs`). Phase 2 began at
`14b53872236d2af262d24a2ad9529c4b652c2756` (`Start Event projection foundation`) and closes at
`e0c6bb3184bf87fc1bb2186ddc7f5ffb97a39c96` (`Cut over Event delivery ownership`). Phase 3 starts at
`184ea6b05643c49d34a7f3bfdbfed86f26753162` (`Establish SourceDriver recovery contracts`). The
intermediate private EventScan/source/read/wake/Project/subscription/EventInput/disclosure/custom-publication/
model-tool/Handler/universe checkpoints remain in Git history. The local `origin/main` tracking ref is stale
and is not publication truth.

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

**Next Phase-3 slice:** integrate exact live source-aware admission with the original
Actor/Turn journal and context/queue gates, coordinating the concurrent journal owner
before touching their paths. Keep non-native/Job/Process routes unavailable until
their own containment and authority participation are complete. Independent host
approval provisioning, incomplete-effect reconciliation, multi-source policy, history-only
baseline, custom ACL/task, candidate/retention reconciliation, quiescent binding
mutation, Vault and checkpoint/Sandbox host RPCs remain pending. Store schema v58.
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
