# DL-200 — R6 simplification review + implementation queue

Base: `f035a00031b8f5879f4d77f3fd14bfe02b89305c`

Status: **R6 + SHARED PROCESS/RELEASE-CORRECTION PUBLISHED / PHASE 0B COMPLETE / PHASE 1 COMPLETE LOCALLY / PHASE 2 UNDERWAY**

## Committed

- [x] `132bfd7` — published R1 DL-200 seal/implementation-plan baseline.
- [x] `7c3800c` — R6 architecture sealed.
- [x] AW shared Process runtime published through `7c16708`; AW main `59d67ba` adds the
      focused bounded-drain regression test only.
- [x] dotlink source `16475e4` + handoff/main `fb1b7b9` — repinned to AW `7c16708`, real
      `aw-process` consumer cutover complete, old shell Process owner deleted.
- [x] AW corrective R6/Phase-0 publication `20a63a4d4547cc20b2a154787823125b82f5e385`.
- [x] dotlink R6 consumer/product-policy publication `85ceccc1088e201c98c994a46a68834440eeab32`,
      atomically repinned to AW `20a63a4`; ordinary pinned qualification green.

## R6 design approval and real implementation queue

- [x] Write one R6 normative owner/locality model and six descriptive internal roles in
      `SPEC.md`, keeping global IDs distinct from transactional/ordering boundaries.
- [x] Propose `Occupancy` instead of Process `Tree`, with unchanged dotlink `tree`
      compatibility wire and existing physical-exit/stop/output semantics.
- [x] Specify one internal store-local `StorePos`, replay/effect vocabulary,
      non-secret ExternalEvidence, projection safe-prefix+swap, cursor envelope,
      EventScan, DriverKey and typed registry staging; no generic manager or new ledger.
- [x] Choose **`aw-process`** as the dedicated Agent-neutral async Process/pipe lifetime
      crate shared by AW and dotlink; retain only policy, output storage and transport at edges.
- [x] Integrate those primitives into existing Phases 0–7, preserving R5 security,
      disclosure, migration, causal, failure and retirement contracts.
- [x] Reduce `README.md` to the developer mental model, shared roles, crate boundaries
      and phase map. Keep detailed normative owner semantics only in `SPEC.md`.
- [x] Make the SPEC single-definition normative: replace the repeated invariant restatement
      with a section index and move R1–R5 resolution history to the archived checklist.
- [x] **R6 design approval/reseal** — explicitly approved 2026-11-21 after the final
      live-owner review. R6 is normative; implementation status remains independently proven.

**Next implementation slices (R6 sealed; no extra phase):**

- [x] **Phase 0 shared corrective convergence published** — `Occupancy`, `aw-process`, the stable
      Linux Spawn Facility/linearizable pre-spawn gate, strict child-terminal proof, stop/exit
      reconciliation and Windows source correction are live in AW `20a63a4`; no parallel Process
      owner was introduced.
- [x] **dotlink-main v0.17.9 convergence + product policy + shared-Process consumer published** —
      dotlink `85ceccc` pins AW `20a63a4`; exact release reference remains v0.17.9 `576d0a1`.
      Ordinary pinned all-target/all-feature build, shell **86/86**, Graph **13/13**, queued real
      Spawn-Lane cancellation, pre-spawn timeout/no-Process projection, canonical spawn refusal,
      strict Clippy, fmt and diff-check are green. Real Bubblewrap worker-retirement and
      Windows-native qualification were explicitly deferred by the user and remain follow-up evidence.
- [x] **Phase 0B COMPLETE and published** — existing policy/guardian/Remote Start owners now carry
      original-invocation, actor, disclosure and lifetime replay correctness without a global replay
      database. Consumed AW policy rows compact only after a durable/effect recovery boundary into a
      <256-byte indexed tombstone while immutable audit retains full evidence; dotlink full Remote
      Start receipts compact after ordinary retention to payload-free never-retry tombstones rather
      than deleting the authorization identity. Unknown outcomes remain tombstones. Long-lived
      admission no longer depends on Session-lifetime audit/event scans, and live capacities remain
      explicitly scoped to active Turns/subscriptions/exposures rather than retained history.
