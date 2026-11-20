# DL-200 — sealed design + implementation gate

Base: `f035a00031b8f5879f4d77f3fd14bfe02b89305c`

Status: **SEALED / IMPLEMENTATION PLAN READY — NOT PROMOTED**

## Committed

None. DL-200 remains uncommitted sealed plan/documentation work; implementation is not promoted.

## Resolved in the current uncommitted design

### A. Immutable history / construction

- [x] Define immutable `HistorySpan` / `HistoryBase` storage identity.
- [x] Define immutable `HistoryGrant` disclosure/provenance closure for cross-Agent sharing.
- [x] Preserve exact inherited attribution across forks.
- [x] Define redacted/anonymized projection as a new HistoryBase.
- [x] Define reachability-based retention/GC without crash-sensitive refcounts.
- [x] Preserve legacy internal SessionStore/branch/journal/recovery ownership.
- [x] Define exact settled-Turn fork boundary and inherited next-Turn state.
- [x] Replace host `agent.fork` with the single `agent.create(from=...)` constructor.
- [x] Rename model ancestry selector to `agent(..., from=...)`.
- [x] Keep `checkpoint.restore` as the stronger exact history+SourceSet recovery API.
- [x] Fix model exact-history default: source Project + inherited source ACL, with trusted
      host narrowing only unless ordinary share/redirect authority permits more.

### B. Revisions/access/groups/principals

- [x] Define persisted owner-local Agent `rev` in `agent_state_current`.
- [x] Seed once from the legacy Session-audit bridge, then stop deriving public rev from audit.
- [x] Keep group membership changes on group `rev`; do not fan out Agent rev writes.
- [x] Define derived `access_generation` from Agent ACL + referenced group/principal security generations.
- [x] Confirm one allow-only direct user/group ACL and no deny entries in v1.
- [x] Define one Domain principal-handle registry across user/group kinds.
- [x] Keep Project as a principal-scoped public view over existing Workspace backing.
- [x] Define minimal host metadata reads: `domain.list/get`, `principal.list/get`, `project.list/get`.

### C. Generic Events substrate

- [x] Make existing immutable `commit_log` the one Event history/order owner.
- [x] Split generic source-local Event revision from recipient-owned pattern subscriptions.
- [x] Keep one durable Event truth; no second bus/database.
- [x] Preserve source-local `event_rev` continuity, including one revision for multiple factual Events from one transition.
- [x] Add stable opaque `Event.id` for provenance/idempotent durable projection.
- [x] Finalize canonical resource -> Event subject projection, including Domain/Principal/Project roots.
- [x] Support typed one-token `k:*` and terminal `>` one-or-more semantics.
- [x] Keep Event patterns typed separately from concrete ExecutionRef.
- [x] Define custom Event namespace as `#name` / `e:#name`; all bare names are system-owned.
- [x] Reject partial-token custom wildcards such as `e:#*`; `e:*` matches one system/custom Event token.
- [x] Define rebuildable broad-pattern Event projection index; no per-source wildcard materialization.
- [x] Define opaque `events.read` cursor state bound to caller + normalized pattern.
- [x] Define 24h idle cursor retention and logical replay floor: all last 7 days and at least latest 65,536 Event rows.
- [x] Prevent hidden source counts/positions/gaps leaking through cursors.
- [x] Define durable subscription generation/baseline/scan/lag semantics.
- [x] Define bounded `events.subscriptions.list` without separate get/edit/delete verbs.
- [x] Define deterministic replay-safe EventInput delivery into the existing Agent input owner.
- [x] Define model `events_subscribe` current-Agent recipient rules.
- [x] Define `events.publish` / `events_publish` as custom-`#` publication only.
- [x] Fix custom Event data: JSON object, canonical encoding <= 4096 bytes, nesting <= 8.
- [x] Fix model custom publication budget: <= 32 successful publishes per Turn.
- [x] Fix active subscription cap: <= 128 per recipient Agent.
- [x] Fix read bounds: limit <= 256, wait <= 30s, internal scan <= 4096 rows/call.
- [x] Fix causal delivery bounds: <= 16 Event hops and <= 256 EventInput deliveries/root.
- [x] Suppress self-delivery in v1.
- [x] Define exact source + recipient + Domain/federation authorization for every concrete delivery.
- [x] Reject a separate EventLink service; ordinary resource/share authority remains authoritative.
- [x] Define v36 SourceEventBook/pending-delivery migration without a long-lived parallel runtime.
- [x] Keep `jobs_get(rev, wait_ms)` as model convenience over owner rev + shared wake substrate.

