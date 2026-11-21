# DL-200 R1–R5 — historical design-resolution checklist

Archived intact from the plan-specific TODO during the R6 documentation simplification.
Checked items record *design decisions*, not code completion or test results.
Current authority is the R6-candidate SPEC/IMPLEMENTATION and R5 sealed baseline;
this file is historical evidence, not a second normative specification.

## R1–R5 resolved design decisions (historical checklist, **not** completed implementation)

The checked items below record design closure only; `SPEC.md` is the sole normative
contract and the queue above/phase plan track **real** remaining implementation. Keep
this history for traceability until the Phase-7 documentation cleanup.

### A. Immutable history / construction

- [x] Define immutable `HistorySpan` / `HistoryBase` storage identity.
- [x] Define immutable `HistoryGrant` disclosure/provenance closure for cross-Agent sharing.
- [x] Make DisclosureManifest a persistent content-addressed bounded-fanout set/DAG with structural sharing; preserve exact fact provenance separately from deduplicated stable disclosure-authority roots so long-lived history does not require flat O(all facts) ACL closure work. Authority roots identify dependency/maximum ceiling and live owners to recheck; they never cache current authorization except for explicitly immutable export grants.
- [x] Preserve exact inherited attribution across forks.
- [x] Define redacted/anonymized projection as a new HistoryBase.
- [x] Define reachability-based retention/GC without crash-sensitive refcounts.
- [x] Preserve legacy internal SessionStore/branch/journal/recovery ownership.
- [x] Define exact settled-Turn fork boundary and inherited next-Turn state.
- [x] Replace host `agent.fork` with the single `agent.create(from=...)` constructor.
- [x] Rename model ancestry selector to `agent(..., from=...)`.
- [x] Keep `checkpoint.restore` as the stronger exact history+SourceSet recovery API.
- [x] Land one private prepare/publish/reconcile construction primitive before restore; Phase 4 public/model Agent constructors reuse it rather than creating a restore-specific or second publication owner.
- [x] Fix model exact-history default: source Project + inherited source ACL, with trusted
      host narrowing only unless ordinary share/redirect authority permits more.

### B. Revisions/access/groups/principals