- [x] **Phase 1 COMPLETE locally** — private `StoreScope` reuses existing `store_id` + nonzero
      `restore_generation`; transparent `StorePos(u64)` carries internal `commit_log.seq`; exact
      Agent lookup reuses indexed `job_exposure_current` with current-authority recheck. **v38
      owner cutover landed:** one exclusive migration transaction creates/seeds
      `agent_state_current` + `agent_acl_current`, freezes exact canonical/backing identity,
      legacy initial rev/title/owner/lifecycle/parent lineage and conservative five-permission direct
      ACL, validates completeness/FKs, then activates epoch 38. New creation/fork/Turn/Input-Accepted/
      Applied-Control/subscription/child/close/recovery paths mutate the exact owner; hot ordinary
      paths remain allocation-free and recovery alone deduplicates arbitrary changed Agents. Agent
      Jobs/history/wake/list authority now uses direct Agent ACL revisions rather than Session
      participation; exact title/access compatibility mutations CAS Agent rev and ACL changes advance
      `acl_rev + rev` together. Legacy Session title/participation remain backing compatibility only.
      Populated v37->v38 seed, branch-local rev/title isolation, direct ACL revoke/regrant fencing,
      mixed exposure, coordination 24/24 and stable lifecycle/control regressions are green. **v39
      principal/group owner landed:** one Domain-scoped user/group handle namespace, stable-user
      backfill from Domain/Agent/ACL ownership, membership-insert registration, lazy authenticated
      user registration for group/direct grants, simple owner-managed non-nested groups with bounded
      complete-set replacement + monotonic rev, tombstone/no-handle-reuse deletion and no Agent-rev
      fanout. Populated v38->v39 backfill and grouped lifecycle tests are green. **Group-aware Agent
      access landed:** one bounded owner/direct-user/group-union authorization path, group-owner
      membership support, shared direct/group compatibility ACL mutation primitive, and 32-byte
      `access_generation` derived from Agent acl_rev + actor security epoch + exact grant/group
      revisions + effective permissions. History cursors and wake v3 authority digests bind to it;
      wake mint->persist revalidates the full generation without a new durable column; mixed exposure
      candidate filtering is group-aware before authoritative recheck. End-to-end group removal/re-add
      proves wake/history fencing with unchanged Agent rev. New group audience expansion is
      intentionally fresh/quiescent-branch-only until HistoryGrant/share closure lands. **v40
      immutable history owner landed:** additive lazy `HistorySpan`/`HistoryBase`/`HistoryGrant`
      storage; normalized grant principals; persistent SHA-256 content-addressed DisclosureManifest
      leaves/branches + authority roots with fanout 64; conservative extraction of exact legacy-node
      and materialized-fact provenance; parent/local structural manifest composition; exact checkpoint
      seal memoization in the existing transaction with no Agent rev/Event. **Full seven-permission
      Agent ACL landed:** one `AgentPermissions`/authorization owner for view/contribute/submit/control/
      derive/share/manage; bounded `agent_access_list`; canonical principal-ref `agent_access_set`
      replacing complete user/group grants under rev CAS; old five-permission setters reduced to thin
      adapters over the same path. New history audience expansion requires current Agent share,
      bounded manifest-root reauthorization and mints one immutable superseding HistoryGrant in the
      same transaction; already-disclosed principals can be re-added without share, narrowing never
      rewrites grants, and materialized external-policy expansion fails closed pending a reviewed
      source-share contract. Same-branch base layers are replaced rather than duplicated on reseal.
      Existing HistoryGrant/group AgentJobs/principal tests + strict aw-store/aw-cli Clippy remain
      green. **Exact Agent-ref history fork landed:** one lower
      `create_agent_from_agent_history(private|inherit)` transaction requires current derive, seals
      latest committed source history (using active run `produced_from_head`/turn when source is busy),
      validates disclosure roots, creates a physically fresh destination Session/Agent, fresh rev/
      runtime/input state, fresh independent ACL rows, new destination HistoryGrant lineage, inherited
      `next_turn`, and no copied history nodes. `history_in`/activation now resolve structurally shared
      HistoryBase spans + local tail with recorded-node digest verification. Inherit/private ACL
      profiles are non-broadening; source/destination ACLs diverge independently. **Exact `from`
      selectors/adapters landed:** canonical Store `create_agent_from_history_ref` accepts Agent or
      settled-Turn refs; settled Turn resolution handles both local attached runs and inherited
      HistoryBase facts while active/unsettled Turns fail closed. Model `agent(...,from?)` resolves the
      existing contextual ref grammar to a same-Project canonical source, fingerprints that canonical
      identity, and attaches history inside the existing durable child-allocation transaction before
      publication. `from="."` and equivalent absolute refs replay identically; no-`from` fingerprint
      shape stays backward-compatible. Focused Store/model/no-from replay tests and strict Clippy are
      green. **First external host Agent state-shell cut landed locally:** project-scoped row-authorized
      list/get, title edit, admission, canonical access list/set adapters and the lower close-request
      seam compose `agent_state_current`; fresh/exact-history create keeps trusted current Project +
      execution binding outside caller/model fields and installs private/inherit/custom access inside
      the same immediate existing-constructor transaction. Real Actor future-Input admission now checks
      Agent lifecycle/admission before the legacy pause gate. **Stable-Agent active close is now
      preemptive:** one shared close/cancellation primitive tracks only the current Turn token; close
      cancels that token before worker settlement, background construction arms it before handoff, and
      repeated close does not enqueue duplicate close commands. One real blocking-provider regression
      proves the child reaches factual cancelled settlement instead of waiting forever. **Durable
      receiver retirement is also complete without a new Event schema:** close removes the receiver
      from existing source books, drops pending deliveries + durable wake admission, bumps each affected
      source Agent once, and delegated close evicts its cached wake admission before signalling the
      worker. Replay repairs retirement idempotently. The transitional implementation intentionally uses
      a cold close-path scan rather than prebuilding Phase-2 indexing. **Finite Job close ownership is
      now reusable too:** the canonical per-Agent `JobRuntime` can be retained by the host via
      `configure_with_runtime`; `close_owner` fences new finite work, cancels through the existing
      JobHost/JobManager + exact runner tokens, waits for quiescence, and refuses terminal success while
      JobManager still reports ambiguous/in-flight/live-writer state. No parallel Job lifecycle or index
      was added. **Stable-child terminal composition is now complete:** preparation retains the exact
      configured JobRuntime; close awaits finite-work quiescence before the existing atomic child/Agent
      settlement, keeps the live worker in durable `closing` on unresolved work, and permits one later
      close retry without duplicate in-flight close commands. **Root/default lower close seams are now
      complete without a new lifecycle owner:** trusted host close can cancel the exact active Actor Turn
      through the existing durable Actor Control owner keyed by close revision, and the Store can publish
      `closed` only from `closing` after rechecking that no journaled Turn is active. The Store terminal
      seam deliberately does not inspect finite Jobs. **Current-root terminal composition is now complete:**
      native Host retains one `RootAgentClose` that composes durable close request + pending queue retirement,
      exact pre-claim withdrawal/active-Turn Control, retained JobRuntime `close_owner`, factual Turn
      quiescence, terminal Store close and live-router retirement. Stdio exposes the same current-root owner
      concurrently as `agent_close(ref="."|canonical, rev?)` with no authority-bearing fields. **Taskless
      host-created Agent activation is now reusable too:** `HostAgentActivation` reconstructs only an
      already-published Agent into the normal Actor/Router/JobRuntime worker, and stable + host-created
      workers share one live-control/wake primitive. This is reconstructible post-commit runtime state,
      not another publication path. **Prepared/atomic initial `task` custody + dispatch is now complete as
      a lower host primitive:** candidate Agent IDs + an audit-backed preparation receipt are claimed before
      encrypted SecretStore custody; one final Store transaction publishes Agent + ordinary Pending queue
      + Applied receipt; exact Applied replay returns the same Agent and uncertain outcomes never auto-retry;
      activation wakes the normal queue owner only after worker readiness. **Native/public current-Project
      `agent.create` is now composed too:** trusted provider selection is rebuilt for the destination
      canonical stable-user principal (personal/OAuth stay owner-only; persisted server accounts retain
      current Store grants), `Router::for_actor` gives root/local and child/stable principals exact-actor
      facades over one shared live registry, exact replay uses live-owner fast-path + insert-if-absent, and
      stdio exposes only the sealed create fields with RPC id as durable request identity. **Logical Sandbox
      ownership + Agent binding are now implemented in v41:** canonical SandboxRef is the durable ID;
      Project+layout state owns `rev/open|closed`, migration binds every existing Agent without collapsing
      distinct layouts, generic constructors auto-bind only once v41 exists, trusted `sandbox=auto` uses the
      current Agent's immutable Sandbox, explicit refs must be open + exact destination Project + exact
      trusted layout, and no Agent rebind verb exists. Store `sandbox_get/list/create_fresh/close` primitives
      are present; SourceBindings/SandboxGrant/driver realization remain deferred and frozen 017 is untouched.
      **Native/stdio Sandbox list/get/close is now exposed for the trusted current Project** through one
      concurrent `SandboxPort`; it accepts no raw layout/mount/provider authority, supports bounded list
      continuation + logical snapshot reads, and refuses cross-Project refs. `include=["sources"]` is empty
      until SourceBindings exist. `sandbox_create` is deliberately advertised unavailable: a reviewed
      row-local replay shortcut was rejected because true fresh/derived Sandbox materialization needs the
      sealed Prepared claim before external SourceDriver/filesystem effects, and the current lower logical
      helper must not falsely promise isolation. **Host non-current Agent close routing is now complete:**
      one `HostAgentClose` preserves full root composition for `.` while other canonical live Agents pass
      Agent-control/revision preflight before Router topology lookup, atomically enter `closing/held` through
      the existing Store owner, then signal their existing live close controller; host-created and delegated
      runtimes retain their separate factual settlement owners. Already-closed replay is authorized and
      idempotent; a post-request controller race remains inspectable `closing` rather than fake success.
      **Cross-Project host `agent.create` redirect is now implemented for the native/public trust model:**
      destination `i:` must equal the authenticated stable user, active destination Domain membership and
      current Workspace writer are revalidated before preparation and final publication, history redirect
      additionally requires source `share` plus `derive`, and the shared lower history constructor validates
      the destination owner/audience disclosure closure. Redirected create requires an explicit open Sandbox
      in the destination Project; `auto` fails until Phase-3 materialization can truthfully create/derive one.
      Task-bearing redirected create reuses the existing Project+Sandbox fingerprinted Prepared receipt and
      stable-principal activation path. Raw `project=` never impersonates another principal. **The v42
      staged input-plan owner is now implemented:** one per-Agent `inputs_rev`, opaque `q:` IDs, encrypted
      SecretStore text, sparse ordered drafts with transactional rebalance, immutable creator/source +
      last-editor provenance, followup/contribute vs steer|interrupt/control permissions, manage for other
      users' drafts, exact Turn validation, close freeze, and one `agent.changed aspect=inputs` fact per
      successful mutation without Agent-rev churn. **Atomic staged Send is now internally complete:** v43
      adds one stable-user/request replay owner; `HostAgentInputs` authorizes the complete set before Router
      lookup; any number of followups plus at most one Runtime-pending steer/interrupt are supported; control
      uses existing Prepared AuthoritySteer + live staging + transactional accept/withdraw/Unknown semantics;
      followups publish through existing prepared ActorQueue custody; the final transaction removes every
      selected draft, advances `inputs_rev` and Agent `rev` exactly once, emits the input-change fact and
      records Applied replay. Same-request followup Send replay is proven not to duplicate execution, and
      existing steer/interrupt segmented-custody regressions remain green. **All five native host input
      methods are now exposed together** through one `AgentInputsPort`; add/edit/delete share the same v43
      stable-user/request replay owner, exact replays perform no new SecretStore write or revision bump, and
      changed semantics under the same request ID conflict. RPC frame ID is the mutation identity, strict
      field whitelists block authority-bearing extras, and only Send touches Router/live Runtime.
      **Committed Turn/Tool/model provenance is now projected from the existing v40 history owner:** one
      batched `TurnFactProvenance` preserves original canonical Agent/Turn/actor/timestamp through exact
      forks, `jobs_read` explicit history records carry it, default result reads remain unchanged, and
      `llm.attempt_settled` commit evidence now names canonical Agent + Turn while retaining internal
      Session/branch locators. No new provenance schema or hot Tool/Invocation fields were added.
      **Finite Process/Job provenance is now complete without a new owner:** immutable finite exposure
      reservations carry canonical Job + original Agent/Turn + initiating actor, publish/release cannot
      rewrite that actor, old events derive the same provenance from typed ref + original commit actor, and
      the hot exposure projection remains unchanged. `JobHost` projects the same value from immutable
      TaskIdentity for bounded finite list/get/ack/send/cancel receipts; raw `jobs_read` bytes and
      `ProcessTerminal` remain untouched. `ModelProcessFact` freezes canonical Agent/Turn/actor at Prepared
      and reduced close/terminate receipts expose it without private Process metadata.
      **Agent exposure/index normalization is now complete:** `agent_state_current` remains authoritative;
      owner-locked Store open compares the rebuildable mixed exposure projection and, only on mismatch,
      replaces Agent rows from authoritative state while recovering immutable original positions from
      `session.created`/`branch.created` commit evidence. No synthetic events/revs/clocks are created, finite
      Job rows are preserved, and exact projections are a zero-write no-op. **Native Agent state RPC
      exposure is now complete:** one concurrent `AgentStatePort` directly composes the existing Store
      `agent.list/get/edit/admission/access.list/access.set` operations; strict headless whitelists reject
      provider/model/Sandbox/owner or other unknown authority-bearing fields before dispatch; only `.` or
      canonical Agent roots are accepted; list/get/access-list remain pure projections; and title/admission/
      ACL mutations stay in their existing owners. The existing host state-shell regression now exercises a
      real group ACL grant through `agent_access_set_ref`; the existing strict headless regression exercises
      all six methods, capability advertisement and authority-field rejection. A fresh SPEC §28 audit found
      no remaining Phase-1 owner/surface gap. Event provenance/cutover remains Phase 2; Vault bindings and
      public fresh/derived `sandbox.create` remain Phase 3; View/Extension enrichment and Phase-4 Jobs
      traversal remain in their sealed later phases. No Phase-2 source was started in this checkpoint.
      Defer reachability/flatten acceleration until a real consumer requires it.