### D. Checkpoints / SourceSet

- [x] Define checkpoint = exact immutable HistoryBase/head + immutable named SourceSet, including captured SourceBinding access/checkpoint-policy layout metadata.
- [x] Allow an empty SourceSet for explicit/history-only capture.
- [x] Define typed `SourceBinding` / `SourceDriver` / `SourceAnchor` contracts.
- [x] Keep Git as the mandatory first SourceDriver.
- [x] Define Git clean commit anchor and pin every live clean checkpoint commit with a private retention ref.
- [x] Define dirty synthetic commit/tree using a private temporary index + private retention ref.
- [x] Define per-source private checkpoint retention refs and reconciled GC.
- [x] Capture final tracked state plus non-ignored untracked regular files; exclude ignored/Git/AW-external state.
- [x] Preserve tracked symlinks as Git entries; do not follow unsafe untracked links.
- [x] Define submodule gitlink behavior; dirty initialized submodules fail closed; restore never auto-fetches.
- [x] Reuse aw-git fail-closed clean/smudge/process-filter and partial-clone/promisor policy.
- [x] Intentionally collapse staged-vs-unstaged distinction into final captured Git tree in v1.
- [x] Define multi-source all-or-nothing mutation fencing/revalidation.
- [x] Define versioned/content-addressed object/bucket manifest anchors without generic archive fallback.
- [x] Define restore into a new Agent + new Sandbox with every captured source materialized.
- [x] Define checkpoint tombstone/provider-retention crash reconciliation.
- [x] Explicitly exclude Vault grants/secrets/live leases/runtime state from checkpoints.

### E. Logical Sandboxes

- [x] Define Sandbox as durable logical mutable execution state, not a persistent OS namespace.
- [x] Keep Sandbox ancestry orthogonal to history ancestry and security authority.
- [x] Define minimal host `sandbox.list/get/create/close`.
- [x] Define only `sandbox.sources.list/set` for named source attachment/removal; no second Source service.
- [x] Keep SourceRef provider/driver identity separate from Vault credentials/authority.
- [x] Define shared mode by binding multiple Agents to one Sandbox ref.
- [x] Define branch mode as `sandbox.create(from=parent)` with structural/COW source sharing where available.
- [x] Define fresh mode as `sandbox.create(project=...)` from Project/Workspace baseline.
- [x] Keep model subagents from selecting raw sandbox/mount/network/device settings.
- [x] Define trusted delegation policy for share/branch/fresh child mode.
- [x] Define per-Agent actor-sensitive SandboxGrant intersection + durable grant evidence; same Sandbox != same authority.
- [x] Keep Agent->Sandbox binding immutable in v1; change environment by creating a new Agent, not live rebind.
- [x] Define Sandbox lifecycle `open|closed`, quiescent source replacement/derivation and no force-close/live-remount semantics.
- [x] Keep actual Process containment fresh per dispatch from current Sandbox state + authority.
- [x] Keep frozen 017 mount/Bubblewrap plan as the separate lower implementation owner.

### F. Vault capability projection

- [x] Define provider-backed `VaultEntryRef` / `VaultGrant` instead of mounting a whole vault.
- [x] Prefer broker/socket/short-lived credential projections; support bounded file/env fallback through provider/host-defined safe projection profiles, never arbitrary env/path injection.
- [x] Define actor-sensitive Vault authority even for Agents sharing one Sandbox; ACL broadening never widens an existing grant.
- [x] Define revocation truthfully: future projections stop/revoke when possible; already-materialized env/file capability requires factual Process termination when immediate cutoff is needed.
- [x] Define minimal `agent.vault.list/set` attachment surface.
- [x] Keep secret creation/rotation/storage with the VaultProvider/embedding host.
- [x] Never return raw secret bytes from ordinary metadata reads.
- [x] Never inherit Vault authority merely from history fork, Sandbox sharing/branching, ACL inheritance or checkpoint restore.