- [x] Define `agent_state_current` as authoritative durable Agent-envelope state, not a rebuildable projection; only indexes over it are rebuildable.
- [x] Seed Agent state+ACL once under exclusive store schema/write migration; activate the new Agent-owner/schema epoch atomically with the complete backfill, with no v1 online dual-write/catch-up; then stop deriving rev/lifecycle/lineage/ownership from audit. SessionStore remains Turn/history owner only.
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
- [x] Fence all Event consumer progress behind one contiguous gap-free `indexed_through_commit_position`; no read/subscription/Handler may advance past a missing committed Event projection.
- [x] Define opaque `events.read` cursor state bound to caller + normalized pattern + stable EventStoreId + **authority-scoped** selector placement generation; hidden Domains cannot stale a caller, and affinity is rechecked before scan and cursor-progress commit.
- [x] Bind durable Agent subscriptions and EventHandler cursors to the same authority-scoped EventStoreId/placement affinity under their installer/service authority; mismatch stops delivery/pinning and requires explicit rebaseline. V1 Agent subscription delivery additionally requires the subscription/Event/causal owner and recipient Agent Input owner to share one transactional store; cross-store Agent delivery fails explicitly.
- [x] Define 24h idle cursor retention and logical replay floor: all last 7 days and at least latest 65,536 Event rows.
- [x] Prevent hidden source counts/positions/gaps leaking through cursors.
- [x] Define durable subscription generation/baseline/scan/lag semantics; repeated active `enabled=true` preserves generation/baseline/backlog, lagged re-enable creates a fresh generation/baseline, disable fences later admissions.
- [x] Define bounded `events.subscriptions.list` without separate get/edit/delete verbs.
- [x] Define deterministic replay-safe EventInput delivery into the existing Agent input owner, with active generation/store affinity + current source/recipient/disclosure authorization + dedupe + causal budget serialized at the same admission linearization point.
- [x] Treat Event authorization/disclosure as allow | proven-deny | unknown internally with source-first ordering: proven source denial advances privately without deeper manifest/provider checks. For visible sources only, external mutable dependency authority must be immutable/non-revocable or provider-fenced through acceptance/read authorization. `events.read` returns candidate-oblivious retryable `authorization_unavailable` with unchanged cursor; durable consumers hold position with bounded backoff.
- [x] Define model `events_subscribe` current-Agent recipient rules.
- [x] Define `events.publish` / `events_publish` as custom-`#` publication only.
- [x] Fix custom Event data: JSON object, canonical encoding <= 4096 bytes, nesting <= 8.
- [x] Fix model custom publication budget: <= 32 successful publishes per Turn.
- [x] Fix active subscription cap: <= 128 per recipient Agent.
- [x] Fix read bounds: limit <= 256, wait <= 30s, internal scan <= 4096 rows/call.
- [x] Fix causal delivery bounds: <= 16 Event hops and <= 256 EventInput deliveries/root.
- [x] Suppress self-delivery in v1.
- [x] Define exact source + recipient + Domain/federation authorization for every concrete delivery.
- [x] Define v1 broad cross-Domain selectors over the host's authoritative placement universe only; hidden/unknown remote Domains do not create discovery side channels, while authorized scope whose complete placement cannot be proven fails/stales rather than returning a partial world.
- [x] Reject a separate EventLink service; ordinary resource/share authority remains authoritative.
- [x] Define v36 SourceEventBook/pending-delivery migration without a long-lived parallel runtime: in the serialized source-owner cutover transaction set `delivery_cutover_rev = current next_event_rev`; entire revisions belong to one path. Accepted legacy Inputs retain identity and receive immutable compatibility disclosure/provenance sidecars; incomplete provenance is conservative `legacy_protected`. Only pending pre-cutover deliveries bridge once deterministically.
- [x] Keep `jobs_get(rev, wait_ms)` as model convenience over owner rev + shared wake substrate.

### D. Checkpoints / SourceSet

- [x] Define checkpoint = exact immutable HistoryBase/head + immutable named SourceSet, including captured SourceBinding access/checkpoint-policy layout metadata.
- [x] Allow an empty SourceSet for explicit/history-only capture.
- [x] Define typed `SourceBinding` / `SourceDriver` / `SourceAnchor` contracts.
- [x] Require every externally effectful SourceDriver **capture or restore** to use the same prepared operation invariant: stable operation-owned recovery identity before the effect, bounded inspect/cleanup descriptor, exact driver registration+recovery schema, and publication evidence that is immutable/non-revocable or provider-fenced through the full AW commit.
- [x] Bound named sources with host/runtime `max_sources_per_sandbox`: default 128, hard absolute 1024; every SourceSet/derive/checkpoint/restore/RecoveryDescriptor set must fit before external effects.
- [x] Keep durable RecoveryDescriptors non-secret: opaque IDs/digests/version proofs/provider-or-Vault capability refs only; no bearer tokens, credential URLs, raw secrets or serialized credentials.
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
- [x] Keep one fresh private Process Scope per dispatch, selected from current Sandbox launch capabilities + authority; Sandbox itself never owns Process lifetime.
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
- [x] Introduce final typed leaf registries at first semantic use: EventType in Events, SourceDriver/SandboxDriver/VaultProvider in resource phases; from day one all leaf mutation uses one private stage/validate/publish primitive so ExtensionRegistry can later compose those same owners atomically without wrappers/migration.
- [x] Register native `SourceDriver`, `SandboxDriver` and `VaultProvider` implementations under ExtensionRegistry; definitions grant no source/Sandbox/secret authority. Live bindings/checkpoints and unresolved Prepared/Unknown descriptors pin exact driver registration+recovery schema. Removal/incompatible replacement is refused, and store-open/startup validates persisted dependencies before source effects; absent/incompatible drivers put affected resources in fail-closed recovery-required state. Compatible replacement must explicitly support the old durable schema.
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
- [x] Define stable `jobs_list` pagination: one traversal is exposure-store-local; first page captures immutable publication high-water; cursor binds caller+anchor+scope+private scan/tree frontier; exact ordering is `(publication_position DESC, canonical_ref ASC)` with both tuple fields retained.
- [x] Make Jobs exposure rebuild gap-safe: normal publication writes projection evidence transactionally where possible; full rebuild captures head H, builds privately, then under final write/swap fence verifies the current head still equals H or catches up `(H,current_head]` before atomic activation; stale replacements never erase newer active rows and no cursor high-water covers a projection hole.
- [x] Treat stored tree frontier as navigation only: on resume reauthorize every Agent frontier/ancestor gate before descending; if a parent becomes hidden, discard its queued subtree even if a descendant remains directly viewable.
- [x] Keep v1 recursive `jobs_list` traversal in one authoritative exposure store; cross-store parent/child federation fails explicitly rather than merging independent publication logs.
- [x] Preserve Graph ownership transitively: nested Agents are allowed within existing bounds, but direct or Agent-mediated nested `batch` is prohibited while any descendant remains owned by an active Graph node; later independently admitted Turns regain ordinary top-level eligibility.
- [x] Converge model observers to Jobs reads + Events.
- [x] Preserve lower exact owner/authority/CAS/Prepared/Unknown/no-retry semantics.
- [x] Move public ACL/identity from Session vocabulary to Agent projection while retaining SessionStore internally.
- [x] Freeze canonical Agent refs during migration and emit them in new public receipts/provenance.
- [x] Keep a single semantic wire/runtime surface; temporary SDK source helpers are client-only wrappers.