- [~] **Phase 2 UNDERWAY** — v44 lands the scan prerequisite first: one canonical
      `aw-tools::event_subject` grammar, one rebuildable parsed `event_projection_index`, and one
      StoreScope-bound contiguous `event_projection_state` watermark over immutable `commit_log`.
      Literal handle `.` encodes as uppercase `%2E`; malformed hierarchy/escaping fails closed.
      The existing commit writer advances the watermark for every row and projects only reserved v4
      DL-200 Event markers; Event-marked writes require an existing transaction. Pre-v44 rows remain
      non-Events. Projection-local dirty triggers make healthy open O(1) while any out-of-band
      insert/update/delete forces owner-open rebuild from immutable truth; rebuild changes only
      projection generation/scope/watermark and creates no Event/resource revision. Focused subject
      and projection regressions are 1/1 each; current-schema aw-tools/aw-store all-target check +
      strict Clippy are green. Broad aw-store lib is 226 green / 4 historical stale-downgrade reds.
      **EventScan boundary landed locally:** one absolute typed `EventPattern` matcher implements
      one-token typed `*` + terminal one-or-more `>` with canonical encoded handles; one bounded private
      scanner rechecks StoreScope/projection generation plus caller-owned placement `Current/Stale/Unknown`
      before scan and before returning candidate/progress. Pattern nonmatch and proven source/deeper deny
      advance privately; Unknown never advances; dirty/rebuilt/scope/placement mismatch is stale. Immutable
      payload is loaded only after matching source allow. Source and deeper checks are statically dispatched
      `Allow(evidence)|Deny|Unknown`; both preliminary evidence values travel with an allowed candidate for
      mandatory consumer-side final reauthorization. The visible candidate alone is boxed to keep skip/
      blocked/end outcomes small. Focused pattern 2/2, scanner 1/1 and projection 1/1 are green with strict
      current-schema all-target Clippy/check. The temporary reachability allowance is now removed by the
      real v46 read consumer below; no fake/public scanner surface was introduced.
      **v45 source-local Event authorship landed locally:** one generic `event_sources_current` owns
      exact source ref + first-unissued event_rev + nullable future delivery cutover. Migration seeds every
      legacy SourceEventBook at its exact next revision without rewriting Event history. New legacy source
      books align from the generic counter, and Agent/Turn result/settled transitions dual-write real
      immutable DL-200 envelopes in the same authoritative transaction while legacy delivery ownership
      remains unchanged. One transition reserves one revision; Turn result+settled share it; legacy and
      generic revisions must match exactly or the transaction aborts. System envelopes retain canonical
      subject, factual actor/Turn provenance and source-only disclosure for these metadata facts. Focused
      legacy 4/4 + subject 2/2 + scanner 1/1 + projection 1/1 + attached-Turn coexistence/migration 1/1 are
      green with strict current-schema all-target Clippy/check.
      **v46 first EventScan consumer landed locally:** one opaque durable read cursor binds principal +
      canonical selector hash + exact-Agent placement + StoreScope + private position; 24h idle/64-principal
      bounds and the seven-day-or-newest-65,536 replay floor are enforced. The first read scope is
      intentionally exact-Agent only: broader selectors fail closed until the placement owner can prove
      one-store completeness. `after=Now` captures the current safe watermark; cursor reads scan <=4096
      rows for <=256 visible Events. One Store transaction caches current Agent-view evidence for the page
      and rechecks it once before visible progress commits; proven denial/nonmatch advances privately,
      protected/indeterminate deeper authority returns candidate-oblivious AuthorizationUnavailable without
      progress, and scope/placement changes stale the cursor. The existing attached-Turn regression now
      proves owner read-once semantics plus revoke->private-skip->regrant non-retroactivity and
      v44→v45→v46 migration. EventScan is production-reachable and its dead-code allowance is gone.
      **Exact-Agent wait/wake landed locally:** `SessionStore::events_read` composes
      `wait_ms=0..30000` around the same single-attempt cursor read using one process-local Tokio watch
      generation. Registration happens before the first durable recheck; committed Agent/Turn Event
      producers signal only after transaction commit; wakes re-run the same bounded source-first scanner;
      timeout performs one final durable recheck, so trusted-local commits from another process remain
      correct without polling/new IPC even though their wake latency may reach the deadline. The existing
      attached-Turn regression proves the reader is pending before publication, commit wakes it to the
      three Events, and a drained 1ms wait returns empty. No Event worker/per-Agent waiter registry was
      added; Tokio `time` is the only runtime feature addition.
      **v47 exact-Project wildcard placement landed locally:** opaque cursors now persist generic
      `placement_kind/ref`; migration preserves existing exact-Agent cursor tokens. The only new broad
      selector shape is exact Domain + Principal + Project with one-token `a:*`; Project-level Events and
      wildcard Project/Principal/Domain remain fail-closed. The Project handle is the existing Workspace ID;
      admission proves the exact Workspace is active/local-hosted in the selector Domain and requires active
      Domain membership as a conservative discovery gate. Concrete matching sources still authorize through
      current Agent `view`, cached once per Agent per page and rechecked before progress. ACL-only
      collaborators retain exact-Agent reads but cannot broaden Project discovery. Missing Agent authority
      is Unknown, not proven deny; Workspace move/archive/Domain deactivation stales Project cursors.
      Existing attached-Turn coverage now proves exact-Project `a:*` returns the same three Events,
      ACL-only observer broad admission is denied, exact-Agent wait/revocation semantics remain intact, and
      v44→v45→v46→v47 migration is green under strict all-target Clippy/check.
      **v48 durable subscription state owner landed locally:** the second selector consumer extracts one
      shared private `event_placement` primitive for exact-Agent/exact-Project same-Store affinity. One
      subscription row is keyed by recipient Agent + normalized subject and stores StoreScope/placement,
      monotonic generation, private baseline+scan, creator/host authority provenance, recipient management
      evidence and active|lagged|disabled state; backlog remains only the shared immutable Event log.
      Enable requires open recipient + current control/manage + selector placement admission. First enable
      baselines at the safe Event tail; repeated current active enable is a zero-write no-op; disabled,
      lagged or stale-affinity re-enable increments generation and rebaselines; disable tombstones without
      changing generation. Every Agent close path disables live rows in the same lifecycle transaction.
      Management-only bounded inspection exposes no baseline/scan/StoreScope/token. The existing attached-
      Turn regression proves baseline/scan equality, active replay timestamp/position stability,
      disable→generation-2 rebaseline, close fencing, private inspection, and
      v44→v45→v46→v47→v48 migration under strict all-target Clippy/check.
      **v49 typed EventInput + subscription delivery landed locally:** `event_authority` shares exact
      Agent source-first evidence + immutable envelope/data validation across read/delivery, while
      `event_retention` shares only the sealed logical retention floor. `agent_event_inputs` is append-only
      typed Agent input fact storage, not a second Event queue; deterministic delivery IDs bind recipient +
      normalized subscription + generation + Event.id. One same-Store admission transaction reloads the
      active generation/affinity, rechecks creator control-or-manage plus recipient submit, source Agent
      view and preliminary envelope evidence, suppresses self-delivery, dedupes before charging the durable
      causal root, inserts the EventInput and advances recipient Agent rev. Fresh Event.id is the causal root
      when none is inherited; the current projection counts <=256 accepted deliveries/root and max hop<=16.
      The explicit lower `event_subscription_deliver` host-scheduler sweep is bounded/no-worker/no-polling:
      accepted/deduped/proven-denied may advance; Unknown holds; proven placement/retention failure lags the
      generation. Current metadata-only system Events deliver; any DisclosureManifest remains Unknown.
      Existing combined Event coverage proves 3 real EventInputs to a separate recipient, 3 self-suppressed
      skips to the source Agent, drained replay no-op, 3 fresh causal roots at count1, all prior privacy/state
      contracts, and v44→v45→v46→v47→v48→v49 migration under strict all-target Clippy/check.
      **Next:** finish protected Event disclosure + causal propagation: real DisclosureManifest/provider and
      recipient-audience reauthorization, EventInput→Turn causal scope, descendant Event root/hop inheritance
      and one bounded durable host-visible exhaustion attention fact. Then land the final one-leaf
      EventTypeRegistry staging owner before custom/model publication. Handler remains the third EventScan
      consumer; wider selector placement and explicit v36 delivery_cutover_rev remain subsequent slices.