### G. Extensions / Pi parity / Histate seams

- [x] Reclaim `Extension` for the installed/composable unit and use one typed `ExtensionRegistry` composition root.
- [x] Define typed subregistries under ExtensionRegistry and one shared `register/remove/get/list/revision` lifecycle where meaningful.
- [x] Stage all contributions from one Extension registration atomically across subregistries; reject half-installed Extensions.
- [x] Namespace registration ownership by Extension and prevent one Extension removing/replacing another's contributions.
- [x] Keep native Rust registration fully typed; erase/serialize only at heterogeneous registry internals or model/web/IPC/storage boundaries.
- [x] Use stable Extension/local IDs + schema versions as durable/wire identity; never persist Rust TypeId.
- [x] Return typed registration handles for direct Rust use; stale on that registration's revision/type/schema, not unrelated root registry generations.
- [x] Rename the current hook-only public `Extension` role to `Hook`; remove Signal and rename hook-level Prompt to Context.
- [x] Define one naming taxonomy: Hook=in-flight interception, Handler=post-commit reaction, Action=explicit invocation, View=read-only derived data, State=local persistence, Driver=pluggable implementation, Policy=actual authority/admission.
- [x] Keep Hooks as typed `Observe | Transform | Gate`, with immutable factual history/receipts protected.
- [x] Use Context Hook for model-visible projection changes without rewriting immutable facts.
- [x] Use ProviderHook for provider transport interception and ProcessHook for prepared Process interception; neither can widen authority.
- [x] Keep extension process execution through Sandbox/Jobs; no raw unsandboxed `exec` capability.
- [x] Keep authoritative ToolRegistry registration and expose registered/callable/visible Tool sets plus exact calls through scoped `ctx.tools`; no ToolLoadout.
- [x] Define one `Action` primitive; slash commands/shortcuts/web buttons/RPC are bindings, not separate logic.
- [x] Use `Resource` for inert skills/prompts/themes rather than a public ResourceProvider concept.
- [x] Use `UI` for semantic dialogs/widgets/status/editor/renderers/actions rather than SurfaceExtension.
- [x] Keep typed Settings as inert data and route secret settings through Vault custody.
- [x] Register typed state keys/schemas through StateRegistry while exposing only scoped runtime `ctx.state`; append-log rows remain storage details.
- [x] Keep stored extension State non-model-visible, outside HistoryBase/Agent rev and non-inherited by default; only exact State-derived bytes deliberately projected to model context affect PrefixContentKey.
- [x] Define typed non-authority `ResourceRef` vocabulary.
- [x] Use typed ViewRegistry registrations for pull/read enrichment requested by stable wire names through `include`; View failures remain per-View and never fail an authorized core snapshot.
- [x] Use typed EventHandlerRegistry definitions for durable at-least-once push/background reaction over the same `commit_log`; registration is inert until explicit activation/baseline.
- [x] Keep EventHandler distinct from `events.subscribe` Agent input delivery and make lag/retention explicit.
- [x] Register native `SourceDriver`, `SandboxDriver` and `VaultProvider` implementations under ExtensionRegistry; definitions grant no source/Sandbox/secret authority.
- [x] Keep `Policy` only for actual authorization/admission decisions.
- [x] Map Pi provider/model/MCP/tool/resource/UI/session/action capabilities onto AW owners without copying Pi's full-process-authority assumption.
- [x] Define project-extension trust ordering and atomic generation-fenced reload.
- [x] Define Histate ownership boundary: Build/product Session/Decision/Evidence/summaries/semantic graph/commit classification stay in Histate.
- [x] Allow Histate product Session to reference many AW Agents + zero/many Sandboxes without resurrecting AW Session.
- [x] Define Histate Checkpoint as product semantic heads/artifact/evidence refs + one/more AW Checkpoint refs according to product scope.
- [x] Keep AW/product cross-store mutation out of ExtensionRegistry atomicity; Histate/products use idempotent saga/outbox reconciliation.
- [x] Keep AW History DAG, Source DAG and Histate Decision DAG independent but cross-referenceable.