### K. Shared Process settlement / 0.17.6 adoption

- [x] R5 historically defined `Tree::{Empty,Live,Unknown}` in aw-tools; R6 proposes the
      **source rename** to `Occupancy` while preserving the same fact/values.
- [x] Make Process one supervised execution scope: private async `Scope` owns the main child + platform containment; direct-host fallback is `DirectScope`, never the semantic Process API.
- [x] Refine ProcessOutputMeta in place with per-stream completeness; do not add a stream metadata hierarchy.
- [x] Make ProcessTerminal the single final factual value: `exit + Option<StopRecord> + tree + output`.
- [x] Keep StopReason/StopEvidence/StopRecord/StopTracker unchanged; no Cleanup/TreeStop parallel types.
- [x] Store ProcessTerminal directly in ProcessReceipt and derive Outcome/completeness/status/stop helpers; retain independent ProcessSettlement Job Status only where the Job result can fail outside the Process.
- [x] Define one settlement order: exit-ready/stop boundary -> freeze immutable pre-cleanup Tree -> cleanup/StopRecord -> reap/finalize -> bounded drain -> terminal; cleanup never rewrites Tree.
- [x] Keep Unix leader waitable through observation/final cleanup; do not risk signalling a reused PGID.
- [x] Keep `Tree::Empty` strong and backend-independent: it proves the semantic Process scope empty. Direct Unix negative group/procfs observation is `Unknown` whenever escaped descendants cannot be excluded; positive remaining members may prove Live.
- [ ] Phase-0 source correction: in the R6 Occupancy rename remove the current weak
      direct-Unix `Ok(false) -> Tree::Empty` semantic mapping and qualify escaped `setsid`
      descendants without changing physical ProcessExit.