- [ ] **Phase 3** — one durable DriverKey and shared Prepared/ExternalEvidence contracts
      for bounded SourceSet capture/restore, checkpoint constructors and driver recovery.
- [ ] **Phase 4** — Jobs consume the store-local position/cursor/projection helpers with
      their own high-water and ancestor-fenced tree traversal.
- [ ] **Phases 5–6** — compose final typed leaf registries through one staging core;
      dynamic Extensions use the same shared Process owner.
- [ ] **Phase 7** — remove old Event/Session/Job compatibility code and duplicated
      semantics after proof, contract legacy cutover/provenance into normal owners, then
      sync final status/handoffs and run grouped cross-repo qualification. Normative document
      deduplication is an R6 design gate, not deferred runtime work.

**Qualification policy:** One grouped failure-boundary check per affected owner and one
cross-repo integration gate at coherent checkpoints. No fixture explosion, artificial
evidence pinning or repeated qualification of unchanged primitives. R5's original gates
remain binding wherever R6 only refactors mechanics.

## Historical R1–R5 design resolutions

The complete prior checked design checklist (including the original Phase-0 and
seal-gate decisions) is preserved in
[`DL200-R1-R5-RESOLVED-DESIGN-CHECKLIST-2026-11-21.md`](../../docs/DL200-R1-R5-RESOLVED-DESIGN-CHECKLIST-2026-11-21.md).
It is **not** implementation evidence. Follow the actual R6 queue above and
the grouped phase gates in `IMPLEMENTATION.md` instead.