### H. Dynamic Extension host

- [x] Reuse/generalize existing `aw-extensions` child-process ABI rather than embed a JS/WASM runtime first.
- [x] Define Process Extension as any explicitly selected executable speaking one versioned bounded stdio protocol.
- [x] Keep native/process/WASM hosts behind the same ExtensionRegistry + scoped `ctx.*` semantics.
- [x] Make the eventual process protocol minimally full duplex so one active host callback may issue nested host-mediated `ctx.*` calls; reject unsolicited child calls outside that scope.
- [x] Bind nested ctx.* calls to opaque host-minted invocation scopes and reuse existing AW uncertainty/no-blind-retry semantics.
- [x] Define the process manifest as a bounded wire projection of ExtensionRegistry definitions and publish it atomically; discovery never activates/grants them.
- [x] Keep v1 dynamic registration to Tools/Hooks/Actions/Resources/Views/EventTypes/EventHandlers/UI/Settings/State; defer dynamic Provider/ProviderHook/ProcessHook/Compactor/SourceDriver/SandboxDriver/VaultProvider/TrustPolicy.
- [x] Make TypeScript/JavaScript the first ecosystem SDK; runtime schemas validate plain JS while TS keeps author-time types.
- [x] Keep npm/pip/cargo/package-marketplace lifecycle outside AW v1; users select already-installed executable/args.
- [x] Preserve current explicit trusted-process mode as bootstrap compatibility; target sandboxed process Extensions by default once Sandbox implementation is promoted.
- [x] Do not embed Wasmtime in v1 solely for extensions.
- [x] Permit immediate WASI experimentation by launching an external `wasmtime run extension.wasm` process speaking the same protocol.
- [x] Keep embedded `WasmExtensionHost` as an optional future optimization/isolation host, not a new extension API.
- [x] Run fresh dynamic-host contradiction/minimality review.
- [x] Check process ABI reuses current framing/safety semantics instead of creating another daemon/transport stack.
- [x] Check external ctx.* calls never become ambient filesystem/network/Vault authority.
- [x] Check native and dynamic Extensions register into the same authoritative registries.
- [x] Check no npm/pip/cargo resolver, marketplace or embedded JS runtime is required in v1.
- [x] Check WASM path is optional and does not force WIT/Wasmtime into the initial implementation.
- [x] Preserve current 256 KiB frame / 128 KiB manifest safety scale and bound nested ctx.* chains (8 callback levels / 256 nested requests per root deadline).
- [x] Reject same-process synchronous re-entry and require absolute selected executables; no PATH/shell lookup.
- [x] Treat external process Tool effect declarations as untrusted metadata; effective effect remains Unknown unless independently trusted.

### I. Host input plan

- [x] Finalize staged `followup/steer/interrupt` representation.
- [x] Define one persisted `inputs_rev` advanced once per successful staged transaction.
- [x] Hide sparse order keys/rebalancing behind before/after.
- [x] Define immutable creator + mutable last-editor provenance.
- [x] Reuse private payload-custody/encryption rather than plaintext audit.
- [x] Define all-or-nothing selected/all Send with deterministic runtime request IDs.
- [x] Define stale exact-Turn steer/interrupt: fail whole Send, leave drafts staged, never retarget.
- [x] Reuse existing ActorQueue/ActorControl uncertainty/claim/no-retry semantics below Send.
- [x] Keep staged changes on `inputs_rev`; Agent `rev` advances only when Send enters runtime custody.

### J. API convergence/migration