- [x] Keep Windows Job active-member observation separate from termination success.
- [x] Define strong Linux backend as tiny AW PID-1 supervisor + main command PID 2 in a per-command PID namespace; use one bounded **unidirectional supervisor→host** report channel independent of user stdio, ensure command/descendants inherit no writable endpoint, validate framing/launch identity/exactly-one-terminal-record, and map supervisor/report failure before main-exit fact to ProcessExit::Unknown without rewriting Tree.
- [x] Keep Sandbox environment and Process Scope orthogonal: unsandboxed/direct is first-class; sandboxing may enable a stronger private Scope but never becomes the lifecycle/Terminal owner.
- [x] Keep Scope backend selection private/host-owned and factual; no model-facing direct/namespace/job/remote selector and no assumption that sandboxed means PID namespace. A launch requirement for closed descendant containment must fail admission rather than silently downgrade to DirectScope.
- [x] Prefer prepare Sandbox -> prepare Scope -> launch into Scope -> publish Process; do not design a generic spawn-then-attach lifecycle.
- [x] Make dotlink consume the same shared Process lifetime owner/settlement semantics, not merely ProcessTerminal fields; retain only dotlink launch/output/product/log/timeout concerns and qualify behavioral parity.
- [ ] Phase-0 dependency gate: make the real dotlink consumer compile against the shared Process owner at the lowest sensible shared layer without importing the full Agent-specific adapter solely for Process lifetime.
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
- [x] Define one authenticated mutation replay identity/fingerprint using existing owner receipts/facts; same identity never repeats, constructors retain exact created canonical IDs, and full replay state may compact in the same owner to a minimal never-execute-again acknowledgement/tombstone.
- [x] Define concrete replay-identity lifetimes: ToolCall tombstones follow retained Agent/history identity; host RPC tombstones may retire only after the authenticated request namespace irreversibly rejects old IDs; TTL alone never re-enables a consumed identity.
- [x] Define exact replay-without-disclosure behavior: return only a minimal/opaque non-retryable acknowledgement allowed by current disclosure, with no target/Job/provider/output metadata leakage and no fresh effect.
- [x] Define private prepare -> atomic AW publication -> reconciled cleanup for Agent/Sandbox/checkpoint construction; before any external/materializing effect, durably claim the existing operation receipt as Prepared with fingerprint/candidate/recovery/source-version/fence identity; crash/replay inspects that claim and never blindly repeats. The final AW transaction revalidates all AW-store-owned source/destination/grant/ACL/lifecycle authority inside the publication transaction, validates external immutable-version/fence evidence, transitions the same claim to Applied and is the sole public visibility point; revoke/replace-first denies publication, publish-first remains factual.
- [x] Validate clean Git checkpoint object retention and explicit submodule-content SourceBinding semantics.
- [x] Freeze Agent execution-adjacent mutation surfaces at closing while preserving closed-Agent history/access/checkpoint management.
- [x] Make metadata reads privacy-aware: ACL rosters/subscription patterns/group members/Vault refs require their management metadata authority.
- [x] Recheck ExtensionRegistry definition-vs-activation, registration-local typed-handle revisions, State CAS, View failure isolation and Handler activation/retention semantics.
- [x] Recheck frozen 017 mount/Bubblewrap plan compatibility without promoting/implementing it.
- [x] Confirm no new public Session, EventLink, Source service, replay ledger, distributed transaction coordinator, cache-control API or Agent-Sandbox rebind API was introduced.
- [x] R1 original seal granted 2026-11-20.
- [x] R2 architectural amendments (Process/Event/replay/Agent-state/registry/pagination) recorded as the prior 2026-11-21 reseal milestone.
- [x] R3 amendments (publication-fenced construction authority, recoverable SourceDrivers, transient Event auth semantics, path-authorized/store-local gap-safe Jobs traversal, scalable DisclosureManifest) recorded as prior 2026-11-21 reseal milestone.
- [x] R4 amendments recorded as the prior 2026-11-21 store/recovery/migration reseal milestone.
- [x] R5 amendments (external Event authority fencing, supervisor-only report authorship, legacy accepted-input provenance sidecars, store-open driver compatibility validation, source-first candidate-oblivious Event auth, source cardinality bound, non-secret RecoveryDescriptors, exact cutover rev) incorporated and resealed by explicit user direction on 2026-11-21; R5 is authoritative, R1-R4 are historical provenance.
- [x] Write/update phased implementation plan through R5 — `IMPLEMENTATION.md`.
- [x] Separate explicit implementation promotion granted 2026-11-21; Phase 0 started.