## Tests and verification

- [x] Design work performed on L5 only.
- [x] AW R6 Process runtime is published and grouped-check green: `Occupancy`, `aw-process`,
      weak-Unix Unknown, AW adapter consumption, global bounded drain and flush-on-bound.
- [x] Phase-0B source checkpoint from the prior owner-hardening slice is preserved.
- [x] dotlink shared-Process consumer/projection source cut is published at `16475e4`; all AW
      Git deps pin exactly `7c16708`; old `shell_process`/ManagedProcessGuard owner is gone.
- [x] No AW `.agents/plans/ux/**` content inspected or modified.
- [x] Sandbox/mount/config-v13 implementation remains frozen.
- [x] DL-200 R5 amendment/reseal recorded; R1–R4 retained as historical provenance.
- [x] R6 design written across README/SPEC/IMPLEMENTATION/TODO and explicitly sealed;
      Phase 0 + 0B are published and Phase 1 is underway.
- [x] `IMPLEMENTATION.md` updated for R6; implementation remains separately promoted on
      2026-11-21 and proceeds under sealed R6.
- [x] R6 comprehensive handoff + copy-ready next-agent prompt created under `.agents/docs/`.
- [x] 2026-11-22 handoff refresh records the full Phase-1 implementation ledger through
      `0f3f3eb` (`Wire Agent from refs`), current cross-repo refs, validation, exact pending work and
      copy-ready onboarding prompt; AW `CURRENT_HANDOFF` + DL-200 README point to the refreshed docs.