- [x] Retire model `jobs_subscribe` in favor of `events_subscribe`.
- [x] Retire separate Signal terminology/type.
- [x] Retire model queue CRUD tools; use host `agent.inputs.*`.
- [x] Retire model `agent_pause_after_turn`; use host `agent.admission`.
- [x] Converge Process stdin/EOF/terminate to `jobs_send` / `jobs_cancel`; arbitrary signal stays trusted-host only.
- [x] Converge model observers to Jobs reads + Events.
- [x] Preserve lower exact owner/authority/CAS/Prepared/Unknown/no-retry semantics.
- [x] Move public ACL/identity from Session vocabulary to Agent projection while retaining SessionStore internally.
- [x] Freeze canonical Agent refs during migration and emit them in new public receipts/provenance.
- [x] Keep a single semantic wire/runtime surface; temporary SDK source helpers are client-only wrappers.

### K. Shared Process settlement / 0.17.6 adoption

- [x] Keep one new shared semantic fact: `Tree::{Empty,Live,Unknown}` in aw-tools.
- [x] Keep platform observation minimal: `ManagedProcessGuard::observe() -> io::Result<bool>` maps to Tree without a duplicate platform enum.
- [x] Refine ProcessOutputMeta in place with per-stream completeness; do not add a stream metadata hierarchy.
- [x] Make ProcessTerminal the single final factual value: `exit + Option<StopRecord> + tree + output`.
- [x] Keep StopReason/StopEvidence/StopRecord/StopTracker unchanged; no Cleanup/TreeStop parallel types.
- [x] Converge ProcessReceipt/ProcessSettlement on ProcessTerminal and derive compatibility fields rather than authoring duplicate truth.
- [x] Define one settlement order: exit-ready/stop boundary -> observe -> cleanup -> one immediate retry on Unknown -> reap/finalize -> bounded drain -> terminal.
- [x] Keep Unix leader waitable through observation/final cleanup; do not risk signalling a reused PGID.
- [x] Require trustworthy negative Linux observation; wrong/filtered/incomplete procfs => Unknown; zombies alone are not live.
- [x] Keep Windows Job active-member observation separate from termination success.
- [x] Keep Bubblewrap/PID-namespace observation in the shared execution backend; do not restore a dotlink-local semantic Guard or promote frozen 017 mounts.
- [x] Keep dotlink-only projection at the edge: `leftovers/unknown`, success, drain flags, signal names and one lifecycle line.
- [x] Restore dotlink non-login Bash invocation (`--noprofile --norc -c`) to match AW.
- [x] Adopt dotlink runtime horizon only at the product layer: ~30-minute checkpoint guidance + 35-minute hard conversational command horizon; AW keeps explicit host-selected Process deadlines.
- [x] Confirm this adds no model Jobs tool and no DL-200 host method.

### L. Seal gate

