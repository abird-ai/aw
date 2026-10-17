Author: GPT-6 (Codex)

# aw comprehensive agent handoff

Prepared on 2026-10-17 at source checkpoint
`04bb9e000ce0d7406972bf564bb9a0c744fe9698`.

This is the user-requested project-wide onboarding document: original design,
historical implementation sequence, current architecture and contracts, decisions,
verification boundaries, unfinished work, and instructions for continuing. The
retained specifications remain beside it under `.agents/docs/` and
`.agents/plans/`; the final coverage map links every retained non-reserved document
in those sets. Read a linked specification in full before changing its behavior.
This synthesis preserves the recorded context; it is not a verbatim archive of
every conversation or an assertion that missing historical material was recovered.

The live checklist remains [the plan index](../plans/TODO.md) and each selected
plan's `TODO.md`. This handoff supplies context, not a second completion ledger.
If it becomes stale, current instructions, actual source, Git and executed tests
take precedence. No task becomes done merely because it appears here.

Navigation: [Start and ownership](#1-start-here-establish-current-context-and-ownership) ·
[Binding decisions](#2-binding-user-decisions-and-working-rules) ·
[Architecture](#3-current-architecture-the-invariants-that-survive-every-phase) ·
[Original phase sequence](#4-foundation-design-and-the-first-distributed-product-sequence) ·
[Runtime and recovery](#5-runtime-policy-context-and-recovery-the-governing-design) ·
[Portable Agent and UX](#6-portable-agent-integrations-cli-and-ux-decision-history) ·
[Recent implementation](#7-post-acceptance-implementation-native-and-browser-views) ·
[Executed checks](#8-executed-checks-and-precise-completion-boundary) ·
[Pending work](#9-all-current-pending-work-including-deferred-and-external-gates) ·
[Source map](#10-source-map-for-implementation-and-review) ·
[Complete document map](#11-retained-document-map-and-historical-reference-limits) ·
[Next-agent prompt](#12-prompt-to-give-the-next-coordinating-agent).

## 1. Start here: establish current context and ownership

1. Read [the plan index](../plans/TODO.md), the selected plan's `TODO.md`,
   [TODO-SPEC.md](../plans/TODO-SPEC.md), and [working instructions](../AGENTS.md).
   Then read this handoff and that plan's optional `README.md`.
2. Confirm the actual branch, HEAD and owned worktree state. The paths in examples
   assume `/home/pvl/spaces/abird/src/aw`; adapt only the checkout location.

   ```sh
   git rev-parse HEAD
   git symbolic-ref HEAD
   git status --short -- . ':(exclude).agents/plans/ux'
   git worktree list --porcelain
   ```

3. Preserve the separately owned `.agents/plans/ux/` subtree and its staging as
   opaque data. Never list, read, write, stage, reset, stash or commit it, or
   inspect its staged entries. Do not use an unrestricted status/diff/file crawl
   that includes it. An isolated worktree must exclude it before checkout.
4. Inspect the affected live contract and actual source. Do not run commands
   copied from a dated plan until confirming that their flags and scripts exist.
   `aw cli schema` and typed Clap metadata define supported CLI names.
5. Create `.agents/runs/<plan>-<session-agent>/TODO.md` with parent plan, owner,
   provenance, owned paths, work and test results. Each agent edits only its own
   scratch. Agree disjoint source ownership and interfaces before implementation.
6. Use subagents for implementation and independent parallel investigation/review.
   The primary agent coordinates, decides, integrates, gates and commits. Give
   overlapping work an isolated worktree with the reserved subtree excluded;
   otherwise share only explicitly disjoint owned paths. Do useful independent
   work while long checks run instead of waiting on a single task.
7. Reconcile completed results and still-open items into the canonical plan by
   temporary sibling file plus atomic rename, checking the expected Git parent.
   Only the coordinator updates shared plans and their index. Never append to
   shared TODOs concurrently. Remove completed scratch after reconciliation.
8. Run real relevant tests and the product gate when product code changes. Record
   exact commands, commit, results and limitations. A source inspection is not
   runtime execution; compilation is not native-platform certification.

At the captured checkpoint there was one worktree (`main`), no remaining side
worktrees, no owned uncommitted product work, and no retained acceptance owner or
browser process. Synthetic acceptance fixtures and completed scratch were removed.
Those are snapshot facts; verify them again rather than assuming they persist.

## 2. Binding user decisions and working rules

### Design and collaboration

- Review for bugs, regressions and correctness together with simple, robust,
  intuitive, scalable, maintenance-light design. Prefer existing owners and
  composable seams; avoid speculative abstractions and duplicated execution paths.
- Preserve one user-facing `aw`, the original Agent/TurnMachine/ToolRegistry/
  ToolPolicy, immutable TurnJournal, four Runtime primitives and two Hook verbs.
  Views and adapters project that system; they do not create a second engine.
- The user requested [left-right](https://github.com/jonhoo/left-right) wherever
  immutable shared data and measured workloads make it appropriate. This is a
  redesign pass with benchmarks, not a blanket lock replacement. Provider-account
  publication already uses the helper; new candidates are described below.
- Finish native Agent views, then native Files/Git containers, then their browser
  counterparts, then docking. The first three slices are implemented and locally
  checked within the scopes recorded below. Docking is still pending.
- Keep communication concise and frequent during work; explain outcomes and
  remaining uncertainty. Use subagents efficiently, with a coordinator responsible
  for integration and exact verification rather than parallel edits to shared state.

### Git, live state and authority

- Preserve signed history. New commits must be signed; never disable signing or
  follow old prose that suggests an unsigned fallback. Historical hashes in dated
  plans may belong to the pre-rewrite history, not this object database.
- Commit intended reviewed main-worktree changes at sane checkpoints under the
  user's standing instruction. Review exact owned paths first. A commit request
  does not authorize push, force-push, tags, publication, deployment or destructive
  operations; obtain explicit authorization for each such action. This handoff
  grants none of them.
- Preserve opaque staging. Existing checkpoint practice uses a temporary
  `GIT_INDEX_FILE`, `git read-tree EXPECTED_PARENT`, explicit owned-path staging,
  staged diff review, a signed `git commit-tree -S`, signature verification and
  `git update-ref ... NEW EXPECTED_PARENT`. Reconcile only those owned paths in
  the real index. Refuse unexpected owned staging or a changed parent; do not
  reset/stash other work to make a commit fit.
- Do not commit credentials, runtime databases, private transcripts, real grants,
  caches, binaries, fixture output or temporary reports. The existing committed
  `web/dist` is the production frontend embedded by Rust; frontend source changes
  require rebuilding and reviewing those existing assets.
- Do not mutate persistent user/service data, keys, account state, DNS or live
  deployments without the applicable explicit authorization. Synthetic private
  local fixtures are the normal acceptance technique; identify and remove only
  fixtures owned by the current run, after their processes stop.
- Do not delete `owner.lock`, replay raw WAL, bypass quarantine, add an ad hoc
  schema change, silently select a fallback account/executor, start an implicit
  listener, invoke a model or grant a tool as a recovery shortcut.

### Accepted history and the simplified record

The user accepted `47ee8a03abecbb3aee84a49c0a590096d76ed1a7` as the source of
truth for previously verified scope. Old evidence dumps, recursive handoff trees,
manifest chains, `phase.json` and generated validation ledgers were removed from
the rebuilt history. The accepted checkpoint does not complete unchecked native,
provider, release, security or restoration gates. The user then explicitly
requested this comprehensive onboarding document; that request does not reinstate
the former mandatory evidence archive or supersede the simple checklist workflow.

Use one `.agents/plans/TODO.md`, one `plans/<plan>/TODO.md`, an optional plan
`README.md` for design and decisions, and ephemeral agent-owned run TODOs. Group
completed work under the known implementation commit. Keep future work under
`## Uncommitted`; keep an explicit `## Tests and verification`. Preserve failed,
cancelled, blocked and unexecuted items as unfinished. A commit carrying metadata
is discoverable in Git; do not make another commit merely to insert its own hash.
See [the process plan](../plans/handoff-process/TODO.md).

## 3. Current architecture: the invariants that survive every phase

The product is a durable AI workspace implemented by one Rust CLI and engine,
with terminal, Workspace, browser, JSONL/RPC, Rust SDK and embedded/C consumers.
The user model is **Domain → Workspace → Session**, with branches and immutable
history. Machines/Fleets describe targets; accounts, payers, credential custody,
inference execution and tool execution remain separate identities.

| Invariant | Consequence for changes |
| --- | --- |
| Runtime / Actor / Message / Envelope | Scheduling, generation-fenced ownership, addressed communication and immutable authority ceilings compose; do not invent an independent queue/Agent authority. |
| Hook verbs `Next` and `Defer` | Continue/consume or park a bounded owned continuation through the established hooks; arbitrary callbacks do not gain effect authority. |
| Intent before effect | Journal exact model/tool intent before dispatch. Preserve paid results even if later attachment fails. Unknown charge/effect outcomes stay UNKNOWN until inspected; explicitly authorized original-owner durable retry policy is a separate contract, described below. |
| Exact identity and CAS | Store/Session/branch/writer epoch, Actor generation, Process incarnation, source kind/ID/epoch, site and effect remain explicit; display names and paths are not substitutes. |
| Monotonic grants | Children only narrow current authority; prompts, instructions, skills, model text, pairing and published snapshots do not create grants. Revocation is checked at effect/read admission. |
| One State owner | Owner/service holds exclusive `owner.lock`; explicit trusted direct mode has its documented transaction/recovery constraints. Readers reuse owners rather than reopening/recovering State per view. |
| Immutable history | Retry creates a new attempt; no deleting, rewriting, zero-cost relabelling or automatic replay of uncertain prior work. |
| Provider privacy | Secrets remain with SecretStore/configured executor. Provider-private reasoning is private; incomplete traces use the authoritative answer or a bounded error. |
| Feature boundaries | Full, JSONL Minimal, stateless and `no_std + alloc` consumers retain separate dependency/capability sets. A UI/helper dependency must not leak into the kernel. |
| Views are presentation | Opening, moving, refreshing or closing a view does not retarget a running Agent, create a Session, start a PTY or send a prompt. Effects require the existing explicit action and admission. |
| Restored data is inert | Filesystem and SQL quarantine, uncertainty and exact fresh-authority prerequisites remain effective. Review, consent records and dry-run results are not runnable restoration. |

Source package version is **0.15.0**, SQLite schema **v33**, JSONL **v1**;
native saved Workspace layout is **v4**, browser scoped views are **memory-only**.
Versions of CLI schema, wire protocols and ABI are separate; consult their own
contracts before changing them. Do not invent Phase69: numbered history stops at
68, with 64/65 indexing/LSP deliberately excluded and E6 firmware parked.

Current contracts: [Architecture](ARCHITECTURE.md), [Domain model](DOMAIN-MODEL.md),
[Runtime](RUNTIME-FOUR-PRIMITIVES-AND-SERVICES-2026-10-11.md),
[Hook trust/policy](RUNTIME-HOOKS-TRUST-AND-POLICY-2026-10-11.md),
[State](STATE.md), [Database](DATABASE-ARCHITECTURE.md), [SDK](SDK.md),
[CLI](CLI.md), [retry/recovery](LLM-RETRY-RECOVERY.md).

## 4. Foundation design and the first distributed-product sequence

This section reconstructs the retained design trajectory through Phase 50. The
historical implementation scope is accepted at
`47ee8a03abecbb3aee84a49c0a590096d76ed1a7`; that acceptance is the provenance
for older completed work, not evidence that every dated validation paragraph,
platform, provider, release or external-review gate ran. Current Git, source,
executed tests and the [plan index](../plans/TODO.md) outrank a phase document's
old status text. The current package remains **0.15.0**, SQLite is **v33**, and
portable JSONL is **v1**. Historical schema numbers below explain the sequence;
they are not instructions to downgrade or create a migration.

The retained top-level phase documents start with
[Phase 2 execution evidence](../plans/PHASE-2-VALIDATION.md) and then
[Phase 11](../plans/PHASE11-HARDENING.md). There are no standalone retained
Phase 0, 1, 3–10 or 29 documents in that set. Do not invent their contents.
Earlier design is recoverable from the live architecture/runtime/SDK contracts,
source, the local-owner/team/context protocols, the database/Fleet amendments,
and the accepted-history checklist. The
[Phases 19–23 continuation record](../plans/PHASE19-23-CONTINUATION-STATUS.md)
also preserves later Phase 24–35 updates, including the Phase-29 semantic
Workspace handoff story. Its old branch/worktree descriptions, “DO NOT RESET”
list, absent helper names, `phase.json` instructions and proposed next steps are
historical context, not directions to act on today's worktree.

The old multi-megabyte comprehensive handoffs and recursive evidence/manifest
chains were deliberately removed after the user accepted the checkpoint. This
new comprehensive onboarding document is explicitly requested; it does not
reinstate an obligation to reconstruct archives, regenerate a proof chain,
restore old scripts or read obsolete handoffs before normal work. The working
instructions and accepted-history checklist refer to an off-repository
pre-rewrite bundle, but the current plan index gives no concrete bundle path. A
receiving agent must not claim that a particular bundle, old hash, tag or removed
helper is locally available without checking.

### The original design choices that still explain the code

Aw is one user-facing Rust binary and one Agent implementation. Presentations,
local attachment, browser controllers, devices, remote MachineLink services,
pipelines and child Sessions all reuse the same Agent/journal/policy machinery.
The later Runtime/Actor/Message/Envelope vocabulary does not authorize a second
Agent loop or a second canonical conversation store.

The user model is **Domain → Workspace → Session**. A Domain owns policy and
administration; a Group groups principals. A stable opaque User is distinct
from Unix/internal/OIDC bindings, Machines, inference executors and provider
accounts. A Workspace owns durable work and policy; it need not be a single
directory. Every Session has exactly one Workspace. The reserved `local`
HostedWorkspace makes the simple local path work without prior topology setup.
A Session captures its own execution binding and history; selecting another
Machine, Git worktree, Files root, pane or tab does not migrate or retarget it.
See [Domain model](DOMAIN-MODEL.md).

HostedWorkspace origins (`BuiltinLocal`, `UserCreated`, `ToolCreated`,
`PeerCreated`) are metadata within one kind, not four competing session systems.
Remote delegation resolves a durable PeerWorkspace from stable source
store/Domain/Workspace identity, rather than a temporary connection. The remote
Session and transcript remain canonical there; the source keeps a
RemoteSessionRef. A Fleet is a named placement group, not credentials,
authorization, a filesystem merge or a database replica set. A FleetWorkspace
is virtual and remains authoritative on its designated Workspace authority.
Member-specific bindings may have different paths; missing bindings remain
Unbound. Every operation pins the membership/binding revision it actually uses.

The [Fleet amendment](FLEETS.md) initially described a “main host” and machine
workspaces. Its appended workspace-first refinement and [AwNet](AWNET.md)
replace any permanent-master interpretation with independent AwNodes and
Domain authorities. `On: Machine-or-Fleet` is a view/execution lens: follower
views can change, pinned resources stay pinned, and admitted Session bindings
stay fixed. Rich Fleet aggregate Files/Git/logs/metrics, preview bridging,
broadcast terminals and transparent local-like remote views are a design
surface, not a consequence of merely having Fleet tables and fan-out receipts.
The [Fleet adaptation matrix](FLEET-ADAPTATION-MATRIX.md) is a Phase-12 gap
analysis, not today's completion checklist.

AwNet answers identity and reachability; Domain/Workspace policy answers what
may be done. Pairing, a route, a MachineLink lease, Domain membership,
Session access, provider selection and tool grants are separate. Effective
authority is their intersection. No downstream feature can widen an upstream
grant. A rendezvous/relay operator provides connectivity and cannot become a
Domain policy authority. Managed and self-hosted connectivity use the same open
protocol and the same `aw` role implementation; deployment and operations are
the hosted distinction.

The storage design is **immutable facts + small mutable projections + explicit
effect coordination**. Conversation ancestry has one parent; cheap forks add
refs instead of copying transcripts. Branch head/revision CAS, Workspace writer
epochs, Domain quorum terms and local process/store generations solve different
problems. SQL writer serialization does not fence a filesystem/provider effect;
a view revision does not grant execution; a local lock cannot fence restored
clones globally. Semantic commit events, immutable content and explicit
snapshots are replication units. SQLite pages and raw WAL are never the aw sync
protocol. See [database architecture](DATABASE-ARCHITECTURE.md),
[baseline adaptation matrix](DATABASE-ADAPTATION-MATRIX.md),
[refinement v2.1](DATABASE-DESIGN-REFINEMENT-V2.1.md) and
[retry refinement v2.2](DATABASE-DESIGN-REFINEMENT-V2.2.md).

Credential custody is equally deliberate. Actor, account owner, payer,
credential custody, inference executor and tool execution location are distinct.
Shared/server provider secrets and OAuth/OIDC client/token material belong in
SecretStore behind opaque versioned refs. Personal credentials remain with their
configured device/native executor. A missing, offline, revoked or unavailable
account fails visibly; it never silently falls back to another account or
vendor. Browser same-origin storage is not equivalent isolation to a native
worker. Transcript access, Fleet membership and participant login do not grant
spending or tool authority.

### Early local, browser and team boundary

[Local owner](LOCAL-OWNER.md) and [local protocol](LOCAL-PROTOCOL.md) preserve
the Phase-7/8 rationale: one explicitly launched owner holds the private store,
account registry and Agent; native attach and an optional browser are
presentations. Local attach authenticates through the platform's private socket
and verified peer identity, not a JSON actor field. Native attach loads no
independent provider configuration. The version-1 length-prefixed protocol
binds every request after Hello to the exact owner instance. Transport IDs order
frames; semantic request IDs deduplicate admitted prompts.

The original local owner shares an active execution view. Its `inspect` path is
read-only and cannot install history into the Agent. Open/fork/checkpoint/reset
use expected revisions. `prompt_status` is lookup-only and resolves the exact
request/final-node link; a missing receipt is not proof that another owner or
store never executed an effect. Dropping an ordinary in-flight request poisons
that transport rather than letting the next command consume a stale reply.
Reconnect and status lookup never resubmit. Socket disconnect/browser close
leaves accepted owner work alive; cancel and owner shutdown are distinct.
Browser logout on the shared owner clears that surface without cancelling the
common native Agent. The loopback browser pairing code grants the owner role;
it is not authentication as an independent teammate.

[Team identity](TEAM-IDENTITY.md) and [team protocol](TEAM-PROTOCOL.md) explain
the Phase-9 boundary: an explicitly launched numeric-loopback invite gateway
constructs one ordinary Agent/controller per configured participant. A random
one-use invitation selects the participant; names, headers and prompt fields
do not. Browser tabs sharing a cookie share one identity/controller. The trusted
operator can see configuration, invitations and stored history. This is not
owner-blind encryption or public multi-tenant certification. Tool and human
Files/Git/PTY surfaces are disabled for this team gateway; another mode's grants
must not leak into it.

The initial wrapper allowed owned-private Sessions only. Phase 10 added a
narrow explicit source-read/release exception for known same-realm participants,
including exact recipient account allowlists; source read still did not grant
destination append or provider disclosure. Release epochs and derived-data
lineage preserve revocation through summaries, imports, checkpoints and later
outputs. Phase 38 later added the explicit shared-draft opener/submit path;
ordinary peer-owned prompt/fork/checkpoint/reset paths remain fenced. The
Phase-9 prose saying OIDC/collaboration is future work is historically scoped;
Phase 30–32 and 38 supply later bounded capabilities without converting this
loopback gateway into a public TLS service.

### Phase chronology: durable effects, networking and distributed authority

| Milestone | Implemented capability and rationale | Boundary at that milestone |
|---|---|---|
| [2](../plans/PHASE-2-VALIDATION.md) | Early static/reference/browser-fake-transport checks explicitly separated authored Rust tests from execution. | No Rust/Cargo/Nix/real CLI ran in that authoring environment. Those historical counters cannot prove current runtime behavior. |
| [11](../plans/PHASE11-HARDENING.md) | Sticky stream rejection survives adapters ignoring sink errors; authorization rechecks after persisted intent; summaries validate purpose/identity/evidence and recheck after hooks; one extension-name/count validator. | Trusted native providers/journals remain cooperative; revocation cannot atomically recall an external effect. Old “not compiled here” text is authoring provenance, not a current compiler finding. |
| [12](../plans/PHASE12-DURABLE-SESSIONS.md), schema 3 | Logical LLM operations own immutable attempts, original head/revision, retry schedules/claims and recovery hints. Commit/ref events become durable. Completed work survives attachment conflict. | Unknown possibly charged attempts remain visible; interactive retry is explicit and current-context retry is a new operation. Autonomous claim primitives do not imply a background runner. |
| [13](../plans/PHASE13-DOMAINS-WORKSPACES.md), schema 4 | Durable Domain/Workspace/Machine/Fleet vocabulary, reserved local Workspace, Session execution targets, semantic resource events and canonical noun→verb CLI foundations. | Initial remote targets deliberately refused local execution; topology metadata was not remote capability. Later phases supplied transport. Historical realm strings remained provenance/compatibility. |
| [14](../plans/PHASE14-AWNODE-QUIC-PAIRING.md) | Stable Ed25519 AwNode identity; Node ID hashes public key; private identity file; one-use short-lived invitation; exact certificate-pinned QUIC; signed pairing transcript; durable peer trust. | Missing private identity after binding fails closed; no silent rekey or re-trust of a revoked peer. Pairing alone exposes no Workspace or execution. Later native checkpoint supersedes its retained pre-native acceptance appendix. |
| [15](../plans/PHASE15-DIRECT-REACHABILITY.md) | Host/STUN server-reflexive candidates, signed rendezvous, same-socket UDP hole punch and authenticated direct QUIC. | Routes are ephemeral; helper has no policy authority. Failure changes no Session target and replays no effect. |
| [16](../plans/PHASE16-ROUTE-FALLBACKS.md) | Ordered direct QUIC → QUIC/UDP relay → TLS/TCP relay with bounded attempt traces; opaque end-to-end forwarding. | Connection-scoped route selection is not replay permission. The reusable UDP service came later; the bounded one-pair TCP fallback is not a general multi-room TCP service. |
| [17](../plans/PHASE17-MACHINELINK.md) | Signed typed MachineLink hello/accept, exact requested/server-ceiling intersection and short capability leases. | A named enum capability is not proof a service is implemented. Generic shell/execute/tunnel capability names remain outside this contract. |
| [18](../plans/PHASE18-REMOTE-READ-SERVICES.md) | Capability-specific Machine metadata, Workspace discovery, bounded scoped file listing/read, hardened Git read and redacted Session summaries. | Serving process fixes root/Git executable locally. No caller path/command authority; PTY was an inactive name, not an implemented remote PTY service. |
| [19](../plans/PHASE19-PEER-REMOTE-SESSIONS.md), schema 5 | Stable PeerWorkspace and RemoteSessionRef; source operation ID/request digest; unique remote origin; pending/unknown/active response-loss semantics. | Remote canonical Session is created once; source has a reference. Creation alone did not configure inference or create generic execution. |
| [20](../plans/PHASE20-FLEET-FANOUT.md), schema 6 | Exact Fleet membership snapshot and immutable per-member operation records; deterministic placement candidates and aggregate status. | Membership change cannot alter an existing operation. Unknown members stay unknown; planning/records were distinct from actual dispatch. |
| [21](../plans/PHASE21-RELAY-SERVICE.md) | Reusable bounded multi-room UDP relay with TTL, room/packet admission and statistics. | Opaque forwarding creates no trust, Domain capabilities or execution authority. |
| [22](../plans/PHASE22-FEDERATION-LEASES.md), schema 7 | Directional durable federation; explicit resource/actions; short leases bind federation/Domain/peer policy/security epochs; revocation invalidates future use. | Pairing is insufficient; descendants/re-delegation default off; current peer, scope and action are still checked. |
| [23](../plans/PHASE23-SEMANTIC-SYNC.md), schema 8 | Bounded append-only semantic inbox/cursors, per-event digest/ID binding, authorized sequence gaps and idempotent retry. | Imported events are evidence and cannot mutate local Sessions/refs/providers/tools. No raw WAL or generic row replication. |
| [24](../plans/PHASE24-REMOTE-ORCHESTRATION.md), schema 9 | Persistent typed remote prompt/status/transcript/cancel and pinned Fleet dispatch; per-member stable operations and durable uncertainty. | Provider secrets remain at executor; route loss/process death cannot prove work stopped. FleetWorkspace is not a merged filesystem or distributed transaction. |
| [25](../plans/PHASE25-SEMANTIC-MATERIALIZATION.md), schema 10 | Owner/admin exact selector policy and separate explicit materialize action produce immutable local remote-fact resources from already observed inbox evidence. | No wildcard/automatic consumer, no replay of the remote mutation and no implicit model context. Revocation stops new projections while historical accepted facts remain inspectable. |
| [26](../plans/PHASE26-DOMAIN-AUTHORITY-HA.md), schema 11 | AuthoritySet strict-majority quorum, durable signed votes/terms and bounded leader lease fence authority-changing writes; q1 stays zero-coordination. | Already-admitted bounded leases survive leader-lease loss until their own expiry. Pairing does not opt a voter in. This alone was not standby state replication. |
| [27](../plans/PHASE27-AUTHORITY-FAILOVER-COORDINATION.md), schema 12 | Signed minimal authority-core snapshots, explicit standby import, fresh majority promotion after old source lease expiry; open multi-room rendezvous role. | Safe Users/memberships/AuthoritySet only, no auth bindings/secrets/Workspaces/Sessions/Fleets. Promotion refuses generic merge into an existing Domain. Coordinator remains reachability-only. |
| [28](../plans/PHASE28-WORKSPACE-WRITER-EPOCHS.md), schema 13 | One current Workspace mutable-ref writer, monotonic epoch/authority term, exact expected-epoch failover, immutable writer history and ref provenance. | Old writer is fenced even if cached head/revision matches; admitted paid result remains durable but cannot attach under a changed epoch. Remote designation alone did not transfer Workspace state. |
| 29, schema 14; preserved in [continuation](../plans/PHASE19-23-CONTINUATION-STATUS.md) and [fresh review](../plans/PHASE30-FRESH-REVIEW.md) | Bounded signed semantic Workspace handoff of immutable Sessions/context/nodes/refs and writer history; typed handoff/ref services; old-writer failback fence; actual ref authoring epoch preserved. Compatible descendants fast-forward; real divergence becomes an explicit conflict branch. | No raw database, auth secret, execution-target rebinding, running operation or unknown effect import. Cross-host login equality was left to Phase 30. |

The route ladder and the application carriage have intentionally different
failure semantics. An exact remote create may resolve the same immutable
operation on retry, while an uncertain provider/tool execution is not replayed.
Status, transcript read, vote, synchronization and materialization have separate
typed authorization. A source control peer is not an arbitrary user impersonator.

### Identity, context and local database hardening

[Phase 30](../plans/PHASE30-IDENTITY-CONTINUITY.md), schema 15, made opaque
`user_id` the cross-host authorization identity. Original Unix/internal author
labels remain immutable provenance. Unknown `user:usr_...` strings cannot create
users. Device private seeds stay in private files; continuity snapshots carry
safe Users, active public keys, issuer+subject metadata and security epochs only.
Target-side source trust is explicit and revocable. Identity metadata sync and
live device authentication have separate capabilities. Device proof binds lease,
request, Domain, target store/node, peer, fresh nonce and optional Session;
metadata import itself is not login. Portable OIDC metadata is not a locally
verified OIDC login.

The [Phase-30 fresh review](../plans/PHASE30-FRESH-REVIEW.md) matters because it
found defects missed by feature tests and tightened the existing architecture:

- Revoking continuity trust also revokes portable bindings; re-accept grants
  trust only, and a fresh signed sync must restore credentials.
- Stable projected ownership is authoritative. Historical principal text is
  only a fallback for genuinely legacy null projections; it cannot bypass a
  suspended stable User or impersonate a remote artifact owner.
- The real v14→v15 migration temporarily adjusts affected immutable guards
  inside its transaction for backfill, then restores them. Narrow null→verified
  stable identity enrichment does not permit historical-field mutation.
- Semantic snapshot hashes exclude volatile export time. Portable row IDs are
  Domain-scoped; credential identity and local projection-row identity differ.
- Import requires the exact accepting user or Domain owner/admin; an unrelated
  continuity grant is insufficient. Read paths do not bootstrap identities.
- Writer-ref wire v2 carries stable `actor_user_id`, not free-form Unix labels.
  Opened private key descriptors are validated after no-follow open; failed key
  registration cleans up the newly created file under its ordinary failure path.
- Status reports schema epoch and SQLite user_version separately; typed CLI
  work later replaced the review's temporary argv/help repair.

[Phase 31](../plans/PHASE31-AUTH-SECRET-STORE.md), schema 16, implemented bounded
rotating native auth families and SecretStore/shared provider custody. Random
refresh bearers are hash-only in SQLite and private-file-only in CLI transport;
rotation keeps a fixed absolute family expiry and reuse revokes the whole
family. Stable-user epochs and exact portable-binding/source/Domain checks fence
derived sessions. Dormant old sessions and legacy server-secret refs are not
promoted into working authority by migration.

`SecretStore::{put,get,delete,exists}` is a narrow backend seam. The local backend
uses private directories/files, authenticated encryption, ref-bound associated
data, immutable version blobs, fsync publication and fail-closed missing master
key behavior. Shared accounts pin provider, endpoint, executor, payer, models,
grants and credential version. SQLite transactions are short; encryption/fsync
happens outside them and metadata activation/rotation uses revalidation/CAS.
Selection and each outbound dispatch revalidate current authority. Endpoint
changes cannot redirect a legitimately selected shared bearer. A long-lived
selected old credential version does not silently become the new version after
rotation. OS keychain/KMS/HSM/managed adapters, passkey/password ceremonies and
a general browser cookie middleware were not delivered merely by this seam.

[Phase 32](../plans/PHASE32-OIDC-OAUTH.md), schema 17, added real authorization
code + PKCE/state/nonce login/link, issuer discovery/JWKS and bounded local
RS256/ES256 verification. Exact verified issuer+subject is the identity key;
manual metadata requires explicit link before it can become login authority.
OAuth connectors are user-owned delegated API accounts, separate from inference
provider accounts. Client secrets and access/refresh token sets remain versioned
SecretStore material; ordinary SQLite carries safe refs/scopes/expiry only.
Authorization cannot grant unrequested scopes, refresh cannot widen granted
scopes, and code exchange is not automatically replayed. OIDC uses the same
native family lifecycle. Fixture success is not proof of a live third-party
issuer or provider billing contract.

[Phase 33](../plans/PHASE33-MATERIALIZED-CONTEXT.md), schema 18, closed the
separate **evidence → materialized fact → candidate artifact → target apply →
model use** boundary. Deterministic projection selects 1–32 exact fact IDs,
validates source and exact target head/revision, and creates a candidate without
model/tool/network effects or branch mutation. Apply is another explicit
operation with target authorization, disclosure and writer/CAS checks. The
artifact is User-role untrusted data, never System/Tool authority.

Original fact provenance and current context-use authorization are distinct.
Artifacts pin the exact authorization policy ID/revision. Revoking policy A and
creating policy B cannot revive artifact A. Fact provenance is inherited through
descendant nodes, so checkpointing or compacting visible text cannot evade the
next model-admission revocation check. Workspace semantic snapshot v3 preserves
that provenance without importing federation/materialization authority. A target
can display historical content yet refuse new inference until independently
authorized. Portable archive import remains static/imported-unverified history,
not a portable live grant.

[Phase 34](../plans/PHASE34-SQLITE-COORDINATION.md), schema 19, implemented
application coordination before exposing direct SQLite clients. Durable process
incarnations bind permanent Store ID and restore generation. Write leases may
be released after fencing; stale execution leases become hazards. Real provider
intent acquires its exact execution lease in the same transaction as
`InvocationStarted`; only then may provider code run. Startup makes unknown
attempt/retry/no-replay state durable before releasing a process-level provider
hazard. Lease release is not retry approval.

This phase also corrected a critical crash boundary: “persist result before
attachment” was previously two statements in one SQL transaction. It became
**Transaction A commits final paid result/attempt/operation; Transaction B
performs writer-epoch + branch CAS attachment**. Crash between them reconciles
the existing result without inference. A conflict keeps output and accounting,
clears the stale active-run marker, and offers deliberate fork/compare/summary.
Bounded Busy/Locked retries concern database acquisition only, never repeated
provider/tool effects. Maintenance fences ordinary store APIs as well as
cooperative lease callers. Same-store restore stages a strictly newer restore
generation and stale-process fencing before restored bytes can become live.
These internal local backup/restore proofs must not be mistaken for activation
of an arbitrary restored State archive; current cold-State quarantine remains.

[Phase 35](../plans/PHASE35-TRUSTED-LOCAL-DIRECT.md), schema 20, exposed the
explicit trusted-local direct Rust API after review found that global startup
recovery would interrupt another live direct process's runs/context/web work.
Ownership of those live state machines became process/store-generation-aware.
Ordinary owner/service mode keeps **exclusive `owner.lock`**; explicit direct
clients hold it **shared**. The modes cannot coexist. Exclusive reacquisition
fences dead prior holders without PID guesses or waiting for an old heartbeat.
Direct mode requires an existing exact-current-schema private store on a
supported local Linux filesystem, never creates/migrates/restores, and uses a
bounded heartbeat. At today's source that exact schema is **33**, not the
historical 20. A conservative `store:ordinary-access` lease serializes ordinary
persistent APIs and closes maintenance check/transaction races. Finer reader or
resource scopes are an optimization requiring proof, not a reason to weaken the
barrier. Network/FUSE/clustered/unknown filesystems and remote SQL remain outside
this supported mode.

### Product shell, collaboration and bounded orchestration

[Phase 36](../plans/PHASE36-CLI-ARCHITECTURE.md) made one typed Clap tree the
public command/help source instead of manual process-argv rewrites and nested
mini-parsers. Purpose-specific argument groups still produce the one runtime
RunArgs. Help construction is side-effect free: it opens no store, listener,
provider, filesystem surface or model.

[Phase 37 onboarding](../plans/PHASE37-FIRST-RUN-ONBOARDING.md) supplied a
cancellable native provider/model flow before a model-bound Session exists,
using the same `ProfileUpdate/apply_profile_update` authority as native and web
Settings. Skip preserves the composer/draft; a later Send can reopen setup.
Configuration Save is network-free; explicit bounded model discovery is a
separate action. Provider/model switching clears the active Agent/Session and
unrelated account/retry state so old context is not silently sent elsewhere.
The original phase deliberately excluded subscription tokens from the Chat
Completions adapter.

The later [unnumbered design adaptation](../plans/PHASE37-DESIGN-ADAPTATION.md)
added the dedicated Responses + supported ChatGPT OAuth path and Solid browser
root while retaining that credential-route separation. Host-side SecretStore
owns OAuth tokens; browser receives setup/status/catalog metadata. Responses
uses `store:false`; provider-private continuation is bounded to one in-flight
tool loop, stays non-renderable and is not written as durable reasoning. The
record describes dynamic registration, PKCE/state/nonce/JWKS, account-serialized
refresh and explicit revoke/disconnect handling. Its automated proof did not
include a real-human account login, richer returning-account reauthorization or
all failed-first-exchange recovery cases. Current live-provider/account refinement
must be judged from source and the live plans, not this dated claim.

The [Phase-37 closeout](../plans/PHASE37-CLOSEOUT-AUDIT-AND-RELEASE.md) records
several unnumbered foundations: transport-neutral bounded UI-extension
contributions; backend-neutral Session API/capability declarations; real
single-owner JSONL persistence; conservative aw/Pi interoperability; platform
facades; and reproducible five-target release design. SQLite is the rich
authority backend, and JSONL is an actual backend with fsynced intent/restart
inspection. JSONL Minimal and stateless/memory-only are separate feature cuts.
Pi input never promotes
foreign system/tool/extension text to aw authority. Postgres/LevelDB remain
future kinds, not implemented backends. A future external UI-extension host
process is distinct from the compiled extension protocol already present.
Cross-build, native execution, signing/notarization, publication and independent
security certification are separate evidence categories. Old closeout workflow
and validator references may have been superseded; current release readiness
and CI source own those gates.

| Milestone | Bounded capability | Authority/recovery rule |
|---|---|---|
| [38](../plans/PHASE38-SHARED-DRAFTS.md), schema 21 | Optional Yrs/Yjs-compatible shared unsent prompt, explicit Off/Auto/On policy, separate draft grants, durable accepted text revisions, exact sealing and one submission identity. | Private draft publication is explicit; typing/presence causes no model call. Presence is ephemeral and not activity metrics. Submitter uses own account; duplicate Send is lookup, never a second run. Audience changes abandon old generation. Executed HTTP cookie-jar smoke was not a rendered two-browser convergence/visual test. |
| [39](../plans/PHASE39-BOUNDED-PIPELINES.md), schema 22 | Immutable linear manual pipelines with bounded deterministic check/transform, explicit approval, ordinary Agent step and emit; durable exact request/step receipts. | Definitions are data, not URLs/secrets/shell/grants. No transaction spans inference or approval. Unknown started external step never resumes by replay. DAGs, automatic triggers and schedules were not part of this first slice. |
| Unnumbered compatibility repair, schema 23 | Owner migration introspects and repairs missing Phase-35 ownership columns/indexes on a legacy schema-22 shape, with integrity/FK checks. | Correct existing stores do not receive duplicate-column failure. Direct clients still cannot migrate. Bounded sanitized SQLite error details aid diagnosis without authorizing replay. |
| [40](../plans/PHASE40-ROUTING-AFFINITY.md), schema 24 | Immutable Fixed/Route once/Route each prompt strategy, secret-free destination-pinned candidates, strict tool-free classifier and per-user Session/branch/role/lineage affinity. | Candidate is a proposal, not a grant. Current ProviderAccounts selection revalidates; no classifier/target fallback. Classifier intent/usage is separate from Chat. One consumed selected route binds exact working admission; same-context retry preserves the original route rather than rerouting. |
| [41](../plans/PHASE41-SUBAGENT-SESSIONS.md), schema 25 | Explicit native-owner opt-in (historical `--subagents`; current canonical `--subagent-enabled`) creates ordinary durable same-Workspace child Sessions, foreground/background lifecycle and family navigation. | Model arguments cannot select authority. Initially exact parent account/model and model-only child; no automatic parent transcript/UI disclosure. Tightly fenced synthetic same-root model-only key is not a tool-effect bypass. Restart interrupts relation and retains journal uncertainty. |
| [42](../plans/PHASE42-ISOLATED-SUBAGENT-WORKTREES.md), schema 26 | Host-granted off/read-only/workspace child file tools in verified detached managed Git worktrees, pinned clean committed HEAD, distinct physical effect roots. | Allocation/create/cleanup has intent before Git entry; ambiguous effect becomes unknown. Dirty/non-Git parent refuses. No bash, model-selected paths/refs/tools, automatic merge/import or cleanup. Worktrees remain for explicit human inspection. |
| [43](../plans/PHASE43-DELEGATION-BUDGETS.md), schema 27 | Exact-root-turn durable envelope bounds created allocations, active children, model attempts, deadline and initial depth 1. | Failed provisioning consumes created slot; model reservation precedes ordinary provider intent. Ordinary result commits before budget settlement. Unknown attempts remain consumed; known tokens plus incomplete usage are not verified money. |
| [44](../plans/PHASE44-RECURSIVE-SUBAGENTS.md), schema 28 | Explicit depth 1–4 recursion and background sibling fan-out reuse one root actor, deadline and counters. Additive recursive columns preserve legacy constrained depth evidence. | Descendants cannot reset budgets. Max-depth child lacks tool. Equal/narrower tools and sibling worktrees require clean direct-parent committed state. Foreground tool calls remain sequential; no claim of parallel foreground execution. |
| [45](../plans/PHASE45-CHILD-ROUTING.md), schema 29 | Host-pinned actor-owned worker routing revision per root, reused Phase-40 router, one consumed selected identity per child allocation. | Classifier consumes shared root model budget. Selection binds before worktree/child dispatch. Route-once is direct-parent-local; recursive descendants cannot change root strategy. Unavailable selection fails without reroute. |
| [46](../plans/PHASE46-MULTI-MODEL-TEAMS.md), schema 30 | Immutable 2–4-member team template, ordinary sequential foreground children and deterministic all-required join. | Full answers remain only in child Sessions; join tables carry IDs/digests/truncation. Failed member blocks join; no member replay. Root parent alone synthesizes/publishes in the existing Agent loop. |
| [47](../plans/PHASE47-INTERACTION-POLISH.md) | Native palette categories/prefixes, exact rendered hit rectangles, narrow folding, scroll indicators, cell-based read-only selection and explicit bounded OSC52 copy. | Presentation/input only, schema 30 unchanged; no incoming model/file OSC execution. Clipboard availability depends on terminal; selection is viewport-bounded. |
| [48](../plans/PHASE48-AGENT-REACTIVATION.md) | Transactional native saved-Agent reactivation uses its saved root/instructions/model/destination; rollback restores prior usable Host. Muted placeholder stops double-DIM rendering. | Ordinary caller-current binding transfer guard stays; unresolved effects block another Session; missing/changed destination fails. No inference during selection or silent root migration. |
| [49](../plans/PHASE49-GENERATIVE-PLANNER.md), schema 31 | Root-only tool-free fixed planner proposes 1–6 strict typed role-allowlisted acyclic tasks; deterministic sequential topological execution through ordinary children. | Planner cannot author executable pipeline/grants. Shared model/deadline budget includes planner without fake child. Dependencies initially order only; predecessor answer disclosure was deliberately deferred. Child facts verified before ID/digest-only join; parent publishes. |
| [50](../plans/PHASE50-GUARDIAN-REVIEW.md), schema 32 | Root-host fixed tool-free guardian over exact team/planner joins; strict pass/deny/needs_human/require_evidence; durable owner approval and separate immutable release receipt. | Joined evidence stays withheld until gate passes. Review pass is no capability grant. Release reconstructs authoritative child prefixes/digest and requires original parent InvocationIdentity; later explicit release needs no new inference. Unknown review never replays; waiting-human state survives. |

### Current source seams for continuing this history

These are current files, not promised archive helpers. Read the affected module
and its tests before changing a protocol or effect boundary.

| Concern | Source seam |
|---|---|
| Original Agent, hooks, purpose-specific tool-free evaluators | [Agent](../../crates/aw-agent-core/src/agent.rs), [journal trait](../../crates/aw-agent-core/src/journal.rs), [context/purpose](../../crates/aw-agent-core/src/context.rs), [routing](../../crates/aw-agent-core/src/routing.rs), [planner](../../crates/aw-agent-core/src/planner.rs), [guardian reviewer](../../crates/aw-agent-core/src/reviewer.rs). These evaluators add bounded purposes; they do not replace Chat. |
| Full/minimal runtime adapters and typed command definition | [full Host](../../src/main_full.rs), [minimal Host](../../src/main_minimal.rs), [resource CLI](../../src/resources_cli.rs). |
| Durable store open, migrations, current schema and cold refusal | [store root](../../crates/aw-store/src/lib.rs). `open` takes exclusive owner.lock, `open_trusted_local_direct` takes shared lock and validates exact schema; both reject unapproved cold restore before normal operation. |
| Immutable intent/result and attachment | [SQLite journal](../../crates/aw-store/src/journal.rs). `finish_sync` commits `persist_finish_in`, then separately calls `attach_settled_run`; receipts remain authoritative over presentation Turn metadata. |
| Domain/Fleet/federation/materialization and semantic events | [resources](../../crates/aw-store/src/resources.rs), [durability](../../crates/aw-store/src/durability.rs), [authority](../../crates/aw-store/src/authority.rs), [authority replica](../../crates/aw-store/src/authority_replica.rs), [Workspace writer](../../crates/aw-store/src/workspace_writer.rs), [Workspace replica](../../crates/aw-store/src/workspace_replica.rs). |
| Local process/lease/maintenance authority | [coordination](../../crates/aw-store/src/coordination.rs); ownership locks remain separate from logical writer/quorum fences. |
| Identity/auth/credential lifecycle | [identity continuity](../../crates/aw-store/src/identity_continuity.rs), [auth session](../../crates/aw-store/src/auth_session.rs), [provider credentials](../../crates/aw-store/src/provider_credentials.rs), [SecretStore](../../crates/aw-store/src/secret_store.rs), [OAuth connectors](../../crates/aw-store/src/oauth_connector.rs), [OIDC flow](../../src/oidc_flow.rs). |
| Context projection/apply | [materialized context](../../crates/aw-store/src/context/materialized.rs), [apply](../../crates/aw-store/src/context/apply.rs), [operations](../../crates/aw-store/src/context/operations.rs). Source authority and target disclosure/CAS remain separate. |
| AwNet capability/carriage layers | [MachineLink](../../src/awnet/machinelink.rs), [reachability](../../src/awnet/reachability.rs), [route ladder](../../src/awnet/routes.rs), [read services](../../src/awnet/services.rs), [remote Sessions](../../src/awnet/remote_sessions.rs), [remote control](../../src/awnet/remote_control.rs), [Fleet dispatch](../../src/awnet/fleet_dispatch.rs), [federation](../../src/awnet/federation.rs), [authority replica](../../src/awnet/authority_replica.rs), [Workspace replica](../../src/awnet/workspace_replica.rs), [identity continuity](../../src/awnet/identity_continuity.rs). |
| Local client and authenticated team | [typed local protocol](../../crates/aw-client/src/protocol.rs), [framing](../../crates/aw-client/src/framing.rs), [team gateway](../../src/web/team.rs), [team router tests](../../src/web/team/tests.rs). |
| Shared drafts | [collaboration kernel](../../crates/aw-collab/src/lib.rs), [durable draft state](../../crates/aw-store/src/multiplayer.rs), [authenticated draft routes](../../src/web/team/multiplayer.rs). |
| Pipelines/routing/delegation/team/planner/review | [pipeline vocabulary](../../crates/aw-pipeline/src/lib.rs), [routing vocabulary](../../crates/aw-routing/src/lib.rs), [store pipelines](../../crates/aw-store/src/pipelines.rs), [store routing](../../crates/aw-store/src/routing.rs), [store subagents](../../crates/aw-store/src/subagents.rs), [native subagent service](../../src/subagents.rs), [teams](../../src/subagent_teams.rs), [planner adapter](../../src/subagent_planner.rs), [review adapter](../../src/subagent_reviews.rs). |
| Provider setup authority | [provider config](../../crates/aw-providers/src/config.rs), [native onboarding](../../src/tui/onboarding.rs); all editors share ProfileUpdate/apply_profile_update. |

### How to interpret the remaining design and verification concerns

The DB-A…DB-I roadmap is a dependency map, not nine unfinished current tickets.
The sequence above delivered substantial DB-B/H semantic durability and sync,
DB-C stable identity, DB-D/E credential/auth, and DB-F/G local coordination/direct
access. It did not require splitting one atomic SQLite transaction into many
physical stores. DB-I broker/large SQL/object/KMS hosting remains optional and
measurement-driven. A broker, if introduced, projects commit_log; it cannot
become a second source of truth. Short pinned reader snapshots, refined read
concurrency, feed retention/snapshot compaction, resumable rolling migration
capability machinery, broad retention/tombstone/redaction administration,
passkey/password/recovery ceremonies and independent audit services are broader
design directions. The historical matrices do not prove that each has a current
user-facing implementation, nor do their unchecked design paragraphs establish
a new live defect. Make a concrete scoped plan before implementing one.

The following concern families must survive reconciliation with later phase and
current-source coverage; some were intentionally deferred at a particular
milestone and later gained bounded implementations. They must not be silently
labelled either complete parity or current bugs solely from these old records:

- **Effects/accounting:** provider-specific read-only status/usage reconciliation;
  unknown charges versus known usage; interrupted Tool/OS process outcomes;
  exact-source retry authority; paid-result retention after any attachment or
  accounting failure. Token evidence is not invoice verification, a currency
  budget, or proof that an interrupted process stopped.
- **Storage/restore:** raw WAL/multi-master remains prohibited; direct mode stays
  explicit/local/current-schema and owner restore stays separate. Historical
  same-store restore tests are not authority to resume copied/restored State.
  The [restored-State plan](../plans/restored-state-authority/TODO.md) remains
  design-only/blocked: independent human presence/revocation, global durable
  linearizable single-winner fencing across clones, original payer/effect/Queue
  reconciliation and real competing-host/crash tests remain prerequisites.
  Privileged mount/root snapshot certification and generic ToolOutput
  encryption/erasure/rotation/cloud retention certification are not supplied by
  internal backup tests.
- **Network/Fleets:** typed remote PTY, preview forwarding, rich Fleet aggregate
  UX, automatic relocation policies, reusable multi-room TCP relay and broad
  managed operations must be judged as separate services. The existence of a
  capability enum, Machine registration, Fleet operation or authority snapshot
  is insufficient. No generic reverse shell, silent broadcast, hidden Session
  migration, credential transport or automatic fleet-member absorption follows.
- **Identity/custody:** loopback invite team gateway limitations, per-user runner
  isolation, public/TLS hosting, passkey/password/account recovery, browser-local
  credential isolation, external vault adapters and richer OAuth returning-account
  refinement remain distinct from device proof, native refresh families and the
  local SecretStore. Local IDs/display names/email never supply cross-host equality.
- **Delegation/orchestration:** dirty-parent snapshot/materialization, non-Git
  managed directories, OS/container sandbox or child trusted shell, automatic
  worktree pruning, automatic patch/stage/commit/merge/import, cross-machine
  children/teams/planner/review, dynamic model-selected authority, independently
  resettable child budgets, pricing/monetary settlement and general parallel
  DAG/join topologies were explicitly deferred here. Later source may add a
  bounded messaging/scheduling/reviewer slice; preserve its exact scope instead
  of treating the earlier no-feature paragraph as a prohibition on accepted work.
- **Presentation:** native collaborative-draft participation, independent per-tab
  Agent ownership, full advanced browser workbench migration, synchronized
  cross-client layouts and broad Git mutation UX were separate work. Read-only
  view changes cannot inherit provider/tool authority. The current
  [workspace views](../plans/workspace-views/TODO.md) and
  [UX convergence](../plans/ux-convergence/TODO.md) checklists own remaining scope;
  recent scoped Git work does not imply checkout/commit/fetch/push parity.
- **Distribution/certification:** actual native execution on missing
  macOS/Windows/ARM hosts, owner-approved version/release, configured identity
  signing/notarization, separately authorized live-provider/billing checks and
  independent security review remain in [release readiness](../plans/release-readiness/TODO.md).
  Cross-compilation, ad-hoc signature/hash checks, loopback fixtures and tests
  cannot close those external gates. Optional embedded hardware/firmware and
  independent certification remain in the [embedded plan](../plans/embedded-platforms/TODO.md).

Finally, old phase stop-for-review instructions document the authorization
boundary at the time, not a demand to undo later user-authorized Phases 38–68.
Likewise, historical “Rust not run,” missing Clippy or read-only Nix-store notes
must stay attached to their original environment. The accepted checkpoint and
newer exact product-gate records supersede stale validation TODOs where their
scope overlaps. Only the current plan checklist can close today's gate after
its named command runs on the recorded commit. No runtime, build or provider
execution was performed merely to assemble this historical foundation section.

## 5. Runtime, policy, context and recovery: the governing design

This section condenses the unnumbered AW-P6 design and its successive implementations. It is an onboarding map to the retained original documents, not an authority to execute their old starting prompts. The current [plan index](../plans/TODO.md), [restored-State authority TODO](../plans/restored-state-authority/TODO.md), Git, actual source and executed checks outrank a dated document's “next” section. The accepted checkpoint 47ee8a03abecbb3aee84a49c0a590096d76ed1a7 covers previously verified work; it does not close current external, release or restored-State gates. No runtime tests or builds were executed while preparing this documentation fragment.

The [October 11 decision ledger](../plans/CURRENT-AW-DECISIONS-AND-PENDING-2026-10-11.md) explains the original product direction: one Rust-first Agent, clean text-first use, progressively richer Workspace/Domain/Session governance and opt-in remote/collaboration surfaces. Its original source SHAs, counts, “current” schema and comprehensive-archive instructions describe an older snapshot. The rebuilt live repository and present instructions supersede its old source inventory and mandatory archive-reading workflow. The earlier archive mechanism in [ARCHIVE-WORKFLOW](ARCHIVE-WORKFLOW.md) is historical Phase 0–7 material; Phase 8 moved development to Git. Do not recreate tar editing, removed helper commands, manifest chains or comprehensive evidence dumps as development infrastructure.

AW-P6 was a user-directed conceptual consolidation inspired by agentc P6. Agentc was a design reference, sometimes dirty/in progress, not a copied implementation, an executable parity target, or a synchronization dependency. The central [master plan](../plans/AW-P6-FOUR-PRIMITIVES-MASTER-PLAN-2026-10-11.md), [four-primitives contract](RUNTIME-FOUR-PRIMITIVES-AND-SERVICES-2026-10-11.md) and [hooks/trust contract](RUNTIME-HOOKS-TRUST-AND-POLICY-2026-10-11.md) contain the complete design and as-built amendments. Initial DESIGN ONLY headings in those documents do not mean their later implementation slices are absent; later appendices and current source refine the earlier targets. Equally, an implemented slice is not evidence that every aspirational P6 surface shipped.

### Four primitives and the services built on them

| Primitive | Meaning and ownership |
| --- | --- |
| Runtime | Host-owned bounded scheduling, admission, wakeups, continuations, Actor directory, time and shutdown. It orchestrates the existing Agent; it does not create a second provider/tool loop or store credentials. |
| Actor | Exactly Agent or Process. Keys include Runtime instance and generation plus Actor slot and generation. A child Agent is an ordinary child Session using the same Agent. A PTY is a Process mode. A PID, label, Session ID or raw ActorKey does not authenticate its caller. |
| Message | Addressed Control, Input, Notify or Hook with typed kind, target, source sequence, priority and bounded body. Runtime messages differ from transcript Messages. Queue is the scheduled Input subset; signals are Control. |
| Envelope | Host-minted immutable authority/resource ceiling. A child narrows its immediate parent. Source metadata, model text, project files, saved history and display focus never mint grants. |

The existing deterministic no_std TurnMachine is retained, not promoted to another Actor-system primitive. ToolPolicy enforces permissions, TurnJournal is the mandatory effect service, and provider/store/platform adapters implement host I/O. Queue, process supervision, child Sessions, cancellation, hooks, settings, prompts, status, media, router/planner/guardian, approval, CLI/RPC/UI and persistence remain services over the four primitives. Conceptual simplicity does not require merging every Rust crate.

Turn means one admitted user/controller prompt through settlement; Step means one model cycle plus its complete tool batch. An Attempt identifies a provider dispatch or external effect. Settlement is a recorded fact; cancellation is a request and cannot imply rollback or zero cost.

The kernel sources [actor.rs](../../crates/aw-agent-kernel/src/actor.rs), [message.rs](../../crates/aw-agent-kernel/src/message.rs), [envelope.rs](../../crates/aw-agent-kernel/src/envelope.rs) and [hook_contract.rs](../../crates/aw-agent-kernel/src/hook_contract.rs) contain the pure alloc-enabled contracts. They do not import Tokio, SQL, sockets, process APIs or credential storage. Hosted [runtime.rs](../../crates/aw-agent-core/src/runtime.rs), [control.rs](../../crates/aw-agent-core/src/control.rs), [hooks.rs](../../crates/aw-agent-core/src/hooks.rs) and [actor_owner.rs](../../src/actor_owner.rs) connect those contracts to current ownership and the original Agent. Full, JSONL-minimal, stateless and embedded hosts expose different supported capabilities while retaining that shared kernel and turn semantics.

### The original design choices, including deliberate deviations

The master records agentc decisions R-D1 through R-D31. Their durable interpretation in aw is:

| Decision IDs | Adopted direction and aw-specific boundary |
| --- | --- |
| D1–D3 | One Agent/startup path, permitted pinned parent model/transport inheritance, and inherited/narrowing trust. Account, payer, custody and executor remain independently selected and checked; children do not receive a hidden cheaper provider. |
| D4–D5 | Reject default workspace RW/automatic authorization. Tools remain Off by default. Within an explicitly authorized scope avoid repetitive prompts; any approval is fingerprint/Actor/site/source/epoch/scope/expiry bound and cannot override a hard deny. |
| D6–D9 | Agent and Process Actors; graceful/force cancellation with factual evidence; one Turn-targeted priority Input queue; only owner/direct controller steers, children notify upward. Urgency never reverses an external effect. |
| D10–D13 | Deliberate Rust cleanup without aliases, preserve separately versioned E5 C ABI; reuse SQLite v33/JSONL rather than importing agentc custom records; deterministic per-Actor admission with hosted concurrent I/O; bounded fail-closed ingress. |
| D14–D18 | No passive downloads/daemons/processes/model calls; behavior references do not create executable parity goals; content and control evidence remain distinct; waiting/deferred work must allow control drain; Agent freeze is cooperative, OS suspension requires actual support and proof. |
| D19–D24 | Earlier five-primitive wording is superseded by final four. Deciders are services, not proxy Agents. Exactly ten typed stages and Next/Defer. All tools share admission; Process tty is a mode. |
| D25–D28 | One typed Actor factory, optional narrow agent/process/queue management capabilities alongside coding tools, inert media seam with live voice deferred, and separately admitted/journaled effects from any reviewer/controller proposal. |
| D29–D31 | Extension-owned typed settings; approved system-prompt anchors; semantic status regions/lifetimes; canonical Turn versus Step terminology. |

Further deliberate restrictions: stale Exact Turn targets reject rather than roll forward; same-UID shell/process groups are not a sandbox; context/view changes do not retarget a running Agent; imported text never imports credentials/grants; optional hooks cannot disable security, journal or revocation; restored facts never become executable commands.

### One tool and Hook boundary

Exactly ten stages exist: Turn, Step, Input, Prompt, Message, Output, Tool, Signal, Event, Lifecycle. There is no extra routing, approval, status or agent-spawn stage. The only Hook outcomes are Next(Some(message)), Next(None), or Defer(continuation). Denial, replacement, branch/spawn and waiting are typed service operations expressed through those verbs and normal Actor/Message admission.

The current [Extension trait](../../crates/aw-agent-core/src/extension.rs) has name, optional wants_stage and one on_hook callback. The old before_/after_ method vocabulary is not a supported compatibility path. Event and Lifecycle are observe-only; optional display faults cannot alter factual results. Prompt transformation can use only approved contributions. ToolCall changes need independently installed host revision policy, revalidation and an immutable ToolRevised before ToolStarted. ToolResult transformations may change the model-visible view, never the factual ToolRecord, call ID, effect or status. A denied complete batch receives no-effect ToolDenied receipts and correlated error results for every original call ID.

Defer is owned by the current Actor/Session, generation, Envelope revision, Turn, handler and deadline. It completes once or fails closed on timeout/cancel/revocation/unload. A standalone executor cannot invent an owning Runtime continuation. Provider-private signed reasoning and continuation are never ordinary public Hook content, log text or cross-provider history. Native linked extensions and selected subprocess extensions run under trusted host OS authority; the type/API boundary is not an in-process malware sandbox. External process extension ABI is currently v2, with prompt/tool.call/tool.result; old v1 paragraphs in older docs are historical.

The canonical effect order remains:

~~~text
authenticate owner and current binding
→ validate current exact source/site/effect/Actor Envelope and ToolPolicy
→ optional typed Hook/approval/revision, then revalidate
→ durable original intent, awaited
→ recheck authority after the await
→ actual provider/Tool/OS effect
→ factual immutable result or Unknown, awaited
→ current disclosure check
→ safe model continuation/output/settlement
~~~

A name-only trusted-host exception was removed during the foundation slice. Host, MCP and Extension grants use the same source kind/ID/epoch, capability name, effect, Actor and execution-site predicate. Tool registration, discovery, MCP readOnlyHint, project trust and a reviewer verdict are not grants. New starts require current writer authority; a truthful terminal receipt for an already-started effect must be preserved even after WriterEpoch changes. The final Turn is a projection of original immutable receipts, not an editable accounting record.

### P6 implementation progression through management services

The plan was dependency-ordered, not a numbered Phase 69:

| Wave | Result and remaining distinction |
| --- | --- |
| 0 / F1 / 1–3 | Inventoried and froze contracts, added pure Actor/Message/Envelope/Hook vocabulary and host Runtime, consolidated exact Host/MCP/Extension grants, removed name-only Host bypass. Foundation alone did not enable product Actor management. |
| 4A | Shared PipeProcess lifecycle/capture reused by Bash; optional PtyProcess delegates to existing aw-terminal. Host selects absolute program/root/args/environment, drains stdout/stderr independently and records bounded prefix/tail. Process metadata admission is not a durable OS spawn receipt. No automatic resurrection or broad model process manager. |
| 5A | Bounded mailbox, stable priority/fairness, pending edits/deletes, reserved-capacity authenticated active-Turn cancel, cooperative pause/resume and safe-point AgentSessionController over the original Agent. Lost claims become Unknown; urgent text remains a separate future Turn. |
| 6A / 6B | One typed extension Hook seam, strict external ABI v2, owned one-shot async Defer, correlated whole-batch no-effect denial and separately authorized/journaled argument revision. Mandatory authorization and journal stay outside optional hook switches. |
| 7A / 7B | Existing child Sessions bound to exact direct-parent Actors. Same-host managed-worktree site transfer reissues only a narrowed audited Host grant subset at subagent:<id> after checking durable worktree/root/Session/provider provenance. Same-site grandchildren narrow their immediate parent. Router/planner/guardian Step guards retain original paid budgets, source hashes and root-only publication. This is not arbitrary cross-machine delegation or an OS sandbox. |
| 9A / 9B1 | Original SQLite v33 audit stores one exact Input's Prepared/Accepted/Dispatching/Settled/Rejected/Unknown/Inspected facts plus independently Prepared/Applied/Rejected/Unknown/Inspected Control facts. WriterEpochFence checks live authority at Agent/Tool/disclosure/Hook boundaries. Accepted is inbox acceptance; Applied is cooperative Runtime control, neither is provider/OS settlement. |
| [9B2A](../plans/AW-P6-9B2-DURABLE-INPUT-QUEUE-AS-BUILT-2026-10-11.md) | Private encrypted multi-Input queue, exact CAS/receiver acknowledgments and restart fences. At most 32 live items per Session/branch/site, 64 KiB text each, priorities -20…19, stable enqueue ties and normal-priority service after four urgent claims. No automatic draining. |
| [9B2B](../plans/AW-P6-9B2B-OBSERVATIONAL-NOTIFY-AND-CUSTODY-AS-BUILT-2026-10-11.md) | Durable private observational Notify with same-process acceptance proof, explicit Accepted-only private read/read-ACK and bounded ledger-proven retired ciphertext reclamation. Notify is never executable Input/Control, a Tool result or an approval; unknown pre-ledger orphan files remain untouched. |
| [9C1](../plans/AW-P6-9C1-AUTHENTICATED-LOCAL-ACTOR-RPC-AS-BUILT-2026-10-11.md) | Explicit local aw rpc --agent-stdio --agent-actor-owner --session-id binds one existing persistent Session with tools Off. Strict typed Actor query/disposition/enqueue/step/private-notice methods use host-minted authority. Unbound ordinary R12 Actor methods remain unsupported. |
| [9C2](../plans/AW-P6-9C2-REMOTE-ACTOR-AND-LIVE-CONTROL-AS-BUILT-2026-10-11.md) | Independent bounded reader permits exact active-Turn local Control. Opt-in Linux signed AwNet MachineLink QUIC reaches the same owner/Agent; pairing and typed lease are supplemented by separate expiring/revocable Session/branch/site/writer/peer-key+security-epoch grants. In-flight Step periodically rechecks authority; private output is reauthorized. Machine identity is not remote human identity. |
| [8A](../plans/AW-P6-8A-EXTENSION-SETTINGS-AND-PROMPTS-AS-BUILT-2026-10-11.md) | Manifest-owned typed settings with exact key CAS and CLI > freshly trusted project > user > default precedence. Approved project files are inert data. PromptRegistry uses Preamble/Tools/Rules/Environment/All and Before/After/Replace; project contributions remain quoted references and cannot replace policy. Opt-in composition uses the original Agent factory. |
| [8B](../plans/AW-P6-8B-ENCRYPTED-SECRETS-AND-STATUS-EVENTS-AS-BUILT-2026-10-11.md) | Active-owner/writer/Actor-bound Secret settings stored in the existing encrypted SecretStore, immutable metadata CAS and known-retired cleanup. UI protocol v2 adds semantic status regions and bounded ephemeral EventRail from factual metadata, without exposing prompts/answers/private continuation or creating another journal. |
| [8C](../plans/AW-P6-8C-MEDIA-APPROVALS-AND-EVENT-PRESENTATION-AS-BUILT-2026-10-11.md) | Inert content types and bounded frames on an already-authorized PipeProcess; current text mailbox denies nontext before sequence consumption. Microphone/Speaker/VideoCapture always deny. Owner-bound views reuse existing guardian decisions; read-only review pending CLI exists. Optional notices appear only in existing Activity and clear on Session switch/restart. |

The queue handshake is important enough to retain explicitly: encrypted body fsync → immutable Pending → writer-CAS Claimed before Runtime send → real mailbox acceptance and 9A Accepted → Queue Accepted after exact recipient/source/Turn/digest/process CAS → actual original Agent dispatch/receipts → hazard-free factual Input Settled → Queue Settled and private-body retirement. A new current Actor generation may explicitly bind untouched Pending under the same Session/branch/site/writer epoch. Claimed/Accepted/Dispatching or uncertain paid work require inspection and are not automatically requeued. Input, Control, Queue and Notify share durable request/sequence collision fences. No persisted Hook/Defer or effectful Control replay worker was introduced.

### Narrow model-visible management, deliberately separate grants

All of these are trusted owning Rust host opt-ins on the same journaled Agent, absent by default from ordinary CLI/RPC/TUI/web/AwNet use. Each needs its own already minted exact Host/trusted-host/epoch1/effect Envelope grant plus matching original ToolPolicy. Installation does not widen the Envelope. Every call binds current Session/branch/site/writer/Actor generation, actual active Runtime Turn, immutable Input Dispatching, selected account/payer/credential and cancellation.

| Slice | Accepted model operation and limitation |
| --- | --- |
| [9D1](../plans/AW-P6-9D1-MODEL-VISIBLE-READ-ONLY-MANAGEMENT-AS-BUILT-2026-10-12.md) | Read agent status; reduced queue list/query; optional process status on at most 16 host-registered opaque original Process handles. No Queue body/hash/ref, PID/program/env/output, provider switch, new process or live control. |
| [9D2A](../plans/AW-P6-9D2A-MODEL-ORIGIN-ENCRYPTED-QUEUE-ENQUEUE-AS-BUILT-2026-10-12.md) | queue_enqueue {op:enqueue,text}, Write. Host derives one canonical mq_ future Pending item per originating Session+branch+Turn, Auto/priority0. No claim or recursive model dispatch. Queue audit is encrypted-body metadata; original ToolCall arguments legitimately retain the text. |
| [9D2B1](../plans/AW-P6-9D2B1-MODEL-QUEUE-UPDATE-DELETE-AS-BUILT-2026-10-12.md) | Distinct queue_update / queue_delete Write grants, exact revision, still-Pending same-model-origin item from an earlier Settled Turn. One shared canonical md_ disposition per current originating Turn across update/delete. Actual first enqueue Tool provenance, effect/site/source, initial body digest and successful receipt are required by later 10D/E hardening. |
| [9D2B2A](../plans/AW-P6-9D2B2A-MODEL-PROCESS-STDIN-CLOSE-AS-BUILT-2026-10-12.md) | process_close_stdin {op:close_stdin,handle}, Write. An existing original owned PipeProcess only; Prepared precedes closing. Applied proves stdin ownership dropped, not Process exit or provider settlement. |
| [9D2B2B1](../plans/AW-P6-9D2B2B1-PROCESS-TERMINATION-AS-BUILT-2026-10-12.md) | process_terminate {op:terminate,handle}, separate Write plus independently terminable registration/random Process incarnation. Shared mp_ once-per-Turn Process intent with stdin-close; irreversible in-memory one-attempt guard. Applied requires direct-child Cancelled and drained output; descendant cleanup remains best effort and billing unproved. |
| [9D2B2B2](../plans/AW-P6-9D2B2B2-FUTURE-AGENT-ADMISSION-HOLD-AS-BUILT-2026-10-12.md) | agent_pause_after_turn {op:pause_after_turn}, Write. Durable ap_ Armed future Session+branch admission hold checked inside Input Prepare and Queue Claim transactions. Current paid Turn finishes normally. Only independently authenticated idle owner may Release after factual source Input Settled and no unresolved admission. This is not live FreezeActor or model self-cancel. |

General model-origin in-flight Cancel/Freeze/Resume, child/remote Actor controls, arbitrary Process stdin/signals/OS freeze/PTY/spawn, raw actor_op and human guardian approval remain disabled. A human decision cannot be fabricated by model text, a browser cookie, copied owner label or paired Node signature. Distinct grants intentionally preserve different destructive powers; merging close and terminate for apparent DRY would widen authority.

### Correctness hardening before composite recovery

The following dated as-builts document reproduced trusted-host/Store-boundary bugs and focused repairs. They do not certify hostile same-UID isolation or authentic commercial invoices.

| Record | Mechanism repaired |
| --- | --- |
| [10](../plans/AW-P6-10-CORRECTNESS-AND-RELEASE-REVIEW-2026-10-12.md) | In-memory Store with no coordinated Process incarnation could match None to None for external Process receipts. Prepare/terminal facts now require the actual coordinated process. |
| [10B](../plans/AW-P6-10B-PROCESS-ACTOR-GENERATION-FENCE-AS-BUILT-2026-10-12.md) | Process handle custody now pins full original ActorKey in one shared bound_to predicate; another valid binding to the same Session/site/writer cannot inherit or unregister it, nor can a retired Actor resolve it. |
| [10C](../plans/AW-P6-10C-CANONICAL-MODEL-INTENT-IDENTITY-AS-BUILT-2026-10-12.md) | Store recomputes canonical length-prefixed domain-separated mp_/md_ identities, sharing helpers with Service rather than accepting plausible aliases. Hashes identify intent, not permission. |
| [10D](../plans/AW-P6-10D-ORIGINAL-MODEL-QUEUE-TOOL-RECEIPT-PROVENANCE-AS-BUILT-2026-10-12.md) | A host-minted mq_ from a Settled Turn is insufficient model custody. Update/delete now proves the actual successful original queue_enqueue Tool proposal/intent/result, provider response, original attached run, effect/site and initial encrypted-body digest in the same transaction. Later authorized host edits do not erase creation provenance. |
| [10E](../plans/AW-P6-10E-ORIGINAL-TOOL-SOURCE-AND-RECOVERY-EVIDENCE-AS-BUILT-2026-10-12.md) | ToolStarted records optional validated ToolSourceEvidence kind/ID/epoch from actual registry. Old source-less records remain readable; they cannot establish exact Host management authority. Revisions require matching source. |
| [10F](../plans/AW-P6-10F-WRITER-EPOCH-TOOL-INTENT-AND-REVOCATION-RACES-AS-BUILT-2026-10-12.md) | Original run writer epoch must still match for new InvocationStarted, ToolStarted or ToolRevised, including failover/failback to the same node. Preserve late factual terminal evidence and already-paid result instead of replaying it. |
| [10G](../plans/AW-P6-10G-TERMINAL-PROVIDER-TOOL-RECEIPTS-AND-RESTART-AS-BUILT-2026-10-12.md) | Missing terminal provider metadata remains unknown across finalization/restart. Genuine trusted not_dispatched is a known no-call, not a phantom unknown bill. Another invocation cannot follow unresolved unknown within that run. |
| [10H](../plans/AW-P6-10H-MULTI-INVOCATION-RECEIPT-ORDERING-AS-BUILT-2026-10-12.md) | Sequential nonoverlapping provider ordinals 1…32 and exact terminal outcome/metadata pairs. Existing indexed receipt range bounds the check; query-plan evidence is not a production latency benchmark. |
| [10I](../plans/AW-P6-10I-IMMUTABLE-FINAL-TURN-RECEIPT-COHERENCE-AS-BUILT-2026-10-12.md) | Final Turn cannot omit/forge provider/Tool receipts, identity, usage or outcomes. One bounded OriginalRunReceipts projection is shared by finalization and recovery. Completed cannot hide unknown or unfinished effects. |
| [10J](../plans/AW-P6-10J-FINAL-TURN-CONTEXT-AND-TOOL-PROJECTION-FENCE-AS-BUILT-2026-10-12.md) | Retained future context must start with original User, retain factual Assistant order/proposals and have genuine Tool receipt authority; no injected System/extra User or fabricated Tool success. Tool-visible text may differ through authorized Hook projection; streamed answer need not byte-match immutable Assistant text. |

The current enforcement is in [store journal.rs](../../crates/aw-store/src/journal.rs), [actor_queue.rs](../../crates/aw-store/src/actor_queue.rs), [model_process.rs](../../crates/aw-store/src/model_process.rs), [model_agent_pause.rs](../../crates/aw-store/src/model_agent_pause.rs), [model_management.rs](../../src/model_management.rs), [model_process_control.rs](../../src/model_process_control.rs) and [model_agent_control.rs](../../src/model_agent_control.rs). Original Tool/paid receipts remain factual even when result presentation or branch attachment later fails.

### Backup, encrypted custody and quarantined cold restore: K through V

A SQLite backup is WAL-aware VACUUM INTO, not a raw sessions.sqlite3/WAL copy. Owner/service holds exclusive owner.lock; supported trusted-local direct is an explicitly selected exact-schema mode with shared lock/process/store-generation fences and no migration/restore privilege. Neither locking mode is a global cross-host fence.

| Record | Implementation or finding and its exact boundary |
| --- | --- |
| [10K](../plans/AW-P6-10K-PRIVATE-BACKUP-CUSTODY-AND-TOOL-OUTPUT-PRIVACY-REVIEW-AS-BUILT-2026-10-12.md) | Failed exclusive backup creation could delete a preexisting snapshot/file/symlink. Cleanup now owns only newly created output; success fsyncs file and parent. Characterization proves generic factual ToolOutput remains plaintext in SQLite/owner export/backup even when model-visible Hook output differs. |
| [10L](../plans/AW-P6-10L-PORTABLE-EXPORT-AND-SECRETSTORE-RESTORE-AS-BUILT-2026-10-13.md) | Partial export write failure cleans only its newly created file. SQLite-only restore with encrypted references cannot silently mint a replacement master. Correct key+item restores payload; missing/wrong key/item fails. SQLite-only backup remains distinct from vault backup. |
| [10M](../plans/AW-P6-10M-OPERATOR-COMPOSITE-BACKUP-DESIGN-AND-NATIVE-EVIDENCE-2026-10-13.md) | Design/native feasibility showed SQL maintenance alone did not fence direct vault writes. Chose owner-only cold/quiesced composite rather than hot distributed backup; toy manifest was not a shipping format. Later N–S resolve individual prerequisites, not a public command. |
| [10N](../plans/AW-P6-10N-ORIGINAL-SECRETSTORE-MUTATION-FENCE-AS-BUILT-2026-10-13.md) | Stable private secrets/.mutation.lock: put/delete take shared lock, new master initialization takes exclusive; exact backup maintenance holds exclusive across cooperating handles/processes. SQL lease releases before OS fence; restore-generation transition releases its exact old fence. Advisory local cooperation is not malicious-host confinement. |
| [10O](../plans/AW-P6-10O-ORIGINAL-ACTIVE-ENCRYPTED-CUSTODY-INVENTORY-AS-BUILT-2026-10-13.md) | Maintenance-gated current Queue/Extension/Provider/OIDC/OAuth reference inventory checks current versions, private files, AEAD and Queue SHA; exposes counts only. Latest Pending/Claimed/Accepted and Active states create obligations; retired/orphans are counted, not adopted/deleted. |
| [10P](../plans/AW-P6-10P-INTERNAL-COMPOSITE-STAGING-AND-SNAPSHOT-CAPTURE-AS-BUILT-2026-10-13.md) | Under one real owner/vault fence, private random .partial captures WAL-aware SQL and bounded original key/items; manifest says INTERNAL-PARTIAL-NOT-RECOVERABLE, complete:false. No recursive backup lease or automatic public artifact. |
| [10Q](../plans/AW-P6-10Q-STANDALONE-VERIFIED-ENCRYPTED-STAGING-AS-BUILT-2026-10-13.md) | Rehashed wrong ciphertext/master could satisfy checksums alone. Standalone staged verification now reuses the original active-reference/AEAD/Queue-digest engine and exact directory allowlist; verifies with source removed. Surviving .partial remains unpublished. |
| [10R](../plans/AW-P6-10R-OPERATOR-ATOMIC-PUBLICATION-V1-AS-BUILT-2026-10-13.md) | Distinct aw.operator-composite/v1 complete manifest; Linux same-parent renameat2(RENAME_NOREPLACE), no overwrite or cross-filesystem fallback. Disarm own cleanup immediately after rename; later failure preserves named artifact and reports uncertainty. Internal-only, not CLI/SDK backup service. |
| [10S](../plans/AW-P6-10S-QUARANTINED-OFFLINE-COMPOSITE-COLD-RESTORE-AS-BUILT-2026-10-13.md) | Source-independent complete archive verifies before copying to an absent aw-restored-* destination. Offline copy increments restore generation, fences old processes/leases and advances writers into read_only. Original hazards/Queue/provider/Tool facts persist. State is quarantined, cannot open owner/direct and cannot execute. |
| [10T](../plans/AW-P6-10T-DUAL-QUARANTINE-INSPECTION-AND-NONACTIVATING-OWNER-REVIEW-AS-BUILT-2026-10-13.md) | Removing one filesystem marker previously permitted owner registration. Independent SQLite store_meta.cold_restore_quarantine denial now complements .aw-cold-restore-pending; either blocks opening before locks/migration/process/recovery. Counts 14 risk classes; owner-of-every-Session review requests never activate. |
| [10U](../plans/AW-P6-10U-SIGNED-DEVICE-CONSENT-PREACTIVATION-AS-BUILT-2026-10-13.md) | Reuses original imported portable Ed25519 identity/continuity grant/epochs for short-lived signed review prerequisite when no outstanding risks exist. Binds Store/archive/generation/risk/challenge; stores hashes, not private key/signature. Possession of copied device key is not independent live-human presence. |
| [10V](../plans/AW-P6-10V-ACTIVATION-DRY-RUN-AND-TRANSACTIONAL-EFFECT-FENCE-AS-BUILT-2026-10-14.md) | Read-only eligibility diagnostic rechecks actual detached signature, current device/grants/epochs/source/risk/writer facts; optional immutable dry-run audit revalidates inside BEGIN IMMEDIATE. A copied hash receipt alone fails. Original and clone can pass local diagnostic but both stay non-runnable. |

Capture is bounded to 32,768 private items, 512 MiB SQL+private bytes, 8 MiB manifest and the original bounded maintenance lease. Private directories/files use 0700/0600/no-follow/single-link checks. Complete manifest hashes, SQLite quick_check and AEAD under a key copied into the same archive establish local coherence/corruption detection, not independent source authentication or an encrypted public archive. The archive includes decryptable master+items and may contain raw prompts, Tool arguments/results, account-related metadata and ciphertext. Treat it as sensitive private owner data. Generic ToolOutput encryption/erasure/key rotation, credential-neutral publication, cloud and retention-law certification remain unimplemented.

Current private [composite_staging.rs](../../crates/aw-store/src/composite_staging.rs), [secret_store.rs](../../crates/aw-store/src/secret_store.rs), [secret_custody_inventory.rs](../../crates/aw-store/src/secret_custody_inventory.rs), [cold_quarantine_review.rs](../../crates/aw-store/src/cold_quarantine_review.rs), [cold_activation_consent.rs](../../crates/aw-store/src/cold_activation_consent.rs) and [cold_activation_dry_run.rs](../../crates/aw-store/src/cold_activation_dry_run.rs) retain those boundaries. Their internal/test reachability is not an operator-facing restore permission.

### Non-executing exact-effect reconciliation: W1 through W9

| Record | Progression and remaining distinction |
| --- | --- |
| [W1](../plans/AW-P6-10W1-NONEXECUTING-EFFECT-RECONCILIATION-PREPARE-AS-BUILT-2026-10-14.md) | Classified the original fourteen unresolved-risk classes into non-executing obligations; zero counts never grant execution. |
| [W2](../plans/AW-P6-10W2-EXACT-PROVIDER-RUN-EVIDENCE-AS-BUILT-2026-10-14.md) | Enumerated exact unfinished Run, invocation ordinal, original LLM attempt and hazard keys; identity consistency does not prove provider billing settlement. |
| [W3](../plans/AW-P6-10W3-EXACT-TOOL-QUEUE-ACTOR-EVIDENCE-AS-BUILT-2026-10-14.md) | Added original Tool call/run, Queue Pending/Claimed/Accepted and Actor Input/Control identities; old separate read connections still left snapshot/provenance work. |
| [W4](../plans/AW-P6-10W4-EXACT-RECOVERY-AND-LEASE-EVIDENCE-AS-BUILT-2026-10-14.md) | Added hazardous execution leases, recovery hints and retry schedules with original keys/generations/holders; never a retry request. |
| [W5](../plans/AW-P6-10W5-SINGLE-SNAPSHOT-COLD-RECONCILIATION-AS-BUILT-2026-10-14.md) | One SQLite read snapshot for exact facts, initially compared to prior T counts. |
| [W6](../plans/AW-P6-10W6-COHERENT-ORIGINAL-RISK-AND-EFFECT-SNAPSHOT-AS-BUILT-2026-10-14.md) | Original T risk counts and all exact facts now use the same read transaction; concurrent WAL writer fixture proves consistency without a second risk implementation. |
| [W7](../plans/AW-P6-10W7-PINNED-COLD-FILES-AND-ORIGINAL-SOURCE-PROVENANCE-AS-BUILT-2026-10-15.md) | Holds original directory/SQLite/marker descriptors, identity metadata and postchecks; retains source kind/ID/epoch/site and original Queue/Actor/process/writer provenance where journaled. Missing legacy provenance is absent, never fabricated. |
| [Post-W7 review](../plans/AW-P6-10W-POST-W7-FRESH-CORRECTNESS-REVIEW-2026-10-15.md) | Reproduced same-call-ID/different-effect Tool completion falsely clearing risk; exact proposal/arguments/effect/site/source/order now match. Removed weaker private prepare wrappers in favor of one canonical verified snapshot. FD/path checks remain bounded honest-host evidence, not a malicious-root certificate. |
| [W8](../plans/AW-P6-10W8-ORIGINAL-OWNER-EXACT-EFFECT-NONEXECUTING-REVIEW-AS-BUILT-2026-10-15.md) | Exact source-specific effect fingerprint and overall snapshot digest; owner-of-all-Sessions transaction rechecks source/AEAD/quarantine/current facts, inactive copied authorities, duplicate subject/ID and bounds, then appends one immutable review request. Only KeepQuarantined or RequestFreshProposalReview, never settlement/activation. |
| [W8A](../plans/AW-P6-10W8A-POSTCOMMIT-UNCERTAINTY-AND-ATOMIC-INDEX-RECOVERY-AS-BUILT-2026-10-15.md) | Post-COMMIT failures explicitly report unknown audit outcome/inspect, preserving an already-written row and denying retry. Its historical Git-helper lessons also preserve prepared opaque index after possibly advanced HEAD; current operating workflow supersedes removed helper names. |
| [W9](../plans/AW-P6-10W9-FRESH-HUMAN-AND-CROSS-CLONE-AUTHORITY-PREREQUISITES-2026-10-15.md) | DESIGN ONLY/BLOCKED. Specifies independent live-human origin, external durable linearizable single-winner clone fencing, exact external-effect/payer reconciliation and crash-safe fresh authority before any runnable transition. Nothing provisions these prerequisites or authorizes activation. |

W8 source [cold_effect_reconciliation_review.rs](../../crates/aw-store/src/cold_effect_reconciliation_review.rs) explicitly records review_only:true; original_effect_settled:false; unknown_provider_cost_preserved:true; independent_live_human_attested:false; globally_unique_new_authority:false; fresh_authority_granted:false; activation_authorized:false; automatic_replay_allowed:false; runnable:false. [cold_effect_reconciliation.rs](../../crates/aw-store/src/cold_effect_reconciliation.rs) is the shared original-fact snapshot path. Both restored copies may independently request review; neither wins execution authority by doing so.

The current [restored-State authority plan](../plans/restored-state-authority/TODO.md) retains these unmet conditions:

1. Verify every unknown provider attempt against original payer/account/credential provider-owned usage/invoice or independently verified human accounting disposition. Missing evidence stays possibly charged/unresolved, not zero.
2. Reconcile original Tool/OS effects with exact host/Process/site/intent/result evidence. Failed/interrupted signals do not establish termination. Unverifiable effects remain unresolved or abandoned only as a fresh future proposal, never certified rollback.
3. Review each original Pending/Claimed/Accepted Queue and Actor Input/Control against Session/branch/writer facts. No old admission/grant survives automatically and no historical Queue item auto-dispatches.
4. Establish independent live-human presence/revocation using trusted-origin single-use short-lived challenge bound to original Store/archive/effect digest, target Host/site, restored generation, fresh scope and coordinator term. User-presence plus user-verification authenticator is required. Copied Ed25519 signatures, owner strings, model text and old review hashes do not suffice. Never copy authenticator private credentials into State/audit/model.
5. Provide an independent durable linearizable single-winner coordinator CAS for original Store/archive across all clones and old owner; monotonic terms survive restart, lease expiry and replay. Partition/unreachable coordination denies activation/effects. A local lock or TTL is not a global fence.
6. Only after independent proofs and fresh authorization, design inert-target plus durable-coordinator pending transfer with fresh owner/writer/Actor/Process generations and original CAS/fsync boundaries. Uncertain outcomes remain Unknown; inspect source of truth, do not retry side effects.
7. Prove first-open zero old provider/Tool/OS effects, zero historical Queue claims, zero unsolicited Actor admissions and no duplicate charge at every crash/reconnect boundary. New effects need fresh human proposals, unique intent IDs, current credentials/exact grants and original journal handshake.
8. Execute real competing-host/partition/quorum/rollback/revocation/authenticator lifecycle tests. Copies of U/V/W8 records must fail as bearer grants. Native Linux/macOS/Windows evidence is separate from optional E6.
9. Show factual progression only: Quarantined → effects need inspection → review requested → independent evidence missing → eligible for a new proposal review. No Ready/Resume/Activate until conditions hold; Unknown is never green.
10. If the threat model requires it, independently analyze FD-bound filesystem/SQL snapshots against privileged mounts/root; current inode anchoring is not that certification.
11. Retain private composite/SQLite Tool output and prompt/key custody limits; no generic encryption/erasure/rotation or credential-neutral/cloud/retention certification is implied by local backup tests.

### Context flow: explicit knowledge transfer preserves authority

[CONTEXT-FLOW](CONTEXT-FLOW.md) is the original Phase 9 design; [CONTEXT-FLOW-IMPLEMENTATION](CONTEXT-FLOW-IMPLEMENTATION.md) records the bounded Phase 10 implementation. [PI-CONTEXT-PARITY](PI-CONTEXT-PARITY.md) records the original reference audit, not current external executable conformance. Plain fork/switch is zero-inference and retains chosen ancestry. A checkpoint cuts projected prefix; an import adds attributed context without discarding destination history. Conversation parentage remains a tree; immutable source/dependency links form a separate bounded backward-reference graph.

The common workflow is Prepare exact authorized source/target snapshot, then explicit manual or tool-free model candidate, preview/human edit, and independent exact Apply. Generation and application are separately identified/receipted. Target advancement preserves the paid candidate and requires a deliberate compatible target choice, never silent rebase/regeneration. Source advancement does not mutate a pinned plan. No branch writer stays locked over network inference.

Sources are Standalone active path, Divergent suffix after common ancestor within a Session, or Explicit range with coherent completed boundaries. Current implementation documents at most 16 sources, 512 KiB evidence, 512 dependencies, 32 MiB raw-path guard, bounded output/time/artifact/receipt counts. Oversized input rejects rather than silently claiming complete coverage. Context is labeled unverified data; file/test claims remain as observed then, and summaries do not read today's files behind the user's back.

Apply supports additive append, fork, new Session, next-prompt one-shot or explicit compaction with declared retained tail. It grants no tools, imports no provider credentials and does not move execution root/account by implication. One-shot consumption is transactional at actual prompt admission, not recreated on retry. Automatic full-SQLite compaction and generalized map/reduce/periodic source refresh are separate limits, not established by explicit compaction.

Read access differs from disclosure to an account/provider and to a destination audience. Source-owner release epochs and account allowlists are rechecked during preparation, generation, preview, application, future model context, viewing and export. Summaries of summaries, derived answers and later compaction retain transitive dependencies. Revocation may conservatively deny an affected derived Session/path; deleting the source hyperlink or rewriting its words is not declassification. Already delivered bytes cannot be recalled. Private candidate generation belongs to the requester, not source authors or every source reader. Cross-owner derived portable export remains refused without independent release.

The implementation uses original SummaryExecutor/SummaryJournal/provider/account and aw-store context tables/projections, not another unrestricted main Agent. Summary attempts journal before paid dispatch and retain interrupted/unknown results without regeneration. Personal-device summaries require explicit protocol2 Summary purpose and exact locally approved prepared-input digest, not a target-chat grant reused for arbitrary histories.

### Retry, paid result preservation and device custody

[LLM-RETRY-RECOVERY](LLM-RETRY-RECOVERY.md) distinguishes a logical LLM operation from immutable provider attempts. An explicit same-context retry creates a separately recorded attempt against the original pinned context and the same actor/account/account-owner/payer/custody/executor/model destination; retry from current branch creates a new logical operation. Current authority is selected and revalidated afresh: grant revision and credential version need not equal the previous attempt, and the new attempt records its actual identity. The original route selection is retained rather than replaced by a new route receipt. Every attempt keeps initiator, account, payer, custody/executor, model and known/unknown usage. Completed paid results persist before branch attachment CAS; attachment conflict or lost client delivery must not cause another inference. External-effect hazards independently block continuation.

There are two different current retry layers and they must not be collapsed:

- Original Agent [retry.rs](../../crates/aw-agent-core/src/retry.rs) permits automatic extra provider polls only for trusted BeforeDispatch, with no streamed bytes, explicit host opt-in default 0/max 2 extras, and 32 total provider-attempt cap. Timeout, 429/503, transport loss, cancellation, partial streams or Tool errors do not prove no dispatch.
- SQLite [durability.rs](../../crates/aw-store/src/durability.rs) retains explicit Interactive versus Autonomous logical-operation policy snapshots, retry_unknown, due times and generation-fenced claim APIs. Interactive unknown needs explicit user action; autonomous new attempts require the exact durable bounded policy/claim plus fresh current authority, the same pinned context/destination dimensions, remaining policy/budgets and no blocking Tool hazard. Merely inspecting a retry row never dispatches it, and copied schedules in cold State remain inert.

Current terminal outcome pairs in store journal are response_received with metadata; not_dispatched without metadata; stream_rejected or failed_or_cancelled without metadata. The last two remain unknown. The internal invocation state received also represents a definitively closed trusted no-call with null metadata; it must not be described as a paid vendor response. New serial ordinals cannot follow a started/unknown receipt. A truthful late finish may be stored after writer change while a new start remains denied.

[CREDENTIAL-PLACEMENT](CREDENTIAL-PLACEMENT.md), [DEVICE-INFERENCE](DEVICE-INFERENCE.md) and [DEVICE-PROTOCOL](DEVICE-PROTOCOL.md) keep human identity, history owner, provider account, payer, credential custody, inference executor, model destination and Tool executor separate. The collaboration owner need not hold a personal key. Localhost belongs to the connecting process's machine. Browser memory/localStorage/IndexedDB/WebCrypto do not defeat malicious same-origin code; a separately trusted native worker must enforce finite local spending/disclosure grants and expose no secret-export API.

The existing personal Bridge/DeviceWorker uses explicit fixed actor/account/executor/model/grant binding, private Unix IPC or independently configured authenticated/encrypted SSH. The mutual nonce HMAC-SHA256 protocol authenticates ordered frames but does not itself encrypt arbitrary TCP or prove a human initiated a request. Current private inference wire is v2; no endpoint/header/key/env/config/executable override is accepted. Worker alone resolves its configured credentials, never runs Tools and returns complete model proposals to the owner's original ToolPolicy/journal path.

Finite device consent bounds exact Session/branch IDs, lifetime, invocation count, context/output/schema/deadline. Ledger reserves semantic Session+global Turn+round identity before provider polling, survives restart and refuses repeated IDs, even with changed content. Disconnect/new connection/grant revision cannot reset accounting. No automatic reconnect or account fallback; offline/revoked personal executor means wait/deny according to explicit policy. Private provider continuation stays bounded in worker memory behind opaque same-turn handles, not raw reasoning in durable history. Device Summary purpose additionally binds exact source list and approved input digest; old chat-only grant does not authorize it. Pairing/key possession is not independent live-human approval, hardware attestation, verified invoice or production Internet security.

### Tools, processes and evidence boundaries

[TOOLS](TOOLS.md) contains useful historical contracts but mixes dated flag/hook/grant descriptions with later appendices. Use current typed CLI schema and source for spelling, defaults and actual availability: old --tools/--tool-root/--no-state, named permit, R7 ABI v1 and before_/after_ examples are not current compatibility promises. Current schema stays v33, JSONL-minimal stays v1 and app version stays 0.15.0 until separately authorized; [STATE](STATE.md)'s early v2/v32 language is historical, not current schema authority.

Coding tools use no-follow rooted file access, bounded UTF-8/schema/input/output and explicit complete-batch validation. Write creates new files; Edit is exact-one-preimage replacement. Search is on-demand/unindexed using existing Rust regex/glob/ignore libraries; skipped indexing/LSP must not return incidentally. Model-directed parallel:true is explicit preference for permitted Read and separately granted trusted Bash waves under host ceilings; mutation/Unknown barriers and original-order factual receipts remain. Remote read-only hints are not permission or proof of safe parallelism.

[process.rs](../../crates/aw-tools/src/process.rs) owns the pipe lifecycle shared by Bash. Cleared environment, cwd, absolute executable and process groups reduce accidental exposure but do not confine same-UID filesystem/network/credential access. Output capture distinguishes bounded head (48 KiB), tail (64 KiB), totals, truncation, EOF/drain failure and model disclosure caps. No full transcript claim after failed spill or inherited-pipe uncertainty. Optional PTY uses existing aw-terminal and must not enter minimal/stateless dependencies accidentally. Group TERM/grace/force and direct child reap do not prove every descendant stopped; Windows/macOS jobs, pidfd/ancestry and real OS freeze need actual independent proof. Human terminal tabs and model Bash remain separately authorized surfaces.

[SOURCES](SOURCES.md) retains primary references consulted during original implementation. References, historical Python source/SQL fixtures, local mock-provider tests, native PTY execution, signed localhost QUIC, portable WASM/C consumers, production billing, hostile-network authentication and per-platform release certification are different evidence classes. Dated native receipts are historical evidence accepted at their checkpoint, not rerun by this handoff. Old helper names explicitly marked removed are not executable current commands.

Remaining broader gates include real paid-vendor usage/account/credential behavior; independent human/OIDC and hostile-network review; native Windows/macOS/ARM process/PTY/ACL/network evidence; pinned release-toolchain and target/installer/signing evidence; natural power-loss/fsync/reconnect/disk-full matrix; pre-ledger orphan quarantine/retention; full product owner/approval/UI convergence; generic Tool-output at-rest privacy and production media consent/TLS/retention. E6 firmware remains parked, numbered 64/65 remain skipped and no Phase 69 is implied.

### Source-checked drift and pending-status interpretation

| Dated statement or unchecked concern | Current interpretation and source authority |
| --- | --- |
| STATE top note calls v32 current; earlier sections call schema2/3/4/21…32 current. | Historical migration contracts remain useful, but current latest schema is 33 in aw-store/src/lib.rs. No new migration is authorized by this handoff. The original JSONL-minimal journal remains version1; portable session-tree/context archive versions are separate formats and must not be conflated. |
| Runtime/Hook docs open DESIGN ONLY; master initially says coherent Actor/queue/hooks are absent. | Pure kernel and bounded hosted slices were subsequently implemented and recorded in the same docs' as-built appendices, focused P6 records and current source. The illustrative signatures and entire aspirational all-host contract are not guaranteed current public APIs. |
| Old TOOLS sections describe ToolPolicy.permit, trusted-host name exception, before_/after_ callbacks, extension ABI1, 2–4 automatic Read waves and old CLI flags. | Current exact ToolPolicy/Envelope predicate is in tool.rs; Extension has one on_hook; extension ABI is 2. Current model-directed parallel preference and host ceiling are separate from grants. Use compiled canonical CLI/schema/source; retain old prose as dated provenance, never reintroduce aliases or bypasses to match it. |
| 9B1 says multi-message custody and external Actor methods are open; 9B2A says Notify/custody GC is open; 9C1 says concurrent Control/remote gateway is open. | Later 9B2A implements private Pending Input custody, 9B2B implements observational Notify and ledger-proven retired-only sweep, and 9C2 implements exact live Control/signed machine gateway. These do not close orphan-file erasure, durable executable Control/Defer replay, remote human identity or production certification. |
| 8A says encrypted Secret setting backend/status/events are open; 8B says media/approval/event consumer follow. | Later 8B supplies encrypted owner-bound Secret settings/status service; 8C supplies inert media/Process frames, existing human-approval projection and opt-in Activity notices. Live audio, browser approval, OIDC initiating-human assurance, retention and OS isolation remain closed/unproved. |
| 9D2A/B describes model queue ownership mainly by mq_ and prior Settled Turn; early Process registry omits ActorKey; early Process/queue IDs are validated by prefix. | 10B pins Actor generation, 10C recomputes canonical IDs and 10D/E requires actual original successful enqueue Tool provenance plus exact Host source. Read focused successors/current predicates rather than reimplementing the weaker historical gate. |
| Same-context retry wording can suggest the entire old InvocationIdentity is frozen. | Store journal same_retry_destination compares actor/account/account-owner/payer/custody/executor/model, deliberately excluding grant_revision and credential_version; main_full reselects fresh current account authority and each attempt records its actual identity. Preserve original pinned context and route, not stale authority. |
| Older retry design describes policy-driven new attempts; later source forbids generic in-Agent ambiguous retries. | Both mechanisms coexist at different boundaries: aw-agent-core retry.rs is trusted BeforeDispatch-only/default0/max2 extras; aw-store durability.rs carries explicit snapshotted Autonomous retry_unknown and durable claim APIs. Neither grants account fallback, replays an external Tool effect, nor turns a copied cold retry schedule into live work. |
| 10M says vault mutation barrier and active-reference inventory are blockers; P/Q/R describe publication/restore as future. | N provides cooperative cross-handle/process fence, O active custody, P/Q staged verification, R Linux no-overwrite publication and S inert cold copy. They remain private/internal, while public composite backup/restore product enrollment, authenticated archive distribution and runnable restore have not shipped. |
| S originally relied only on the filesystem marker; T/U/V use aggregate effect counts and local device proof. | T adds independent SQL denial; later W snapshot/provenance/receipt matching strengthens inspection. Current source refuses either quarantine before owner registration. U/V never supplied independent human presence or cross-clone exclusion; W9/current TODO explicitly retain them as blockers. |
| Early W1–W4 separate prepare entrypoints and W5 precomputed T count comparison; post-W7 audit appears ordinarily fallible. | W6 unified one risk+identity transaction, W7 anchored files/provenance, post-W7 review removed weaker wrappers/strengthened exact Tool pairs, W8 rechecks exact source in write transaction and W8A reports uncertain commit acknowledgments as Unknown/inspect. Current source changes beyond these dated records remain governed by Git and the live plan. |
| Dated receipts, all-target snippets, removed smoke/source helpers and archived exhaustive handoff checks appear as completed gates. | Those are historical receipts at their stated snapshots, not commands rerun for this documentation. Use current TODO-SPEC product checks and live plan gates. Prior accepted work does not close unchecked external/release/native-host/authenticator/coordinator gates. Do not restore deleted evidence infrastructure to satisfy old instructions. |
| Earlier account/device docs label the bridge unimplemented or compile-unvalidated. | Later Phase8/10 amendments and current aw-inference source establish explicit Bridge/DeviceWorker/protocol2 architecture. They do not certify two-machine SSH deployment, commercial bills, malicious-origin UI isolation or modern native-target/security release gates. |

### Runtime-focused next-agent prompt

> Start from the current plan index and selected TODO, then read the relevant live runtime, Hook, State, retry, credential/device and source contracts. Treat dated P6 design/as-built records as linked provenance and current source/Git/executed checks as truth. Preserve one Agent/TurnMachine/ToolRegistry/ToolPolicy/TurnJournal, four Runtime primitives and Next/Defer; preserve version 0.15.0, SQLite v33, JSONL v1 and feature/no_std boundaries.
>
> Select only the user-authorized next bounded slice. For restored-State work, current scope is inspection/design prerequisites: both quarantine guards remain, every old Queue/admission/grant/effect stays inert and uncertain billing stays possibly charged. U signatures, V dry runs, W8 review requests, zero risk counts, local owner strings and a cloned Store cannot grant execution. Do not implement activation until independent live-human, external linearizable cross-clone fencing, payer/effect reconciliation, fresh authorization and first-open zero-replay conditions genuinely exist and are separately authorized.
>
> Inspect current owned source and exact plan gates before changing behavior; preserve the reserved UX subtree and staging as opaque. Use meaningful runtime/negative/crash tests for any implementation, distinguish mocks and local platform proof from external certification, and record unexecuted gates honestly. Do not broaden model Process/Agent/human permissions, retarget active views, migrate schema/ABI/version, resume E6/indexing/LSP, publish, tag or push incidentally.

## 6. Portable Agent, integrations, CLI and UX decision history

This section reconciles the retained design record with current source and the live release, UX and embedded checklists. Historical implementation/test receipts below are within the user-accepted `47ee8a03abecbb3aee84a49c0a590096d76ed1a7` scope; this handoff review did not rerun product tests. The current [release-readiness checklist](../plans/release-readiness/TODO.md), [UX-convergence checklist](../plans/ux-convergence/TODO.md) and [embedded-platforms checklist](../plans/embedded-platforms/TODO.md) decide which gates remain open. A dated plan's “next”, “missing”, “ready to commit”, historical version/schema or validation-helper instructions do not override later implementation, current source or those checklists.

### Why the portable cuts exist

The early [minimal-product gap register](../plans/AGENTC-MINIMAL-RUST-PARITY-2026-10-08.md) asked for an idiomatic lightweight Rust coding Agent while retaining the full product's execution and safety semantics. The [Rust-native master design](../plans/AGENTC-RUST-NATIVE-PARITY-MASTER-PLAN-2026-10-09.md) made the central decision explicit: one user-facing `aw`, one Agent/provider/tool/journal engine, composable optional capabilities and no second minimal or embedded execution loop. An external C implementation was originally a research/scenario reference. The later [canonical-only closeout](../plans/R14-5-CANONICAL-ONLY-R13-CLOSEOUT-2026-10-10.md) explicitly removed that executable comparator as a product requirement or certification dependency. Current development is first-party AW behavior and independently constructed consumers, not a competition/comparator program.

The Cargo cuts are different environments, not synonyms:

| Cut | Meaning and boundary |
| --- | --- |
| Default `full` | Existing std-first complete product; optional SQLite, Workspace, browser, network, identity and coordination capabilities compose around the same Agent. |
| `--no-default-features --features minimal` | Std-based local Agent with the existing JSONL SessionCursor/TurnJournal; deliberately excludes SQLite/full distributed/browser/Workspace authority. Cargo features are additive, so `--features minimal` alone retains defaults. |
| Root `--no-default-features` | Deliberate memory-only/stateless Agent; it is not an embedded/no_std CLI. |
| Portable Tier K | `aw-agent-kernel` without default features: core-only borrowed capability IDs/catalog/grant predicates, no heap/OS/network/store/renderer. |
| Portable Tier E | `aw-agent-kernel` and `aw-embedded` with `alloc`, without std: the same bounded TurnMachine, driven by embedder-supplied ports, allocator and panic policy. |
| Hosted Tier S | `aw-agent-core` retains standard Rust/Tokio and real hosted accounts, conversations, hooks, providers, registries and journals while delegating transitions to the same TurnMachine. |

The [embedded architecture](../plans/AW-EMBEDDED-NOSTD-LIBC-FREE-ARCHITECTURE-2026-10-09.md) deliberately chose positive additive `std` and `alloc` features. `no_std`, allocation-free, libc-independent library, final libc-free executable, static musl binary and actual firmware are separate claims. Reusable safe kernel/embedded crates own neither a global allocator nor panic handler. Standard CLI/TUI/HTTP/TLS/SQLite facilities stay in mature hosted libraries; there is no approved custom libc, TLS stack, executor, filesystem or second Session database in the portable kernel.

Current source points: [Cargo features](../../Cargo.toml), [kernel transitions](../../crates/aw-agent-kernel/src/turn.rs), [hosted Agent](../../crates/aw-agent-core/src/agent.rs), [embedded host ports](../../crates/aw-embedded/src/engine.rs), [SDK](SDK.md).

### R0–R14: what each step added and what it did not prove

The [R-phase roadmap](../plans/AGENTC-PARITY-IMPLEMENTATION-PHASES-2026-10-09.md) is a historical dependency/implementation record. These R/E work packages are unnumbered continuations. Numbered phases end at 68; intentionally skipped 64/65 indexing/LSP and parked E6 firmware do not become new work merely because a dated roadmap lists them.

| Step | Implemented design and retained limits |
| --- | --- |
| R0 / M0 | Reused the JSONL store for local create/list/title/resume/new, explicit memory-only operation and destination/root/system/custody compatibility fencing. Unresolved external effects block conflicting use; resumed public history does not restore grants or private provider continuation. JSONL is single-owner, not a replacement for SQLite's distributed/multiuser authority. |
| [R1 capability kernel](../plans/AGENTC-PARITY-R1-KERNEL-2026-10-09.md) | Shared exact Actor/site/name/source kind/source ID/host-issued epoch/effect grants. ToolSpec is validated and frozen at registration. Discovery/registration grants no execution; ordinary host name permits cannot launder an MCP/extension tool. Model-visible projection and pre-effect dispatch both recheck current permission. Unknown remains Unknown. The first zero-import no_std consumer proved capability logic, not a complete Agent. |
| [R2 search](../plans/AGENTC-PARITY-R2-SEARCH-2026-10-09.md) | Bounded on-demand `ls`, `find`, `grep` use `regex`/`globset`/`ignore` as mature matchers over descriptor-rooted `Root::list/read`; no ambient walker, subprocess PATH dependency, index, watcher or LSP. Named tool selection only narrows an existing grant; child grants were not widened. External rg/fd acceleration and Windows trusted-shell/Job Object behavior remain optional separately scoped R2b work. |
| [R3a Anthropic](../plans/AGENTC-PARITY-R3A-ANTHROPIC-2026-10-09.md) | Native Messages text/tool/SSE and opaque signed thinking continuation share the existing provider/Agent/journal. Explicit API-key/SecretStore account and supported adaptive effort; no consumer subscription OAuth assumption or silent legacy-budget fallback. Selected environment credential digest is pinned for a provider lifetime so later key rotation fails visibly rather than changing payer. |
| [R3b Gemini](../plans/AGENTC-PARITY-R3B-GEMINI-2026-10-09.md) | Native GenerateContent streaming/function calls, exact IDs and signed continuation, native API-key header, model path validation, native setup/account kind. Private continuation is in-memory, ordered, provider/model/destination-bound and cross-checked with public content. Missing signatures/incomplete streams never dispatch tools. Interactions, Vertex identity, multimodal/server-agent features and unsupported thinking budgets are separate. |
| [R4 model catalog](../plans/AGENTC-PARITY-R4-MODEL-CATALOG-2026-10-09.md) | One descriptive catalog used by CLI and setup: configured/bundled/provider-discovered source labels, bounded pagination, explicit refresh, 24-hour private cache, integer rates and explicit user overrides. Cache identity binds endpoint, credential reference and salted actual-key verifier; changed/missing key cannot borrow another payer's metadata/rates. Unknown price/capability stays unknown. Model discovery is not account authorization, quota or invoice evidence. |
| [R5a MCP tools](../plans/AGENTC-PARITY-R5A-MCP-TOOLS-2026-10-09.md) | Optional std-only official `rmcp` client supplies explicit absolute stdio processes or selected Streamable HTTP, current/initialized protocol negotiation, local bounded JSON Schema validation, namespaced tools and exact host grants. All remote effects classify Unknown regardless of readOnlyHint. The selected registry batch is capacity/collision-preflighted before partial registration. No startup connection, browser/owner grant, sampling authority, auto-reconnect or uncertain-effect replay. |
| [R5b legacy SSE](../plans/AGENTC-PARITY-R5B-LEGACY-SSE-2026-10-09.md) | Explicit old 2024 GET-SSE/POST framing bridge feeds the same typed RMCP engine. First endpoint, MIME, frame/deadline and exact scheme/host/port checks prevent bearer forwarding to a changed origin. No automatic modern/legacy fallback, reconnect or replay. |
| [R6 MCP data](../plans/AGENTC-PARITY-R6-MCP-RESOURCES-2026-10-09.md) | Explicit prompts/resources/templates/read/watch over the same selected connections. Exact advertised text URIs and argument schema are required; a resource-reader grant is an exact URI enum/Unknown effect, not generic HTTP access. Conservative level-1 URI templates, bounded pages/content, no binary decoding or system-role promotion. Negotiated change streams invalidate stale source revisions and terminate with source lifetime; notifications never trigger model/tool effects or restore authority after restart. |
| [R7 process extensions](../plans/AGENTC-PARITY-R7-EXTENSIONS-2026-10-09.md) | Explicit trusted absolute executable, cleared environment and bounded JSONL v1 describe/tool/prompt/gate protocol. Tool grants and advertised `before_prompt`/`before_tool`/`after_tool` hook approvals are separate. Crash, malformed frame, timeout or cancellation poisons the child; no retry. C11/Rust process examples are independent consumers, not agentc binary plugins or stable Rust dylibs. This is not OS sandboxing or an installable package ecosystem. |
| [R8 / E2–E3 shared Agent](../plans/AGENTC-PARITY-R8-EMBEDDED-AGENT-2026-10-10.md) | Hosted and embedded execution use the same TurnMachine for model/batch admission, global tool-call identity, durable intent acknowledgement, post-flush grant recheck, effect settlement and receipt-before-continuation. Independent embedded fake host actually ran a model→tool→journal→answer turn and matched the hosted transition trace; constructor is inert. Embedder supplies clock, secure entropy, real durable append/flush, authenticated model transport, exact registration/grant, schema validation and executor. Blocking callbacks own their own in-flight deadline. |
| [R9 initial reads](../plans/AGENTC-PARITY-R9-PARALLEL-TOOLS-2026-10-10.md) | Introduced bounded nonspawned read futures, whole-batch admission, all wave intents before effects and original-order receipts/results/hooks. Its initial unconditional four-read ceiling and serial-Bash rules are historical, superseded by the explicit follow-up below. |
| [R9 model-directed follow-up](../plans/AGENTC-PARITY-R9-MODEL-DIRECTED-2026-10-10.md) | Model requests `parallel:true` only on already-granted read/search/trusted Bash tools; omitted/false is ordered. Host can force serial. Shared default ceiling 16, absolute max 128, optional stricter read/Bash ceilings. Write/edit/Unknown and non-Bash Process remain barriers; Bash stays Process and retains uncertain-effect inspection. Source/account checks, ordered receipts and 1 MiB context/trace bounds remain; truncation is explicit. A model's claimed independence is not race freedom or OS confinement. |
| [R10 compaction/retry](../plans/AGENTC-PARITY-R10-COMPACTION-RETRY-2026-10-10.md) | Pure bounded compaction policy defaults Ask. Actual tool-free paid generation uses SummaryExecutor/SummaryJournal; Auto needs known context budget plus explicit paid-auto consent. JSONL appends paid start/finish and source-head-CAS active-tail checkpoint, retaining all immutable raw turns and monotonic IDs. Settled saved summaries apply after restart without another call; unknown paid attempts block for inspection. Default retries are zero; only a trusted positive `BeforeDispatch` attestation permits up to two explicit extra same-identity attempts. Generic HTTP/timeout/stream/Transport and Retry-After are not proof of no billing. Built-in HTTP does not assert that attestation. SQLite keeps its own ContextFlow rather than copying it into minimal. |
| [R11 project resources](../plans/AGENTC-PARITY-R11-PROJECT-RESOURCES-UX-2026-10-10.md) | Explicit canonical project root plus exact approved SHA-256 source manifest, bounded ancestor instructions/named templates/SKILL.md/themes. Trust/revoke is an explicit human action; every new prompt revalidates files/additions/deletions. Templates are User input, source data grants no tools/account/executor authority, themes are typed semantic values. Local single-user Agent only; browser/owner/Workspace trust remains deliberately denied pending a separate authenticated protocol. No new minimal vault or subscription login was introduced. |
| [R12 headless/RPC](../plans/AGENTC-PARITY-R12-HEADLESS-RPC-2026-10-10.md) | Bounded JSONL v1 event and stdin/stdout transport around the same hosted Agent, not another owner. Explicit accepted/streamed/settled stages and exact target IDs for cooperative abort. Read-only state/model/message methods cause no discovery. Independent Rust/Python clients treat pipe loss as TransportUncertain, never resend automatically. Request IDs are process-local, not a global idempotency oracle. Privileged direct-shell/extension-control RPC stubs are unsupported. Protocol diagnostics expose only event names/frame lengths. |
| [R13 local conformance](../plans/AGENTC-PARITY-R13-CERTIFICATION-2026-10-10.md) | Independently executed full/minimal/stateless local fake-provider scenarios and separate footprint measurements. It also repaired one-shot prompt words being parsed as subcommands. Local conformance, declared native CI, cross-compilation, real native execution, live provider billing and independent security are different evidence classes. Its comparator requirement and broad “E4/E5 absent” text were superseded later. |
| [R14 taxonomy](../plans/AW-CLI-TAXONOMY-R14-2026-10-10.md) | One typed Clap schema normalized every long flag into semantic family/help categories while retaining original Arg IDs, shorts, environment names, static defaults, parsers/conflicts and feature cuts. Full command construction/parsing uses a bounded 32 MiB stack worker for the deep tree; small cuts parse directly. Initial hidden aliases/migration mapping were an intermediate state. |
| [R14.5 canonical-only](../plans/R14-5-CANONICAL-ONLY-R13-CLOSEOUT-2026-10-10.md) | Explicit user direction removed all legacy long aliases, compatibility switches, argv migration and comparator obligations. `aw cli schema` v2 publishes supported flags only. Canonical naming is breaking; source still 0.15.0 pending a separately authorized version/distribution decision. Short flags and ordinary typed permissions did not change. Historical migration tables are archives, not current guidance. |

The later P6 consolidation also supersedes two integration details in these early R records: all Host, MCP and Extension grants now use the exact source/epoch/Actor/site/effect predicate, with no name-only Host bypass; the Rust Extension trait has only `on_hook` (plus identity/stage interest), and the external process protocol is v2 with `prompt`, `tool.call`, `tool.result` hooks. The R7 v1 `before_prompt`/`before_tool`/`after_tool` descriptions are implementation history, not current accepted spellings or an alternate hook API. The separate presentation-only UI-extension protocol has its own current v2 semantic status/events contract; these protocols must not be conflated. Read the runtime section and [current Extension source](../../crates/aw-agent-core/src/extension.rs) / [process extension source](../../crates/aw-extensions/src/lib.rs) before implementing an extension.

Use current [CLI](CLI.md) and `aw cli schema` for actual invocations. Examples in old R/Phase plans commonly use intentionally removed `--profile`, `--model`, `--print`, `--no-state`, `--tools`, `--tool-root`, `--mode`, `--json` or RPC `--stdio`. Current examples use `--provider-profile`, `--model-id`, `--output-print`, `--state-memory-only`, `--tools-mode`, `--tools-root`, `--ui-mode`, `--output-json` and `rpc --agent-stdio`. Full RPC role flags follow the subcommand; minimal's root flags precede it. No compatibility implementation should be reconstructed from the old examples.

### Embedded proof boundaries and deferred expansion

[E4 native Linux](../plans/AGENTC-PARITY-E4-NATIVE-LIBC-FREE-2026-10-10.md) independently linked and executed the real shared fake Agent example as a Linux x86-64 libc/CRT-free ELF, with host-owned entrypoint, bounded allocator, compiler-memory primitives, panic policy and raw write/exit syscalls. The final image audit checked no loader, dynamic libraries/relocations or undefined symbols; negative expected-witness and panic-abort cases failed closed. This was a native Linux process requiring the kernel, not firmware. Its tiny fake-host image omits networking/UI and must not be compared to the complete CLI as a footprint claim.

[E5 C ABI](../plans/AGENTC-PARITY-E5-C-ABI-2026-10-10.md) added a narrow isolated unsafe FFI crate over the existing safe embedded/kernel crates. ABI v1 has 11 core exports, copied identity/config/callback data, exact array stride/version/size checks, caller-owned bounded answer/event buffers, dynamic exact grant checks, a disjoint BUSY reentry gate and explicit unresolved-effect destruction refusal. The independent strict-C11 consumer executed positive and negative callback/ABI/receipt cases in debug/release. That C consumer intentionally uses a normal hosted C runtime/staticlib wrapper; it is not the E4 libc-free image or a cross-platform audited ABI. The portable rlib installs no allocator/panic handler. Model callback failure after committed paid intent remains uncertain, including later assistant/turn receipt failure.

The accepted evidence hierarchy is therefore:

1. E1 core-only grant consumer: linked zero-import WASM permission logic.
2. E2/E3 shared no_std+alloc Agent: linked zero-import WASM and actually executed injected fake model/tool/journal turn, with hosted/embedded transition parity.
3. E4: separately audited and actually executed libc/CRT-free Linux x86-64 fake-host ELF.
4. E5: separately compiled actual C11 process using the portable versioned ABI; hosted C process uses libc.
5. E6: actual board/firmware is parked. No proof above supplies hardware boot, flash durability, real entropy, interrupts, physical memory/stack budgets, production transport or recovery certification.

The dated E/R documents often used installed Rust 1.95.0 while noting a then-unverified 1.98.1 pin. Those are historical toolchain-scoped results. Later accepted Nix/build evidence and current release checklist take precedence; do not turn the old missing-pin status into a fresh failure or infer a current successful target run from old receipts. Optional ARM/RISC-V/OS variants need an exact target/link/import/ABI/execution witness and explicit scope before any claim. Optional R2b external search needs measured benefit and an approved executable/root policy; Windows trusted-shell selection/Job Object cancellation remains unimplemented in the current embedded checklist.

### Native UX sequence: ownership and content before visual polish

The numbered UX phases explain why the current frontend uses its existing presenter, shared composer and renderer. Historical SQLite v32 references below record their original boundary; current SQLite is v33, introduced by Phase 67 schedules. Old mode commands, status defaults and popup rules must be read through their successors.

| Phase | Design that landed / later qualification |
| --- | --- |
| [51 native transcript](../plans/PHASE51-NATIVE-AGENT-UX.md) | Shared semantic prompt/assistant/thinking/tool/menu palette, explicit lossless presentation switching, compact slash menu, bounded sanitized output, no provider reasoning or execution changes. Its original inline/fullscreen ownership coupling and unboxed marker-only prompts were refined later. |
| [52 Markdown](../plans/PHASE52-NATIVE-MARKDOWN.md) | One bounded Unicode-cell-aware settled-assistant Markdown renderer, same painted and copy/search geometry, malformed/HTML literal fallback. Live partial text remains sanitized plain text. Tools/prompts/status are not Markdown. No network/file/URL execution or OSC-8. |
| [53 syntax](../plans/PHASE53-NATIVE-CODE-HIGHLIGHTING.md) | Shared lazy Syntect catalog/theme service for file/diff and settled fences, with embedded TOML/Nix grammar and bounded metadata-only language aliases. Workspace/full can highlight; minimal/no-Workspace keeps neutral code fallback and dependency isolation. |
| [54 modes](../plans/PHASE54-AGENT-PRESENTATION-MODES.md) | Separate Agent application from full Workspace, and Scrollback/Inline/Fullscreen physical Agent presentations. One presenter survives edit/menu/stream/settlement/resize; same Host, draft and authority survive explicit switch. Fullscreen Agent is not Workspace. The original full-primary Inline reservation model was replaced in post-58 hardening. |
| [55 lifecycle/chrome](../plans/PHASE55-TERMINAL-LIFECYCLE-AND-MINIMAL-CHROME.md) | One generation-safe idempotent TerminalSession owns raw/cursor/paste/mouse/alternate-screen/exit restoration, owner panic and catchable Unix shutdown; idle double Ctrl+C differs from live cancellation. Shared Full/Embedded Agent chrome reports only admitted resources. Numeric tab prefixes and redundant Explorer title removed without losing keyboard selection. |
| [56 async UI](../plans/PHASE56-ASYNC-UI-AND-SCROLLING.md) | Bounded shared human-UI executor and typed generation/cancellation services for Files/Git/highlight/layout/terminal launch; UI input and paint use cached data. Plain text appears before progressive syntax. One viewport model, fast local wheel/page motion, bounded EOF overscroll/scrolloff preserve real source-row identity. Agent/durable execution stays on its existing owner. |
| [57 natural flow](../plans/PHASE57-NATURAL-FLOW-AND-SCROLLBACK-EXIT.md) | Short Inline/Scrollback grows with actual content, then bottom-anchors after saturation; clean scratch collapse returns shell adjacent to committed output. Its one-above-composer default was temporary, superseded by two typed below-composer rows. |
| [58 saturation](../plans/PHASE58-SCROLLBACK-SATURATION.md) | Dynamic scratch rows replace a fixed large reservation. One persistent aw presenter may rebuild its internal Ratatui Inline viewport only for required row/physical-size changes, with sticky bottom anchoring after real saturation and no fake history. |
| [59 loaded resources](../plans/PHASE59-LOADED-RESOURCE-PROVENANCE.md) | Runtime-authored typed system/instruction/import/compaction/skill labels and byte metadata, not filesystem guessing. Header fingerprint invalidates relevant caches; Scrollback emits resources-only updates rather than rewriting old output. Saved flattened prompts cannot invent original instruction filenames/skills. |
| [60 explicit skills](../plans/PHASE60-EXPLICIT-SKILL-LOADER.md) | Named explicitly selected UTF-8 instruction resources, max 16 active / 64 KiB each / combined 128 KiB system envelope. Provider receives name/content, not local source path. No scripts, tool grants or auto-discovery. Later R11 adds a separately approved source-pinned project SKILL.md path. |
| [61 registry](../plans/PHASE61-PERSISTENT-SKILL-REGISTRY.md) | Persistent secret-free catalog and explicit add/list/show/remove; registered is not active. Exact one-run source form and named activation share admission; catalog edits do not mutate running/historical bindings. |
| [62 Solid parity](../plans/PHASE62-SOLID-AGENT-PARITY.md) | Active Solid root uses runtime-authored Context/Skills labels, canonical settings, compact hidden-tool evidence and safe thinking activity. Only authoritative available status values display; no invented TPS or browser-side permission. Retained historical workbench code is not an active second frontend. |
| [63 rich Markdown/media](../plans/PHASE63-RICH-MARKDOWN-MEDIA.md) | Tables/nested/task lists/strike and inert media references. Native remains bounded text without graphics or OSC-8; browser raw HTML stays off and image syntax becomes an explicit safe external link, never automatic `<img>` fetching. |
| 64/65 | Intentionally skipped filesystem-indexing/LSP work; no implementation is inferred. |
| [66 skill bundles](../plans/PHASE66-PORTABLE-SKILL-BUNDLES.md) | Deterministic bounded versioned JSON instruction bundle with SHA-256 integrity, content-addressed managed source, explicit registry install/update/export. Installed remains inactive; digest is not a signature or grant; no network marketplace/scripts or automatic garbage deletion. |
| [67 schedules](../plans/PHASE67-GOVERNED-PIPELINE-SCHEDULES.md) | SQLite v33 bounded interval occurrences over immutable active caller-owned user-scoped task pipeline binding. Generation/claimant fences and deterministic occurrence request IDs reuse the existing pipeline executor; run-due handles at most one. No cron, webhook, model-created schedule, Session scope or extra execution loop. |
| [68 release contract](../plans/PHASE68-RELEASE-HARDENING.md) | Exact five-target version/hash/size/format manifest, sidecars, independent audit and same aggregate through build/materialize/publish. Source phase completion is not hosted CI/native execution/signing/publication. Its installer candidate-execution rule was explicitly tightened by the next hardening pass. |

### Post-phase repairs that define the shipping model

The [first post-58 review](../plans/POST-PHASE58-REVIEW-2026-10-06.md) found and repaired owner-thread panic restoration, initial/manual layout pending/generation handling, stale recursive search cancellation, nonblocking progressive syntax publication, one in-flight rich transcript projection and closed-resource cache cleanup. It deliberately retained serialized Git mutations and bounded child-PTY control. Its proposed provenance, config and decomposition debt was subsequently addressed; do not reopen those as absent merely from this old list.

The [post-58 UX pass](../plans/POST-PHASE58-UX-HARDENING-2026-10-06.md) introduced shared Unicode editor movement/deletion/newline, full-width muted-blue prompt bands, compact tool states, top-rule work animation, runtime-only performance, independent bounded Agent/Workspace padding, exact tab paint/hit rectangles, Help Esc and resize/exit cleanup. Initial global hidden-session audit state was subsequently replaced by per-user archive semantics.

The [post-58 command/UX hardening](../plans/POST-PHASE58-UX-COMMAND-HARDENING-2026-10-07.md) established one slash registry and shared composer reducer, wrapping palette/history behavior, real typed reasoning effort, semantic System colors, auxiliary tool-free `/btw` and title generation with paid intent/settlement, one typed left/right status composition, and eligibility checks so tool-loop wall time cannot masquerade as model TPS. Current default rows are below-composer directory/Agent versus tools/state, then trustworthy current-process average TPS versus model/provider/effort, mirrored for Workspace fullscreen status. Restored histories do not acquire fabricated timing. Config persistence is bounded/coalescing with an explicit flush barrier.

That pass also refined Inline: normal CPR-capable terminals use a dynamically sized Ratatui Inline viewport, drop loaded history without re-emitting it, and commit only newly settled runtime overflow after actual saturation. CPR-less fallback is explicitly degraded and never claims an unsent history commit. Scrollback commits settled output once above dynamic transient scratch. No mode replays model/tool effects or rewrites old terminal-owned history.

The [fresh post-58 repair](../plans/POST-PHASE58-FRESH-REVIEW-HARDENING-2026-10-07.md) separated physical commit-log scanning cursor from authorized disclosure so filtered pages can advance without granting access, made multi-session archive/restore one authorized bounded transaction, fixed Workspace use of the shared composer/history, protected focused Ctrl+P/PTY ownership, made tmux's immediate capability result decisive, retained failed latest config state for explicit retry and normalized titles by both graphemes and UTF-8 durable byte bounds.

The [Workspace decomposition](../plans/POST-PHASE58-WORKSPACE-DECOMPOSITION-2026-10-07.md) kept one `Ui` state machine and split its implementation into façade, commands, interaction, async coordination and rendering. The child files extend the same type; they are not new controllers, queues, stores, registries or Agent engines. Preserve the shipping `Ui::draw`/renderer architecture and make later extractions only for a demonstrated ownership problem.

The [post-59 UX repair](../plans/POST-PHASE59-UX-HARDENING-2026-10-08.md) unified interactive mode grammar to `/tui scrollback|inline|fullscreen|workspace`, with `/workspace` shortcut and Workspace `/minimal` return preserving configured Agent presentation. Old `/inline`, `/scrollback`, `/fullscreen`, `/agent` slash entries were removed. Semantic bands gained one filled row above/below content; configured horizontal padding alone controls inset, so zero is zero. Active Solid CodeMirror gets readable plain foreground, cleared/stale-fenced syntax decoration, exact document/theme generation checks and top-left reset only on newly switched file identity; retained scoped views keep their own editor state.

The [post-68 correctness/release repair](../plans/POST-PHASE68-FRESH-REVIEW-HARDENING-2026-10-08.md) added durable process-incarnation recovery for schedule claims, exact existing-run reconciliation, reserved `sched:` admission, paused-claim fences and no surprise catch-up bursts. One locked `config::mutate`/three-way `save_changes` authority preserves unrelated concurrent edits and conflicts on same-field stale edits. Relative config path handling and bundle no-replace publication were fixed. Native audit probes scrub caller credentials; installers validate VERSION metadata/exact sidecars and do not execute a downloaded candidate before installation. Checksums remain integrity, not publisher identity.

### V1.5 design and remaining native/browser convergence

The [Herdr-inspired v1.5 spec](../plans/AW-UX-V1.5-HERDR-MINIMAL-VISUAL-LANGUAGE-2026-10-15.md) is additive to retained v1.4 information architecture, commands, agent hierarchy, accessibility and permissions. It asks for semantic roles across System/Light/Dark and 16/256/truecolor terminals, borderless primary canvas, one sidebar boundary and necessary split separators, one compact filled active-tab row, quiet inactive/overflow state, one context title, focus distinct from selection, muted-blue user bands, unboxed assistant prose and compact factual green/red/neutral tool states. UNKNOWN/unsettled cannot appear successful/free. No decorative boxes/toolbars, duplicate title/header, removed capability or weakened confirmation should be introduced to imitate a screenshot.

Its staged design sequence was inventory/tokens → shell/panels/tabs → shared transcript → product-wide convergence → actual native/browser visual review. Some shell/palette/focus slices have since shipped; the live [UX-convergence TODO](../plans/ux-convergence/TODO.md) remains open. The spec alone never authorizes another Agent, renderer, runtime primitive, Hook, new provider/tool authority or work in another owner's reserved subtree.

Current source supersedes one old design example: the shared palette is `>` Actions, `!` Windows, `@` Files, `#` Sessions, `$` Terminals, `/` Commands, `&` Agents, not `% Windows`. `src/workspace/palette.rs` reexports the same `tui::palette`; there is no parallel Workspace grammar. Empty-query Enter remains unarmed, selected identity is stable across async insertion, invalidation is inert, and cancel preserves exact draft/caret. F1/Ctrl+Shift+P and existing browser Ctrl/Cmd+K coexist with focused prompt history and child-PTY key ownership. Some live long-form docs retain explicitly historical paragraphs; source and the live checklist control these conflicts.

Current remaining work is to inventory actual shipping gaps, apply shared semantic roles/fallback contrast, finish restrained panels/tabs/overflow/titles and transcript/composer/status details, then converge Explorer/Git/Diff/terminals/Agent hierarchy/Sessions/Queue/activity/Settings/modal presentations. Test real 80×24/narrow and Unicode geometry, independent focus/selection, split/maximize, expand/collapse, modal cancellation, repeated resize, physical native frames/terminal restoration and shipping browser light/dark screenshots. Native buffer tests, actual PTY output, historical synthetic studies and production-browser acceptance are distinct evidence; none certifies another. Viewer/container/dock work stays under the live [workspace-views plan](../plans/workspace-views/TODO.md); docking and cross-client layout convergence are not supplied by visual polish.

### Live SDK, surfaces and browser authority contracts

The [SDK](SDK.md) keeps headless `aw-agent-core` independent of concrete HTTP, SQLite, filesystem and rendering. Explicit device Bridge/DeviceWorker adapters keep provider keys with the configured executor and SecretStore; an account label or workspace/machine lens cannot choose another payer. Provider-private signed continuation remains on its owning runtime, is bounded and same-turn/custody-bound, and never enters ordinary history, browser output or reasoning text. An unavailable delegated worker fails visibly; no local fallback. Authenticated participant controllers may have separate Agent instances, but reuse the original implementation and shared store owner; a roster label is not authentication.

[UI extensions](UI-EXTENSIONS.md) are a separate presentation-only contract: typed Manifest/Request/Response and bounded semantic Line/Span contributions in above/below/after-turn/fullscreen slots. The registry validates identity, declared slots, counts and safe text before rendering. It grants no model context, credentials, tool hooks or terminal handles. R7's process execution foundation does not mean a process-backed UI projection host, installable UI packages, stable Rust dylib ABI or arbitrary drawing has shipped. Status/performance formatting remains extension-owned; runtime records only factual eligible measurements.

[Native Workspace](WORKSPACE.md), [Git SDK](GIT.md), [terminal service](TERMINALS.md) and [surface SDK](SURFACES.md) separate human view resources from model authority. File/Git reads do not become provider context automatically; explicit same-root review feedback copies into the unsent composer. Git is a bounded trusted subprocess adapter with exact byte path/review identity, no credential prompts/hooks/external filters/lazy fetch, fresh exact-patch stage/unstage through the existing serialized owner and honest unknown outcomes. It grants no model Git tool, checkout/commit/fetch/push or mutate-all shortcut. External shells/editors remain independent concurrent writers.

A real PTY is retained independently of tabs. Closing/moving a view does not terminate or restart it; only explicit launch starts a configured shell, and hidden PTYs continue draining. Ordinary keys belong to the focused child except the small documented global set. Cells/styles are emulated/serialized; child/model escape streams never become host actions. Native launch/input are unsandboxed OS-user effects, not durable model-tool journal entries or security isolation. Sanitized startup environment does not protect same-UID credentials from deliberately trusted shell code. Stop/quit cannot promise to terminate disowned/escaped children or undo OS effects.

The [active browser](BROWSER-WORKSPACE.md) is compiled Solid, using the existing authenticated Rust owner APIs; historical advanced workbench modules remain reference/compatibility code, not another loaded root. Recent independent read-only Agent and Files/Git views bind exact Store/owner lifetime/Session/branch or surface-source/scope/inventory/Review identities. Selecting them never activates another Agent, retargets a private draft, reads another root or gains a tool grant. Draft text/caret survives an external target change while Send/private publication blocks until explicit target consent. Refresh rechecks authorization even at an unchanged head; denied/expired/closed/source-replaced views fence late replies and clear protected derived Find/selection/editor state. Cached File/Diff bytes remain explicit snapshots, not live file promises. Native and browser layout schemas/retention are separate; no shared docking/import/sync claim follows from equivalent controls.

[Web protocol](WEB-PROTOCOL.md) distinguishes ordinary numeric-loopback no-pair LocalWeb, explicitly paired/shared native-owner modes and named-participant team mode. LocalWeb can open Settings and browse authorized metadata before inference is configured; no model request or Session creation is implied. Exact Host/Origin/Fetch Metadata, strict JSON/body, cookie/authority and lock checks remain per mode. No browser-supplied actor/payer/root/program/environment/credential/fetch destination becomes authority. Provider OAuth is an explicit local setup action; tokens stay in SecretStore. This development loopback authority must not be treated as remote multiuser authentication through a proxy or wildcard bind.

Browser terminal grants are additional explicit owner authority, not a consequence of model tools or file access. The existing terminal surface uses confirmed launch IDs, owner-lifetime bounded receipts, expiring cooperative control leases and sequence-numbered exact-last-input deduplication. Queue acknowledgement is not command completion; ambiguous input is inspected and retried only exactly under the same valid lease, never silently replayed. Closing a view keeps the resource; logout/expiry/shutdown requests cleanup. The retained screen renderer/polling/keyboard subset does not certify full terminal/IME/graphics parity or multiuser OS isolation.

Configured Website surfaces are bounded external HTTPS frames with restrictive sandbox/permissions and no server fetch proxy, provider credentials, DOM extraction or agent control. Cross-origin page URL/history/form state is unverified. Local/remote development preview tunnelling needs separate mapping/trust/mixed-content design; a frame's localhost is the browser device. Browser theme bootstrap/CSP/code-editor nonces preserve same-origin script policy; presentation hints in localStorage are not a credential vault. Markdown media remains inert until explicit safe link activation.

### Explicit drift and supersession register

These are reconciliation points for this handoff, not fresh code-review findings or an instruction to rewrite all dated documents.

| Historical statement/source | Current disposition and authority |
| --- | --- |
| Minimal/master/early R roadmaps say Anthropic, Gemini, MCP data, extensions or complete embedded fake turn are missing/next. | Subsequent R3a/b, R5–R8 and accepted history implemented bounded slices. Preserve their explicit limitations rather than turning their old missing rows into new backlog. |
| R9 initial design automatically overlaps only four reads and serializes every Process. | The explicit model-directed follow-up and current [kernel](../../crates/aw-agent-kernel/src/turn.rs)/[Agent](../../crates/aw-agent-core/src/agent.rs) allow approved Read/trusted Bash waves selected per call, default16/max128 with host serial override; Write/Unknown/non-Bash Process remain barriers. |
| Early embedded plan uses E4/E5/E6 labels for prospective fixed-buffer/native/release work and says no complete no_std Agent exists. | Concrete R8/E3, E4 Linux native ELF and E5 C ABI implementations supersede those early progress labels. Current [embedded TODO](../plans/embedded-platforms/TODO.md) parks E6 board firmware and requires distinct target proofs. |
| R13 says the external reference executable is required and E4/E5 have not run. | Later canonical-only closeout removes comparator dependence and accepts separate local ELF/C proofs; native host/live vendor/security gates remain open for their own reasons. |
| Old R13/E-step receipts say a specific toolchain/native build is missing or an old connector cannot link/write Nix. | Those are dated platform/toolchain limits, not current failures. Later accepted build/checklist records outrank them; no current target run is inferred without its receipt. |
| R1 preserves legacy Host-name permits and R7 uses process ABI v1 plus before_/after_ callbacks. | Later P6 removed the Host-name bypass, unified Rust `on_hook`, and versioned external process hooks to ABI2 `prompt`/`tool.call`/`tool.result`; current source and the runtime handoff govern. UI projection protocol v2 is separately bounded and does not grant execution. |
| R14 initial design preserves hidden long aliases and proposes migration. | Explicit R14.5 user direction removed aliases and migration support. Current [normalizer](../../src/cli_taxonomy.rs) rejects long aliases and exports schema v2 supported names only. Do not reintroduce compatibility. |
| Phase51/54 first Inline models, Phase55 immediate bottom anchoring and Phase57 fixed scratch reservation. | Post58 normal Inline uses dynamic CPR-capable Inline ownership and real settled-runtime overflow only; Scrollback is dynamic and naturally saturates; Fullscreen is alternate-screen. Current [terminal contract](TERMINALS.md) and source supersede old geometry. |
| Phase57 and a retained paragraph in live CLI say default status is above composer. | Current [UiConfig defaults](../../crates/aw-providers/src/config.rs) and [UI extension contract](UI-EXTENSIONS.md) define two BelowComposer rows mirrored as FullscreenStatus rows; custom rows remain explicit and exact old defaults migrate. |
| Phase54 and retained historical live Workspace paragraphs expose separate `/inline`, `/scrollback`, `/fullscreen`, `/agent`. | Post59 unified `/tui` plus `/workspace`; Workspace `/minimal` preserves the configured Agent TUI mode. Old slash entries are removed. |
| Phase59 header reports no skills and forbids guessing original file provenance. | Phase60 explicit resources, Phase61 registry and Phase66 bundles add real skills without weakening provenance. R11 adds only separately approved source-pinned project discovery; registry/install is not activation. |
| Phase47/Herdr examples show `% Windows`; some live Workspace sections list six categories. | Current [shared palette](../../src/tui/palette.rs) is `> ! @ # $ / &`, including `! Windows` and `& Agents`; Workspace reexports it. |
| First post58 pass uses owner-wide session.hidden/restored audit state. | Later command/fresh-review hardening uses stable-viewer local per-user archive and one atomic bounded batch; legacy owner visibility is compatibility read state only, not current deletion/security authority. |
| Phase67 original schedule claims have no post-crash recovery/cadence refinement. | Post68 uses durable process incarnation fencing, exact existing-run linkage, reserved request IDs and at most one catch-up; pause/resume does not create burst backfill. |
| Phase68 installer validates by executing downloaded candidate. | Post68 installers verify release VERSION/exact checksum sidecar without pre-install execution. Publisher-side native audit intentionally executes under scrubbed environment. Neither supplies identity signing. |
| Retained GIT-WORKFLOW archive-era snippets broadly show commit/tag/bundle steps. | Current [working instructions](../AGENTS.md) require exact owned-path review, signed expected-parent commit and explicit separate authority for tag/push/publish/destruction; archival examples grant no permission. |
| Historical browser docs show four-group workbench/site/terminal/context modules as though they were the main frontend. | Active compiled Solid root and its newly implemented exact views are the product surface; retained modules and backend APIs do not prove all advanced UI is migrated. Browser Git mutation, shared docking and full PTY/site parity remain distinct unclaimed slices. |
| Old deferrals say schedules/autonomy or any richer browser behavior is wholly absent. | Phase67/post68 govern interval task occurrences and later Solid/read-only/container slices implement bounded behavior. No automatic broad event/cron/webhook scope, child-to-child disclosure, arbitrary topology/placement, automatic merge/import, monetary budget or richer account-reauth promise follows. Keep genuinely requested remaining work in current plans instead of transcribing old deferred lists as authoritative TODOs. |

### Release and externally gated work

The canonical release matrix in historical Phase 68 is Linux x86-64 and aarch64 musl/full, Windows x86-64 GNU and aarch64 gnullvm/minimal, macOS aarch64/full. Local/header/cross-build evidence does not certify native target execution. Additional macOS Intel or Linux RISC-V artifacts are optional target work; a six-host workflow declaration does not change the shipped five-asset contract or certify an unexecuted job.

The current [release-readiness TODO](../plans/release-readiness/TODO.md) accepts earlier five-target builds/local assembly/Linux fixture/macOS ad-hoc checks in their real scope and records subsequent exact Linux pinned product/native-package gates. It still leaves open remaining native asset-host execution, breaking CLI/version review and release authorization, Developer ID/Authenticode signing/notarization decision and execution, separately authorized configured live-provider/billing checks and independent security review. Source version remains 0.15.0, SQLite v33 and JSONL v1. Version 0.16.0 is a proposal, not an implemented or authorized release. Checksum agreement, local/ad-hoc signatures and product tests do not supply publisher identity, notarization, real vendor invoice compatibility or independent certification.

No old phase-specific smoke helper, manifest chain, source-check count or frozen comprehensive-handoff hash should be resurrected as the current runnable gate. The live [TODO specification](../plans/TODO-SPEC.md) defines bootstrap, formatting, locked Cargo workspace tests and the six named Node suites as the shared product gate. Use affected target/ABI/PTY/browser tests when actually changing those boundaries, record exact command/commit/platform scope, and keep absent external execution unchecked. This documentation-only review establishes coverage and reconciled intent; it does not upgrade historical source checks into new runtime results.

## 7. Post-acceptance implementation: native and browser views

The exact chronological Git list follows later in this section. The controlling
design and completed test records are the [Workspace README](../plans/workspace-views/README.md)
and [Workspace TODO](../plans/workspace-views/TODO.md). This section explains why
the changes exist and how their contracts fit together.

### Native read-only Agent views and Find lifetimes

`1c2ec6e` extracted strict exact-ID `SessionStore::read_branch_view` and a private
host-authorized reader. Activation still supports its existing ID-or-name behavior;
browsing must not mint a SessionHandle. The host retains one SQLite Store for
unsaved browsing, activation and the first prompt, preserving exclusive ownership.
Target identity is Store + exact Session + exact branch; names only label it.

Native layout v2 added bounded AgentView descriptors with explicit consistent v1
migration. Each read-only pin has independent scroll/follow/Find/selection and no
composer. Sessions `o` opens a read-only view; Enter remains activation. Reads are
bounded, lifetime-fenced and independently reauthorized even for unchanged heads.
Denied or revoked content is cleared from render, Find and copy-derived state.

`2809fa5` corrected asynchronous Find status: Searching lasts until an admitted
completion, rather than premature success/miss. `3b0dbb5` preserved committed Find
intent across changed/pending/missing content and inactive refresh, prevented n/N
flash selection from capturing later input, and used checked UI-lifetime IDs to
reject old close/restore replies. Real executor regressions failed before the fix
and passed afterward; native PTY checks were separate from TestBackend coverage.

`9fbb8a3` fixed a vault initialization lock lifetime exposed during this work:
release the initialization lock explicitly after durability work or error/unwind,
even when a duplicate open-file description survives. The maintenance lock remains
real. The precise original failing fork timing was not captured; deterministic
duplicate-descriptor controls establish the implemented correction.

### Native Files containers and physical roots

`6b96f01` retained Windows root/ancestor/descendant metadata handles that deny
delete sharing, preserving no-follow leaf, hardlink, cancellation and edit-CAS
rules. Cross-target compilation passed; actual Windows filesystem execution is
still open. Renaming/deleting pinned Windows directories is unavailable while
the corresponding handles live.

`3851dbf` added flat Files container membership, per-scope/path identity and
explicit folder open/reconnect on the existing executor. Human Files roots remain
independent of Agent presentation changes. Saved IDs/labels request authorization;
they never grant access. A moved File tab reconnects its own scope; launch reconnect
must preserve the canonical launch root. Unused capabilities are pruned.

Layout v3 migrates consistent v1/v2 without adding panes, changing geometry,
focus, legacy IDs or exhausted allocators; mixed/malformed versions fail before
installation. Atomic/no-follow and 64 KiB saved-layout protections remain. Async
read/list/search/highlight/Find work is bound to exact capability, owner, resource
and checked lifetime through close/reconnect/restore. Ordinary moves retain completed
view state. New previews follow their invoking container and keep Explorer focus.
Human review disclosure still uses the current Agent's exact canonical root.

### Native Git containers and immutable reviews

`491f366` retained physical checkout, worktree Git-directory and common-directory
identities, validating Git mappings and fresh index admission. External Git writers
can still race; this is not a filesystem/subprocess sandbox or exclusive external
writer claim.

`b33a418` added independent per-navigator Git contexts alongside Files scopes.
Layout v4 preserves old Files fields, structure/IDs/geometry and safe refusal of
unavailable allocation. Explicit open/reconnect retains exact context; saved paths
or names do not reopen a grant. Dormant Files/Git bindings survive navigator-kind
switching without retargeting other views.

Delayed reviews populate the origin container's remembered group without stealing
focus. A moved/open Diff retains its original immutable review and completed Find,
viewport and selection. Selected-index staging is an explicit fresh owner action;
reconnect/restore/final closure revoke old admission. Explicit terminal launch uses
the exact focused Git/Diff context and validates it on the executor before spawn;
unknown/revoked/replaced contexts deny. Files and Agent roots/draft/caret do not
change with human Git navigation. No uncertain effect is replayed.

### Browser Agent views and draft consent

`a0ffa3e` reused the same strict authorized transcript reader through authenticated
HTTP and retained one LocalWeb Store from startup. Browsing an unsaved Agent does
not create/activate a Session. Every view read checks exact Store, owner lifetime,
Session/branch, membership and local target; browser authentication alone supplies
no Store authority. Memory-only pins own independent Find/viewport/selection,
bounded work/cache, checked lifetimes and membership freshness/expiry. Closing,
source loss or denial clears content and labels before late replies can reinstall it.

Durable receipt lookup precedes lazy Session creation or account selection. Accepted
retries retain prior results without another Session/inference, including account
revocation/owner-replacement paths. Memory-only deduplication stays distinct.

`68629d8` bound private unsent text to minimal owner/Session/branch identity.
An external owner retarget preserves text/caret but disables Send and publication
until the user explicitly chooses **Use draft here**; that consent does not submit.
Confirmed local branch-open consent applies only to unchanged text/origin and its
exact canonical target, including matching SSE-before-HTTP. Quiet observations,
unrelated targets and new text grant no consent. Browser-lifetime fencing rejects
late navigation/reset UI replies and preserves text rebound during accepted sends.

`94697a7` moved the consent notice into composer flow with pointer events and long
label wrapping, making it reachable at 390 px. The production bundle was rebuilt
and actual Chrome checked the same composer DOM node, exact draft/caret, independent
branch state and revocation. Simulated transport expiry was labelled separately
from unexecuted live paired/shared authentication scenarios.

### Browser scoped Files/Git: retained capability, bounded model, stable editors

`57a6059` completed the shipping Solid Explorer and tab-strip slice. It does not
make the historical `web/workspace-model.js` reducer the shipping layout owner.
Browser descriptors remain memory-only; sharing native serialized layout and
docking is deferred.

**Backend contract.** SurfaceHost retains one launch `WorkspaceFiles` and physical
directory binding. Legacy and scoped Files reads reject pathname replacement.
Discovery retains the nested `{enabled, instance, capabilities: {...}}` shape;
capabilities add random owner `source_id` and opaque `view_root` under schema 1.
The strict additive read wrapper is:

```json
{"op":"view","source_id":"OWNER","scope_id":"SCOPE","request":{"op":"file","path":"relative.txt"}}
```

Replies carry exactly `{source_id, scope_id, result}`. Nested operations are only
`files`, `file_search`, `file`, `git_list`, `git_open`, and `review`; no nested
terminal/mutation/feedback action or arbitrary root/context field is admitted.
Existing flat SDK commands remain compatible. A launch scope grants Files/Git;
selecting a physically verified existing worktree grants Git only. Additional
browser Files roots require a future explicit trusted-owner grant contract.

Git inventories own opaque IDs, opens name the original inventory and item,
and review IDs remain scoped to their exact repository. Captured worktree directory
identity rejects a replacement checkout even with a copied `.git` pointer. Scope
IDs never silently retarget a new physical repository. Sixteen contexts, twelve
FIFO inventories and eight FIFO reviews bound server state. Expired handles demand
Refresh. Full encoded 4 MiB replies are checked before context insertion or cache
eviction, so an oversized new read cannot destroy older admitted handles.

Typed failures distinguish `source_changed` (403, all workspace views),
`scope_denied` (403, confirmed invalid scope), `read_denied` (403, individual
resource) and `refresh_required` (409, expired review/inventory). Echoed IDs are
bounded metadata, never grants. Four physical blocking HTTP slots remain held
through actual read/encoding work, with authorization after queueing and again
after completion. Legacy TerminalStart resolves an accepted receipt first, then
validates physical root/repository before any new receipt or spawn.

**Pure controller.** Eight combined Files/Git containers, 32 total document tabs
including the permanent Agent and its pins, four physical outstanding reads even
when abort is ignored, 4 MiB replies, 8 MiB retained Files/Git content, 64 KiB
navigator metadata and 128 cached directories are explicit bounds. Agent pins have
their separate eight-pin/cache rules. At capacity, preserve admitted authorized
state and refuse new work visibly rather than silently evicting another view.

File identity includes source/scope/path and container; same path in two containers
has distinct view IDs, while a duplicate within one container is reused. A Diff
keeps its original scope and immutable Review. Allocate a pending destination
before awaiting so late replies fill only that tab and never select another.
Navigator query, expanded/collapsed directories, Tree/List, Git mode and worktree
selection remain independent. Ordinary Refresh preserves compatible navigator
state; changing worktree resets its navigator without retargeting existing Diffs.

Files/Diffs are explicit snapshots with Refresh; they do not inherit Agent pins'
fifteen-second membership freshness behavior or automatic polling. Close/source/
authentication/confirmed scope denial scrubs protected state before subscribers
and abort callbacks run. Individual resource denial preserves unrelated snapshots.

**Shipping UI.** Stable primitive row keys preserve the clicked Explorer node and
focus. Model change reconciliation is untracked in Solid, preventing accidental
active-tab subscriptions from rereading Git. Palette search pins its origin and
fences close/refresh/requery replies; visible-navigator focus avoids a retained
hidden Sessions input. Labels/tooltips retain original checkout/path/basis and
view ordinal even when a visible tab displays only a basename.

Mounted File/Diff components retain their own CodeMirror EditorViews, selection,
Find state and viewport. Find is bounded to 512 query characters and 4,096 hits.
Hidden document DOM selection is cleared without discarding the editor's saved
selection. Modal ownership prevents global Find from stealing dialog input.
Denied/closed editors receive an empty EditorState before destruction; Shiki
lifetime/path fencing prevents a late highlight from retaining or restoring old
plaintext.

`0159f0b` fixed a real browser-discovered Merge sizing regression: CodeMirror's
merge CSS forced `height:auto !important`, producing clipped non-scrollable Diff
editors. The narrow `.code-diff .cm-mergeViewEditor > .cm-editor` height override
restores bounded independent scrolling on both sides at desktop and 390 px.
The earlier `57a6059` full gate/package was cancelled after this finding; partial
results there were never called a full pass. Final exact gates reran on `0159f0b`.

### Other post-acceptance changes and exact commit sequence

Configuration/state default paths now use `abird/aw` under XDG config/state roots.
`4203780` introduced that namespace and `1effd14` removed the temporary legacy
`aw` fallback. Existing explicit config/state path options remain the mechanism
for using another location; no silent lookup/migration of old paths is implied.
See `crates/aw-providers/src/config.rs`, `src/sessions.rs` and
`src/minimal_sessions.rs`.

CI (`aecebbc`) now has pinned Nix product-gate and separate native build/version
jobs. The current workflow is `.github/workflows/ci.yml`; inspect that file instead
of old assembly/validation workflow instructions. A former GitHub account billing/
spending block led the user to request local completion/emulation; local successful
commands do not certify a hosted run or erase native-host/release gates. No current
GitHub billing status has been checked for this handoff.

| Commit | Landed change |
| --- | --- |
| `43ca8bacec8cd5d1db01ec49d085dcebb2b502b1` | docs(plans): repoint accepted provenance to the rebuilt checkpoint |
| `119374ec08cdebe884d63578abafb1b5d0773bcb` | docs: update license attribution and README layout |
| `aecebbc8fe6239280f53eee28f64cc67043e1775` | ci: add single-gate and product build workflow |
| `7fc9c82aeda82a94c1cd52dfec4740af4d91e723` | docs(agents): document plan layout and TODO spec |
| `1fcc2f6ff7e7fcb54fa9ee7b9d15d60e0aa1f1ae` | docs(contributing): align guide and AI disclosure |
| `5a8e90ae5000fdf5edeaea3fb90be21fec086de6` | docs(agents): gate pushes and destructive actions |
| `42037806e784273ca24a97ff44c8bdd67a3b4e99` | refactor(paths): namespace user files under abird/aw |
| `1c2ec6eff437f506fd2ea404ed6dcc3cf66c22a3` | feat(workspace): add read-only Agent views |
| `1effd149245f86fe5435ef4f8b469032508990fc` | refactor(paths): drop the legacy aw XDG fallback |
| `9fbb8a3456b717c810c901460817ba1cc2317d59` | fix(store): release completed vault init locks |
| `2809fa58902df4dd9a23fb5a9a8fbd5e57d43189` | fix(workspace): settle asynchronous find status |
| `3b0dbb502005570cca30e2cd52cba60c052f3587` | fix(workspace): preserve Find lifetimes |
| `8d905c4bbe807c5cd47c59e91bdfd012afdd4d70` | docs(plans): record native view checkpoint |
| `6b96f010995ffa49414486ae63234a90ec9102e6` | fix(windows): pin scoped directory bindings |
| `3851dbffd808491916e07a3642422bd0f7b465b0` | feat(workspace): add scoped Files containers |
| `635910ce6d0681e2f88a840c79c543bf75bb00bb` | docs(plans): record scoped Files checkpoint |
| `491f3665b4de7f9fce3cfb4b4a14066d80e205d2` | fix(git): reject replaced repositories |
| `b33a4183f6032a024d1b73409d2c9134ea0aab24` | feat(workspace): add scoped Git containers |
| `12e16c4534b3887081e7a1fdc28bc0eb3d50096c` | docs(plans): record scoped Git checkpoint |
| `a0ffa3ec9e3fb3d3c713d8cea1a8c694543d002e` | feat(web): add read-only Agent views |
| `68629d8ddcdba4bb502ef21ea04d5bae006f5fdd` | fix(web): bind drafts to their target |
| `94697a7899ae52eb11167519c9abc05ee3517756` | fix(web): make draft consent reachable |
| `49c74196bd16bd61ad5760d685a7f5f94d57e23c` | docs(web): close Agent view checkpoint |
| `57a6059a1d401186ffdf1ecbd4080645290a3063` | feat(web): add scoped Files and Git views |
| `0159f0b119779f74b22d93d2ecd552306bbd7584` | fix(web): constrain merged diff editors |
| `04bb9e000ce0d7406972bf564bb9a0c744fe9698` | docs(web): close scoped view checkpoint |

## 8. Executed checks and precise completion boundary

The canonical [Workspace TODO](../plans/workspace-views/TODO.md) records each
native/browser implementation and its executed checks. [Accepted history](../plans/accepted-history/TODO.md)
covers older work at the user-approved checkpoint; retain its limits. The latest
full product candidate is `0159f0b119779f74b22d93d2ecd552306bbd7584`.

| Check | Recorded result on the exact final product candidate |
| --- | --- |
| Pinned bootstrap and format | `bash scripts/bootstrap.sh` and `cargo fmt --all -- --check` passed. |
| Workspace Rust gate | `cargo test --workspace --locked`: 1,662 passed, 2 existing ignored, zero failures. |
| Browser model gate | Existing six Node suites: 147 passed, zero failed/cancelled/skipped. |
| Web-only boundary | `cargo test --locked -p aw-cli --no-default-features --features web --lib`: 132 passed. |
| Workspace-only boundary | `cargo check --locked -p aw-cli --bin aw --no-default-features --features workspace` passed; two existing dead-code warnings remained. |
| Production frontend | Signed source rebuilt with pinned Node/Vite into a private output; all eight artifact names and bytes matched committed `web/dist`. |
| Immutable native Nix package | Exact-revision `#aw` build passed; `bin/aw --version` printed `aw 0.15.0` and exited 0. |
| Shipping assets | JS/CSS served by the immutable executable matched committed product bytes, with no frontend asset/CSS overrides. |
| Actual Rust HTTP | Real source/scope/inventory/review and strict-input/root/grant refusals passed against the private owner. |
| Actual Chrome 155 | Duplicate Files and two Git worktrees retained independent 160-hit Find, selections and 1,100/1,750-pixel scroll, stable EditorViews/Explorer focus and unchanged Agent/composer/draft/caret; switching dispatched zero extra workspace reads. |
| Mobile and limits | Both Diff sides scrolled at 390 px, Find controls fit; ninth container/33rd total tab refused before dispatch, existing documents stayed usable and a closed slot was reusable. |
| Late replies and disclosure | Held actual File reply could not reopen a closed tab; physical root replacement disclosed no new bytes and cleared both retained File editors/Find/DOM selection/protected labels. |
| Transport simulations | Held actual discovery after simulated `source_changed` required fresh explicit discovery; simulated 401 plus held discovery/old SSE could not revive DOM. This is not live paired-cookie expiry proof. |
| Fixture invariants and shutdown | Exact Git HEADs and raw/logical indexes unchanged; sessions/runs/run_events/invocation_receipts/pipeline_runs/team_runs all zero. Targeted browser closed; owner exited 0; synthetic fixture removed. |

The product package derivation was
`/nix/store/95fmgk3cdijir0zn8a20ciz79zqy5cg5-aw-0.15.0.drv`, output
`/nix/store/i10xv18yshdrpklmxv9n4kvqld4fgz3j-aw-0.15.0`.
The documentation-only `04bb9e0` checkpoint evaluated to the identical derivation.
These store paths identify that local build, not permanent artifacts required by
a future agent. Rebuild the exact source if they have been garbage collected.

The latest aggregate count does not newly execute actual Windows/macOS/ARM hosts,
live providers/OAuth/billing, firmware, hostile-network tests, signer identity,
release publication or runnable restored-State authority. Native Linux PTY,
TestBackend, browser execution, target compilation and independent security review
are different results; no one is a substitute for another.

### Reproduce the current gate

Use the repository's pinned dev shell so Rust, Node, `AW_GIT`, `AW_BASH` and the
WASM linker are the intended tools. Bootstrap only resolves/prepares locked
dependencies; it starts no provider or service. Keep the real `Cargo.lock` and
`web/package-lock.json`, and use `--locked`/no lock updates for verification.

```sh
nix develop --no-update-lock-file
bash scripts/bootstrap.sh
cargo fmt --all -- --check
cargo test --workspace --locked
node --test tests/web_model.test.mjs tests/web_workspace.test.mjs \
  tests/web_context_flow.test.mjs tests/web_multiplayer.test.mjs \
  tests/web_agent_parity.test.mjs tests/web_rich_markdown.test.mjs
```

Feature-boundary checks follow the changed seam, rather than claiming every cut
from a default build. Native package/version:

```sh
nix build .#aw --no-link --print-out-paths --no-update-lock-file
```

For exact immutable proof, use the reviewed revision in a Git flake URL as in the
plan, then run the resulting `bin/aw --version` and acceptance on that executable.
Changing `web/src` also requires `npm --prefix web run build` with the pinned
toolchain and a review of committed dist; use a private `--outDir` for byte
reproduction. Do not leave generated diagnostics or fixture State in the repo.

For actual UI acceptance, use private synthetic State/config/roots, tools off and
no paid provider calls. Inspect baseline IDs, branch heads, raw/logical indexes
and execution counts; verify them afterward. Drive the shipping renderer/browser
against the real owner, include delayed replies/denial/resize and preserve actual
draft/caret/focus. Stop only your exact owner and targeted browser, confirm exit
and terminal restoration where relevant, then remove your exact marked fixture.
Earlier private fixture paths and browser session names are gone; recreate them.

## 9. All current pending work, including deferred and external gates

This is a captured reading of the live TODOs, not a second task owner. Follow
their links and current checkboxes for updates. Historical unfinished classes map
here; do not reopen accepted work merely because an old report has an unchecked
box, and do not close external gates on historical acceptance alone.

### Read-only Agent views and composable Files/Git

Active. Canonical checklist: [workspace-views/TODO.md](../plans/workspace-views/TODO.md).

- **Pending · Uncommitted:** Add docking last with explicit cross-window ownership, reparenting, origin close and focus/lifetime transfer; moving never starts/clones resources.
- **Pending · Uncommitted:** Evaluate left-right publication only at measured read-heavy shared ownership seams; benchmark before migrating locks and drop guards before render/I/O/await.
- **Pending · Tests and verification:** Further native narrow/maximize, modal-focus and child-PTY acceptance, plus docking-specific checks when implemented — remain open beyond this checkpoint.
- **Pending · Tests and verification:** Windows scoped-directory and DirectoryIdentity filesystem tests — execute on an actual Windows host; reparse-point case requires symlink privilege or Developer Mode. Compilation does not supply this runtime result.
- **Pending · Deferred:** Remote views — remain unavailable until exact machine/member/revision/expiry/offline tests exist; no local fallback or execution retargeting.

### Remaining native and browser UX convergence

Not started as a separate plan. Canonical checklist: [ux-convergence/TODO.md](../plans/ux-convergence/TODO.md).

- **Pending · Uncommitted:** Inventory remaining gaps from actual shipping views; keep accepted behavior/capability differences and mark unavailable resources honestly rather than inventing parity.
- **Pending · Uncommitted:** Apply shared semantic roles across native/browser with System/Light/Dark and 16/256/truecolor fallback: readable primary/secondary text, independent focus/selection and factual status words/icons, not color alone.
- **Pending · Uncommitted:** Complete restrained chrome where gaps remain: borderless primary surfaces, one sidebar boundary, necessary split separators, single-row compact filled active tabs, quiet inactive tabs/overflow and one context-bearing title.
- **Pending · Uncommitted:** Preserve user prompt emphasis, unboxed assistant prose and one compact tool block: success green, failure/denial/interruption red, running/UNKNOWN neutral or attention; unsettled outcomes must never appear successful/free.
- **Pending · Uncommitted:** Preserve composer/menu/multiline/cancellation behavior, compact factual status rows and all security/source/process metadata; no extra decorative toolbar or nested box to fill space.
- **Pending · Uncommitted:** Converge remaining Explorer/Git/Diff/terminals/Agent hierarchy/Sessions/Queue/activity/Settings/modal presentations without removing commands or weakening destructive/permission confirmations.
- **Pending · Uncommitted:** Validate keyboard/mouse geometry and focus across narrow 80×24, Unicode/graphemes, overflow, split/maximize, expand/collapse, modal cancellation and repeated resize; inspect real native frames/PTY restoration and shipping browser light/dark screenshots.
- **Pending · Uncommitted:** Implement viewers/containers/docks only through [workspace views](../plans/workspace-views/TODO.md), preserving resource identity, authority, pending work and exact focus ownership.

### Release readiness

Active; external gates open. Canonical checklist: [release-readiness/TODO.md](../plans/release-readiness/TODO.md).

- **Pending · Uncommitted:** Complete release-asset execution on remaining target hosts, locally or with hosted jobs when available; Linux emulation cannot certify missing macOS/Windows/ARM hosts.
- **Pending · Uncommitted:** Review breaking CLI/version surfaces, agree any version change and obtain release authorization before distribution; 0.16.0 remains a proposal.
- **Pending · Uncommitted:** Decide/configure Developer ID/Authenticode signing and notarization requirements; ad-hoc signatures and checksums do not supply those identities.
- **Pending · Uncommitted:** Run live-provider/billing compatibility checks only when separately authorized and configured.
- **Pending · Uncommitted:** Obtain an independent security review; product tests do not supply independent certification.
- **Pending · Tests and verification:** Release-asset native execution — exact assets and remaining hosts; unfinished external gate.
- **Pending · Tests and verification:** Version/release approval — explicit owner decision; unfinished external gate.
- **Pending · Tests and verification:** Identity signing/notarization — actual configured identities and execution; unfinished external gate.
- **Pending · Tests and verification:** Live-provider/billing checks — separately authorized real execution; unfinished external gate.
- **Pending · Tests and verification:** Independent security review — unfinished external gate.

### Optional embedded targets and independent certification

Parked optional work. Canonical checklist: [embedded-platforms/TODO.md](../plans/embedded-platforms/TODO.md).

- **Pending · Uncommitted:** E6 firmware: choose actual board/target, allocator/memory/stack budgets, clock/interrupt/transport capability boundaries and a minimal witness before implementation; prove real hardware boot/execution/recovery rather than substituting desktop tests.
- **Pending · Uncommitted:** RISC-V or additional E4/E5 OS/architecture variants remain optional; specify linker/ABI/import/export and execution witnesses per target before claiming portability.
- **Pending · Uncommitted:** Optional R2b: consider external rg/fd acceleration only after measured need and explicit safe executable/root policy; Windows trusted-shell selection/Job Object cancellation remains unimplemented and separately gated by native authority/cancellation tests. No new default shell permission.
- **Pending · Uncommitted:** Preserve one Agent/TurnMachine/ToolPolicy/journal and four Runtime primitives/two Hook verbs; no new execution authority, autonomous replay, implicit listener or hidden indexer from an embedded adapter.
- **Pending · Uncommitted:** Independent hostile-network/security or live-account billing certification, if required, needs a separately approved scope, real evidence and privacy/custody controls; fake providers and local paired transports cannot supply it.

### Blocked restored-State authority prerequisites

Blocked, design-only. Canonical checklist: [restored-state-authority/TODO.md](../plans/restored-state-authority/TODO.md).

- **Pending · Uncommitted:** Verify every unknown provider attempt against provider-owned usage/invoice or an independently verified human accounting disposition for its original payer/account/credential. Missing evidence stays possibly charged/unresolved, never zero-cost.
- **Pending · Uncommitted:** Reconcile original Tool/OS effects from exact host/Process/site/intent/result records. Failed/interrupted signals do not prove the process stopped; unverifiable effects remain unresolved/abandoned for a fresh proposal, not successfully executed/rolled back.
- **Pending · Uncommitted:** Reconcile Pending/Claimed/Accepted Queue and Actor Input/Control admissions individually against original Session/branch/writer facts. No old grants/admissions automatically survive and no historical Queue item auto-dispatches.
- **Pending · Uncommitted:** Establish independent live-human presence and revocation: trusted-origin, single-use short-lived challenge bound to source Store/archive/effect digest, target Host/site, restored generation, fresh scopes and coordinator term. A verified user-presence/user-verification authenticator is required; copied signatures, local identity/model text and prior review hashes are insufficient. Never copy private authenticator credentials into State, model or audit.
- **Pending · Uncommitted:** Provide an independent durable, linearizable single-winner coordinator CAS for original Store/archive across clones and old owner; monotonic terms survive crashes/restart/lease expiry/replay. Partition/unreachable coordination denies activation/effects; a local lock/TTL is not a global fence.
- **Pending · Uncommitted:** Only after independent proofs and fresh authorization, design crash-safe transfer: inert target plus durable coordinator pending record, fresh owner/writer/Actor/Process generations and original CAS/fsync boundaries. Every uncertain outcome remains UNKNOWN; inspect the source of truth instead of retrying the side effect.
- **Pending · Uncommitted:** Prove first-open zero old provider/tool/OS effects, zero historical Queue claims/unsolicited Actor admissions and no duplicate charge at every crash/reconnect boundary; all new effects need fresh human proposals, unique intent IDs, current credentials/exact grants and the original journal handshake.
- **Pending · Uncommitted:** Require real competing-host/partition/quorum/rollback/revocation and authenticator lifecycle tests, plus copies of old consent/dry-run/review receipts rejected as bearer grants. Native Linux/Mac/Windows evidence is separate from optional E6.
- **Pending · Uncommitted:** Keep user-visible progression factual: Quarantined → effects need inspection → review requested → independent evidence missing → eligible for new proposal review. No Ready/Resume/Activate control before its conditions truly hold; UNKNOWN is never green.
- **Pending · Uncommitted:** If the threat model requires it, separately analyze fd-bound filesystem/SQL snapshots against privileged mounts/root; current inode anchoring is not that certification.
- **Pending · Uncommitted:** Keep sensitive v1 composite/SQLite Tool outputs/prompts, ciphertext and master-key custody private. Generic ToolOutput encryption/erasure/rotation, credential-neutral publication and cloud/retention-law certification remain unimplemented; do not infer them from internal Linux backup tests.


### Next local implementation sequence and stop conditions

1. **Measure the left-right candidates.** Reuse
   `crates/aw-agent-core/src/publication.rs` rather than another publication layer.
   Begin with immutable browser Session display snapshots in `src/web.rs` and the
   bounded 128-row presence projection in `src/web/team/multiplayer.rs`. Separate
   display publication from receipt/cancel/run/admission state. Preserve fresh
   context/account/Store authority, sequence ordering, generation and TTL filtering.
2. Compare the existing lock-based owner with publication using actual read-heavy
   and write-heavy ratios, long-lived readers and cold registration. Proposed
   benchmark matrix: writes:reads 1:4, 1:16 and 4:1; 1/4/8 consumers; snapshots
   32 KiB/256 KiB/4 MiB; presence 8/128 rows. Record p50/p95 reader and publisher
   latency, CPU, allocation and peak retained bytes while owned Arcs are held.
   This matrix is a proposal, not a completed benchmark or a performance claim.
3. Register one reader per long-lived consumer. `Reader` is `Send` for appropriate
   data and deliberately `!Sync`; per-request registration takes an upstream lock
   and may erase gains. Do not hide one Reader behind a shared Mutex. Guards
   remain inside the helper and return owned snapshots before render/I/O/await.
   Keep bounded versions and no unpublished backlog; test panic/close/error paths.
4. Migrate only a candidate with measured end-to-end improvement and preserved
   authority/resource/ordering behavior. A measured decision to keep the simpler
   owner is a valid outcome. Existing production RwLock uses were already absent
   in the previous scout; recheck the current source instead of promising removals.
   Admission, receipt, SQLite/CAS, PTY/parser/input, lease/queue/resource locks and
   single-owner caches retain their current design unless separate measurements
   and invariants justify change.
5. Run independent UX inventory and remaining local native acceptance in parallel
   with disjoint ownership. Do not take over the reserved UX subtree. Docking comes
   last: define cross-window ownership, reparenting, origin close and focus/lifetime
   transfer before implementation; moving a view cannot start/clone its resource.

No remaining release/provider/signing/restoration gate is implicitly authorized by
this sequence or by the handoff request.

## 10. Source map for implementation and review

| Concern | Primary source locations |
| --- | --- |
| Typed CLI and entry points | `Cargo.toml`, `src/main.rs`, `src/lib.rs`, `src/cli_taxonomy.rs`, `src/resource_cli_args.rs`, `src/model_cli.rs`; source-derived `aw cli schema`. |
| Portable turn transitions | `crates/aw-agent-kernel/`; no UI, database, process or hosted-network ownership. |
| Original hosted Agent and publication | `crates/aw-agent-core/`, especially `src/publication.rs`; account readers remain independently authorized. |
| Providers and config custody | `crates/aw-providers/`, `crates/aw-inference/`, `src/device*`, `src/config_persistence.rs`; SecretStore and configured executor contracts. |
| Store abstraction and durable facts | `crates/aw-store-api/`, `crates/aw-store/`, `crates/aw-store/migrations/`, `crates/aw-store-jsonl/`, `crates/aw-session-format/`, `src/sessions.rs`, `src/minimal_sessions.rs`. |
| Authorized transcript presentation | `src/sessions/branch_view.rs`, `src/workspace/branch_view.rs`, `src/workspace/agent_view.rs`, `src/workspace/ui/agent_views.rs`, `web/src/agent-views.js`. |
| Native layout and renderer | `crates/aw-workspace/`, `src/workspace/ui.rs` and child `ui/{commands,interaction,async_work,render,files_views,git_views}.rs`, `src/workspace/persistence.rs`, `src/workspace/palette.rs`. Reuse shipping `Ui::draw`; no second UI authority. |
| Physical Files and platform identities | `crates/aw-platform/`, `crates/aw-platform-unix/`, `crates/aw-platform-win/`, `src/workspace/files.rs`. |
| Git and terminal resource owners | `crates/aw-git/`, `crates/aw-terminal/`, `src/workspace/git.rs`, `src/workspace/terminals.rs`; exact fresh owner admission for explicit effects. |
| SurfaceHost and browser HTTP | `crates/aw-surfaces/src/{contract,lib,views}.rs`, `src/web.rs`, `src/web/workspace.rs`, `src/web/team/multiplayer.rs`. |
| Shipping browser state/editors | `web/src/main.jsx`, `web/src/api.js`, `web/src/workspace-views.js`, `web/src/agent-views.js`, `web/src/code-surface.jsx`, `web/src/styles.css`, `web/dist/`. Historical `web/workspace*.js` is not a substitute for the shipping Solid owner. |
| Team, identity, context and distributed services | `crates/aw-identity/`, `crates/aw-collab/`, `crates/aw-client/`, `src/awnet/`, `src/web/team/`; read exact live Team/Domain/AwNet/context contracts first. |
| Routing/pipelines/extensions/tools | `crates/aw-routing/`, `crates/aw-pipeline/`, `crates/aw-extensions/`, `crates/aw-mcp/`, `crates/aw-tools/`, `crates/aw-ui-extension/`, `crates/aw-editor/`, `src/model_*`; compose the original engine and policy. |
| Embedded/C hosts | `crates/aw-embedded/`, `crates/aw-embedded-ffi/`, `examples/sdk-kernel/`, `examples/sdk-embedded/`, `examples/sdk-embedded-native/`, `examples/sdk-embedded-ffi-staticlib/` and their README contracts. |
| Builds, release and CI | `flake.nix`, `nix/{source,native,cross,release,dev}.nix`, `scripts/bootstrap.sh`, `scripts/build-release-artifacts.sh`, `.github/workflows/ci.yml`. |
| Tests | Crate unit/integration tests, `tests/`, native UI/executor/TestBackend tests and private actual PTY/browser acceptance. Each proves its stated layer only. |

## 11. Retained document map and historical-reference limits

The tables below cover all non-reserved `.agents/docs/*.md`, top-level retained
`.agents/plans/*.md`, and canonical plan checklists/design files present at the
handoff checkpoint. Original documents stay in place: their exact detailed
contracts and dated decisions are preserved rather than replaced by this summary.
The reserved `.agents/plans/ux/` is intentionally neither inventoried nor read.

Historical plan dates, version numbers, passed-test counts, tags, absent helpers,
old source trees and full-parity claims do not override current source/checklists.
Some retained files still contain links to removed comprehensive handoffs or
validation outputs. Follow the live contract and current TODO; do not recreate
an archive or claim an absent command ran. Old `GIT-WORKFLOW.md` phase-delivery,
tagging and validation examples are historical context, not current authorization.

The rebuilt Git history is progressive source/crate/docs commits ending in the
accepted `47ee8a0` tree and later ordinary commits. The current checkout had no
local tags when surveyed; this says nothing about off-repo historical tag custody.
Dated hashes may not resolve here. Instructions mention an off-repo pre-rewrite
bundle, but the current index does not name its concrete path. Do not invent that
location or promise old objects are locally available; ask the owner for the
preserved bundle if exact removed history is needed. No absent early phase record,
uncommitted experiment or verbatim conversation has been reconstructed for this
handoff. Existing retained documents and current source provide the initial design
and implementation progression described above.

Coverage at preparation: **193 documents/instruction files**, including this handoff. Every retained non-reserved documentation/plan path is represented below; current task checkboxes are preserved in their canonical files.

### Operating instructions and entry points

| Document | Context and decision content | Pending/status interpretation |
| --- | --- | --- |
| [AGENTS.md](../AGENTS.md) | Binding working instructions, reserved subtree ownership, exact authority/version/cfg boundaries and signed expected-parent Git safety. | Read first; no implicit publication/live effect/restoration authority, no reserved subtree inspection. |
| [plans/DOCUMENT-INDEX.md](../plans/DOCUMENT-INDEX.md) | Document entry point to canonical index and start guide. | Complete retained-corpus coverage is linked from the comprehensive handoff; no removed archive required. |
| [plans/README.md](../plans/README.md) | Planning entry points: canonical TODO index/start guide and live .agents/docs. | Current pointer; detailed original history mapped in this handoff and retained files. |
| [plans/ROADMAP.md](../plans/ROADMAP.md) | Roadmap entry point to canonical TODO index/start guide. | Current task state is in plan checklists, not a duplicate roadmap ledger. |
| [plans/START_HERE.md](../plans/START_HERE.md) | Startup read order, expected ownership, agent scratch, exact local product gate and comprehensive onboarding link. | Current guidance; source execution, native hosts and live/external gates remain separate. |
| [plans/TODO-SPEC.md](../plans/TODO-SPEC.md) | Flat plan layout/authorship, exact command/commit checks, atomic coordinator reconciliation and one runnable product gate. | Binding current process; one requested handoff adds no mandatory archive or manifest chain. |
| [plans/TODO.md](../plans/TODO.md) | Seven canonical plan links with accepted/completed/active/pending status. | Single current plan index; context snapshot cannot close unchecked external or future work. |

### Live documentation and this handoff

| Document | Context and decision content | Pending/status interpretation |
| --- | --- | --- |
| [docs/ARCHITECTURE.md](ARCHITECTURE.md) | One source-derived CLI and original Agent across hosted/full/minimal/stateless/embedded surfaces, Domain/Workspace/Session hierarchy and native execution/presentation progression. | Live architecture with appended dated milestones; current source/contracts and canonical pending plans override old status/default/helper prose. |
| [docs/ARCHIVE-WORKFLOW.md](ARCHIVE-WORKFLOW.md) | Historical Phase0-7 append-only tar workspace, exact patch/CAS/fsync/dirty-tail/export secrecy; explicitly superseded by Git in Phase8. | Provenance only. Removed awtar helpers/archive development/evidence trees must not be recreated or treated as current instructions. |
| [docs/AWNET.md](AWNET.md) | One-binary flat connectivity, stable peer identity, QUIC route ladder, open managed/self-hosted protocol and Domain CP/Workspace writer semantics. | Top Phase23/native-pending status is stale relative to accepted source; planned join/full stream/relay/managed service examples require actual source confirmation; no generic VPN/tunnel/global controller or WAL replication. |
| [docs/BROWSER-WORKSPACE.md](BROWSER-WORKSPACE.md) | Active Solid root, exact read-only Agent and independent Files/Git, source-scope invalidation, draft consent, CSP/theme, inert media and retained advanced surfaces. | Live current container/view sections reviewed; historical advanced modules do not establish active terminal/site/docking parity or remote authentication. |
| [docs/BUILD-STATUS.md](BUILD-STATUS.md) | Current capability table, one pinned bootstrap/Cargo/six-suite Node product gate and distinct native/provider/signing/release proof boundaries. | Current validation instructions; latest exact0159 results belong to Workspace TODO, no new product run claimed for documentation. |
| [docs/CLI.md](CLI.md) | Canonical-only typed run/resource grammar, current provider/tool/MCP/resource/context/RPC/skill/schedule surfaces and feature cuts. | Live canonical front sections authoritative; retained old above-composer/guided-provider examples need source precedence, no aliases restored. |
| [docs/CONTEXT-FLOW-IMPLEMENTATION.md](CONTEXT-FLOW-IMPLEMENTATION.md) | Explicit source prepare/manual-or-tool-free generation/preview/apply, additive imports/compaction/one-shot, compatible target CAS, current source release/account restrictions, export/device protocol2 integration. | Historical implemented bounded path; top compile-unvalidated/schema2 status dated. Source revocation/derived disclosure remain mandatory; richer automation/platform/vendor/privacy gates not closed by explicit path. |
| [docs/CONTEXT-FLOW.md](CONTEXT-FLOW.md) | Dated proposed zero-inference fork versus compaction/carryover/multisource service; immutable source snapshots, separate generation/apply, audience/account/release dependencies and staged acceptance. | Phase10 implementation supersedes initial absent-feature status. No automatic full SQLite compaction/map-reduce/periodic refresh or per-node/federated declassification inferred; current tests/plans decide evidence. |
| [docs/CREDENTIAL-PLACEMENT.md](CREDENTIAL-PLACEMENT.md) | Identity/history/account/payer/custody/inference/destination/tool executor separated; no personal-key upload/fallback, malicious-origin browser/spending-oracle limits and later device/team/context/SecretStore refinements. | Earlier no-bridge/uncompiled statements historical; current protocol2 Bridge exists. Browser vault, hostile-origin/authentic human/public Internet/vendor billing assurance remain unproved/closed. |
| [docs/DATABASE-ADAPTATION-MATRIX.md](DATABASE-ADAPTATION-MATRIX.md) | Historical Phase11/schema2→v2 gap matrix and Phase12 update covering stable identity/feed/audit/custody, leases, direct mode and retry. | Many new/partial cells subsequently accepted as bounded implementations; not a new backlog; generic passkeys/passwords/browser vault, full rolling migration/retention and optional hosted stores require scoped evidence before claiming runtime. |
| [docs/DATABASE-ARCHITECTURE.md](DATABASE-ARCHITECTURE.md) | SQLite immutable facts/small refs, semantic feed, separate audit/auth/SecretStore/effects, DB-A…I staging and appended schema3–32 checkpoints/current cursor and vault notes. | Old Phase11/single-owner/no-auth status sections superseded by appended implementation/source; rolling migration/read concurrency/retention/snapshot/hosted adapters remain design directions; current schema33/cold quarantine overrides older restore claims. |
| [docs/DATABASE-DESIGN-REFINEMENT-V2.1.md](DATABASE-DESIGN-REFINEMENT-V2.1.md) | Four first-class additions: semantic sync/feed, stable auth identities, separate audit and secret custody; transaction-aware logical DB-A…I sequence. | Design-only provenance label retained, later phases implement much of sequence; no mandatory physical split/broker/Postgres, no deletion of owner.lock, no general authenticator/retention/hosted implementation proof. |
| [docs/DATABASE-DESIGN-REFINEMENT-V2.2.md](DATABASE-DESIGN-REFINEMENT-V2.2.md) | Logical operations versus immutable attempts, interactive choice versus policy-authorized new autonomous attempt, exact context/account and honest unknown usage. | Phase12/later runtime supplied primitives; generic provider reconciliation/autonomous integration is capability-specific and later-phase scoped; restart/Busy/lost wakeup/branch conflict alone never authorizes inference. |
| [docs/DEVICE-INFERENCE.md](DEVICE-INFERENCE.md) | Fixed personal Bridge/DeviceWorker binding, explicit local/SSH pairing, finite exact grants, local durable reservation ledger, private continuation handles and no tool/reconnect/account fallback; Summary digest amendment. | Current source protocol2 consistent; dated CLI examples should use current canonical schema. Two-machine/real provider/human-origin/OS security certification not claimed or rerun. |
| [docs/DEVICE-PROTOCOL.md](DEVICE-PROTOCOL.md) | Private protocol2 mutual nonce/HMAC length-prefix framing/strict sequences, fixed secret-free inference envelope, worker reservation-before-provider, bounded result receipts and exact Summary source/digest grants. | VERSION2 source verified. HMAC authentication is not arbitrary TCP encryption/human attestation/invoice; old protocol1 historical only and chat grant1 cannot authorize summaries. |
| [docs/DOMAIN-MODEL.md](DOMAIN-MODEL.md) | Durable Domain→Workspace→Session, Hosted/Peer/Fleet workspace semantics, explicit lens and authority intersection with independent credential placement. | Design direction retained; Phase12-only status is historical, later phases implement bounded portions; parent visibility, broad inheritance, full aggregate lens/HA UX and optional isolation modes are not blanket implementation claims. |
| [docs/FLEET-ADAPTATION-MATRIX.md](FLEET-ADAPTATION-MATRIX.md) | Phase12 comparison map for machine link/lens, registered roots, Fleet binding/operations and explicit remote preview/terminal capabilities. | Baseline gap map is not live TODO; transport/resources/targets/fan-out later implemented, full local-like remote projections/preview/PTYS/broadcast need separate proof and current-plan scope. |
| [docs/FLEETS.md](FLEETS.md) | Phase12 design amendment plus authoritative workspace-first/PeerWorkspace/AwNet refinements; snapshots/partial outcomes and independent account placement. | Foundations and bounded fan-out supplied13–24; rich Follow/Pinned aggregate Files/Git/PTY/log/metric/preview/broadcast/autonomous placement remain distinct features, not inferred from records; no permanent-master reading. |
| [docs/GIT-WORKFLOW.md](GIT-WORKFLOW.md) | Historical Phase0–11 archive-to-Git import/bundle/phase delivery and provenance rules. | Retained historical instructions; current signed expected-parent/explicit push-tag authorization applies, no local tags or concrete pre-rewrite bundle path verified. |
| [docs/GIT.md](GIT.md) | Trusted bounded Git process adapter, exact review/index patch, independent human contexts/terminal binding and native layout4. | Live source-scoped contract; manual Git is not model journal and external writers/unknown results require inspection. |
| [docs/HANDOFF.md](HANDOFF.md) | This comprehensive onboarding synthesis: initial design, all retained phase/spec records, current source, decisions, exact verification limits and successor prompt. | Requested context snapshot; canonical current progress remains in plan TODOs and retained originals remain available. |
| [docs/LLM-RETRY-RECOVERY.md](LLM-RETRY-RECOVERY.md) | Logical operation versus immutable attempts, explicit interactive/new autonomous policy attempts, preserved paid result before attachment, pinned context/destination with freshly selected authority and per-attempt identity, no Tool replay and durable retry claim design/amendments. | Source confirms core BeforeDispatch-only bounded retry separately from explicit SQLite Autonomous retry_unknown schedules/claim APIs. Cold copied schedules remain inert; vendor reconciliation/global authority not certified. |
| [docs/LOCAL-OWNER.md](LOCAL-OWNER.md) | Phase7 same-UID one-owner multi-presentation socket/browser lifecycle, exact receipts/status lookup, independent inspect and Phase8 personal links/Phase10 context. | Historical authoring-validation labels superseded by accepted gate scope; local protocol is not remote identity/public hosting/per-tab ownership, no implicit client provider config or durable outbox; stale socket path removal requires inspection. |
| [docs/LOCAL-PROTOCOL.md](LOCAL-PROTOCOL.md) | Version1 bounded length-prefixed local SDK frames, owner-instance/request IDs, exact prompt/status, poisoned dropped calls and separate observation/cancel/context operations. | Matched-build local protocol, not universal remote API; old helper/test-not-run appendix historical; lost reply to non-idempotent view mutation requires inspect, unknown status never reruns. |
| [docs/PI-CONTEXT-PARITY.md](PI-CONTEXT-PARITY.md) | Original Phase9 aw/Pi semantic audit distinguishes checkpoint/fork from branch summary and selects optional carryover plus reusable arbitrary permitted source summaries. | Historical design reference only. No current external executable parity, binary comparison or automatic feature claim. Follow implemented context/source contracts and current user exclusions. |
| [docs/RUNTIME-FOUR-PRIMITIVES-AND-SERVICES-2026-10-11.md](RUNTIME-FOUR-PRIMITIVES-AND-SERVICES-2026-10-11.md) | Four semantic primitives, canonical Turn/Step, kernel/host split, exact authority, service map, invariant registry and appended hosted as-builts. | Normative target plus historical bounded implementations; illustrative APIs are not universal shipping claims. Current source confirms contracts; universal product convergence/native certification remains limited. |
| [docs/RUNTIME-HOOKS-TRUST-AND-POLICY-2026-10-11.md](RUNTIME-HOOKS-TRUST-AND-POLICY-2026-10-11.md) | Exactly Next/Defer and10 typed stages; immutable evidence/privacy, narrow Envelope lattice, scoped approvals/reviewer proposals and appended precise Host/owner/grant limits. | Current Extension/on_hook and stage source verified. Old opening DESIGN ONLY and early absent-slice prose refined by appendices; generic model/human/process privileges remain closed. |
| [docs/SDK.md](SDK.md) | Canonical subprocess CLI; one core/embedded/shared turn, exact device custody, compaction/retry/RPC/process consumers and E4/E5 proof boundaries. | Live API contract reviewed against key source; removed helper strings are archival, not runnable tests. |
| [docs/SOURCES.md](SOURCES.md) | Primary API/standards/research references consulted across early phases, with explicit difference between reference prose, source tests and compiled/native/security evidence. | Provenance bibliography; dated dependency/build/source archive pointers may be superseded or removed. No new external verification or current compatibility/certification inferred. |
| [docs/STATE.md](STATE.md) | Durable Session journal, intent/result/hazard/CAS, owner/direct history, context migration/restrictions, browser receipts/import/export and schema progression through guardian state. | Top current v32 and earlier schema/version/runtime-uncompiled claims are dated; actual source latest v33. Original safety rules retained; quarantine/recovery current plan and source govern. |
| [docs/SURFACES.md](SURFACES.md) | Trusted blocking composition SDK for exact bounded Files/Git/PTY grants, opaque source/scope/inventory identities and retained resources/receipts. | Live API; embedder owns auth/worker/disclosure, leases are cooperating-view control and metadata never grants arbitrary root/actor execution. |
| [docs/TEAM-IDENTITY.md](TEAM-IDENTITY.md) | Invite-only numeric-loopback per-participant controllers/allowed accounts, private state realm stamp, explicit personal worker identity and bounded lifecycle, Phase10 releases/Phase38 drafts. | Old no-OIDC/collaboration text scoped toPhase9 and later bounded amendments; public TLS/hostile-user isolation/per-user tools/native team attach/password recovery not inferred; same profile tabs share identity and operator is trusted. |
| [docs/TEAM-PROTOCOL.md](TEAM-PROTOCOL.md) | Authenticated invite/cookie selects controller before routing, exact request/account attribution, stream isolation, context release and explicit shared Session/draft opener. | Owned-only remains default outside explicit shared path; tools/surfaces false; typed shared Send alone may enter peer-owned prompt admission; old unrun-Rust helper notes historical, public service/security certification remains separate. |
| [docs/TERMINALS.md](TERMINALS.md) | Persistent real PTY resources independent from views, safe cell rendering, bounded queue/lease/input and host lifecycle/key ownership. | Live current amendment controls old authored-only/launch-root/browser spellings; same-UID shell is unsandboxed, no rollback guarantee. |
| [docs/TOOLS.md](TOOLS.md) | Rooted coding/search tools, complete proposal/schema/grant/journal boundaries, model-directed Read/Bash waves, external source adapters, no sandbox and unknown-effect inspection plus child authority history. | Mixed dated flags/caps/permit/before-after/ABI1 paragraphs superseded by current source and later appendices. Tool defaults Off; exact source grants, original-order receipts, no unknown replay and child-root bounds remain. |
| [docs/UI-EXTENSIONS.md](UI-EXTENSIONS.md) | Bounded typed presentation slots and host protocol, status/performance ownership, truthful eligible metrics and no arbitrary execution. | Live contract; R7 tools process does not implement external UI projection host/package ecosystem. |
| [docs/WEB-PROTOCOL.md](WEB-PROTOCOL.md) | Distinct LocalWeb/paired/shared/team authority, durable prompt admission/deduplication and exact read-only/scoped readers plus protected snapshots. | Live protocol; acceptance is not settlement, browser fields cannot select authority, no proxy-based remote multiuser certification. |
| [docs/WORKSPACE.md](WORKSPACE.md) | One Ui/presenter/composer and bounded human workers; distinct Agent/Workspace modes, recent scoped readers/Files/Git and retained historic lineage. | Live top/current scoped sections reviewed; old flags/% category/schema/mode wording is historical, docking/concurrent Agent contexts not implied. |

### Canonical current plans and design

| Document | Context and decision content | Pending/status interpretation |
| --- | --- | --- |
| [plans/accepted-history/TODO.md](../plans/accepted-history/TODO.md) | User-accepted47ee8a0 historical scope, feature/custody/runtime/portable/CI work and proof limits. | Closed historical checkpoint; acceptance does not supply missing native/security/provider/release/restoration gates. |
| [plans/embedded-platforms/TODO.md](../plans/embedded-platforms/TODO.md) | Parked E6/architecture targets, optional search acceleration, Windows shell cancellation and independent certification boundaries. | Optional separately scoped work; accepted WASM/libc-free/C ABI results do not certify firmware or other native hosts. |
| [plans/handoff-process/TODO.md](../plans/handoff-process/TODO.md) | Accepted simple TODO workflow and this explicitly requested comprehensive onboarding documentation task. | Context is consolidated here; canonical task/test status stays in TODOs and temporary agent coverage/scratch is removed after reconciliation. |
| [plans/release-readiness/TODO.md](../plans/release-readiness/TODO.md) | Current release workflow/product checkpoints and outstanding native asset/version/signer/provider/security gates. | Active external release work;0.16.0 proposal only, no publication authorized. |
| [plans/restored-state-authority/TODO.md](../plans/restored-state-authority/TODO.md) | Exact unknown effect/charge accounting, human authenticator, global winner coordinator and crash-safe new-proposal prerequisites. | Design-only and blocked; restored activation and historical Queue/Actor replay remain unavailable. |
| [plans/ux-convergence/TODO.md](../plans/ux-convergence/TODO.md) | Further semantic theme/chrome/interaction convergence, actual keyboard/mouse/narrow/resize checks and preserved ownership. | Not started as a separate plan; inventory shipping gaps, never infer full parity or touch reserved UX ownership. |
| [plans/workspace-views/README.md](../plans/workspace-views/README.md) | Binding native Agent→Files→Git→browser→docking design, exact scopes/lifetimes/layout migrations, left-right measurement policy and browser capability contract. | All local viewer/container slices completed; measured Session/presence publication and docking remain future work, browser extra Files roots require explicit grants. |
| [plans/workspace-views/TODO.md](../plans/workspace-views/TODO.md) | Commit-grouped exact native/browser implementation and validation history with current open work. | Canonical active plan: left-right/docking/further native checks/actual Windows filesystem execution/remote authority remain unfinished. |

### Retained historical plans and specifications

| Document | Context and decision content | Pending/status interpretation |
| --- | --- | --- |
| [plans/AGENTC-MINIMAL-RUST-PARITY-2026-10-08.md](../plans/AGENTC-MINIMAL-RUST-PARITY-2026-10-08.md) | Original one-way minimal gap ledger; std JSONL cut, same Agent, durable resume/fingerprint/uncertain-effect fencing. | Historical baseline; later R/E work supersedes missing rows and R14.5 removes comparator obligation. |
| [plans/AGENTC-PARITY-E4-NATIVE-LIBC-FREE-2026-10-10.md](../plans/AGENTC-PARITY-E4-NATIVE-LIBC-FREE-2026-10-10.md) | Independent Linux x86-64 no_std fake host: allocator/entry/compiler primitives/syscalls, audited actual ELF execution and negative controls. | Accepted separate native Linux scope; not firmware or full CLI size, C ABI, real-provider or cross-OS proof. |
| [plans/AGENTC-PARITY-E5-C-ABI-2026-10-10.md](../plans/AGENTC-PARITY-E5-C-ABI-2026-10-10.md) | Isolated portable FFI, eleven versioned exports, copied data, exact buffer/layout ownership, BUSY reentry and uncertain teardown refusal. | Accepted independent native C11 debug/release proof; hosted C client uses libc, no firmware or audited cross-platform ABI inferred. |
| [plans/AGENTC-PARITY-IMPLEMENTATION-PHASES-2026-10-09.md](../plans/AGENTC-PARITY-IMPLEMENTATION-PHASES-2026-10-09.md) | R0-R13 dependency roadmap with core no_std capability foundation and later shared Agent extraction. | Historical sequencing; neither an unchecked live backlog nor authority to run new numbered phases. |
| [plans/AGENTC-PARITY-R1-KERNEL-2026-10-09.md](../plans/AGENTC-PARITY-R1-KERNEL-2026-10-09.md) | Core-only source/epoch/effect/Actor/site grants and frozen ToolSpec consumed by hosted registry/policy. | Accepted historical implementation; first kernel proof is narrower than R8 complete embedded fake turn. |
| [plans/AGENTC-PARITY-R10-COMPACTION-RETRY-2026-10-10.md](../plans/AGENTC-PARITY-R10-COMPACTION-RETRY-2026-10-10.md) | Ask-default pure compaction plan and paid SummaryExecutor journal/CAS active tail; default-zero positive predispatch retry policy. | Accepted JSONL append-only behavior; raw log bound, richer budgets/tokenizers and automatic SQLite compaction remain separate. |
| [plans/AGENTC-PARITY-R11-PROJECT-RESOURCES-UX-2026-10-10.md](../plans/AGENTC-PARITY-R11-PROJECT-RESOURCES-UX-2026-10-10.md) | Explicit SHA256 project trust, bounded ancestor instructions/templates/SKILL.md/native semantic theme; per-prompt validation. | Accepted local single-user Agent; browser/owner/Workspace trust and new minimal vault/OAuth deliberately not implemented. |
| [plans/AGENTC-PARITY-R12-HEADLESS-RPC-2026-10-10.md](../plans/AGENTC-PARITY-R12-HEADLESS-RPC-2026-10-10.md) | Bounded public JSONL/RPC around same host, accepted/settled, cooperative exact abort, independent uncertainty-aware clients. | Accepted local protocol; no authenticated remote RPC, direct privileged shell/control or global idempotency. |
| [plans/AGENTC-PARITY-R13-CERTIFICATION-2026-10-10.md](../plans/AGENTC-PARITY-R13-CERTIFICATION-2026-10-10.md) | Local full/minimal/stateless actual-binary conformance and separate host-specific footprint measurements; command/prompt regression fix. | Historical local proof; comparator and missing E4/E5 later superseded; native/live/signing/security certification still separate. |
| [plans/AGENTC-PARITY-R2-SEARCH-2026-10-09.md](../plans/AGENTC-PARITY-R2-SEARCH-2026-10-09.md) | Bounded rooted ls/find/grep using mature regex/globset/ignore matching; grant narrowing and no watcher. | R2a accepted; optional external rg/fd and Windows trusted shell/Job Object remain separately open. |
| [plans/AGENTC-PARITY-R3A-ANTHROPIC-2026-10-09.md](../plans/AGENTC-PARITY-R3A-ANTHROPIC-2026-10-09.md) | Native Messages SSE/tool/signed thinking, selected-key lifetime pinning and current account/model continuation privacy. | Local fake-provider acceptance only; no consumer subscription OAuth, live billing or complete capability guarantee. |
| [plans/AGENTC-PARITY-R3B-GEMINI-2026-10-09.md](../plans/AGENTC-PARITY-R3B-GEMINI-2026-10-09.md) | Native GenerateContent model paths/key header/function IDs/signatures, complete-stream checks and private continuation. | Local fake-provider acceptance; Interactions/Vertex/media and unsupported thinking budgets remain separate. |
| [plans/AGENTC-PARITY-R4-MODEL-CATALOG-2026-10-09.md](../plans/AGENTC-PARITY-R4-MODEL-CATALOG-2026-10-09.md) | One source-labelled catalog, bounded paginated explicit discovery, salted credential-scoped cache and integer pricing. | Accepted descriptive metadata; availability/permission/quota/invoice/current-rate claims remain unsupported. |
| [plans/AGENTC-PARITY-R5A-MCP-TOOLS-2026-10-09.md](../plans/AGENTC-PARITY-R5A-MCP-TOOLS-2026-10-09.md) | Official RMCP stdio/HTTP tools, bounded local schemas and atomic registry preflight under exact Unknown source grants. | Accepted local fixtures; no implicit startup, browser grants, uncertain replay or live-service audit. |
| [plans/AGENTC-PARITY-R5B-LEGACY-SSE-2026-10-09.md](../plans/AGENTC-PARITY-R5B-LEGACY-SSE-2026-10-09.md) | Explicit old two-endpoint SSE framing feeds same RMCP, with exact origin bearer safety and bounded parser. | Accepted local fixtures; no modern fallback/reconnect/replay; R6 later adds data and notifications. |
| [plans/AGENTC-PARITY-R6-MCP-RESOURCES-2026-10-09.md](../plans/AGENTC-PARITY-R6-MCP-RESOURCES-2026-10-09.md) | Explicit prompt/resource/template/read/watch, exact URI grants, text-only bounds and source-revision invalidation. | Accepted local fixtures; advanced templates/media/elicitation/browser/restart subscriptions not implied. |
| [plans/AGENTC-PARITY-R7-EXTENSIONS-2026-10-09.md](../plans/AGENTC-PARITY-R7-EXTENSIONS-2026-10-09.md) | Explicit bounded trusted-process JSONL tools/prompts and separately approved veto gates; independent C/Rust consumers. | Accepted foundation; no OS sandbox, full lifecycle/provider/status/command/package/UI-host ecosystem. |
| [plans/AGENTC-PARITY-R8-EMBEDDED-AGENT-2026-10-10.md](../plans/AGENTC-PARITY-R8-EMBEDDED-AGENT-2026-10-10.md) | Same TurnMachine used by hosted and host-driven no_std+alloc Agent; durable intent and receipt transitions, real executed zero-import WASM. | Accepted E2/E3 proof; not firmware, production transport, actual flash durability or cross-toolchain certification. |
| [plans/AGENTC-PARITY-R9-MODEL-DIRECTED-2026-10-10.md](../plans/AGENTC-PARITY-R9-MODEL-DIRECTED-2026-10-10.md) | Model requests parallel on already-granted reads/trusted Bash; host serial override, default16/max128, ordered bounded results. | Accepted current semantic basis; no grant, race-free dependency graph or OS-confinement guarantee. |
| [plans/AGENTC-PARITY-R9-PARALLEL-TOOLS-2026-10-10.md](../plans/AGENTC-PARITY-R9-PARALLEL-TOOLS-2026-10-10.md) | Initial four-read-only nonspawned futures, all intents before effects and source-order result/receipt/hook settlement. | Historical initial design; model-directed follow-up supersedes defaults/limits and trusted Bash scheduling. |
| [plans/AGENTC-RUST-NATIVE-PARITY-MASTER-PLAN-2026-10-09.md](../plans/AGENTC-RUST-NATIVE-PARITY-MASTER-PLAN-2026-10-09.md) | Master design: one idiomatic Rust engine, explicit authority, providers/MCP/process hooks, compaction/retry, resources, events, feature/host proof. | Historical decisions retained; initial A/R sequencing, aliases, comparator and old gate tooling superseded. |
| [plans/AW-CLI-TAXONOMY-R14-2026-10-10.md](../plans/AW-CLI-TAXONOMY-R14-2026-10-10.md) | Complete original typed Clap normalization, category help/schema and bounded full-tree stack, initially hidden aliases. | Historical R14.1-R14.4 accepted; alias/migration requirements explicitly removed by R14.5 successor. |
| [plans/AW-EMBEDDED-NOSTD-LIBC-FREE-ARCHITECTURE-2026-10-09.md](../plans/AW-EMBEDDED-NOSTD-LIBC-FREE-ARCHITECTURE-2026-10-09.md) | Tier K/E/S and positive std/alloc features, one kernel, explicit optional host ports and final artifact proof definitions. | Foundational design accepted; its early not-built and E-step names superseded by concrete R8/E4/E5/current parked E6. |
| [plans/AW-P6-10-CORRECTNESS-AND-RELEASE-REVIEW-2026-10-12.md](../plans/AW-P6-10-CORRECTNESS-AND-RELEASE-REVIEW-2026-10-12.md) | Fresh bounded correctness/release review reproduces missing coordinated Process incarnation None-equality receipt bug and fixes with original current coordination. | Historical reviewed fix implemented; wider P6/release/platform/security, broad model self/Process/human powers and then-format debt not closed by local mock tests. Later current plan/source decides remaining checks. |
| [plans/AW-P6-10B-PROCESS-ACTOR-GENERATION-FENCE-AS-BUILT-2026-10-12.md](../plans/AW-P6-10B-PROCESS-ACTOR-GENERATION-FENCE-AS-BUILT-2026-10-12.md) | ProcessRegistry pins full original ActorKey and uses one owner/Session/branch/site/writer/generation predicate for observation/termination/unregister, including retirement. | Implemented generation-custody repair; trusted registration still requires independently authorized/journaled original Process spawn. pidfd/descendants/sandbox/platform proof and broad controls remain open. |
| [plans/AW-P6-10C-CANONICAL-MODEL-INTENT-IDENTITY-AS-BUILT-2026-10-12.md](../plans/AW-P6-10C-CANONICAL-MODEL-INTENT-IDENTITY-AS-BUILT-2026-10-12.md) | Store recomputes canonical length-prefixed domain-separated mp_/md_ SHA identities with shared Service helpers, rejecting plausible alternate aliases without new grants/schema. | Implemented lower-Store identity repair. Digest is correlation only; original active Turn/owner/writer/source/effect and no-replay checks still required. |
| [plans/AW-P6-10D-ORIGINAL-MODEL-QUEUE-TOOL-RECEIPT-PROVENANCE-AS-BUILT-2026-10-12.md](../plans/AW-P6-10D-ORIGINAL-MODEL-QUEUE-TOOL-RECEIPT-PROVENANCE-AS-BUILT-2026-10-12.md) | Queue model dispositions prove original successful queue_enqueue Assistant/Tool intent/result, received invocation, attached completed run and first encrypted body digest in same transaction. | Implemented custody ancestry repair. Host-minted mq_ from Settled Turn alone is insufficient; later host edits preserve original creation evidence. 10E adds exact Tool source evidence. |
| [plans/AW-P6-10E-ORIGINAL-TOOL-SOURCE-AND-RECOVERY-EVIDENCE-AS-BUILT-2026-10-12.md](../plans/AW-P6-10E-ORIGINAL-TOOL-SOURCE-AND-RECOVERY-EVIDENCE-AS-BUILT-2026-10-12.md) | Optional validated immutable ToolStarted source kind/ID/epoch from real registry; revision/source coherence; management ancestry demands exact Host/trusted-host/epoch1 proof. | Implemented additive source evidence with old source-less records readable but non-authorizing for model management. No external vendor signature or trusted-host malware isolation inferred. |
| [plans/AW-P6-10F-WRITER-EPOCH-TOOL-INTENT-AND-REVOCATION-RACES-AS-BUILT-2026-10-12.md](../plans/AW-P6-10F-WRITER-EPOCH-TOOL-INTENT-AND-REVOCATION-RACES-AS-BUILT-2026-10-12.md) | New provider/Tool/revision starts require immutable run writer epoch to match current writer inside transaction, while old factual terminal evidence survives failover/failback and revocation. | Implemented source/writer race hardening; paid results preserved unattached on CAS loss. No new effects from stale handle or terminal receipt, no automatic replay. |
| [plans/AW-P6-10G-TERMINAL-PROVIDER-TOOL-RECEIPTS-AND-RESTART-AS-BUILT-2026-10-12.md](../plans/AW-P6-10G-TERMINAL-PROVIDER-TOOL-RECEIPTS-AND-RESTART-AS-BUILT-2026-10-12.md) | Terminal provider no-metadata uncertainty stays unknown; genuine not_dispatched is known no-call, aggregate started-or-unknown retained, restart preserves factual Tool/provider receipts. | Implemented unknown-billing repair. Trusted adapter proof is not a commercial invoice; no second provider ordinal after unknown in same run, no unknown Tool replay. |
| [plans/AW-P6-10H-MULTI-INVOCATION-RECEIPT-ORDERING-AS-BUILT-2026-10-12.md](../plans/AW-P6-10H-MULTI-INVOCATION-RECEIPT-ORDERING-AS-BUILT-2026-10-12.md) | Original invocation receipts enforce exact sequential nonoverlapping ordinals1..32 and allowed outcome/metadata pairs using existing indexed bounded transaction checks. | Implemented receipt coherence. Query-plan bound is not production benchmark; late truthful finishes may append but stale new starts deny. External bills/power-loss/platform evidence remain separate. |
| [plans/AW-P6-10I-IMMUTABLE-FINAL-TURN-RECEIPT-COHERENCE-AS-BUILT-2026-10-12.md](../plans/AW-P6-10I-IMMUTABLE-FINAL-TURN-RECEIPT-COHERENCE-AS-BUILT-2026-10-12.md) | One OriginalRunReceipts projection validates final Turn invocation/Tool/metadata facts and Completed versus unknown/unclosed effects; recovery reuses same original ledger. | Implemented final projection integrity. Does not cryptographically attest original trusted Host or compare arbitrary streamed answer/Hook-visible text as factual receipt. |
| [plans/AW-P6-10J-FINAL-TURN-CONTEXT-AND-TOOL-PROJECTION-FENCE-AS-BUILT-2026-10-12.md](../plans/AW-P6-10J-FINAL-TURN-CONTEXT-AND-TOOL-PROJECTION-FENCE-AS-BUILT-2026-10-12.md) | Final future context requires original User, factual ordered Assistant/proposals and actual Tool result authority; rejects invented System/User/Tool success while permitting authorized Tool text projection. | Implemented context authority repair. Generic raw ToolOutput privacy/encryption remains open; historical empty and factual partial failed traces remain valid. |
| [plans/AW-P6-10K-PRIVATE-BACKUP-CUSTODY-AND-TOOL-OUTPUT-PRIVACY-REVIEW-AS-BUILT-2026-10-12.md](../plans/AW-P6-10K-PRIVATE-BACKUP-CUSTODY-AND-TOOL-OUTPUT-PRIVACY-REVIEW-AS-BUILT-2026-10-12.md) | Backup failed exclusive-create cleanup formerly deleted existing snapshot/file/symlink; cleanup now owns only new output, fsyncs success; characterizes plaintext factual ToolOutput versus redacted model view. | Implemented data-loss fix. SQLite/JSONL/backup plaintext owner state remains sensitive; generic encryption/erasure/key rotation and universal durability not implemented. |
| [plans/AW-P6-10L-PORTABLE-EXPORT-AND-SECRETSTORE-RESTORE-AS-BUILT-2026-10-13.md](../plans/AW-P6-10L-PORTABLE-EXPORT-AND-SECRETSTORE-RESTORE-AS-BUILT-2026-10-13.md) | Portable export cleans only newly created partial output on actual write failure; SQLite-only restore with encrypted refs refuses silent unrelated master-key initialization. | Implemented failure/custody repair. Correct key/items are separate required recovery resources; no automatic all-state bundle, at-rest Tool encryption or natural power-loss certification. |
| [plans/AW-P6-10M-OPERATOR-COMPOSITE-BACKUP-DESIGN-AND-NATIVE-EVIDENCE-2026-10-13.md](../plans/AW-P6-10M-OPERATOR-COMPOSITE-BACKUP-DESIGN-AND-NATIVE-EVIDENCE-2026-10-13.md) | Design/native feasibility proves SQL maintenance does not yet fence direct vault mutation, selects cold owner-only coherent composite with active-ref/AEAD/staging/publication prerequisites. | Dated M barrier/inventory blockers later addressed by N/O and bounded internal P-S chain. Toy manifest never shipped; no public command, cloud-safe key archive or runnable authority implied. |
| [plans/AW-P6-10N-ORIGINAL-SECRETSTORE-MUTATION-FENCE-AS-BUILT-2026-10-13.md](../plans/AW-P6-10N-ORIGINAL-SECRETSTORE-MUTATION-FENCE-AS-BUILT-2026-10-13.md) | One stable private vault mutation file fence integrates shared put/delete and exclusive initialization/maintenance across direct handles/processes, with exact lease release and restore-generation liveness. | Implemented cooperative owner-local quiescence prerequisite. Full composite capture/verify/publish later P-R; hostile same-UID/root/network filesystem and physical erasure not certified. |
| [plans/AW-P6-10O-ORIGINAL-ACTIVE-ENCRYPTED-CUSTODY-INVENTORY-AS-BUILT-2026-10-13.md](../plans/AW-P6-10O-ORIGINAL-ACTIVE-ENCRYPTED-CUSTODY-INVENTORY-AS-BUILT-2026-10-13.md) | Maintenance-gated active Queue/Extension/Provider/OIDC/OAuth refs verified against current versions/private item shape/original AEAD and Queue SHA; count-only result and explicit bounds. | Implemented current encrypted custody inventory. Retired/orphan extras counted without promotion/deletion. Does not itself capture/publish backup or expose secrets/refs to model/remote callers. |
| [plans/AW-P6-10P-INTERNAL-COMPOSITE-STAGING-AND-SNAPSHOT-CAPTURE-AS-BUILT-2026-10-13.md](../plans/AW-P6-10P-INTERNAL-COMPOSITE-STAGING-AND-SNAPSHOT-CAPTURE-AS-BUILT-2026-10-13.md) | Private random incomplete .partial captures WAL-aware SQL and bounded original master/items under original owner/vault fence with false complete marker and own-stage cleanup. | Implemented internal nonrecoverable capture scaffold; Q/R/S later add standalone verify/publication/inert copy. No user-facing bundle API or automatic partial promotion. |
| [plans/AW-P6-10Q-STANDALONE-VERIFIED-ENCRYPTED-STAGING-AS-BUILT-2026-10-13.md](../plans/AW-P6-10Q-STANDALONE-VERIFIED-ENCRYPTED-STAGING-AS-BUILT-2026-10-13.md) | Standalone staged verifier reuses current-ref/AEAD/Queue digest engine and exact file allowlist; rejects rehashed ciphertext/master/SQL mismatch and verifies after source removed. | Implemented semantic staged verification and process-exit witnesses. Valid .partial remains unpublished; hashes are corruption checks, not remote signature or archive encryption. |
| [plans/AW-P6-10R-OPERATOR-ATOMIC-PUBLICATION-V1-AS-BUILT-2026-10-13.md](../plans/AW-P6-10R-OPERATOR-ATOMIC-PUBLICATION-V1-AS-BUILT-2026-10-13.md) | Distinct strict complete aw.operator-composite/v1 manifest and Linux same-parent renameat2 NOREPLACE publication; postrename failure retains inspectable named artifact. | Implemented private Linux publication, not public backup/restore command. S later adds inert offline copy only; natural power-loss, cross-platform/no-overwrite and authenticated cloud distribution remain open. |
| [plans/AW-P6-10S-QUARANTINED-OFFLINE-COMPOSITE-COLD-RESTORE-AS-BUILT-2026-10-13.md](../plans/AW-P6-10S-QUARANTINED-OFFLINE-COMPOSITE-COLD-RESTORE-AS-BUILT-2026-10-13.md) | Offline source-independent composite copy into absent private target increments restore generation, fences old Process/leases/writers to read-only, preserves original risks and denies owner/direct opening. | Implemented quarantined non-runnable State only; T adds SQL denial against single-marker deletion. No automatic old Queue/Tool/provider execution or fresh writer authority. |
| [plans/AW-P6-10T-DUAL-QUARANTINE-INSPECTION-AND-NONACTIVATING-OWNER-REVIEW-AS-BUILT-2026-10-13.md](../plans/AW-P6-10T-DUAL-QUARANTINE-INSPECTION-AND-NONACTIVATING-OWNER-REVIEW-AS-BUILT-2026-10-13.md) | Independent filesystem plus SQL cold quarantine,14-class bounded risk counts and owner-of-all-Sessions immutable review requests; reproduced marker-deletion bypass closed. | Implemented inspection/nonactivating owner request. Later W strengthens exact provenance and coherent snapshot; no local review/zero counts can settle effects or activate. |
| [plans/AW-P6-10U-SIGNED-DEVICE-CONSENT-PREACTIVATION-AS-BUILT-2026-10-13.md](../plans/AW-P6-10U-SIGNED-DEVICE-CONSENT-PREACTIVATION-AS-BUILT-2026-10-13.md) | Original portable Ed25519 device/continuity/epochs reused for five-minute Store/archive/generation/risk-bound signed future-review prerequisite, hashes only in audit. | Implemented private prerequisite, not independent live-human consent or global nonce authority. V rechecks actual signature; W9/current plan retain external authenticator/coordinator blockers. |
| [plans/AW-P6-10V-ACTIVATION-DRY-RUN-AND-TRANSACTIONAL-EFFECT-FENCE-AS-BUILT-2026-10-14.md](../plans/AW-P6-10V-ACTIVATION-DRY-RUN-AND-TRANSACTIONAL-EFFECT-FENCE-AS-BUILT-2026-10-14.md) | Source-independent actual signed-proof dry run and optional immutable nonrunnable audit revalidate risk/device/writer/source facts inside BEGIN IMMEDIATE and reject scan-to-write races. | Implemented private diagnostic; original and cloned receipts may pass locally but both remain quarantined. Independent live-human/global arbitration/effect settlement/activation absent. |
| [plans/AW-P6-10W-POST-W7-FRESH-CORRECTNESS-REVIEW-2026-10-15.md](../plans/AW-P6-10W-POST-W7-FRESH-CORRECTNESS-REVIEW-2026-10-15.md) | Post-W7 fresh review reproduces same-ID wrong-effect Tool receipt risk clearing, hardens exact proposal/source/effect/site/order and removes obsolete weaker prepare routes; records bounded filesystem proof. | Implemented historical correctness cleanup; FD/path anchoring not malicious-root proof. W8 adds exact-effect owner requests; W9 retains authority prerequisites. Removed historical commit/source helpers are not current workflow. |
| [plans/AW-P6-10W1-NONEXECUTING-EFFECT-RECONCILIATION-PREPARE-AS-BUILT-2026-10-14.md](../plans/AW-P6-10W1-NONEXECUTING-EFFECT-RECONCILIATION-PREPARE-AS-BUILT-2026-10-14.md) | Private original14-risk nonexecuting reconciliation preparation; every output explicitly nonrunnable/nonreplayable, even zero counts. | Implemented first classification slice; exact identities/snapshot/provenance later W2-W7. Never final reconciliation, fresh human proof or execution authority. |
| [plans/AW-P6-10W2-EXACT-PROVIDER-RUN-EVIDENCE-AS-BUILT-2026-10-14.md](../plans/AW-P6-10W2-EXACT-PROVIDER-RUN-EVIDENCE-AS-BUILT-2026-10-14.md) | Exact original unfinished Run/invocation ordinal/LLM attempt/hazard identities with bounded count consistency and no private account/body disclosure. | Implemented identity enumeration; later W6 shares one snapshot and W7 anchors files. Provider-owned charge evidence and final settlement still external/open. |
| [plans/AW-P6-10W3-EXACT-TOOL-QUEUE-ACTOR-EVIDENCE-AS-BUILT-2026-10-14.md](../plans/AW-P6-10W3-EXACT-TOOL-QUEUE-ACTOR-EVIDENCE-AS-BUILT-2026-10-14.md) | Original Tool call/run and latest Queue/Input/Control unresolved identities from original decoders, preserving Pending/Claimed/Accepted distinctions. | Implemented initial identity projection; later W6/W7 and review repair snapshot/source/generation/exact terminal coherence. Old separate wrappers removed; never replayable. |
| [plans/AW-P6-10W4-EXACT-RECOVERY-AND-LEASE-EVIDENCE-AS-BUILT-2026-10-14.md](../plans/AW-P6-10W4-EXACT-RECOVERY-AND-LEASE-EVIDENCE-AS-BUILT-2026-10-14.md) | Execution lease scope/run/generation/Process holder, recovery hint IDs and retry schedule operation/claim/generation identities added to bounded nonexecuting inventory. | Implemented data only; later unified snapshot retains evidence. Leases/retries do not authorize copied old Process/provider dispatch. |
| [plans/AW-P6-10W5-SINGLE-SNAPSHOT-COLD-RECONCILIATION-AS-BUILT-2026-10-14.md](../plans/AW-P6-10W5-SINGLE-SNAPSHOT-COLD-RECONCILIATION-AS-BUILT-2026-10-14.md) | One read-only SQLite transaction enumerates exact W2-W4 facts and compares restored generation/previous T counts with quarantine rechecks. | Implemented interim snapshot approach superseded by W6 same-snapshot T count+identity inspection; no indivisible OS+SQL hostile-writer proof. |
| [plans/AW-P6-10W6-COHERENT-ORIGINAL-RISK-AND-EFFECT-SNAPSHOT-AS-BUILT-2026-10-14.md](../plans/AW-P6-10W6-COHERENT-ORIGINAL-RISK-AND-EFFECT-SNAPSHOT-AS-BUILT-2026-10-14.md) | Extracted original T risk reader reused inside same SQLite snapshot as all exact evidence, with real competing WAL writer consistency fixture. | Implemented coherent SQL snapshot; filesystem/source checks before/after remain separate. W7 adds anchors/provenance, not global authority. |
| [plans/AW-P6-10W7-PINNED-COLD-FILES-AND-ORIGINAL-SOURCE-PROVENANCE-AS-BUILT-2026-10-15.md](../plans/AW-P6-10W7-PINNED-COLD-FILES-AND-ORIGINAL-SOURCE-PROVENANCE-AS-BUILT-2026-10-15.md) | Held directory/SQLite/marker descriptors plus device/inode/owner/mode/size/time checks; exact Store/SQL quarantine and historical Tool/Actor/Queue/process/source provenance retained. | Implemented bounded local anchoring; raw SQLite path is not a formal fd-VFS/root/mount certificate. W8 review and W9 prerequisites remain nonactivating. |
| [plans/AW-P6-10W8-ORIGINAL-OWNER-EXACT-EFFECT-NONEXECUTING-REVIEW-AS-BUILT-2026-10-15.md](../plans/AW-P6-10W8-ORIGINAL-OWNER-EXACT-EFFECT-NONEXECUTING-REVIEW-AS-BUILT-2026-10-15.md) | Exact one-effect source fingerprint and bounded overall snapshot digest; original owner-of-all-Sessions immutable KeepQuarantined/RequestFreshProposalReview rechecks facts under BEGIN IMMEDIATE. | Implemented review requests only; duplicate clones can each request but neither executes. Original settlement/fresh authority/human/global uniqueness flags remain false; no quarantine removal. |
| [plans/AW-P6-10W8A-POSTCOMMIT-UNCERTAINTY-AND-ATOMIC-INDEX-RECOVERY-AS-BUILT-2026-10-15.md](../plans/AW-P6-10W8A-POSTCOMMIT-UNCERTAINTY-AND-ATOMIC-INDEX-RECOVERY-AS-BUILT-2026-10-15.md) | Actual postcommit failure becomes explicit unknown-audit inspect/no-retry; historical atomic Git helper preserves prepared opaque index when HEAD may have advanced. | Implemented historical uncertainty fixes. Current source/plan supersede removed helper invocations; not effect settlement or activation. No runtime check newly rerun for handoff. |
| [plans/AW-P6-10W9-FRESH-HUMAN-AND-CROSS-CLONE-AUTHORITY-PREREQUISITES-2026-10-15.md](../plans/AW-P6-10W9-FRESH-HUMAN-AND-CROSS-CLONE-AUTHORITY-PREREQUISITES-2026-10-15.md) | Design-only independent live-human challenge/authenticator and linearizable durable cross-clone coordinator prerequisites, exact payer/effect reconciliation and crash-safe zero-replay first-open gates. | BLOCKED DESIGN ONLY per current restored-State TODO. No provisioning, live authenticator/coordinator proof, final reconciliation or runnable activation authorized/implemented. |
| [plans/AW-P6-8A-EXTENSION-SETTINGS-AND-PROMPTS-AS-BUILT-2026-10-11.md](../plans/AW-P6-8A-EXTENSION-SETTINGS-AND-PROMPTS-AS-BUILT-2026-10-11.md) | Extension-owned bounded typed settings, exact per-key config CAS, CLI/project/user/default precedence, approved project data and anchored host/project prompt composition. | Implemented historical bounded SDK slice; old Secret/status/media OPEN items are partly superseded by 8B/8C. No project-data authority, default extension execution, live voice or remote human approval. |
| [plans/AW-P6-8B-ENCRYPTED-SECRETS-AND-STATUS-EVENTS-AS-BUILT-2026-10-11.md](../plans/AW-P6-8B-ENCRYPTED-SECRETS-AND-STATUS-EVENTS-AS-BUILT-2026-10-11.md) | Current-owner/WriterEpoch/Actor-gated encrypted Secret settings with immutable CAS/retired cleanup; protocol2 semantic status regions and metadata-only ephemeral EventRail. | Implemented bounded local Secret/status service. 8C later adds notice consumer/inert media/approval projection; unknown orphans, physical erase, browser secret APIs and broad platform/privacy certification remain open. |
| [plans/AW-P6-8C-MEDIA-APPROVALS-AND-EVENT-PRESENTATION-AS-BUILT-2026-10-11.md](../plans/AW-P6-8C-MEDIA-APPROVALS-AND-EVENT-PRESENTATION-AS-BUILT-2026-10-11.md) | Inert Message content types, already-authorized PipeProcess data framing, unconditional mic/speaker/video-capture denial, original guardian cards/pending CLI and opt-in Activity notices. | Approved typed settings/prompt/local secrets/status/inert-media/human-review projection scope implemented. Live audio/device consent/TLS/retention, browser approvals and independent OIDC human origin remain unimplemented/unproved. |
| [plans/AW-P6-9B2-DURABLE-INPUT-QUEUE-AS-BUILT-2026-10-11.md](../plans/AW-P6-9B2-DURABLE-INPUT-QUEUE-AS-BUILT-2026-10-11.md) | Encrypted durable Pending Input queue, writer/revision/source CAS, priority/fairness bounds and Pending-to-Claimed-to-mailbox/9A Accepted-to-factual Settled handshake. | Implemented explicit host-driven Input queue; later 9B2B supplies observational Notify/known-retired reclamation and 9C transports. Claimed/Accepted/Unknown are inspect-only after loss; no auto-drain, old-writer adoption or orphan-erasure promise. |
| [plans/AW-P6-9B2B-OBSERVATIONAL-NOTIFY-AND-CUSTODY-AS-BUILT-2026-10-11.md](../plans/AW-P6-9B2B-OBSERVATIONAL-NOTIFY-AND-CUSTODY-AS-BUILT-2026-10-11.md) | Observational child Notify Prepared/Accepted/Read/Unknown facts, same-process real mailbox proof, Accepted-only private read and explicit read ACK; retired-custody sweep from original ledger. | Implemented bounded observation/custody slice. No executable Control/Defer replay or safe deletion of unknown pre-ledger blobs. Remote/user auth and operations later narrow to 9C; retention/power-loss/erasure certification stays open. |
| [plans/AW-P6-9C1-AUTHENTICATED-LOCAL-ACTOR-RPC-AS-BUILT-2026-10-11.md](../plans/AW-P6-9C1-AUTHENTICATED-LOCAL-ACTOR-RPC-AS-BUILT-2026-10-11.md) | Explicit local persistent owner R12 mode binds one selected Session with tools Off; strict Actor query/disposition/enqueue/step/private-notice methods and no-replay SIGKILL/EOF semantics. | Local slice implemented; 9C2 supersedes old live-Control/remote OPEN status. Ordinary unbound R12 still denies Actor operations. No raw JSON authority, automatic queue work, remote human identity or platform/vendor certification. |
| [plans/AW-P6-9C2-REMOTE-ACTOR-AND-LIVE-CONTROL-AS-BUILT-2026-10-11.md](../plans/AW-P6-9C2-REMOTE-ACTOR-AND-LIVE-CONTROL-AS-BUILT-2026-10-11.md) | Independent reader applies exact active-Turn Control; signed MachineLink QUIC adds expiring/revocable exact Session/branch/site/writer/peer-key grants, in-flight rechecks and local-only live revoke. | Bounded Linux local/paired-machine transport slice implemented. Machine identity is not a human; tools stay Off. Broader owner UX/auth/approval, hostile-network and paid-vendor/native-platform certification remain separate. |
| [plans/AW-P6-9D1-MODEL-VISIBLE-READ-ONLY-MANAGEMENT-AS-BUILT-2026-10-12.md](../plans/AW-P6-9D1-MODEL-VISIBLE-READ-ONLY-MANAGEMENT-AS-BUILT-2026-10-12.md) | Host-only opt-in Read agent/queue/process Tools using double exact grants, actual current Dispatching Input and original active Turn; reduced metadata on at most16 registered Process handles. | Implemented observation only. Later narrow D2 Write Tools do not expand D1 permission. No queue plaintext/hash/ref/PID/raw Process output, model grant enrollment or generic actor_op. |
| [plans/AW-P6-9D2A-MODEL-ORIGIN-ENCRYPTED-QUEUE-ENQUEUE-AS-BUILT-2026-10-12.md](../plans/AW-P6-9D2A-MODEL-ORIGIN-ENCRYPTED-QUEUE-ENQUEUE-AS-BUILT-2026-10-12.md) | Separate Host Write queue_enqueue creates one canonical mq_ encrypted future Pending Input per originating Turn, Auto/priority0, no model-selected target or dispatch. | Implemented narrow host opt-in. Future owner claim/run is explicit; original ToolCall arguments may retain text. D2B/10D-E add independent dispositions/provenance, not queue recursion or free replay. |
| [plans/AW-P6-9D2B1-MODEL-QUEUE-UPDATE-DELETE-AS-BUILT-2026-10-12.md](../plans/AW-P6-9D2B1-MODEL-QUEUE-UPDATE-DELETE-AS-BUILT-2026-10-12.md) | Distinct queue_update/delete Write grants, exact Pending revision and earlier model-origin Turn custody; same-transaction canonical md_ once-per-current-Turn marker and postcommit ciphertext retirement. | Implemented; 10C-E later strengthen canonical IDs and actual source-attested successful enqueue Tool ancestry. Prefix/Settled Turn alone never grants current mutation. No user/claimed-item alteration or postcommit retry. |
| [plans/AW-P6-9D2B2A-MODEL-PROCESS-STDIN-CLOSE-AS-BUILT-2026-10-12.md](../plans/AW-P6-9D2B2A-MODEL-PROCESS-STDIN-CLOSE-AS-BUILT-2026-10-12.md) | Exact Write process_close_stdin on an opaque existing owned original PipeProcess; canonical shared mp_ Prepared before IO, same-incarnation terminal fact and restart inspect-only. | Implemented bounded stdin ownership drop, not Process exit or paid settlement. 9D2B2B1 adds separately approved terminate. Arbitrary bytes/signals/spawn/freeze and model human/Agent control remain disabled. |
| [plans/AW-P6-9D2B2B1-PROCESS-TERMINATION-AS-BUILT-2026-10-12.md](../plans/AW-P6-9D2B2B1-PROCESS-TERMINATION-AS-BUILT-2026-10-12.md) | Distinct process_terminate Write plus independently terminable Process registration/random incarnation; once-per-Turn shared mp_ and irreversible one-attempt OS teardown/reap/drain. | Implemented direct-child bounded attempt; descendants best effort and provider settlement unproved. Normal registry registration cannot terminate. Generic Process IO/signals/Agent self-control/human approval stay disabled. |
| [plans/AW-P6-9D2B2B2-FUTURE-AGENT-ADMISSION-HOLD-AS-BUILT-2026-10-12.md](../plans/AW-P6-9D2B2B2-FUTURE-AGENT-ADMISSION-HOLD-AS-BUILT-2026-10-12.md) | agent_pause_after_turn Write appends canonical ap_ Armed future Session/branch admission hold checked inside Input Prepare and Queue Claim; current Turn settles normally. | Implemented safe future hold rather than self-cancel. Only independently current idle owner can Release after factual originating Input Settled and no unresolved admissions. Unknown remains held; no universal FreezeActor. |
| [plans/AW-P6-FOUR-PRIMITIVES-MASTER-PLAN-2026-10-11.md](../plans/AW-P6-FOUR-PRIMITIVES-MASTER-PLAN-2026-10-11.md) | Normative four-primitives/two-Hooks design, R-D1..31 adoption/adaptation, P6-0..10 dependency plan and appended foundation-through-management as-built progression. | Bounded foundation, process, Hook, child, queue, RPC and narrow management slices implemented; entire aspirational all-owner/platform/approval contract and external certification not blanket complete. Current TODO/source supersede dated next prompts and removed archive gates. |
| [plans/AW-UX-V1.5-HERDR-MINIMAL-VISUAL-LANGUAGE-2026-10-15.md](../plans/AW-UX-V1.5-HERDR-MINIMAL-VISUAL-LANGUAGE-2026-10-15.md) | Additive v1.5 semantic roles, borderless canvas/separators/minimal tabs, blue prompt/unboxed prose/factual tools and all-surface visual QA stages. | Design plus later bounded shipping slices; current live TODO stays open, % Windows example superseded by ! and & Agents; no reserved UX access. |
| [plans/CURRENT-AW-DECISIONS-AND-PENDING-2026-10-11.md](../plans/CURRENT-AW-DECISIONS-AND-PENDING-2026-10-11.md) | October11 product rationale, module/feature cuts, Domain/Session/account authority, Phase0..68 and R/E map, canonical-only CLI decision and 19-row open register. | Dated source SHAs/schema/required comprehensive-read and evidence-tree workflow are superseded by rebuilt Git/current instructions. Native targets, release, real vendor/security and deliberately closed/parked features remain current-plan gates; no Phase69 or compatibility aliases. |
| [plans/PHASE-2-VALIDATION.md](../plans/PHASE-2-VALIDATION.md) | Early Python/JS/static/fake-browser receipts explicitly distinguish authored Rust tests from executed runtime. | Historical authoring-only limitations accepted as provenance; do not treat old unexecuted Rust/Nix gates or absent archive artifacts as current bugs/gates. |
| [plans/PHASE11-HARDENING.md](../plans/PHASE11-HARDENING.md) | Sticky stream-fault state, post-intent authorization rechecks, summary-purpose/evidence checks and shared extension validation. | Accepted historical hardening; trusted native blocking/cooperative effects and non-atomic external revocation limits persist; old compiler-host TODO superseded by later scoped product evidence. |
| [plans/PHASE12-DURABLE-SESSIONS.md](../plans/PHASE12-DURABLE-SESSIONS.md) | Schema3 logical operation/attempt lineage, retry hints/schedules/claims, original context pins, semantic/ref events and attachment-conflict preservation. | Accepted; autonomous scheduler primitive was not a runner, provider status reconciliation remains capability-specific and unknown tools block automatic retry; old native checklist is historical. |
| [plans/PHASE13-DOMAINS-WORKSPACES.md](../plans/PHASE13-DOMAINS-WORKSPACES.md) | Schema4 Domain/Workspace/Machine/Fleet identity/target foundation, reserved local and canonical resource CLI. | Accepted; transport/federation/fan-out/HA/auth/direct-mode missing-at-phase13 supplied only by later bounded phases; richer aggregate UX/managed surfaces remain separate, no local execution of remote target. |
| [plans/PHASE14-AWNODE-QUIC-PAIRING.md](../plans/PHASE14-AWNODE-QUIC-PAIRING.md) | Stable Ed25519 node identity, private seed, exact certificate-pinned one-use QUIC invitation and signed peer trust. | Accepted native checkpoint supersedes retained pre-native appendix; pairing-only boundary, fail-closed missing identity/rekey/revoked trust and no credential grant remain; later transport/services separate. |
| [plans/PHASE15-DIRECT-REACHABILITY.md](../plans/PHASE15-DIRECT-REACHABILITY.md) | STUN/signed rendezvous candidates, same-socket UDP hole punch and direct authenticated QUIC. | Accepted; later fallback/MachineLink steps implemented separately; route failure remains no target mutation, policy grant or effect replay. |
| [plans/PHASE16-ROUTE-FALLBACKS.md](../plans/PHASE16-ROUTE-FALLBACKS.md) | Ordered direct/opaque UDP QUIC relay/end-to-end TLS TCP fallback and route traces. | Accepted; typed MachineLink came later; reusable multi-room TCP service was not delivered by bounded one-pair fallback, and generic tunnels remain excluded. |
| [plans/PHASE17-MACHINELINK.md](../plans/PHASE17-MACHINELINK.md) | Typed signed bounded capability negotiation, requested/server-ceiling intersection and short leases. | Accepted; service enum names do not implement services; read services and remote Session operations came later, no generic shell/execute authority. |
| [plans/PHASE18-REMOTE-READ-SERVICES.md](../plans/PHASE18-REMOTE-READ-SERVICES.md) | Exact-capability metadata/Workspace/file/Git/redacted Session reads with serving-host fixed roots/executables. | Accepted; remote PTY remained an inactive placeholder at this boundary, generic command/forwarding excluded; Phase19+ separately supplied durable Session control. |
| [plans/PHASE19-23-CONTINUATION-STATUS.md](../plans/PHASE19-23-CONTINUATION-STATUS.md) | Composite historical Phase19–23 implementation/native continuation plus later24–35 updates, including Phase29 handoff/failback record. | Accepted historical scope; old dirty-worktree/native gate/tag/phase.json/helper instructions obsolete; latest composite updates outrank its old next steps, current plans own actual unfinished external gates. |
| [plans/PHASE19-PEER-REMOTE-SESSIONS.md](../plans/PHASE19-PEER-REMOTE-SESSIONS.md) | Schema5 stable PeerWorkspace and exact-operation RemoteSessionRef creation/idempotency/response-loss unknown state. | Accepted; remote transcript authority stays remote; generic execution/inference binding was absent until later typed control, no competing source transcript or unknown replay. |
| [plans/PHASE20-FLEET-FANOUT.md](../plans/PHASE20-FLEET-FANOUT.md) | Schema6 pinned membership operation/member snapshot and durable per-member/aggregate status. | Accepted; dispatch came inPhase24; retry never re-resolves membership, unknown member effects remain individually unresolved, rich Fleet UI/placement is separate. |
| [plans/PHASE21-RELAY-SERVICE.md](../plans/PHASE21-RELAY-SERVICE.md) | Reusable bounded multi-room UDP relay with TTL/capacity and opaque traffic statistics. | Accepted; connectivity-only, no trust/policy/capability role; general managed operations/multi-room TCP remain outside this slice. |
| [plans/PHASE22-FEDERATION-LEASES.md](../plans/PHASE22-FEDERATION-LEASES.md) | Schema7 directional scoped federation with short exact epoch/peer/action leases and revocation. | Accepted; typed network carriage/later orchestration supplied separately; no transitive re-sharing, descendants or peer-pairing grant by default. |
| [plans/PHASE23-SEMANTIC-SYNC.md](../plans/PHASE23-SEMANTIC-SYNC.md) | Schema8 immutable bounded inbox/cursors, source/event digest identity and exact retry with authorization sequence gaps. | Accepted; network carriage/materialization supplied later; evidence import remains incapable of canonical mutation/raw WAL replay, snapshot retention/hosted scale remain design concerns. |
| [plans/PHASE24-REMOTE-ORCHESTRATION.md](../plans/PHASE24-REMOTE-ORCHESTRATION.md) | Schema9 typed persistent remote prompt/status/transcript/cancel and pinned per-member Fleet dispatch. | Accepted; dropped route/dead process leaves durable uncertainty; no raw DB replication/merged filesystem/credential transfer/generic tunnel; old helper assertions are not current runnable tools. |
| [plans/PHASE25-SEMANTIC-MATERIALIZATION.md](../plans/PHASE25-SEMANTIC-MATERIALIZATION.md) | Schema10 exact owner/admin selector policy plus explicit source-event materialization into immutable local remote facts. | Accepted; context projection supplied byPhase33, Domain HA by26–27; no wildcard/automatic inbox consumer or source-mutation replay; revocation prospective and historical facts remain. |
| [plans/PHASE26-DOMAIN-AUTHORITY-HA.md](../plans/PHASE26-DOMAIN-AUTHORITY-HA.md) | Schema11 AuthoritySet majority terms/votes/leader leases, explicit foreign voter acceptance and CP policy-write fencing. | Accepted; standby state transfer/promotion supplied by27; already-admitted bounded capability survives independent leader expiry, no global SQL multi-writer. |
| [plans/PHASE27-AUTHORITY-FAILOVER-COORDINATION.md](../plans/PHASE27-AUTHORITY-FAILOVER-COORDINATION.md) | Schema12 signed minimal authority-core standby snapshot/promotion after old lease expiry and policy-neutral reusable rendezvous. | Accepted; Workspace writer/handoff and human auth supplied28–30; generic Domain merge, login/secret/full resource replication excluded, managed operations not certified. |
| [plans/PHASE28-WORKSPACE-WRITER-EPOCHS.md](../plans/PHASE28-WORKSPACE-WRITER-EPOCHS.md) | Schema13 exact writer-epoch failover/CAS and immutable ref provenance, paid results preserved without stale attachment. | Accepted; operational semantic Workspace replica/ref service supplied29; no raw WAL/LWW/second owner, old Clippy/Nix caveat historical and current release gates separate. |
| [plans/PHASE30-FRESH-REVIEW.md](../plans/PHASE30-FRESH-REVIEW.md) | Post-MVP concrete trust-reaccept, stable-principal, artifact collision, migration, digest/idempotency, Domain ID, import authority, writer-wire and key/CLI/status fixes. | Accepted fixes; no newPhase31 claim from this review; maintain no raw principal fallback when stable projection exists, no credential revival without fresh sync and no read-side identity creation. |
| [plans/PHASE30-IDENTITY-CONTINUITY.md](../plans/PHASE30-IDENTITY-CONTINUITY.md) | Schema15 stable-user cross-host authorization, explicit signed public continuity metadata and live device proof with independent revocation. | Accepted core distributed MVP boundary only; native auth/SecretStore/OIDC came31–32, password/passkey/browser middleware and complete public-hosted/security parity not implied. |
| [plans/PHASE31-AUTH-SECRET-STORE.md](../plans/PHASE31-AUTH-SECRET-STORE.md) | Schema16 hash-only rotating native auth families and versioned encrypted SecretStore/shared account grants, endpoint and dispatch pinning. | Accepted; OIDC/OAuth supplied32; passkey/password ceremonies, browser middleware, external KMS/HSM/keychain and generic secret replication remain separate/unimplemented by this phase. |
| [plans/PHASE32-OIDC-OAUTH.md](../plans/PHASE32-OIDC-OAUTH.md) | Schema17 exact verified issuer+subject PKCE/state/nonce/JWKS OIDC using native families and distinct user-owned OAuth connectors with SecretStore token refs. | Accepted; context projection supplied33; live external provider/SSO interoperability not proved by loopback RSA/JWKS fixtures, no scope widening/email merge/automatic code replay. |
| [plans/PHASE33-MATERIALIZED-CONTEXT.md](../plans/PHASE33-MATERIALIZED-CONTEXT.md) | Schema18 exact non-generative fact projection then separate target apply, bound context policy and transitive revocation provenance; Workspace snapshotv3. | Accepted; local coordination/direct supplied34–35; materialized/history/archive data cannot mint target/provider authority; handoff preserves provenance without source policy grants. |
| [plans/PHASE34-SQLITE-COORDINATION.md](../plans/PHASE34-SQLITE-COORDINATION.md) | Schema19 process/store generations, write/execution hazards, maintenance barrier, bounded Busy/Locked, staged restore and real two-transaction paid settlement. | Accepted; public direct API supplied35 after review; raw WAL/network SQLite and expiry-as-stopped excluded; historical internal restore proof cannot activate quarantined restored archives. |
| [plans/PHASE35-TRUSTED-LOCAL-DIRECT.md](../plans/PHASE35-TRUSTED-LOCAL-DIRECT.md) | Schema20 process-owned live-state recovery, exclusive owner/shared explicit direct locks, exact-schema Linux local opener and conservative ordinary-access lease. | Accepted; exact-current schema is now33, not20; no create/migrate/restore/network-FS/direct fallback; finer concurrency optimization requires new proof; CLI/onboarding supplied36–37. |
| [plans/PHASE36-CLI-ARCHITECTURE.md](../plans/PHASE36-CLI-ARCHITECTURE.md) | Typed composable Clap root/resource/role tree replaces argv reparsing and makes help side-effect free. | Accepted; onboarding supplied37; no authority/schema changes, old helper/profile counts not live gates. |
| [plans/PHASE37-CLOSEOUT-AUDIT-AND-RELEASE.md](../plans/PHASE37-CLOSEOUT-AUDIT-AND-RELEASE.md) | Unnumbered UX/UI-extension, backend API/JSONL/Pi, platform abstraction and five-target reproducible release closeout/audit. | Accepted bounded foundations; later38+ supersede old multiplayer/pipeline/routing deferrals; external extension-host process, advanced Solid migration/Git UX, native platform certification/signing/live OAuth/release gates remain separately scoped. |
| [plans/PHASE37-DESIGN-ADAPTATION.md](../plans/PHASE37-DESIGN-ADAPTATION.md) | Unnumbered reviewed native pointer/navigation/layout changes, Solid root and dedicated Responses plus supported ChatGPT OAuth/SecretStore flow. | Accepted; original no-subscription-token-through-Chat-Completions invariant remains; live account login/returning reauthorization/failed-first-exchange refinement not proved by old automation; later UI/status docs own current shape. |
| [plans/PHASE37-FIRST-RUN-ONBOARDING.md](../plans/PHASE37-FIRST-RUN-ONBOARDING.md) | Shared profile-update authority for native first-run/Settings/web setup, skip/draft preservation, explicit catalog GET and context-clearing provider switch. | Accepted; dedicated subscription Responses path supplied unnumbered adaptation; save stays network-free, explicit login/discovery separate; old stop-before38 reflects historical authorization later superseded. |
| [plans/PHASE38-SHARED-DRAFTS.md](../plans/PHASE38-SHARED-DRAFTS.md) | Schema21 explicit private-to-shared Yrs/Yjs prompt collaboration, separate stable-user draft ACL, exact durable seal/submission and ephemeral presence. | Accepted narrow browser collaboration; native draft participation and true rendered two-browser visual execution not supplied by HTTP-client smoke; no file/PTY collaboration or credential/tool grant. |
| [plans/PHASE39-BOUNDED-PIPELINES.md](../plans/PHASE39-BOUNDED-PIPELINES.md) | Schema22 immutable manual linear pipelines/approvals/ordinary Agent steps plus SQLite diagnostics and native view fixes. | Accepted; schema23 compatibility repair and40+ routing/delegation came later; old deferred schedules/DAGs must be reconciled with later phases, no replay of unknown external step or arbitrary pipeline code. |
| [plans/PHASE40-ROUTING-AFFINITY.md](../plans/PHASE40-ROUTING-AFFINITY.md) | Schema24 strict tool-free bounded main-target routing, destination-pinned configured candidates, actor/branch-lineage affinity and consumed working receipt. | Accepted; children/routing/team/planner supplied41–50; no unauthorized candidate/fallback/hidden classifier or automatic price optimization; loopback evidence not live billing proof. |
| [plans/PHASE41-SUBAGENT-SESSIONS.md](../plans/PHASE41-SUBAGENT-SESSIONS.md) | Schema25 native-owner opt-in ordinary child Sessions, exact parent/invocation lineage, model-only same-root exception and durable foreground/background state. | Accepted; isolated file tools/budgets/recursion/routing/team/planner supplied42–50; native/browser/shared/cross-machine spawn authority not implied, no automatic parent transcript/UI disclosure or replay. |
| [plans/PHASE42-ISOLATED-SUBAGENT-WORKTREES.md](../plans/PHASE42-ISOLATED-SUBAGENT-WORKTREES.md) | Schema26 durable planned/ready/bound/cleanup/unknown managed Git worktrees, clean HEAD pin and host-granted read-only/workspace child tools. | Accepted; recursion/routing/budgets supplied43–45; dirty-parent/non-Git snapshots, trusted shell/sandbox, auto-merge/stage/commit/import/prune and cross-machine/shared spawning remain separate milestone deferrals. |
| [plans/PHASE43-DELEGATION-BUDGETS.md](../plans/PHASE43-DELEGATION-BUDGETS.md) | Schema27 one root-turn durable created/active/model-attempt/deadline envelope, conservative failure consumption and result-first usage settlement. | Accepted; depth>1/routing/teams supplied44–46; per-child reset budgets, monetary accounting, trusted shell and remote/browser spawning not granted; unknown usage remains incomplete. |
| [plans/PHASE44-RECURSIVE-SUBAGENTS.md](../plans/PHASE44-RECURSIVE-SUBAGENTS.md) | Schema28 additive bounded depth1–4 lineage, same-root accounting and equal-or-narrower tools with clean sibling worktrees. | Accepted; child routing/team/planner/guardian supplied45–50; no foreground parallelism claim, reset envelope, dirty-state delegation, shell, auto Git import or remote/shared spawning. |
| [plans/PHASE45-CHILD-ROUTING.md](../plans/PHASE45-CHILD-ROUTING.md) | Schema29 root-pinned owned worker strategy, shared classifier budget, one consumed route per child and exact identity before worktree/provider entry. | Accepted; teams supplied46; model-selected policies, per-child budget/strategy reset, monetary settlement, shell/auto merge and remote/shared placement remain outside this slice; no reroute of failed/unknown operation. |
| [plans/PHASE46-MULTI-MODEL-TEAMS.md](../plans/PHASE46-MULTI-MODEL-TEAMS.md) | Schema30 immutable2–4-member sequential foreground children/all-required deterministic join and parent-only synthesis, IDs/digests not duplicate answers. | Accepted; planner/guardian supplied49–50, later reviewer/messaging must be reconciled; alternate joins, parallel candidates, team-of-teams, role routes, independent synthesis, monetary budgets and remote/auto-merge not implied. |
| [plans/PHASE47-INTERACTION-POLISH.md](../plans/PHASE47-INTERACTION-POLISH.md) | Native palette filters, rendered hit geometry/narrow folding, scroll offsets, bounded read-only cell selection and local OSC52 copy. | Accepted presentation-only/schema30; auto-scroll selection and guaranteed OSC52 terminal support not claimed, no filesystem/model/authority change. |
| [plans/PHASE48-AGENT-REACTIVATION.md](../plans/PHASE48-AGENT-REACTIVATION.md) | Transactional saved-binding native Session reactivation and rollback, exact provider fingerprint/root reconstruction, placeholder DIM correction. | Accepted schema30; ordinary transfer guard/unresolved-effect refusal remain; no root migration, profile repair, account-selector restoration, remote dispatcher or implicit context transfer. |
| [plans/PHASE49-GENERATIVE-PLANNER.md](../plans/PHASE49-GENERATIVE-PLANNER.md) | Schema31 root-only strict fixed planner proposes1–6 typed acyclic role-allowed tasks; sequential ordinary children and verified digest-only join. | Accepted; guardian supplied50 and later result messaging/schedules need reconciliation; parallel execution, recursive plan tool, arbitrary code/grants, price budgets, remote placement and auto merge absent from this slice. |
| [plans/PHASE50-GUARDIAN-REVIEW.md](../plans/PHASE50-GUARDIAN-REVIEW.md) | Schema32 restricted fixed tool-free review over verified team/planner joins with pass/deny/needs_human/require_evidence, durable owner approval and exact reconstruction release. | Accepted; current schema33 comes later; review pass never grants capability; multiple review DAGs, automatic evidence solicitation, monetary budgets, remote placement/auto-merge and unknown-call replay not implied. |
| [plans/PHASE51-NATIVE-AGENT-UX.md](../plans/PHASE51-NATIVE-AGENT-UX.md) | Shared semantic transcript/tool/menu palette and explicit lossless switches; private reasoning stays private. | Accepted historical foundation; later prompts, modes/ownership, popup/status geometry supersede original specifics. |
| [plans/PHASE52-NATIVE-MARKDOWN.md](../plans/PHASE52-NATIVE-MARKDOWN.md) | One settled-answer safe bounded Markdown projection with exact rendered/copy/search geometry and literal unsafe fallback. | Accepted; partial streaming remains plain and tools/prompts/status are not interpreted Markdown. |
| [plans/PHASE53-NATIVE-CODE-HIGHLIGHTING.md](../plans/PHASE53-NATIVE-CODE-HIGHLIGHTING.md) | Shared lazy Syntect catalog/theme for file/diff/settled fences with neutral minimal fallback and no I/O from language labels. | Accepted feature-bound pure projection; no new parser/theme authority or compulsory minimal syntax dependency. |
| [plans/PHASE54-AGENT-PRESENTATION-MODES.md](../plans/PHASE54-AGENT-PRESENTATION-MODES.md) | Agent versus Workspace application ownership, three physical presentations and one persistent presenter/state through switches. | Accepted foundation; original full-primary Inline reservation and standalone mode slash verbs later superseded. |
| [plans/PHASE55-TERMINAL-LIFECYCLE-AND-MINIMAL-CHROME.md](../plans/PHASE55-TERMINAL-LIFECYCLE-AND-MINIMAL-CHROME.md) | Generation-safe shared terminal restore, idle double-Ctrl+C, shared runtime-truth chrome and quieter Workspace labels. | Accepted; later owner-thread panic repair/natural flow/default status refine behavior; no SIGKILL/power-loss restore guarantee. |
| [plans/PHASE56-ASYNC-UI-AND-SCROLLING.md](../plans/PHASE56-ASYNC-UI-AND-SCROLLING.md) | One bounded human-UI work pool, typed generations, progressive syntax and local viewport/overscroll, owner separate. | Accepted; no durable/model work in pool and no indexing/watch/LSP. Later stale/cancel/backpressure repairs retained. |
| [plans/PHASE57-NATURAL-FLOW-AND-SCROLLBACK-EXIT.md](../plans/PHASE57-NATURAL-FLOW-AND-SCROLLBACK-EXIT.md) | Content-sized natural Inline/Scrollback flow and clean scratch handoff; short content does not fabricate bottom reservation. | Accepted substrate; fixed scratch and temporary above-composer default superseded by Phase58/post58. |
| [plans/PHASE58-SCROLLBACK-SATURATION.md](../plans/PHASE58-SCROLLBACK-SATURATION.md) | Dynamic visible-chrome scratch rows, actual-content sticky saturation, exact reanchor and clean shell exit. | Accepted substrate; later status row count/padding and Inline history policy take precedence. |
| [plans/PHASE59-LOADED-RESOURCE-PROVENANCE.md](../plans/PHASE59-LOADED-RESOURCE-PROVENANCE.md) | Typed runtime-admitted Context/Skills metadata, header/cache fingerprints and truthful saved prompt provenance. | Accepted; actual skills later enabled by Phase60; no render-time discovery or reconstructed filenames. |
| [plans/PHASE60-EXPLICIT-SKILL-LOADER.md](../plans/PHASE60-EXPLICIT-SKILL-LOADER.md) | Explicit named bounded UTF8 skill instructions, path privacy, runtime metadata and unchanged tool/account authority. | Accepted; old --skill is archival spelling, use canonical --agent-skill; R11 adds separate explicit project trust. |
| [plans/PHASE61-PERSISTENT-SKILL-REGISTRY.md](../plans/PHASE61-PERSISTENT-SKILL-REGISTRY.md) | Secret-free explicit local skill catalog with named or one-run activation through same loader. | Accepted full management surface; installed is not active and edits do not mutate prior Session binding. |
| [plans/PHASE62-SOLID-AGENT-PARITY.md](../plans/PHASE62-SOLID-AGENT-PARITY.md) | Active Solid runtime resource labels/canonical settings/safe thinking and tool-hidden evidence, factual status only. | Accepted bounded browser slice; historical workbench is not active root and no browser authority/TPS invention. |
| [plans/PHASE63-RICH-MARKDOWN-MEDIA.md](../plans/PHASE63-RICH-MARKDOWN-MEDIA.md) | Native/browser tables/tasks/nesting/strike and inert media links, no automatic fetch/HTML/terminal controls. | Accepted; full media/graphics and indexing/LSP not included. |
| [plans/PHASE66-PORTABLE-SKILL-BUNDLES.md](../plans/PHASE66-PORTABLE-SKILL-BUNDLES.md) | Versioned deterministic instruction-only bundle, content digest/addressed managed install and explicit registry operations. | Accepted; integrity is not signature/grant, no executable package or auto-activation/unknown-file deletion. |
| [plans/PHASE67-GOVERNED-PIPELINE-SCHEDULES.md](../plans/PHASE67-GOVERNED-PIPELINE-SCHEDULES.md) | SQLite33 interval occurrence layer over exact existing active user task bindings, claim/generation/idempotent pipeline reuse. | Accepted with post68 recovery/no-burst/reserved-namespace fixes; no cron/webhook/model/Session scheduling implied. |
| [plans/PHASE68-RELEASE-HARDENING.md](../plans/PHASE68-RELEASE-HARDENING.md) | Five-target artifact matrix/manifest/sidecars/format audit and one materialize/publish contract. | Accepted source/release architecture; old environment blockers and candidate-execution installer rule superseded; external signing/native release gates open. |
| [plans/POST-PHASE58-FRESH-REVIEW-HARDENING-2026-10-07.md](../plans/POST-PHASE58-FRESH-REVIEW-HARDENING-2026-10-07.md) | Physical cursor versus authorized sync disclosure, atomic local archive, shared composer/PTY ownership, immediate tmux capability, retryable config and bounded titles. | Accepted fixes; stale phase.json metadata statements are history because blob was removed. |
| [plans/POST-PHASE58-REVIEW-2026-10-06.md](../plans/POST-PHASE58-REVIEW-2026-10-06.md) | Panic-owner/layout generation/search cancellation/syntax backpressure/cache lifetime fixes and initial decomposition/provenance/config debt. | Accepted fixes; subsequent phases address proposed debt, do not report old absent features as live findings. |
| [plans/POST-PHASE58-UX-COMMAND-HARDENING-2026-10-07.md](../plans/POST-PHASE58-UX-COMMAND-HARDENING-2026-10-07.md) | One slash/palette/composer, real effort, auxiliary paid receipts, typed status/performance, current Inline overflow and coalescing config. | Accepted current semantic basis; canonical long CLI naming and & Agents source added later. |
| [plans/POST-PHASE58-UX-HARDENING-2026-10-06.md](../plans/POST-PHASE58-UX-HARDENING-2026-10-06.md) | Editor/readline family, blue prompt, work animation, padding, initial status/session visibility, exact tab hits and resize/exit cleanup. | Accepted first pass; later shared reducer/per-user atomic archive/TPS/current status semantics replace intermediate choices. |
| [plans/POST-PHASE58-WORKSPACE-DECOMPOSITION-2026-10-07.md](../plans/POST-PHASE58-WORKSPACE-DECOMPOSITION-2026-10-07.md) | One Ui state partitioned into façade/commands/interaction/async/render modules, preserving typed existing services. | Accepted maintenance architecture; no second controllers, registries/pools or renderer replacement. |
| [plans/POST-PHASE59-UX-HARDENING-2026-10-08.md](../plans/POST-PHASE59-UX-HARDENING-2026-10-08.md) | Unified /tui plus /workspace and Workspace /minimal; exact configured horizontal inset/semantic vertical bands; fenced readable CodeMirror. | Accepted current mode grammar; old standalone slash modes removed; original browser-test limitations scoped historically. |
| [plans/POST-PHASE68-FRESH-REVIEW-HARDENING-2026-10-08.md](../plans/POST-PHASE68-FRESH-REVIEW-HARDENING-2026-10-08.md) | Schedule orphan recovery/exact existing-run attach/no-burst, locked three-way config merge, skill atomic paths/export and safe release installer probes. | Accepted successor; same-origin checksums not publisher identity and external release/signing/live gates remain separate. |
| [plans/R14-5-CANONICAL-ONLY-R13-CLOSEOUT-2026-10-10.md](../plans/R14-5-CANONICAL-ONLY-R13-CLOSEOUT-2026-10-10.md) | Explicit canonical-only long flags/schema v2, no aliases/migration/comparator, first-party conformance and parked E6. | Latest dated CLI direction verified in live source; breaking release/version/distribution and external gates remain open. |

## 12. Prompt to give the next coordinating agent

```text
Continue work in /home/pvl/spaces/abird/src/aw.

First read .agents/plans/TODO.md, .agents/plans/workspace-views/TODO.md,
.agents/plans/TODO-SPEC.md and .agents/AGENTS.md. Then read
.agents/docs/HANDOFF.md in full and the affected live specifications/source.
Confirm current HEAD and owned worktree state; the handoff's starting source
checkpoint is 04bb9e000ce0d7406972bf564bb9a0c744fe9698, not a demand to reset.

Use subagents for implementation and independent parallel work. Act mainly as
coordinator, architect, reviewer, integrator and gatekeeper. Agree exact owned
paths/interfaces first; use isolated worktrees for overlapping edits and exclude
the reserved .agents/plans/ux/ BEFORE checkout. Never list/read/write/stage or
inspect that subtree or its staging. Preserve all other owners' work.

Start the measured left-right redesign described in the Workspace plan: benchmark
the existing publication helper against browser Session display snapshots and
bounded multiplayer presence. Preserve fresh authority, sequence/TTL, receipt,
CAS and effect-admission boundaries; migrate only measured wins. Run useful
independent UX inventory/native acceptance in parallel. Docking follows afterward.

Keep one original Agent/TurnMachine/ToolRegistry/ToolPolicy/TurnJournal and Runtime,
Actor, Message, Envelope with Next/Defer. Views never retarget execution. Version
0.15.0, SQLite33, JSONL1 and current native layout v4 remain unless authorized.
Preserve provider privacy, account/payer/executor custody, exact grants/revocation,
intent-before-effect, UNKNOWN outcomes and filesystem/SQL restoration quarantine.
No new numbered phase, old Queue replay or runnable restoration is authorized.

Track own work in .agents/runs/<plan>-<session-agent>/TODO.md. Only the coordinator
atomically reconciles shared plan TODOs at an expected-parent Git checkpoint.
Execute real tests in the pinned Nix shell, record exact commit/commands/results
and preserve unexecuted native/provider/release gates. Commit reviewed owned work
with signing at sane checkpoints; never push/tag/publish/deploy or perform
destructive/history operations without explicit authorization for that action.
Remove only your stopped synthetic fixtures and reconciled scratch. Continue
autonomously within authorized scope, with concise progress updates.
```

To continue a different plan, replace only the selected plan and bounded next task
in this prompt. Its repository, ownership, safety and verification instructions
continue to apply. A successor should refresh this file's snapshot sections only
when that helps onboarding; authoritative progress always goes into the plan TODO.