- [x] 2026-11-23 final Phase-1 native state cut exposes `agent.list/get/edit/admission/access.list/
      access.set` through one `AgentStatePort` over the authoritative Store, extends only the existing
      state-shell/headless regressions, and passes strict all-target Clippy. Fresh SPEC §28 audit closes
      Phase 1 locally; Phase 2 is next and was not started in this checkpoint.
- [x] Phase 0 shared convergence corrective published after the v0.17.7 worker-retirement finding;
      historical v0.17.6 evidence remains provenance for unchanged semantics, while the stable Spawn
      Facility/launch-gate correction is live in AW `20a63a4` and consumed by dotlink `85ceccc`.
- [x] Phase-0 shared Process/release-correction gate — **design SEALED / AW `20a63a4` + DOTLINK `85ceccc` PUBLISHED AND PINNED; real-Bubblewrap + Windows-native evidence explicitly deferred**: one private logical Linux
      Spawn Facility in `aw-process`, with O(1) fixed/add-only process-lifetime Spawn Lanes and
      **N=1 initially**, not one thread per Process. Spawn admission is async + bounded; only
      `Command::spawn` + exact `DirectScope` attach runs on a lane, while `Process: Send` remains an
      O(n) lightweight Tokio-owned lifetime that can migrate across workers. Transfer private
      `Spawned { Child, DirectScope, pid }` with cancellation-safe RAII; closed-before-spawn means
      no effect, lost receiver after spawn cleans the new scope and never retries. Lanes never
      retire/shrink; per-request panic poisons launch admission but leaves lifetime-anchor threads
      alive for existing `PDEATHSIG` children. N>1 is internal/benchmark-driven only (concurrency
      1/8/32/128 × lanes 1/2/4; throughput, queue p50/p99, handshake p50/p99, CPU), never public
      configuration. Also require strict non-reaping main-child `waitid` before cleanup (pidfd
      readiness is wake only), reconcile stop vs physical exit without `StopRecord`-only causal
      attribution, use Windows `CREATE_SUSPENDED | CREATE_NO_WINDOW`, and port post-v0.17.8
      `81ef891` plus v0.17.9 Windows occupancy settling: exact Job accounting is sampled after
      main exit until Empty, error=>Unknown, or nonzero survives the full **uninterrupted** bounded
      settle window=>Live; cancellation/deadline interrupts grace and sweeps immediately with
      Unknown unless Empty was already proven. Async settle lives in `aw-process`, cheap synchronous
      accounting in `aw-platform-win`, with no process-name exemptions/blocking Tokio sleep.

      Also port v0.17.9 stale-launch admission into the sealed Spawn Facility: the runner
      establishes one absolute execution deadline when entering Running; the lane rechecks both
      cancellation and expiry immediately before fork via a private linearizable launch gate.
      Cancelled/expired before `Launching` => no child, no ProcessTerminal/StopRecord/kill claim;
      once `Launching` wins, a real Process follows normal RAII/stop settlement. Spawn-lane queueing
      consumes that same execution deadline; pre-Running product queue time does not. Dotlink main
      is repinned; stale-launch tests are green while real Bubblewrap + Windows remain deferred. No `spawn_blocking`/
      `block_in_place` lifetime shortcut, local dotlink Process owner, launcher-process detour,
      generic spawn/pool framework, or thaw of frozen 017 sandbox/mount work. Local L5 evidence:
      aw-process 7/7; aw-platform-unix 18 executed + 1 explicit cross-UID ignore; aw-tools 126/126;
      adapter Process 27/27; focused foreground/Graph shell smokes green; aw-cli tests type-check;
      affected strict all-target/all-feature Clippy + fmt/diff green. L5 has no Windows target, so
      Windows native/cross execution remains pending rather than inferred.