- [x] Fresh end-to-end contradiction/design review after the latest SourceSet/Sandbox/Vault/extension/Event/API refinements.
- [x] Check no duplicate owner/service/runtime was introduced.
- [x] Check all public APIs remain minimal and orthogonal.
- [x] Check history ancestry cannot grant Sandbox/Vault/source authority.
- [x] Check shared Sandbox does not imply shared Agent authority.
- [x] Check checkpoint restore reconstructs SourceSet but never credentials/leases.
- [x] Check wildcard Events cannot grant authority or enter mutation APIs.
- [x] Check `#name` Event grammar has no reserved-name or wildcard contradiction.
- [x] Check exact forks preserve attribution/cache prefix semantics.
- [x] Check public AW Session remains removed while Histate product Session composes externally.
- [x] Run fresh extension naming/semantic contradiction review after the vocabulary simplification.
- [x] Check every public extension term obeys the Hook/Handler/Action/Tool/View/State/Resource/UI/Provider/Driver/Policy taxonomy.
- [x] Check Extension/ExtensionRegistry remains composition/registration only, not a second authority/runtime.
- [x] Check View/EventHandler/extension State ownership does not become a second resource/Event/history truth.
- [x] Check Hook/ProviderHook/ProcessHook cannot leak or widen credential/Sandbox authority.
- [x] Check Action/UI bindings do not create hidden mutation paths around canonical host APIs.
- [x] Check `ctx.tools.set_visible` cannot create Tool grants.
- [x] Check public AW Session remains removed while Pi session lifecycle use cases map to Agent/history/product owners.
- [x] Resolve/revalidate every pre-implementation item in SPEC section 31 after the naming refinement.
- [x] Run fresh typed-registry/API contradiction review.
- [x] Check typed subregistries are views over authoritative owners, not duplicate catalogs/databases.
- [x] Check atomic Extension registration/removal/reload cannot expose partial generations.
- [x] Check stable IDs/schema versions, not Rust TypeId, own durable/wire identity.
- [x] Check type erasure/JSON appears only at registry internals or actual transport/storage/model boundaries.
- [x] Check typed StateRegistry + ctx.state and ToolRegistry + ctx.tools preserve the registry/runtime-context split.
- [x] Check typed Histate registration needs no serde_json::Value domain model or stringly dispatch in native Rust.
- [x] Revalidate every pre-implementation item in SPEC section 31 after the typed-registry refinement.
- [x] Revalidate every pre-implementation item in SPEC section 31 after the dynamic-host refinement.
- [x] Run fresh full pre-seal architecture audit across identity, authorization, lifecycle, persistence, replay, recovery, Events, Sandbox/Vault/Source, extensions and migration.
- [x] Fix Event projection ambiguity for canonical handles containing `.` via reversible canonical `%2E` token encoding.
- [x] Separate immutable HistoryBase identity from invocation PrefixContentKey; lazy sealing is idempotent internal memoization with no Agent rev/Event.
- [x] Keep parent/delegation lineage independent from history ancestry and row-authorize Jobs lineage traversal; no ghost Jobs for inherited historical work.
- [x] Define transitive immutable DisclosureManifest propagation across History/Event/EventInput/Resource/View/Tool/model outputs and enforce it on Event read/delivery/Handlers.
- [x] Persist/replay-protect Event causal root/hop/delivery budgets so restart or descendant publication cannot reset loop bounds.
- [x] Validate live `commit_log` schema/transaction seam: reuse event_id/seq, keep full subject envelope in payload, do not overload compact entity_id.
- [x] Define one authenticated mutation replay identity/fingerprint using existing owner receipts/facts; no per-API request-id clutter or second replay ledger.
- [x] Define private prepare -> atomic AW publication -> reconciled cleanup for Agent/Sandbox/checkpoint construction; never expose half-created resources.
- [x] Validate clean Git checkpoint object retention and explicit submodule-content SourceBinding semantics.
- [x] Freeze Agent execution-adjacent mutation surfaces at closing while preserving closed-Agent history/access/checkpoint management.
- [x] Make metadata reads privacy-aware: ACL rosters/subscription patterns/group members/Vault refs require their management metadata authority.
- [x] Recheck ExtensionRegistry definition-vs-activation, registration-local typed-handle revisions, State CAS, View failure isolation and Handler activation/retention semantics.
- [x] Recheck frozen 017 mount/Bubblewrap plan compatibility without promoting/implementing it.
- [x] Confirm no new public Session, EventLink, Source service, replay ledger, distributed transaction coordinator, cache-control API or Agent-Sandbox rebind API was introduced.
- [x] Explicit user approval to seal DL-200 — granted 2026-11-20 in this conversation.
- [x] After seal, write phased implementation plan — `IMPLEMENTATION.md`.
- [ ] Only after separate explicit promotion, begin implementation.

## Current verification

- [x] Design work performed on L5 only.
- [x] No AW implementation source changed by DL-200 work.
- [x] No dotlink implementation source changed by DL-200 work.
- [x] No AW `.agents/plans/ux/**` content inspected or modified.
- [x] Sandbox/mount/config-v13 implementation remains frozen.
- [x] DL-200 seal recorded after explicit user approval.
- [x] `IMPLEMENTATION.md` written; implementation promotion remains separate and unchecked.