- [x] Phase-0B replay disclosure: immutable replay/no-rerun is preserved while current
      disclosure is composed at the real owners—AW Session audience + ToolBinding/Actor Envelope,
      then dotlink current issuer trust + prepared filesystem/shell/Job authority. Withheld replay
      is one opaque/minimal non-retryable acknowledgement. No cross-host preflight protocol,
      generic Tool hook or duplicate replay/resource owner was added.
- [x] Phase-0B participant Provider/payer: one full-identity `select_exact` owner is reused at
      fresh policy/subagent/participant admission; Alice retains Session/delegation resource
      ownership while Bob's exact provider/payer identity remains independent, with no owner
      fallback. Participant policy delegates are top-level/current-Submit/non-routed only; Store
      model attempts bind to the child relation identity and lifecycle binds to the resource owner.
- [x] Phase-0B compact replay/lifetime gate: AW policy current rows retain exact replay identity in
      indexed columns and compact consumed detail only post-boundary; grouped store qualification
      proves fresh policy admission after 8,205 unrelated audit rows. Remote Start owner tests 5/5
      and policy-runtime 4/4 are green. Dotlink keeps authorization-id tombstones across receipt TTL,
      uses HashMap lookup + amortized FIFO compaction (no lifetime O(n) scan), excludes payload-free
      tombstones from the active/full 4,096 receipt budget, and consolidated remote_start tests are
      16/16. Strict AW root + dotlink all-target/all-feature Clippy, test-graph checks, fmt/diff are
      green. Publication/repin is complete; real Bubblewrap and Windows-native qualification were
      explicitly deferred by the user and no longer block Phase 1.
- [~] Phase 1/4: StoreScope/StorePos foundation live; remaining work is exclusive Agent-state
      activation + exact indexed lookup, scalable live-authority-rechecked DisclosureManifest, and
      StoreScope-bound/gap-safe/path-reauthorized high-water `jobs_list` cursor pages with swap
      catch-up and race-free revision waits.
- [ ] Phase 2: gap-free Event projection progress, authority-scoped EventStore affinity, same-store Event→Agent Input transaction boundary, source-first candidate-oblivious retryable auth-unavailable with external authority fence semantics, exact source-owner delivery cutover rev + legacy accepted-input provenance sidecars, subscription/admission race linearization and explicit cross-store refusal/rebaseline.
- [ ] Phase 3: bounded/non-secret symmetric capture/restore Prepared recovery descriptors + exact driver-schema pinning across store-open/restart + immutable/held-through-commit external publication evidence + publication-transaction authority fencing.
