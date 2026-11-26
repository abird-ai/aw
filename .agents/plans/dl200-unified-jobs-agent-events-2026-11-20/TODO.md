# DL-200 — R6 simplification review + implementation queue

Base: `f035a00031b8f5879f4d77f3fd14bfe02b89305c`

Status: **R6 + SHARED PROCESS/RELEASE-CORRECTION PUBLISHED / PHASE 0B COMPLETE / PHASES 1–2 COMPLETE AND PUBLISHED / PHASE 3 UNDERWAY / PHASES 4–7 PENDING**

Current implementation: quiescent SourceDriver and VaultProvider maintenance (this checkpoint; local parent `05e552380b56d457fbb2860e28769ad7f29064b4`). See [CURRENT_HANDOFF](../../docs/CURRENT_HANDOFF.md) for the latest delta and validation. Store v60, Git recovery v4. Independently verified published AW remote main `0f9807f4` before this local slice. The signed chronology through `3c67319`, current owner map and historical design lineage are in the [November 27 comprehensive handoff](../../docs/AW-DL200-PHASE3-FULL-COMPREHENSIVE-HANDOFF-2026-11-27.md), with the [copy-ready next-agent prompt](../../docs/AW-DL200-PHASE3-NEXT-AGENT-2026-11-27.md); CURRENT_HANDOFF supersedes historical next steps.

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
- [x] **Phase 1 COMPLETE and published** — private `StoreScope` reuses existing `store_id` + nonzero
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
- [x] **Phase 2 COMPLETE and published** — v44 lands the scan prerequisite first: one canonical
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
      **v50 disclosure + causal execution closure landed locally:** accepted EventInputs now retain the
      existing v40 manifest DAG plus a concrete `agent_source` authority root and immutable normalized
      recipient audience. Reader authorization rechecks the current reader; delivery snapshots the recipient
      Agent's current owner/view ACL audience and requires source `share` when that audience broadens beyond
      source viewers. Agent-source and legacy-session roots reauthorize through existing owners; exact active
      materialized policy can authorize an individual reader, while whole-recipient provider/materialized
      expansion remains Unknown until a principal-level provider fence exists. History sealing unions
      EventInput manifests from causal prompt ancestry, retaining source/share dependencies across forks.
      `event_subscription_deliver` returns stable ready input IDs; host-only
      `SessionHandle::bind_event_input` binds one to the normal begin transaction, which records exact
      delivery/root/parent-hop on the run. Descendant system Events inherit the root and increment hop once;
      parent hop16 suppresses hop17 while still reserving source revision. Count256 or hop exhaustion emits
      exactly one non-deliverable host-visible attention commit fact. Combined coverage proves retained
      source-manifest denial after revocation, EventInput→real Turn binding, descendant root/hop 0→1 with
      publication-time max-hop projection, retry inheritance without reusing the EventInput ID, all prior
      Event semantics and v44→…→v50 migration; focused history disclosure tests + strict Clippy/check are
      green.
      **v51 EventTypeRegistry + custom publication landed locally:** the first definition leaf introduces
      one shared private staged-registry primitive over the existing left-right immutable publication owner:
      complete successor build+validation once, exact base-generation CAS publish, lock-free registered
      readers, no placeholder root clock. EventType durable identity is Extension/local id + schema version;
      typed Rust handles pin registration revision/TypeId so unrelated definitions do not stale them while
      exact remove/re-register does. Schema tags ride ordinary Event envelopes/read/EventInput metadata;
      same-subject raw Events remain generic and cannot typed-decode. Host custom publication requires exact
      open Agent + current control and derives the Agent disclosure ceiling. Lower Turn/model publication
      derives source/actor from the exact running Turn, inherits v50 disclosure+causal scope and durably caps
      successful custom publishes at 32/Turn. Pre-cutover custom revisions advance an existing legacy
      SourceEventBook without fabricating legacy deliveries, so the next system transition stays on one
      source-local sequence. Combined coverage proves generic+typed model publication, raw host collision on
      the typed subject, schema-only decode, causal inheritance, legacy revision alignment, and
      v44→…→v51 migration; EventType staging **1/1**, Job Event **4/4**, strict all-target Clippy/check green.
      **model Events tool exposure landed locally:** `aw-tools-adapter::events` adds only
      `events_publish` + `events_subscribe` definitions and a composition-only EventRouterSlot. They use
      normal exact Host/Control ToolPolicy grants + Actor Envelope; no Store capability registry and no fake
      OperationId/OperationRouter entry. The existing durable Agent router is reused as the Store-backed
      EventRouter and rechecks exact Host/Control provenance, ToolContext actor, journal scope, canonical
      execution owner and Store binding. Host-created root + stable delegated Agents install that same router
      in both Jobs/Events slots; legacy one-shot children stay unchanged. The existing durable top-level
      Agent, whose router is available only after persistence load, registers the same two trusted host tools
      at that bind point; remote MCP execution-site owners leave them unavailable rather than mislabel local
      Store effects as remote. Model publish has implicit current source/actor/Turn and reaches the v51
      running-Turn owner directly; this fixed the rejected re-activate-during-active-Turn seam by extracting
      `SessionStore::events_publish_turn`, with SessionHandle remaining a convenience delegate. Model
      subscribe has implicit current recipient, resolves Agent/Project-relative patterns and permits only
      exact same-Project Agent or current-Project `a:*` placement; wider scope stays fail-closed. Focused
      adapter **1/1** proves exact grants/installed-router gating/unchanged ToolContext, Store-backed router
      **1/1** proves relative subscription + real Turn publication/readback + cross-Project denial, and the
      existing v51 combined Event lifecycle remains green. Zero-test filtered historical subagent commands
      are not counted as evidence.
      **v52 EventHandler third consumer landed locally:** EventHandlerRegistry is the second typed leaf on
      the same staged-registry core and owns inert definitions only; registration creates no cursor/authority.
      Store activation persists one authority/StoreScope/placement-bound cursor plus at most one deterministic
      in-flight receipt keyed by activation generation + Event.id, with no payload/output/product-state copy.
      `now|earliest_available` baselines reuse the shared logical retention floor. Poll is EventScan's third
      consumer: proven skips may advance only before a receipt, Unknown holds, structural placement loss
      lags, while service-authority loss blocks. A receipt replays the same immutable Event until explicit ACK;
      ACK re-runs exact source + DisclosureManifest authorization before deleting the receipt/advancing the
      cursor. Suspend preserves cursor/receipt but stops delivery/pinning; compatible activation resumes the
      same generation; lag requires explicit rebaseline/new generation. Handler callback/product state and
      external-effect idempotency stay host-owned; no EventInput/Agent rev/causal budget or second Event
      queue exists. Shared selector hashing/public Event projection/earliest-retained-floor mechanics were
      reused rather than cloned. Focused Handler registry **1/1**, real receipt lifecycle **1/1**, EventScan
      **1/1**, projection **1/1**, EventType **1/1**, combined Event migration/lifecycle **1/1** and strict
      affected Clippy/check are green; migration now reaches v52.
      **v53 same-store cross-Domain Event universes landed locally:** the reviewed broad Agent-subtree
      grammar is exact/wildcard Domain + exact/wildcard Principal + wildcard Project + wildcard Agent. One
      private `universe` placement shape reuses StoreScope and the existing Domain/Workspace owners; for the
      scanning principal it enumerates only currently-authorized active Domains and requires each included
      Domain's authority Store to equal this StoreId and every active Workspace to be locally hosted.
      Hidden remote Domains do not perturb affinity; an authorized remote Domain makes read stale and
      subscription/Handler lagged rather than returning a partial world. Concrete source membership + Agent
      view + DisclosureManifest checks remain per Event, and EventInput admission rechecks the same placement-
      aware source evidence after scan. No topology generation or merged cursor was added: production
      Domain/Workspace owners do not retarget Event-store placement in place; Workspace writer epochs are
      intentionally not misused as Event placement generations. v53 only widens the existing three durable
      placement-kind CHECKs to include `universe` while preserving progress/receipts. Model contextualization
      now exposes only these same provable exact/Project/universe shapes. One grouped regression uses real
      Agents in two distinct local Domains: read sees both Events, subscription self-suppresses one and admits
      the cross-Domain EventInput, Handler ACKs both; hidden remote Domain is invisible to affinity, while an
      authorized remote fixture stales/lags all three. EventSubject **2/2**, cross-Domain grouped **1/1**,
      Handler lifecycle **1/1**, combined migration/lifecycle **1/1**, model contextualizer **1/1**, model
      router **1/1**, affected all-target check and strict Clippy are green; migration now reaches v53.
      **v54 final Event delivery cutover landed locally / Phase 2 complete:** one schema-owner IMMEDIATE
      transaction proves each legacy SourceEventBook next revision equals `event_sources_current.next_event_rev`,
      writes `delivery_cutover_rev = current next_event_rev`, translates exact result/settled/attention rules,
      bridges only genuinely pending pre-cutover rows through the normal typed EventInput owner, clears old
      pending JSON and fences all later non-empty legacy subscription admission. Whole revisions stay on one
      owner. Same-revision same-effective-mode legacy names coalesce under deterministic
      source/revision/recipient/generation/mode identity; existing ordinary exact DL-200 subscriptions are
      preserved via a `shadow_existing` compatibility overlay rather than overwritten. Already-accepted
      legacy `qi_...` Inputs keep their exact factual identity and receive only an immutable
      `legacy_protected` disclosure sidecar. Its frozen ceiling contains only the factual recipient actor;
      a later ACL viewer is not inferred as historical audience. Where the legacy sender hash uniquely
      reconstructs the source Agent, the normal `agent_source` root is retained so future view/share can
      explicitly reauthorize expansion; otherwise the fact stays hard-frozen. New v54 sources are born with
      cutover rev 1. Runtime reconciliation is worker-free: existing source-transition wake points drive only
      translated compatibility subjects, accepted EventInputs project into existing ActorQueue custody under
      their exact `edi_...` ID, and live-owner registration/restart reprojects any unconsumed compatibility
      Input with no factual run binding. Focused cutover **1/1**, combined v44→...→v54 migration/lifecycle
      **1/1**, Agent Jobs group-authority **1/1**, affected all-target check and strict Clippy are green; generic
      zero-test name filters are not counted.
      **EventInput adapter reconciliation:** the three uncommitted forwarding
      edits required by original v54 Actor queue calls are retained in signed
      history; BudgetJournal now forwards both trusted causal-binding methods
      to the original SQLite owner. Real stable child accepted EventInput
      reaches its original budgeted Turn and descendant result, with exact
      causal root/hop, duplicate suppression and no next-Turn hint leakage.
      No new schema/owner/scheduler. Journal 14/14, Store Event
      2/2, Actor queue and wake/disclosure 1/1 each, stable child 2/2,
      strict affected Clippy, durable-only and minimal all-target checks,
      full CLI and scoped fmt/diff passed. Restore workspace-only child
      private-state feature guard; the previous durable-only binary
      compilation blocker is corrected without widening runtime authority.
      Exact mechanical formatting follows in a separate commit.
      **Next:** begin sealed Phase 3 — one durable DriverKey plus shared Prepared/ExternalEvidence contracts
      for bounded SourceSet capture/restore, checkpoint constructors and driver recovery. Legacy cutover
      contraction remains Phase 7 after cross-version drain/restart proof.
- [~] **Phase 3 UNDERWAY** — one durable DriverKey and shared Prepared/ExternalEvidence contracts
      for bounded SourceSet capture/restore, checkpoint constructors and driver recovery.
      **first SourceDriver leaf/prerequisite landed locally:** `aw-agent-core::source_driver` reuses the
      Phase-2 private staged-registry core; durable `DriverKey` is stable built-in/Extension namespace +
      local ID only, while `DriverSchemaPin` keeps binding/anchor/recovery schema adjacent and independent
      of runtime registry generation. Exact typed handles pin registration revision/type; durable resolve
      uses DriverKey + artifact schema and survives unrelated leaf publication. The leaf is deliberately
      add-only until Store can prove the complete live binding/anchor/unresolved-recovery dependency set;
      no caller-supplied incomplete pin list can authorize replacement/removal. Shared validated vocabulary
      now includes SourceName/access/checkpoint policy, symmetric Capture/Restore Prepared descriptors,
      `NotStarted|Completed(evidence)|Unknown` inspection, typed non-secret recovery fields and immutable
      vs opaque-held-fence ExternalEvidence. Opaque fence handle is not publication authority; later
      constructor must hold the live provider fence through AW commit. Host effective SourceSet limit
      defaults to 128 and may only be lowered; absolute safety ceiling remains 1024. Focused SourceDriver
      **1/1** + repository fmt, full workspace all-target check, strict aw-agent-core Clippy and scoped diff-check are green.
      **v55 binding/restart owner landed locally:** `sandbox_source_bindings_current` retains exact
      named SourceBinding identity, DriverKey + binding_schema, access, checkpoint policy and binding_rev;
      v54 migration seeds empty SourceSets. Trusted fresh Sandbox publication atomically admits the
      complete already-materialized set under the effective host limit, with absolute bounded decoding.
      Owner/direct startup and staged SQLite backup restore resolve every live binding through the
      installed registry; incompatible/missing pins retain their exact rows with durable recovery_required
      diagnostics, and compatible reinstall revalidates without advancing revisions. Snapshot metadata
      omits protected source/locator values. Agent selection and source-bearing close fail closed until
      realization/lifecycle support; no implicit replacement, rebind or external effect.
      Grouped Sandbox owner **2/2** and SourceDriver **1/1** are green. Full workspace all-target
      check, strict affected-crate all-target Clippy, repository fmt and scoped diff-check are green.
      **Git clean/dirty capture and retention landed locally:** concrete `aw-git/source-driver`
      registers builtin/git (binding schema 1, anchor 1–2, recovery 1–3) in the existing leaf;
      the default Git library stays Agent-independent. Shared bounded tree observation and read-only
      Prepared planning bind exact source/checkout/base, Git-normalized tree and index/untracked
      fingerprints. Two independent private indexes seeded from HEAD produce the same dirty tree;
      the deterministic synthetic commit binds the preparation identity and original base parent.
      Final tracked contents/deletions and nonignored untracked regular files are captured; tracked
      symlinks are entries, untracked links are not traversed. Clean initialized gitlinks capture
      exact nested HEAD, uninitialized empty gitlinks retain the indexed OID; dirty nested sources
      fail closed. Clean/dirty capture uses one create-only private retention ref, immutable content
      anchor and exact read-only recovery inspection, including schema-1 compatibility. Missing,
      changed or damaged retention is Unknown; no blind retry, original HEAD/index mutation or
      retention cleanup. Snapshot-local object closure and synthetic identity are verified; ref
      retention and live AW barriers remain the future publication owner's responsibility.
      **Isolated Prepared Git restore landed locally:** recovery schema 3 binds exact retained anchor,
      origin checkout, absent private destination and parent identity. The fresh linked worktree
      remains detached at original base HEAD with its real index based on that commit. Dirty content
      is materialized through a private index; synthetic HEAD is never exposed. Shared tree observation
      verifies captured contents including formerly tracked additions, independent of base index.
      Unchanged gitlinks are empty/uninitialized; new/changed gitlinks and ambiguous directory
      replacements fail closed before materialization. No submodule fetch/initialization. Recovery
      requires exact operation-marked registration and complete matching materialization; absence,
      partial or changed effects are Unknown, no automatic retry or removal. HeldFence is identity
      only; the future Store owner must hold the live local source/destination barrier through commit.
      Real Git suite **32/32**, grouped capture/restore, retention, restart, legacy schema and failure
      boundaries. Workspace all-target check, strict Git feature Clippy, default-feature check,
      repository fmt and scoped diff-check are green on L5.
      **v56 checkpoint storage landed locally:** common validated SourceAnchor/SourceSet, typed
      Git anchor roundtrip (no Store Git dependency), immutable checkpoints and checkpoint_sources
      with live HistoryBase retention, captured layout, version/retention identities and independently
      versioned driver schemas. Retirement preserves replay identity without a live history root.
      The private complete-set publication helper is qualified at the storage boundary only;
      source-bearing production publication remains closed pending Prepared/fences. Trusted Rust
      history-only construction uses the latest settled HistoryBase, current view, and indexed
      stable authenticated request provenance; replay precedes ref resolution and rechecks disclosure,
      no Agent revision mutation. Metadata reads omit protected source identities. Owner/direct
      startup and staged backup restore use the same registry snapshot/compatibility transaction
      for bindings and retained anchors; retired rows awaiting cleanup remain dependencies.
      Missing/incompatible schema keeps exact anchors and marks recovery_required, not materialization
      authority. Grouped checkpoint owner **1/1**, Sandbox owner **2/2**, SourceDriver **1/1**, real
      Git suite **32/32** (typed anchor roundtrip). Strict affected-crate all-target Clippy with
      Git source-driver, full workspace all-target check, repository fmt and scoped diff-check
      are green.
      **v57 construction receipt/source recovery custody landed locally:** existing immutable
      construction audit owner now has one indexed replay lookup across historical task-only and
      current typed receipts. Task-bearing Agent construction uses the shared claim/CAS transition;
      legacy receipts decode their original stable creator without rewriting audit. Named
      PreparedSourceSet uses the common source bound, exact operation/preparation ID, unique names/
      effect identities and driver recovery schema. The indexed prepare identity is reserved once.
      Unresolved capture/restore receipts retain private candidates/descriptors; history-only/
      task-free constructors cannot bypass a consumed effectful request. Security audit metadata
      omits protected descriptors. Startup/staged restore resolve current unresolved dependencies
      from indexed original claims and the same SourceDriver snapshot as bindings/anchors.
      Missing/incompatible definitions leave exact Prepared/Unknown custody; no blind retry,
      external effect or terminal source transition. Grouped construction receipt **1/1**, actual
      task-bearing Agent create/replay **1/1**, checkpoint owner **1/1**, Sandbox owner **2/2**,
      SourceDriver **1/1**, strict affected-crate Clippy, workspace all-target check, fmt and
      scoped diff-check are green. Broader Agent-host **1/2**: unchanged legacy
      subscription fixture refuses at v54 cutover (not claimed green).
      **Store-local source mutation barrier landed:** validated binding/anchor sets share
      DriverKey + SourceRef scopes, deduplicated after the original source-count bound. Existing
      coordination execution leases acquire the whole set atomically for the exclusive owner
      incarnation and preparation identity (no timed renewal); active or hazard conflicts refuse,
      independent sets proceed. Exact live token revalidation and settlement in the publication
      transaction, rollback retains claims, Unknown/abandon/restart preserve hazards. Generic raw
      lease APIs cannot bypass reserved source scopes. Existing fresh Sandbox source metadata
      admission checks the same scopes transactionally. No new lease table, migration or driver
      effect. Source-bearing Agent execution remains closed; cross-Store/external fences and
      full runtime writer/structural mutation participation are not yet implemented.
      Grouped source barrier **1/1**, affected coordination **3/3**, Sandbox **2/2**,
      construction receipt **1/1**, strict Store Clippy, workspace all-target check, fmt and
      scoped diff-check are green.
      **Receipt-bound checkpoint capture preparation landed:** trusted source-bearing planning
      selects the current required or explicit named bindings, resolves exact binding schemas,
      checks current Agent view/open same-Project placement and settled HistoryBase/run hazards.
      Read-only host planning runs outside SQLite with the owner-minted candidate ID. Final
      pre-effect transaction reobserves exact binding/Sandbox revisions, history and access
      generation and commits the complete source barrier and Prepared receipt together. Replay
      precedes planning; missing, partial, incompatible and stale inputs refuse. Additive capture
      boundary retains selected bindings and note in the existing receipt; startup resolves
      binding and recovery dependencies. Live Prepared capture rechecks the exact receipt and
      complete barrier; Unknown settles all scopes as hazards atomically. Source grants/external
      fences remain separately authorized by the trusted host; production source-bearing
      realization/publication stays closed. Empty automatic selection routes to the future
      history-only wrapper. Grouped capture preparation **1/1** (test-only source-bearing fixture),
      checkpoint owner **1/1**, source barrier **1/1**, construction receipt **1/1**, Sandbox
      **2/2**, task-bearing Agent create **1/1**, strict Store Clippy, workspace all-target check,
      fmt and scoped diff-check are green.
      **Exact Prepared capture inspection/recovery landed:** authenticated original receipt lookup,
      current view and compatible driver schema checks; complete same-generation hazard set
      reclamation in one existing coordination transaction, with no new operation or live-token
      theft. Read-only host inspection receives the exact original named bindings/descriptors
      under the barrier; only the complete matching Completed set returns an inspected token.
      Missing, partial, NotStarted/Unknown and mismatched results preserve unresolved custody;
      no blind replay or generic release. Evidence is not authority. Stale original boundary
      can be inspected but fails publication revalidation. Grouped capture owner **2/2** covers
      real disk restart and fixture effects (source-bearing association test-only).
      Source barrier **1/1**, construction receipt **1/1**, checkpoint owner **1/1**,
      coordination **1/1**, strict Store Clippy, workspace all-target check, fmt and
      scoped diff-check are green.
      **Evidence-fenced checkpoint publication landed:** generic core SourcePublicationProof
      distinguishes immutable/non-revocable evidence from a live provider-owned fence guard.
      Trusted verifier revalidates exact descriptors/results/fingerprints/current authority;
      saved HeldFence handles cannot substitute for guards. Shared private construction receipt
      publication owns the complete matching proof set through commit/rollback and checks
      the exact unresolved receipt/preparation barrier. Checkpoint owner rechecks current
      Store authority/history/binding boundary in the final transaction; all anchors, local
      provenance, Applied receipt and lease settlement commit together. Failure preserves
      Unknown custody; replay uses original fingerprint and current disclosure without effects.
      No new registry or migration; reviewed concrete external fences and runtime realization
      remain separate. Grouped capture-owner **2/2** includes final authority revocation,
      live guard lifetime, saved handle refusal, atomic rollback and exact replay. Checkpoint
      owner, receipt, barrier, task-bearing Agent create and SourceDriver **1/1 each**;
      strict Core/Store Clippy, workspace all-target check, fmt and scoped diff-check green.
      **Receipt-bound restore preparation/inspection landed:** exact live checkpoint HistoryBase,
      retained named SourceSet and current Derive/Share/HistoryGrant checks use the existing
      history owner, current destination Project writer and fresh Agent/root/branch/Sandbox IDs.
      Receipt references retained checkpoint anchors, pins exact source set/layout and original
      literal request, preserves captured layout and complete restore descriptors; startup
      resolves anchor/recovery schemas. Capture/restore share claim, exact receipt/barrier check,
      Unknown settlement and existing hazard recovery. Planning is read-only; restart inspection
      never replans/reexecutes. Complete matching bindings/evidence only; retired checkpoint
      blocks publication. Grouped restore **1/1**, including source history advance, rollback
      of the ordinary exact-history constructor, private candidate identity, partial effects
      and restart. History owner **4/4**, capture owner **2/2**, checkpoint, receipt, barrier
      and task-bearing Agent create **1/1 each**; strict Store Clippy, workspace all-target
      check, fmt and scoped diff-check green. Source-bearing Agent realization/publication
      remains closed; no migration.
      **Atomic source-bearing restore publication landed:** existing Sandbox owner publishes
      fresh candidate bindings; constructor-owned FreshSandbox selection enters the ordinary
      resource/Agent/exact-history constructor without an intermediate auto Sandbox. Complete
      live driver proof set, current retained history/destination/writer/registry checks,
      fresh layout, new Agent/Sandbox/HistoryGrant, commit-log provenance, Applied receipt and
      complete lease settlement share one commit. Rollback preserves unresolved custody.
      Original fingerprint replay survives checkpoint retirement. Restored Agents are readable
      but held; direct Session activation/model-context admission, Actor/queue admission and
      reopening refuse pending actual source-aware execution authority. Startup accepts only
      held quiescent exact-layout associations; missing drivers degrade metadata. No copied
      grants/secrets/live state or new visibility owner. Grouped restore **1/1** includes actual
      publication/rollback/restart; capture **2/2**, checkpoint **1/1**, Sandbox **2/2**,
      History **4/4**, receipt/barrier/task-bearing create **1/1 each**, Actor admission **2/2**,
      Session view **6/6**, strict Store Clippy, workspace all-target check, fmt/diff-check green.
      **V58 independent SandboxGrant evidence landed:** one row per exact Agent/Sandbox,
      no implicit grants; trusted exact actor/stable user + host policy/authority identity,
      current Agent access-generation dependency, bounded exact source binding pins.
      Project writer-authorized CAS issue/revoke with retained revisions and no Agent/Sandbox
      revision fanout. Issuance refuses stale host subject or binding; current read-only
      preflight checks actor/access/host identity and binding compatibility, intersects
      binding/grant/current host/tool source ceilings, denies omitted names. Direct exact
      actor ceiling; no group-wide expansion or new physical policy owner. Runtime
      source-bearing execution remains held; observation is not an effect capability.
      Grouped grant **1/1** covers v57 migration, same-Sandbox independent authority, ACL
      broadening, narrowing, stale source/authority rejection, revoke/reissue/restart.
      Sandbox **2/2**, checkpoint/capture/restore **4/4**, receipt/barrier/SourceDriver
      **1/1 each**, strict Core/Store Clippy, workspace all-target check, fmt/diff-check green.
      **Typed SandboxDriver and fenced host effect handoff landed:** native typed leaf
      reuses StagedRegistry and NativeDefinition with SourceDriver, registry-bound handles,
      add-only dependency policy. Trusted live policy/tool guard, exact requested source
      modes and Store/writer/layout observation precede inert driver preparation outside
      SQLite. One-shot start rechecks the boundary and claims the whole read/write source
      set before handing off to the original effect owner. Guard survives start, not
      Process lifetime. Caller owns durable intent/identity; no separate execution ledger.
      Factual settlement releases claims and advances Sandbox rev once for settled write
      capability; NotStarted is neutral and Unknown preserves hazards. No runtime dispatch
      promotion, concrete containment claim or Agent rev fanout. Grouped effect **1/1**
      with real fixture file effects, revocation race, full barrier, guard and restart.
      Sandbox owner **4/4**, checkpoint/capture/restore **4/4**, receipt/barrier/SourceDriver
      **1/1 each**, strict Core/Store Clippy, workspace all-target check, fmt/diff-check green.
      **Original Process host composition landed:** SQLite-only aw-host source-effects
      consumes the prepared driver command through the shared PipeProcess/aw-process
      path. ProcessToolAuthority reuses the existing exact authenticated tool binding
      and synchronous publication fence for the Store claim; revocation after admission
      uses original cancellation/watch semantics. Typed launch pins and source-policy
      guard transfer into the actual spawn lane, including dropped awaiters. No second
      Process or policy owner; ordinary PipeSpec/driver launches share output setup.
      Definite no-spawn vs uncertain setup preserved without invented ProcessTerminal.
      Original effect owner records actual facts before settlement; known terminal plus
      driver-confirmed source quiescence required for release, otherwise Unknown.
      Grouped real host Process **1/1** covers revocation, pre-cancel/OS refusal, actual
      mutation and terminal, lease release/revision, unknown restart. Fixed fixture
      command is not arbitrary shell containment; frozen physical plan untouched.
      Host **14/14**, PipeProcess **27/27**, Sandbox **4/4**, strict Host/Adapter/Store
      Clippy, workspace all-target check, minimal check/no SQLite dependency, fmt/diff-check
      green (minimal retains two existing CLI dead-code warnings).
      **Direct journal-linked source custody landed:** exact active original ToolStarted
      (Turn/call/source/effect/site/actor/writer epoch) identifies one source effect.
      ProcessToolAuthority supplies the live binding; tool origins cannot use the generic
      Host ID namespace. One immutable local commit-log custody fact plus existing
      source barrier commit before launch; exact named modes and versioned-binding digest,
      no duplicated ToolRecord or Process lifecycle. Original intent cannot relaunch.
      Matching ToolFinished and factual quiescence/no-start are required for release;
      release fact references original terminal and commits with barrier/rev settlement.
      Missing terminal/uncertainty preserves hazards. Terminal-linked recovery reclaims
      exact set under current disclosure/Project writer and compatible drivers, not a
      new grant/effect; released replay performs no inspection/write. Existing host
      regression uses real TurnJournal and real Process, including duplicate start,
      missing terminal, unproven quiescence, restart and idempotent release.
      Host **14/14**, Sandbox **4/4**, checkpoint/capture/restore **4/4**, barrier/receipt
      **1/1 each**, strict Host/Store Clippy, workspace all-target check, fmt/diff-check green.
      **Pinned native filesystem route landed:** shared original canonical request/engine/
      limits/output over existing platform CapabilitySet. One exact independently resolved
      source directory and tool-root identity, effective read/write capability, no ambient
      or legacy fallback, no multi-source/Graph/Process admission. Original host router
      and ToolResult hook, exact bound Agent/Turn/ToolCall including actual arguments,
      current ToolBinding + Actor Envelope commit; shared SourceToolAuthority for native
      and Process. Weak registration-side authority prevents router/registry ownership
      cycle; explicit install refuses mismatched owner or occupied slot. Pinned capability
      and host guard follow blocking worker. Positive quiescence is durably recorded
      before original ToolFinished with complete claims retained; post-receipt hook or
      restart uses the same release transaction, no pending completion map/duplicate
      receipt. Unknown stays fenced. Grouped native regression uses real pinned read/
      write, symlink escape refusal, changed arguments, post-journal hook and missed-hook
      restart. Actual Actor Service installation/materialization authority still pending.
      Host **15/15**, native adapter **99/99**, router **3/3**, tool authority **6/6**,
      Sandbox **4/4**, checkpoint/capture/restore **4/4**, barrier/receipt **1/1 each**;
      strict Core/Host/Adapter/Store Clippy, workspace all-target, minimal/no SQLite,
      fmt/diff-check green (minimal retains two existing CLI dead-code warnings).
      **Concrete Git native resolver + root-free host builder landed:** actual Store
      registered typed Git driver verifies independently approved checkout path/physical
      identity and original source/materialization stamps before realizing native capability.
      Existing Repository pins survive until the narrowed capability pins the same root.
      Git admin/common/pointer exclusions use platform CapabilitySet; host live policy
      and private-artifact registry remain independent. Resolver runs on blocking worker;
      final effect admission revalidates original Store boundary. SourceFilesystemRoute
      configure uses the original registration/policy/parallelism/router/hook owners,
      explicit canonical native-only selection, exact authority/limits, no ambient root
      open or fallback, no shell/Jobs/control/remove. Optional Git source-effects feature.
      Existing grouped native regression uses real Git when enabled; original restore
      regression qualifies exact restored materialization and wrong root/identity/admin
      mapping refusal. Held production admission stays closed. Git **32/32**, Host
      **15/15** with Git and **15/15** without, native adapter **99/99**, router **3/3**,
      Sandbox/checkpoint **4/4 each**; strict Git/Host/Adapter/Store Clippy, workspace
      all-target, minimal/no Git or SQLite dependencies, fmt/diff-check green.
      **Live native approval and Actor host selection landed:** independent exact
      directory/source/Agent/actor/Store approval with retained physical pin, real policy
      digest and existing CAS grant publication. Live revocation waits for original
      guards and permanently withdraws the approval; persisted grant alone insufficient.
      GitNativeSourceHost composes the existing driver handles, live approval, root-free
      builder and fresh per-build router. HostAgentActivation selects it explicitly and
      checks the original Store gate before any ambient tool-root realization. No native
      Job/control/wake grant or production source-bearing admission opened. Grouped
      native regression covers separate publication, exact owner/backing binding,
      tool selection and withdrawal in both feature variants; source-free task activation
      remains green. Revalidated interrupted work without touching concurrent journals:
      Host **15/15** with Git and **15/15** without, Sandbox/checkpoint **4/4 each**,
      router **3/3**, task-bearing activation **1/1**. Strict Host/Store/Adapter
      Clippy, workspace all-target, durable-only/minimal and minimal dependency
      exclusion, fmt/diff-check green (two existing minimal CLI warnings).
      **Quiescent metadata-only source binding mutation landed:** trusted host
      sandbox_sources_set uses original Project writer + Sandbox rev CAS, compatible
      incoming driver, bounded selection, and complete old/incoming source scopes free
      of active/Unknown claims in the same transaction. Shared legacy run/hazard
      quiescence check with capture includes all backing Sessions of sharing Agents.
      First attach only unused isolated known layout; existing source-bearing Agents
      held; final removal blocked pending safe source-free layout. Caller verifies
      complete physical layout and independent source authority; no external effect
      or new durable receipt for metadata-only changes. Changed binding uses next
      Sandbox rev, never reuse name generations; unaffected binding and Agent revs
      unchanged, idempotent no-op, local source-change provenance. Grouped grant
      regression now uses real mutation, including remove/reinsert identical source,
      complete-set barriers, stale grant/CAS and restart. Sandbox **4/4**,
      checkpoint/capture/restore **4/4**, barrier/receipt **1/1 each**, real Host
      source effects **2/2**; strict Store Clippy, workspace all-target, fmt/diff-check green.
      **Checkpoint retirement and retention reconciliation landed:** bounded live
      list (current View, exact immutable cursor), current Manage idempotent retirement
      retains tombstone/replay identity, no Agent rev fanout; unresolved restore
      pins history/anchors and blocks retirement. Trusted cleanup uses retired anchor
      rows as original intent and existing complete source claims; no second ledger.
      Current Manage, independently authorized external cleanup/fences, compatible
      schemas and no shared live retention. Inspect outside SQLite; exact Unknown
      hazards recover with original set even under lowered limit; final complete
      row deletion + claim settlement atomic, replay no inspection. Actual Git ref
      release verifies repository identity and exact private ref, no-deref expected
      OID CAS + absence verification; changed/symbolic refs fail closed, no worktree/
      object/HEAD/index deletion. Existing grouped checkpoint/restore and Git
      regressions exercise retirement, shared/unknown custody, restart and CAS release.
      Trusted driver/Store seam only; automated production cleanup and RPC pending.
      Git **32/32**, checkpoint/capture/restore **4/4**, Sandbox **4/4**, barrier/
      receipt **1/1 each**; strict Git/Store Clippy, workspace all-target, fmt/diff-check green.
      **Source-bearing Sandbox logical close landed:** original Project writer and
      exclusive Store owner; all bound Agents closed, no live exact-path sharing
      Agent, legacy layout quiescence and whole retained-source set unleased. Missing
      driver or lowered admission limit cannot prevent metadata retirement; original
      binding/dependency rows remain, closed compatibility revalidated on restart.
      Exact known closed retained cwd/tool-root paths fenced across Store constructors,
      automatic/explicit selection, legacy execution and binding mutation (physical
      alias/ancestor/cross-Store authority remains host-owned). No ambient reuse,
      physical deletion or new lifecycle owner. Existing restore regression exercises
      close/refusal, Unknown recovery, missing driver, retained files/pins and restart.
      Sandbox **4/4**, checkpoint/capture/restore **4/4**, barrier/receipt **1/1 each**,
      real Host source effects **2/2**; strict Store Clippy, workspace all-target,
      fmt/diff-check green.
      **Closed-Sandbox retained-layout release landed:** trusted reconciliation of
      exact original versioned bindings and closed layout; shared existing close/run/
      hazard checks, compatible driver, open shared-materialization/known layout/child
      dependencies and complete source barrier. Checkpoint and Sandbox cleanup reuse
      one claim-or-recover helper; retained rows are intent, no new ledger. Caller proves
      exact physical release and whole layout safe, retaining returned external guard
      through commit. Unknown retains complete bindings/claims; successful atomic
      release removes bindings, advances Sandbox rev once and records provenance.
      Historical source Session remains fenced by immutable release fact; fresh clean
      baseline may reuse released layout. Existing grouped restore regression covers
      open dependency, partial cleanup, restart, lowered limit and idempotency.
      Trusted seam only; actual managed Git worktree cleanup and production physical
      authority remain pending. Sandbox **4/4**, checkpoint/capture/restore **4/4**,
      barrier/receipt **1/1 each**, real Host source effects **2/2**; strict Store
      Clippy, workspace all-target, fmt/diff-check green.
      **Managed Git restore-materialization release landed:** original Prepared
      descriptor + exact published binding, existing parent/source/checkout identity,
      detached base/operation lock, exact original tree/index verification. Unchanged
      clean/captured-dirty release via exact Git worktree removal under independent
      physical fences; later edits, extra files, changed identity/marker/index, missing
      content or relocated registration fail closed. No broad prune or shared backing
      deletion. Require absent target/registration/operation marker, including restart.
      Checkpoint ref may already be retired. Original Applied restore preparation
      resolves through existing indexed claim/current receipt and exact candidate/
      bound Agent/result identity; compatible recovery schema, retained closed Sandbox
      and trusted Project/source authority. No new durable ledger. Existing Git/Store
      grouped regressions cover actual cleanup, changed/foreign worktrees, absent
      replay and exact provenance after retired anchor cleanup/restart. Git **32/32**,
      checkpoint/capture/restore **4/4**, Sandbox **4/4**, receipt/barrier **1/1 each**;
      strict Git/Store Clippy, workspace all-target, fmt/diff-check green.
      **Metadata-only fresh baseline restore landed:** explicit None/empty Auto
      capture shares immutable checkpoint owner; empty Auto verifies no required
      bindings and keeps Auto replay identity/coverage, latest committed history.
      Restore exact history through ordinary fresh Agent/Sandbox constructor, same
      candidate checks and literal replay as captured-source restore. Independently
      authorized fresh current baseline may contain already-materialized sources
      (only ephemeral for empty Auto); no captured-source fallback. Complete incoming
      scopes unleased, source-bearing admission held, known exact open/retained
      execution paths excluded across Store. One atomic Applied receipt without
      fictitious Prepared or empty descriptors, no new ledger/schema. Existing grouped
      regressions cover latest history, required-source refusal, authority, current
      baseline, original replay after retirement/restart. Effectful baseline materialization
      and production policy remain with their Prepared/host owners.
      Checkpoint/capture/restore **4/4**, Sandbox **4/4**, receipt/barrier **1/1 each**,
      real Host source effects **2/2**; strict Store Clippy, workspace all-target,
      fmt/diff-check green.
      **Unpublished capture/restore candidate reconciliation landed:** original
      Prepared live abandon or exact restart hazard recovery; current inspection and
      independent cleanup authority, indexed initial/current receipt and unpublished
      candidate checks, complete source set. Trusted proof of release/absence with
      external guard held through atomic Rejected + barrier settlement. Unknown
      retains original custody; Rejected replay never inspects/replans; Applied
      cannot abandon. Reuse candidate absence and recovery primitives; no new ledger.
      Git releases exact prepared capture retention using original descriptor and
      conditional ref deletion; exact completed or untouched registered restore
      candidates use existing verified removal. Foreign/modified/partial content,
      changed refs and uncertain scratch/registration remain fenced. Existing
      grouped regressions qualify partial cleanup/restart, lowered limits, replay
      and real Git release. Production reconciliation/physical authority and
      arbitrary partial/scratch cleanup remain pending. Git **32/32**,
      checkpoint/capture/restore **4/4**, Sandbox **4/4**, receipt/barrier **1/1 each**;
      strict Git/Store Clippy, workspace all-target, fmt/diff-check green.
      **Shared custom initial history ACL landed:** ordinary history-derived
      agent.create and both restore paths use existing AgentCreateAccessGrant,
      destination principal resolution, current manifest disclosure and initial ACL
      owner. One complete HistoryGrant + initial ACL, no sequential expansions or
      source authority. AgentHistoryForkAccess Custom (Clone, existing unit encoding
      unchanged); literal receipt semantics; current preflight and final revalidation.
      Grouped history/restore tests exercise actual custom viewer history, invalid
      audiences before planning, one grant and no source grant. Unchanged broad
      agent_host state-shell test hits retired legacy Job Event admission; original
      task-bearing creation passes. Initial task integration remains separate.
      Final focused checkpoint **4/4**, history **4/4**, task creation **1/1**,
      Sandbox **4/4**, receipt **1/1**; strict Store Clippy, workspace all-target,
      fmt/diff-check green.
      **Captured-source restore initial task landed:** shared InitialTask mints
      original candidate queue identity, encrypted custody and trusted site, used
      by ordinary Agent create and restore. Optional task digest in request
      fingerprint, old taskless serialization unchanged. Prepared receipt + complete
      source barrier before task write, verify custody before final commit, one
      ordinary Pending input atomic with fresh Agent/Sandbox/history/ACL/Applied.
      Held source admission remains; abandon releases private task only after
      complete source cleanup, Unknown/replay preserve original custody. No new
      queue owner or ledger. Existing grouped restore covers restart, rollback
      after queue insertion, exact pending input, replay and abandonment.
      Baseline task explicitly unsupported until task-only Prepared recovery.
      Checkpoint **4/4**, task creation **1/1**, receipt **1/1**, Sandbox **4/4**,
      encrypted queue restart **1/1**; strict Store Clippy, workspace all-target,
      fmt/diff-check green.
      **Task-only baseline restore landed:** None/empty Auto with optional initial
      task through original InitialTask/Prepared/queue owners; trusted site, exact
      original layout and canonical baseline digest, no source lease. Existing
      exclusive Store connection serializes bounded local vault write + recovery
      + publication; shared custody methods on original vault, no connection reentry
      or external callbacks. Missing master refuses regeneration; unresolved task
      receipts retain checkpoint history. Resume verifies original ciphertext and
      current exact baseline authority, never rewrites missing input or materializes.
      Abandon deletes only private task, Rejected replay inert, Applied protected.
      Atomic Agent/Sandbox/history/ACL/Pending/Applied, ordinary source-free
      admission or held source-bearing admission. Grouped restore covers failure,
      restart, retained history, exact selection, abandonment and replay.
      Checkpoint **4/4**, receipt **1/1**, task create **1/1**, Sandbox **4/4**,
      SecretStore **6/6**, queue restart/missing master **2/2**; strict Store Clippy,
      workspace all-target, fmt/diff-check green.
      **Fenced Git checkpoint restore host consumer landed:** registered driver +
      original Store Prepared/recovery/publication/cleanup owners, no extra ledger.
      Replay before layout/approval; exact whole-set live independent host fence
      covers repository administration, retention, source disclosure and fresh
      destination isolation. Approved canonical checkout/parent identities and
      original descriptor/anchor/target checked before effects or inspection.
      Restore only after Prepared; resume inspects without executing/replanning;
      publish revalidates under the actual live guard through Applied; abandon
      releases only original unpublished candidates through Rejected. Two real
      dirty Git sources, incomplete approval, failed commit, restart, wrong
      destination refusal, original recovery, replay, abandonment and unchanged
      source HEAD/index in one grouped Host regression. Host **16/16**,
      Git **32/32**, checkpoint owner **4/4**; strict affected all-target/all-feature
      Clippy, workspace all-target/all-feature check, scoped fmt/diff green.
      Trusted captured-Git composition
      only: actual production physical/provider approval provisioning, mixed
      drivers, effectful baseline, partial/scratch cleanup and RPC remain open;
      source-bearing execution stays held, schema v58 unchanged.
      **Managed closed-Sandbox Git retirement consumer landed:** reuse original
      retained bindings, Applied descriptors, source barrier, independent live
      whole-layout fence and Store settlement; exact approved origin/target
      verification shared with restore. Require unchanged initial binding
      generations and complete disjoint locations. One checkout root or a
      private parent containing only direct managed checkout children; cwd must
      be contained. Refuse foreign entries, verify exact releases and absent/empty
      whole root; never delete extra files or shared backing. Partial release
      retains whole-set custody across restart/lowered source limit. Released
      replay performs no authority/driver call. Extended real grouped Host
      regression covers checkpoint-retention release, ordinary close, wrong
      locations, foreign files, later work and partial cleanup/recovery.
      Host **16/16**, Git **32/32**, strict affected Clippy, workspace all-target/
      all-feature check and scoped fmt/diff passed. Other layouts, changed bindings, mixed drivers and production physical/
      provider authority provisioning remain separate. Schema v58 unchanged.
      **Effectful versioned baseline restore landed:** original Prepared restore
      constructor and source barrier cover independently selected retained destination
      versions without altering the checkpoint SourceSet or coverage. None permits
      required/ephemeral sources; empty Auto only ephemeral; captured sources cannot
      be replaced. Receipt owns exact baseline selection; startup compatibility,
      recovery, encrypted initial task, fenced publication and abandonment reuse
      existing owners. Git host shares replay-first restore flow. Extended grouped
      regression covers real dirty Git baseline, forbidden substitution/Auto,
      failed commit, history retention, missing-driver restart, exact resume/replay,
      and task-free ephemeral Auto abandonment. Host **16/16**, checkpoint
      **4/4**, construction receipt **1/1**, strict affected Clippy, workspace
      all-target/all-feature check and scoped fmt/diff passed. No schema or second ledger. Selection/retention authority is independently supplied; new snapshot creation
      and unversioned baseline effects are still pending.
      **Fenced Git capture and retired retention host consumer landed:** registered
      driver, exact whole-set approved checkout identities, shared canonical
      checkout pin/descriptor checks, replay-first read-only planning, durable
      Prepared before capture, inspect-only resume and exact abandon. Real
      independent GitSourceFence held through capture publication and retired
      retention settlement; content OID alone is not authority. Retired refs
      are conditionally released, including proven absence on recovery; no
      backing GC or new ledger. Grouped Host lifecycle uses the actual host,
      covers incomplete approval, failed capture commit/restart, wrong location,
      exact recovery/replay, separate abandon and failed retirement commit with
      already-absent refs. Host **16/16**, Git **32/32**, strict affected Clippy,
      workspace all-target/all-feature check and scoped fmt/diff passed. No schema
      change, source admission remains held.
      **v59 Vault references + actual broker consumer landed:** final staged typed
      VaultProvider leaf, named Agent grants, exact provider projection/entry/scope,
      actor stable identity/access generation and original Store scope. Manage plus
      independent live provider attach authority and Agent CAS; one rev advance,
      idempotent no-op and null cleanup even closed. Bounded redacted metadata,
      no raw secret/target/key locator, no ACL/history/Sandbox inheritance. Concrete
      registered SigningVaultProvider uses existing encrypted SecretStore under its
      provider-owned root. Real live provider guard and Store writer transaction
      fence bounded domain-separated HMAC; no secret-read/model/Process route.
      Rotation/revoke/restart cannot silently reactivate old grant or fanout Agent
      writes. Grouped real migration/broker regression covers independent attach,
      actor/generation/rotation/restart, non-inheritance and held/closed boundaries.
      Final focused Vault **1/1**, checkpoint **4/4**, construction **1/1**,
      Sandbox **4/4**, Host **16/16**, strict affected all-target/all-feature
      Clippy, workspace check and scoped fmt/diff passed. Broad Store lib **235/244** before
      four obsolete v58 assertion corrections (all four passed on rerun); five
      unchanged legacy Event/ACL/remote fixtures still fail, not claimed green.
      See CURRENT_HANDOFF for exact failures and limits; no unrelated repair.
      **Fenced Process Vault proof projection landed:** opt-in Agent-neutral one-shot
      SpawnControl fence at the original OS spawn/attachment, final cancellation and
      deadline check, exact factual no-spawn/uncertainty and driver pins retained.
      Original source claim/ToolStarted, complete lease pin, Sandbox/tool/writer
      authority and current VaultGrant revalidated under actual provider and Store
      fences. Concrete SigningVaultProvider Env projection uses fixed
      AW_VAULT_SIGNING_PROOF, domain-separated exact-effect proof only; no key,
      arbitrary targets, reserved-name overwrites, alternate Process or ledger.
      Real grouped source Process regression covers stale rotation, collision,
      successful proof-bearing child and original settlement. Shared Process
      regression covers cancellation inside fence. Process **7/7**, adapter
      Process **27/27**, Vault **1/1**, Sandbox **4/4**, Host **16/16**, strict
      affected Clippy, workspace all-target/all-feature check and scoped fmt/diff
      passed. No schema or held-admission change; relying-service freshness,
      Windows runtime and production containment remain separate.
      **Interrupted source custody recovery landed:** one original release owner
      accepts exact settled Interrupted Turn when ToolFinished is missing; numeric
      ToolFinished release encoding remains compatible. No synthesized terminal,
      inferred physical result or Run-hazard acknowledgement. Independent positive
      quiescence/no-start proof still required; Unknown retains claims. Exclusive
      Store reopen permits only held unresolved Runs with original source custody
      for their exact Agent/Sandbox/scope; direct/staged paths remain quiescent-only.
      Grouped real Process regression covers missing terminal, original owner reopen,
      missing driver, Unknown, release and degraded-reopen replay with unchanged
      journal/history/hazard. Sandbox **4/4**, checkpoint **4/4**, source barrier
      **1/1**, Host **16/16**, strict affected Clippy, workspace all-target/all-feature
      check and scoped fmt/diff passed. No schema or held-admission change.
      **Worker-owned native quiescence landed:** existing non-cloneable source
      barrier owns one cheap Store reference, lease uses the same owner, and
      Prepared admission can transfer the actual capability/authority/lease to
      its worker. Native blocking executor commits quiescence or Unknown before
      releasing live authority/returning, even if async awaiter disappeared.
      No new ledger, lease owner or schema. Existing grouped native regression
      pauses real staged write, drops awaiter, checks active exclusion/withdrawal,
      completes worker, reopens original interrupted Run and reuses recorded proof
      without inspection. Normal missed-hook recovery and Git exclusions preserved.
      Source barrier **1/1**, construction **1/1**, checkpoint **4/4**, Sandbox
      **4/4**, Host **16/16**, strict affected Clippy, workspace check and scoped
      fmt/diff passed. Actual process loss/failed proof commit remain unresolved; no arbitrary
      Process containment, artifact GC or whole-Run hazard acknowledgement.
      **Worker-owned finite Process quiescence landed:** ordinary source launch
      shares the Vault final exact-intent/grant/lease/tool authority fence, so
      revoke/regrant after admission cannot follow the queued launch. Original
      launch boundary + lease transfer by value to one finite PipeProcess owner;
      dropped result awaiter cancels only that execution, not the parent Turn.
      Wait/stop/reap and driver-proven physical settlement continue independently.
      Factual receipt and deferred proof result remain distinct; refusal/panic
      preserves Unknown, no synthesized ToolFinished. Existing grouped regression
      covers post-claim revocation, definite no-start, factual terminal with proof
      refusal, dropped live Process awaiter and original interrupted Turn recovery
      from worker-recorded proof. Sandbox **4/4**, Vault **1/1**, Host **16/16**,
      strict affected Clippy, workspace check and scoped fmt/diff passed.
      No new persistence, Process owner or admission.
      Production containment and interactive/Vault automatic settlement remain open.
      **Marked Git private-index scratch reconciliation landed:** original Prepared
      operation, shared capture/restore private-index primitive, synced bounded
      sibling ownership marker before index writes; exact protected parent/directory
      identity. Successful execution requires explicit known cleanup. Failed subprocess
      preserves scratch for original abandonment; no recursive Drop deletion. Read-only
      inspection/revalidation require absence. Authorized recovery removes only known
      regular index/lock files and exact directory before marker retirement. Same-mode
      replacement, unexpected entries, unmarked/malformed evidence or surviving writers
      remain unresolved. Grouped real Git tests inject failure after index write and
      checkout, preserve original checkout/index and settle exact scratch. No schema,
      descriptor or dependency change. Final Git repository **32/32**, Git library/doc
      tests, Host **16/16**, strict affected Clippy, workspace check and scoped fmt/diff
      passed. Independent physical namespace authority required; legacy/pre-marker
      gaps and partial checkout cleanup still open at that checkpoint.
      **Exact-subset unpublished restore abandonment landed:** shared bounded physical
      tree verification, exact base index/registration, absent unresolved index lock;
      expected directories/leaves only, empty unchanged gitlinks, no followed links or
      ignored/empty foreign content. Missing leaves allowed only for original unpublished
      private candidate cleanup; raw partial blob equality, complete Git normalization
      preserved. Published retirement/publication still require full tree. Original
      scratch and worktree release owners reused. Real grouped regression exercises
      partial Git checkout, changed/foreign refusal, exact abandonment/replay, and
      published missing-leaf refusal. Final Git repository **32/32**, Git library/doc
      tests, Host **16/16**, strict affected Clippy, workspace check and scoped fmt/diff
      passed. No new descriptor, schema or dependency. Host namespace protection
      remains required; ambiguous/truncated partial content and uncertain registration
      remain Unknown.
      **Verified metadata-only final detach landed:** existing source mutation owner
      accepts final-null only with exact sole binding, unused isolated Sandbox, CAS,
      Project writer, complete old source exclusion and legacy quiescence. Live verifier
      guard spans original transaction; no extra receipt. Concrete registered Git host
      verifies independently approved canonical private empty unchanged cwd/root,
      disjoint from the exact old checkout and administration/backing by path/identity.
      No physical deletion, layout change, Agent rebind or admission. Normal unverified
      final-null still refuses; current-rev empty no-op skips verification, reattachment
      gets fresh binding revision. Grouped real Store/Host tests cover existing Agent,
      shared layout, active claim, stale rev, wrong pin, nonempty root and exact detach/
      reattach preserving original Git state. Sandbox **4/4**, Git repository **32/32**,
      Git library/doc tests, Host **16/16**, strict affected Clippy, workspace check
      and scoped fmt/diff passed. Physical alias/writer/mount exclusion is
      independently host-owned; effectful final removal still pending. Frozen lower
      mount convergence remains untouched.
      **Named multi-source native realization landed:** one live approval/guard and
      canonical bounded root set with per-root artifact owner, original whole-root
      single-source encoding retained. Disjoint direct named children, no parent
      capability; exact physical pins and current source grants checked by original
      driver/route. Git resolver verifies each checkout and excludes administration
      across the selected roots. Reads narrow to RO; mutation requires writable
      source and preserves RO ceilings. Original journal effect claims every exposed
      root. Grouped native regression covers mixed RO/RW, unapproved sibling/parent,
      Git exclusions, per-root barriers, worker ownership and interrupted recovery;
      non-Git native variant exercises same primitives. Unreleased host API unified
      around the set. Final non-Git native **1/1**, Host **16/16**, strict Host
      Clippy for both feature variants, workspace check and scoped fmt/diff passed.
      No schema/new executor/admission or lower mount changes. General Process
      containment and independent host provisioning still pending.
      **Durable native artifact custody/recovery landed:** same adapter FileStore and
      cache, persistent source-owner discriminator prevents ambient cleanup of that
      namespace on reopen. Explicit durable provisioning/recovery-only open, no
      source-file cleanup during opening or silent volatile fallback. Native worker
      keeps unresolved writable artifact custody Unknown even with factual successful
      ToolOutput. Existing Git native resolver composes original journal recovery,
      exact registered driver/intent/set, independent permanent quiescence and original
      durable namespace authority, shared physical pin/admin exclusions and existing
      exact artifact reconciler. No new operation ledger, Run acknowledgement or
      execution admission. Missing namespace/RO records/protected administration
      refuse; writable records must retire before release. Adapter real durable
      reopen plus grouped native fixture simulate private Stage loss under original
      claim, preserve successful terminal/unowned files, recover/replay and retain
      detached-worker tests using actual native mutation lock. Host must independently
      establish exact original state and physical exclusion; not automatic recovery
      or authority from empty registry. Unsupported private-file platforms deny.
      Artifact owner **8/8**, non-Git native **1/1**, Host **16/16**, strict affected
      Clippy (including Host source-effects-only), workspace and scoped fmt/diff
      passed. Adapter broad **254/255**; unchanged `skipped_node_read_does_not_wait_for_sibling`
      acknowledgement assertion fails isolated rerun too; parent not rerun, no
      full adapter green or proven pre-existing failure claim at that checkpoint.
      **Exact Store-backed native approval provisioning landed:** trusted read of
      exact named bindings from original owner, Project writer + actor Submit + open
      canonical Agent/Sandbox + rev + registered schemas; independent host must
      authorize metadata and exact physical location/revision/modes. Git resolver
      checks whole selection before durable artifact provisioning, pins through live
      approval creation, verifies actual roots and exact current Store observation.
      Existing grant publication CAS remains separate and revalidates the actual
      source/actor pins. No raw public metadata, implicit grant, new persistence or
      admission. Original generic approval spec reused; no separate policy registry.
      Existing Store/Host regressions cover wrong manager, stale/duplicate selection,
      wrong physical pin, no state on refusal, actual two-source grant and recovery.
      Previous Job ACK failure was an obsolete suffix assertion; corrected to verify
      ACK and canonical provenance together, with no runtime change. Final adapter
      **255/255**, Sandbox **4/4**, non-Git native **1/1**, Host **16/16**, strict
      affected Clippy (both Host feature variants), workspace check and scoped
      fmt/diff passed. Earlier Store-wide legacy failures not rerun.
      **Shared finite Vault Process settlement landed:** ordinary/Vault interactive
      start and finite run share one statically selected private spawn projection
      and the existing finite worker. `run_source_process_with_vault` retains
      original lease through wait/stop/reap and physical proof even on awaiter loss;
      factual result remains separate from deferred quiescence. Grouped real
      Process regression exercises stale Vault/collision no-spawn, interactive
      proof success, actual finite proof-bearing child, provider withdrawal,
      dropped awaiter/local cancellation and original interrupted recovery.
      No extra Process/Job/receipt owner, dependency or schema. Independent
      quiescence/containment required, no retroactive proof erasure. Interactive
      settlement remains explicit. Standalone effectful Sandbox construction
      needs Project-owned receipt scope, not synthetic Agent audit origin.
      Final Host **16/16**, strict Host Clippy (both feature variants), workspace
      check and scoped fmt/diff passed. Unchanged Store/adapter suites not repeated.
      **Standalone Project-scoped versioned Sandbox construction landed:** v60 widens
      the existing immutable audit/receipt origin (Session XOR Project) with legacy
      seq/data and indexes preserved. No synthetic Agent, Session or new ledger.
      Exact Project writer/creator/request/fingerprint/candidate/layout and independently
      retained baseline versions plus original Restore descriptors are held by the
      common Prepared/source barrier. Existing inspection validation, fenced publication,
      replay, Unknown recovery and Rejected cleanup reused. Final transaction publishes
      only fresh Sandbox + bindings + Applied receipt; same Git driver materialization
      and physical fences as checkpoint restore. Original standalone Applied provenance
      supports closed retirement. Real grouped Host lifecycle tests failed publication,
      missing driver, restart/resume, exact replay, retirement, no fabricated Agent/
      Session and no-start abandonment. Existing receipt regression preserves legacy facts.
      Broad Store **239/244**, exact same five previously recorded legacy failures.
      Final Host **16/16**, strict Store/Host all-target/all-feature Clippy,
      workspace check and scoped fmt/diff passed. Fresh retained versions only; mutable parent derive, unversioned baseline effects,
      effectful structural mutation and public RPC remained open at that checkpoint.
      **Same-Project required-source Sandbox derivation landed:** same Project receipt,
      barrier, inspection/publication and exact cleanup; original versioned required
      parent bindings and rev, current writer/quiescence, fresh isolated destination,
      exact parent fingerprint and lineage. One `Derive` descriptor, Git recovery
      schema 4 (schemas 1–3 unchanged): original capture + destination fixed before
      effects, reuse deterministic retained snapshot and original RestorePlan/outer
      lock identity, no new planner on inspection. No Agent/Session/public checkpoint
      or extra ledger. Release child before private retention; missing retention
      requires original target and marked registration absent. Parent physical
      retirement and binding mutations blocked by retained children even after
      logical close; derived binding edits held pending effectful structural custody.
      Real grouped Git capture-only failure and Host dirty linked-parent derive,
      partial-set abandonment, failed publication/reopen/resume/replay, exact child
      and parent retirement; no parent HEAD/index rewrite. Baseline encoding retained.
      First realization nonempty all-required same-SourceRef Git; ephemeral/source-free
      parents/new backing identities need explicit policy. Independent physical authority
      remains mandatory. Schema v60 unchanged. Validation: core 1/1, Git 33/33,
      construction 1/1, Sandbox 4/4, Checkpoint 4/4, Host 16/16; strict Clippy,
      workspace all-target/all-feature check, scoped fmt/diff passed. Earlier
      broad Store 239/244 was not rerun.
      **Exact managed-source structural removal landed:** one original Applied
      restore/derive descriptor + reserved exact Sandbox CAS release claim in the
      existing complete-set source coordinator. No fresh materialization plan,
      parallel receipt ledger or schema. Independent read-only preflight, same
      writer/revision/layout/binding/dependency checks at claim/commit; old rows
      and hazard retained on uncertainty. Shared sources.changed mutation CAS
      publishes the factual release; stable original-descriptor event ID proves
      which original descriptors may be omitted during later closed retirement.
      Managed metadata edits cannot discard original custody. Concrete Git
      removal limited to unused private named-child layouts, cwd=root, independent
      complete physical fence. Only selected original worktree/derived retention
      released; sibling identity retained without requiring unchanged content.
      Final removal keeps original empty directory. Grouped Host lifecycle covers
      failed publication/restart, wrong claim refusal, independently edited sibling,
      exact subset retirement and final-source removal. Attachment/replacement and
      other removal cases still open. Final construction 1/1, Sandbox 4/4,
      Checkpoint 4/4, source barrier 1/1, Host 16/16; strict affected Clippy,
      workspace all-target/all-feature check and scoped fmt/diff passed.
      Earlier broad Store 239/244 not rerun. Schema v60 unchanged.
      **Versioned managed-source attachment landed:** original Project-scoped
      Prepared receipt with private attachment candidate, exact old Sandbox
      CAS/writer/layout/bindings and incoming retained anchor; one Restore
      descriptor + complete old/incoming mutation claim before external effect.
      Resume inspects and abandon reconciles the exact original candidate.
      Atomic source.changed publication + Applied receipt + claim settlement;
      new binding rev equals next Sandbox rev. Exact per-binding materialization
      provenance via original construction or attachment changed fact + existing
      Prepared index. Reattach requires proven prior release; heterogeneous
      release/retirement uses actual original descriptor per binding. Existing
      homogeneous view remains compatible. Shared managed observation and
      mutation CAS; no schema, driver operation or parallel ledger. Concrete
      Git path: nonempty already-managed unused named-child layout, cwd=root,
      fresh independently authorized incoming retained version and full-set
      physical fence. Grouped Host lifecycle covers abandoned candidate,
      failed publication/restart, missing driver, exact replay, dirty sibling,
      reattached name/new rev and mixed-origin retirement. Replacement,
      empty-layout attachment and bound-Agent mutation remain held. Final
      construction 1/1, Sandbox 4/4, Checkpoint 4/4, source barrier 1/1,
      Host 16/16; strict affected Clippy, workspace all-target/all-feature
      check and scoped fmt/diff passed. Earlier broad Store 239/244 not rerun.
      Schema v60 unchanged.
      **Atomic versioned managed-source replacement landed:** same attachment
      Prepared owner (explicit replace selection, old attach encoding preserved),
      original old bindings + exact incoming Restore descriptor, complete old/
      incoming backing identity claim at resulting named-slot limit. Reuse
      original release and materialization; single binding CAS, old-release
      identity + incoming provenance in same changed fact, Applied and barrier
      settlement. Git permits occupied destination only when exact old checkout
      releasable; independently authorized complete physical fence covers both
      origins. Reject old edits and incoming dependence on released derive
      retention. Publication proves exact new checkout and old registration/
      retention gone. Inspect-only resume; untouched abandon only; explicit
      post-release gap continuation requires positive absence of surviving new
      effects and reconciles original release before original restore. Unknown
      partial incoming remains held. Real grouped Host lifecycle covers old
      dirty refusal, untouched abandon, third backing at two-name limit,
      gap/unsafe-abandon refusal, continuation, failed publication/restart,
      exact replay, preserved sibling, derive retention release, mixed-origin
      retirement and same-common replacement/final removal. Schema unchanged;
      first realization unused private named layout, cwd=root. Empty-layout
      attachment, partial replacement recovery and bound-Agent cases remain.
      Final Git 33/33, construction 1/1, Sandbox 4/4, Checkpoint 4/4, source
      barrier 1/1, Host 16/16; strict affected Clippy, workspace all-target/
      all-feature check and scoped fmt/diff passed. Earlier broad Store
      239/244 not rerun.
      **First-source attachment landed:** same Project Prepared attachment and
      original Restore descriptor, with exact root/cwd layout scopes in the
      existing source claim when the old set is empty. No new schema/ledger;
      scopes do not consume named-source slots. Reserve before effects, block
      competing backing selection, Sandbox create/close, metadata changes and
      ambient Agent binding/execution. Empty preflight refuses retained Agent
      or Session aliases at either exact path; after publication legacy
      ambient alias admission remains denied. Independent physical fence
      still required for alias/ancestry/mount/cross-Store exclusion. Recover
      original layout/source hazard together; settle on Applied or proven
      abandonment. Per-binding provenance permits no initial construction
      receipt, while legacy rev-1 without original descriptor remains held.
      Managed attachment facts prevent metadata-only custody loss after
      final release. Grouped Host lifecycle covers empty/private layout,
      competing first attachment, abandonment, failed publication/restart/
      missing driver, exact replay, replacement/removal, reattachment and
      closed retirement. Bound-Agent and partial replacement cases remain.
      Stronger ambient alias gate updated the existing checkpoint regression
      and two Host source-effect fixture publication orders, not runtime bypasses.
      Broad Store 231/244 initial; corrected alias 1/1 and stable-executable
      coordination 24/24 resolved one old expectation and seven spawn ENOENTs
      from overlapping rebuild; five known legacy failures remain (239/244
      combined). Host 14/16 initial + corrected fixture retest 2/2. Strict
      affected Clippy, workspace all-target/all-feature and scoped fmt/diff passed.
      **Exact partial replacement recovery landed:** explicit continuation uses
      original Prepared + live complete-set fence, verifies incoming registration
      and old release (shared publication proof), retains a completed candidate
      or reconciles exact unpublished partial effects with existing restore
      cleanup. Re-execute original descriptor only after positive target and
      registration absence. Foreign/ambiguous/modified effects stay held;
      normal resume remains inspect-only. No new schema, operation or state
      machine. Existing grouped real Host lifecycle covers partial checkout,
      foreign entry refusal/preservation, recovery, failed publication,
      completed candidate retention and restart inspection. Host 16/16,
      Git 33/33, strict affected Clippy, workspace all-target/all-feature
      check and scoped fmt/diff passed. Broad Store not rerun.
      **Enforced minimum Process containment landed:** one reusable
      aw-process ContainmentRequirement and spawn_with_containment. Strict
      closed descendants refuses Direct Unix before fence/lane/OS spawn;
      existing Windows suspended Job is the current closed backend, runtime
      qualification still deferred. No weaker fallback or terminal evidence
      changes. PreparedPipeLaunch preserves requirement through custody
      and both adapter launch routes. Independent SourceToolAuthority minimum
      combines monotonically at shared finite/interactive/Vault handoff;
      unsupported becomes original factual NotStarted and source settlement.
      Existing weak path still needs independent quiescence. Runtime/adapter
      regression and grouped Host journaled refusal; no lower mount changes,
      new backend, schema, dependency or Process owner. Production physical
      authority/provider configuration and source-aware dispatch remain held.
      Process 8/8, adapter 258/258, Host 16/16, source-effects-only Host 2/2;
      strict affected Clippy, workspace all-target/all-feature and scoped
      fmt/diff passed. Broad Store not rerun.
      **Trusted local shell containment landed:** configured host-native shell
      minimum (default BestAvailable) feeds the same PreparedPipeLaunch for
      direct/finite/Graph dispatch, with typed Job launch failure and factual
      retained diagnostics. Strict minimum enters sealed runtime identity;
      legacy identities stay exact. No Unix strict fallback, Process terminal
      invention, model selector, new execution owner or source-admission
      promotion. Grouped real no-spawn regression 1/1, adapter 259/259,
      original source Process host 2/2, strict adapter Clippy, full/minimal
      CLI all-target checks and scoped fmt/diff passed on L5's installed
      Rust 1.95.0 (pinned Nix shell unavailable offline; minimal two existing
      warnings). Shared checkout includes concurrent dependency work; exact
      owned tree only for signing. Windows/Bubblewrap and broad Store/workspace
      runtime not requalified. Original journal owners still occupied.
      **Production Git SourceDriver owner-open registration landed:** existing
      typed leaf installed before startup compatibility on eager/lazy main,
      OAuth, scheduled and realm-checked team owners. Trusted executable
      selection only; missing driver degrades, invalid explicit executable
      refuses before Store open. Typed installed handle resolution reuses the
      original leaf. No source probe/grant/activation, new registry or schema.
      Real Git/SQLite restart, registry and Store migration regressions **1/1**
      each; strict AgentCore/Store Clippy, full/durable-only/minimal product
      checks and scoped fmt/diff passed (L5 Rust 1.95.0; pinned Nix shell not
      qualified, existing durable/minimal dead-code warnings). Resource CLI
      owner remains occupied, so its separate open paths still need composition.
      Original source-bearing admission stays held; no automatic recovery or
      physical authority from the installed definition.
      **Persistent signing Vault provisioning landed:** explicit trusted
      existing private config/key backend activation, no initialization or
      secret copies. Reuses exact provider policy/registry/grants, broker and
      Process projection. Configured keys remain with configuration owner;
      live revoke does not delete them. Host must withdraw live authority
      before editing persistent config. Fresh authority on explicit restart,
      old grants stale, key-ref substitution denied at attach/use. No implicit
      product startup/CLI grant, physical isolation or new schema. Signing
      2/2, SecretStore 6/6, original source Process host 2/2, strict Store/Host
      Clippy, full product check and scoped fmt/diff passed (L5 Rust 1.95.0;
      pinned Nix and platform-native qualification not rerun). Journal and
      resource-CLI owners remain occupied. Source-bearing admission held.
      **Exact SourceDriver ownership and artifact-state preflight landed:**
      runtime handles pin their live registry owner without changing durable
      schema compatibility. Complete verified Git source/admin set checked
      before provision/reopen of artifact namespaces; duplicate physical
      checkout identities denied, existing recovery custody and verified repo
      pins reused. Actual mount/ancestor-alias and external-writer exclusion
      remains the independent host's responsibility; no source admission
      promotion. Registry 1/1, private artifacts 8/8, source Process/native
      route 2/2, grouped Git checkpoint/restore/construction 1/1, product
      restart 1/1, strict affected core/adapter/Host Clippy, full product
      check, scoped fmt/diff passed (L5 Rust 1.95.0; pinned Nix and native
      platform qualification not rerun). Journal and resource CLI occupied.
      **Trusted native-shell startup minimum landed:** explicit host-only
      `AW_LOCAL_SHELL_CONTAINMENT` selects the existing typed minimum before
      Agent setup or scheduled claims; full/minimal/stateless and web-owner
      composition preserve it through existing tool options. Default unchanged,
      invalid fails startup, strict unsupported Unix launches refused by the
      original shared Process owner. No global containment, model selector,
      grant, external route or schema change. Full/stateless config 1/1 each,
      original direct/finite/Graph regression 1/1, full/minimal all-target
      checks, strict full-product Clippy, full binary build + metadata-only
      startup smoke, scoped fmt/diff passed (L5 Rust 1.95.0; pinned Nix and
      platform-native qualification not rerun). Source admission still held.
      **Source-aware single-link native capability landed:** opt-in Unix
      link-count policy in original descriptor-rooted content, metadata,
      staged publication and bounded lifecycle engine; ordinary capability
      behavior unchanged. Actual source approval/layout and recovery consume
      it; aliases to unapproved/protected Git files denied. Independent host
      exclusion of concurrent external writers/mount aliases remains required;
      Windows strict constructor refuses unsupported mode. Unix filesystem
      42/42, Git source Process/native 2/2, non-Git native 1/1, strict affected
      Clippy, full product check and scoped fmt/diff passed (L5 Rust 1.95.0;
      pinned Nix and native cross-platform qualification not rerun). No schema,
      admission or frozen physical Sandbox change.
      **Configured Git native source authority landed:** explicit owner-private
      `git-native.json` host selection, exact Store generation/manager/Agent/
      Sandbox and named physical roots, original typed Git driver and approval
      owner, complete layout/administration/host state checks. Fresh live
      authority per activation; separate grant CAS; no implicit discovery,
      grant, automatic reload or Agent admission. The independent host must
      still protect physical roots/state and revoke before config changes.
      Configured real native regression 1/1, Git Process/native 2/2, non-Git
      native 1/1, strict Host Clippy, full and durable-only checks and scoped
      fmt/diff passed (L5 Rust 1.95.0; pinned Nix/native cross-platform gates
      not rerun; existing durable dead-code warning). No schema change.
      **Protected source metadata and checkpoint reads landed:** existing
      Sandbox read port projects actual redacted bindings and supports
      `sandbox_sources_list`; optional checkpoint read port uses the same
      authenticated current-Project state owner and original Store view,
      immutable history, source summaries and cursor. Raw paths, source/
      anchor/provider identities withheld; no driver effects or mutations,
      independent protected disclosure remains held. Bounded response and
      capability negotiation. Real Store-backed port 1/1, headless protocol
      8/8, full/minimal checks, strict full-product Clippy, scoped fmt/diff
      passed (L5 Rust 1.95.0; pinned Nix/platform gates not rerun). No schema
      or source-bearing admission change.
      **Replay-safe history checkpoint construction landed:** explicit local
      Actor-owner stdio `--agent-checkpoint-create` capability, original Store
      history/empty-Auto publication and caller-scoped receipt. Literal ref
      fingerprint/replay precedes current Agent resolution, current view and
      Project checks, no retarget after rebind or retirement. Ordinary RPC
      request-ID gate before constructor; default off; required/selected source
      capture and restore/delete RPC remain held. No new schema or authority.
      Original checkpoint owner 1/1, grouped Store-backed port 1/1, headless
      8/8, full/minimal checks, strict Store/full-product Clippy, binary build
      and opt-in smoke, scoped fmt/diff passed (L5 Rust 1.95.0; pinned Nix and
      native platform gates not rerun).
      **Replay-safe empty-baseline checkpoint restore landed:** explicit
      local Actor-owner `--agent-checkpoint-restore-root` host-selected
      existing canonical private empty layout, task-less history-only/empty
      Auto restore through original constructor and receipt, same Project and
      current disclosure. Replay before physical layout probe; ordinary RPC
      ID gate, no caller path or implicit materialization/activation. Host
      protection of all other physical layouts and external writers remains
      required; known exact occupied roots denied by original Store. Captured
      source/task-bearing restore and retirement RPC remain held. Headless
      8/8, Store-backed port 1/1, original restore owner 1/1, strict Host/full
      Clippy, durable/minimal checks, full binary build + opt-in smoke and
      scoped fmt/diff passed (L5 Rust 1.95.0; pinned Nix/platform qualification
      not rerun). No schema/admission/frozen Sandbox change.
      **Replay-safe logical checkpoint retirement landed:** explicit local
      Actor-owner stdio `--agent-checkpoint-delete`, default off, `id` only,
      ordinary RPC gate; current Project/Manage in the original retirement
      transaction, unresolved restore refusal and unchanged history/Agent
      rev. Existing indexed immutable commit-log identity retains caller +
      request semantics, including new idempotent acknowledgements on
      retired rows. Exact replay appends nothing; mismatch denied, current
      disclosure loss returns opaque `prior_invocation`. No new ledger,
      schema, physical effect or release authority. Original retained anchors
      and independent cleanup owner unchanged. Checkpoint owner 4/4, headless
      8/8, Store-backed port 1/1, strict Store/CLI all-target/all-feature Clippy,
      durable/minimal checks, full binary build/opt-in smoke and scoped fmt/diff
      passed (L5 Rust 1.95.0; pinned Nix/platform qualification not rerun).
      **Configured signing Vault activation + binding RPCs landed:** explicit
      local Actor-owner `--agent-signing-vault-root`, original tool-free
      durable binding and typed provider registry/handle. No implicit grant;
      host protects the provider root independently. Redacted current-Agent
      list and exact typed set/null removal require current Manage; fresh
      attach retains live configured provider authority through the original
      Store grant commit. Indexed immutable replay fact covers literal
      selector, full request semantics and original resulting revision;
      mismatch denied, lost disclosure opaque, replay never regrants. No
      new schema/ledger or broker/Process admission. Corrected tools-off
      local startup's unintended model Event registration to permit the
      existing Actor-owner guard. Signing Vault owner 2/2,
      headless 8/8, source-Process 2/2, actual configured Host/Actor/stdio
      1/1; strict Store/Host/CLI Clippy, durable/minimal checks, binary build,
      actual flag/Actor-owner smoke and scoped fmt/diff passed (L5 Rust
      1.95.0; pinned Nix/platform qualification not rerun).
      **Offline Git checkpoint retention reconciliation landed:** the original
      retired-source owner authorizes read-only outside SQLite before the
      cleanup claim, then atomically rechecks exact anchors/current authority.
      Denied preflight creates no new Unknown custody; actual failure retains
      original set and hazard. Original Git host uses pinned checked Repository
      objects and exact conditional retention release; lazy registered-driver
      lookup skips settled replay. Explicit workspace CLI `checkpoint-retention`
      accepts only a private, exact Store/manager/checkpoint approval file;
      live advisory approval lease, ordinary private checkout roots and
      independent host exclusion required. No Agent activation, deletion of
      live checkpoints/worktrees, GC, general containment or new schema/ledger.
      Store checkpoint 4/4, real Git 33/33, Host lifecycle 1/1, offline
      reconciliation 1/1; strict Store/Git/Host/CLI Clippy, durable/minimal
      checks, full binary build and required-approval CLI smoke passed.
      See CURRENT_HANDOFF for validation corrections and remaining
      platform/inventory qualification.
      **Configured offline Git checkpoint capture/recovery landed:** the existing
      configured native approval acquires a separate exact host-operation
      fence after Store source selection, before physical probes. Explicit
      offline `checkpoint-capture` uses original capture Prepared/inspect/
      publication owners with the live approval guard, advisory config lease
      and private ordinary checkout pins. Manager/actor/current OS principal
      and complete named selection must match; no SandboxGrant or activation.
      Exact Applied replay before config/driver lookup; explicit recover
      matches original semantic fingerprint before receipt/barrier reclamation,
      never recaptures an unresolved invocation. Shared offline Git maintenance
      helpers, no new schema/ledger/config format. Independent host physical
      exclusion remains mandatory. Store checkpoint 4/4, Host lifecycle 1/1,
      native source Process 2/2, grouped offline capture/recovery/retention
      1/1; strict Store/Host/CLI Clippy, durable/minimal checks, full binary,
      actual command help/required-argument smoke and scoped fmt/diff passed.
      See CURRENT_HANDOFF for remaining platform qualification.
      **Offline captured Git restore/recovery landed:** exact independent
      private Store/manager/checkpoint/layout/backing approval and held file
      lease, private named destination, taskless same-Project/private history,
      original Prepared/inspect/publication owners; restored Agent remains held,
      no inherited grants or credentials. Replay before config/Git probes;
      exact recovery checks original request fingerprint and layout digest,
      excludes baseline substitution before original receipt reclamation;
      hold fence through failed inspection settlement too. Shared offline Git
      approval loader/Store scope and checkout pins. No new durable schema,
      recovery descriptor or ledger. External exclusion remains host-owned.
      Grouped offline lifecycle 1/1, Store checkpoint 4/4, Host
      checkpoint lifecycle 1/1; strict Store/Host/CLI Clippy, durable/minimal
      checks, full CLI build, actual command/required-argument smoke and
      scoped fmt/diff passed. See CURRENT_HANDOFF for limitations.
      **Offline original capture/restore abandonment landed:** explicit mutually
      exclusive recover/abandon modes on the original commands, same private
      approvals and exact request namespace. Original semantic fingerprint,
      plus captured restore layout digest, checked before original custody
      reclamation; published candidates refused and Rejected replay skips
      config/driver/physical selection. Existing exact Git release and
      all-source barrier; guards survive Unknown settlement after cleanup or
      final settlement failure. Modified/ambiguous effects remain held. No new
      schema/ledger/config format. Continuous host exclusion mandatory.
      Grouped offline lifecycle 1/1, Store checkpoint 4/4, Host lifecycle 1/1;
      strict Store/Host/CLI Clippy, durable/minimal checks, full CLI build,
      actual mode-conflict smoke and scoped fmt/diff passed.
      **Exact closed Git Sandbox retention reconciliation landed:** explicit
      offline private named-child layout approval pins closed revision and
      complete retained original Applied descriptors. Fenced authorization
      before the original cleanup claim, exact whole-set/current authority
      recheck before effect, same Git release and original custody settlement.
      Refused preflight creates no new Unknown hazard; partial or uncertain
      release retains the closed reservation. Exact terminal replay verifies
      existing release fact/revision, skips driver/physical probes. Shared
      named-layout validation with restore; no new durable owner or schema.
      No automatic closure, shared object pruning, checkpoint release or
      execution authority. Independent host exclusion remains mandatory.
      Grouped offline lifecycle 1/1, Store checkpoint 4/4, Store Sandbox 4/4,
      Host lifecycle 1/1; strict Store/Host/CLI Clippy, durable/minimal
      checks, full CLI build, actual required-approval smoke and scoped
      fmt/diff passed. See CURRENT_HANDOFF for remaining limitations.
      **Production signing broker consumer landed:** separate explicit local
      stdio startup entry selection for one exact configured provider decision,
      current authenticated Actor/Agent and writer epoch. No implicit grant,
      generic Vault invoke or model-facing tool. The original live provider
      guard and immediate Store transaction revalidate current exact grant,
      admission, actor/access generation, Store scope and owning writer epoch
      at actual HMAC use. Shared SDK and host signing implementation; per-call
      reader on the original registry, no second owner. Strict bounded
      grant_rev/message RPC returns only derived tag, no client selectors or
      protected metadata. Revoke/rotation/restart require current authority;
      no durable replay ledger or Process admission change.
      Configured Host broker 1/1, Store Vault 2/2, headless 8/8, source Process
      2/2; strict affected all-feature Clippy, durable library/minimal checks,
      full CLI and startup dependency smoke passed. Expanded durable-only
      binary check blocked by existing non-workspace subagent field error;
      see CURRENT_HANDOFF for qualification and corrections.
      **Provider-authorized Vault entry metadata landed:** existing configured
      signing provider per-entry disclose_entry_ref defaults false; distinct
      from grant attach/use and binding metadata authority. Existing stdio
      list uses original Store Manage/bounded snapshot and actual provider
      read guard, current exact policy, stable manager and grant generation.
      Optional typed entry ref only when approved, default/stale/revoked/
      wrong manager stays redacted; no secret/backend/scope disclosure,
      new RPC/registry/schema or execution admission. Real L5 cgroup v2
      read-only/no effective capabilities; strict closed-descendant Process
      remains unsupported, original journal paths occupied.
      Store Vault 2/2, real Host/stdio lifecycle 1/1, strict Store/CLI Clippy,
      durable library/minimal checks, full CLI/help and scoped fmt/diff
      passed; see CURRENT_HANDOFF for held boundaries.
      **Protected source identity metadata landed:** explicit local stdio
      private approval file selects exact Store scope/actor/stable user/Project
      and named Sandbox binding revisions or immutable checkpoint anchor
      versions. Reread actual policy under held private lease per read;
      original current Project/Agent View + same snapshot/owner, exact
      typed identity only, default unselected/stale redacted. No retention,
      materialization, private descriptor or backing lookup; no new RPC,
      schema or execution grant. Shared bounded approval loader with
      previous Git maintenance. Unavailable driver/objects do not make
      authorized metadata into a materialization proof.
      Grouped offline lifecycle 1/1, actual stdio 2/2, Store checkpoint 4/4,
      Store Sandbox 4/4; strict affected Clippy, durable library/minimal
      checks, full CLI/startup dependency smoke and scoped fmt/diff passed.
      See CURRENT_HANDOFF for scope and remaining limitations.
      **Standalone Git Sandbox derive consumer landed:** explicit offline
      exact same-Project parent/request and independently approved current
      parent rev, complete required Git set, private named-child destination.
      Existing Prepared owner/claim/driver/held publication; exact original
      parent rev/layout checked before recovery reclaim. Recover inspects,
      abandon releases only original unpublished effects, terminal replay
      requires no driver/config; ambiguous/modified candidates stay held.
      No Agent, public checkpoint, grant, model tool, new schema or owner.
      Only ordinary private parent checkouts, independent host exclusion.
      Grouped offline lifecycle 1/1, Store Sandbox 4/4, Host lifecycle 1/1;
      strict affected Clippy, durable library/minimal checks, full CLI
      and required/mode-conflict smoke, scoped fmt/diff passed.
      **Standalone retained-version Git baseline consumer landed:** explicit
      offline exact Project/request and private complete selected required
      anchor set, independent original retention and materialization
      approval. Shared original Prepared constructor, exact baseline/
      parent and layout check before recovery claim, common Git inspector
      and held settlement; no blind retry, new retention/Agent/checkpoint/
      grant or schema. Shared lease-backed pinned named-layout guard for
      restore, derive, baseline and closed-Sandbox maintenance.
      Grouped offline lifecycle 1/1, Store Sandbox 4/4, Host lifecycle 1/1;
      strict affected Clippy, durable library/minimal checks, full CLI
      and required/mode-conflict smoke, scoped fmt/diff passed.
      **Unused-Sandbox Git attachment consumer landed:** exact private host
      approval for incoming retained anchor, original Sandbox CAS revision,
      whole named layout and original retained sibling provenance. Shared
      original Prepared owner and first-source layout claim, exact semantic
      fingerprint before inspection/abandonment, terminal replay under exact
      request without Git/physical layout. Actual approval held through
      failed inspection/cleanup Unknown as well as publication/Rejected.
      No replacement/removal, bound-Agent mutation, grant, schema or RPC.
      Grouped offline lifecycle 1/1, Store Sandbox 4/4, Host lifecycle 1/1;
      strict affected Clippy, durable library/minimal checks, full CLI
      and required/mode-conflict smoke, scoped fmt/diff passed.
      **Unused-Sandbox Git replacement consumer landed:** shared attachment
      approval/owner, explicit original old origin plus complete incoming/
      sibling layout and exact original anchor/CAS fingerprint. Separate
      continuation flag and default-deny host continuation decision;
      original exact request checked before recovery reclaim, inspect-only
      ordinary recovery, intact predecessor required for abandonment, actual
      driver-proven post-release continuation, no blind retry or new owner.
      Terminal replay under original semantic approval without Git/layout.
      Grouped offline lifecycle 1/1, Store Sandbox 4/4, Host lifecycle 1/1;
      strict affected Clippy, durable library/minimal checks, full CLI
      and required/mode-conflict smoke, scoped fmt/diff passed.
      **Unused-Sandbox exact Git source release consumer landed:** existing
      original Applied descriptor and full retained layout under separate
      named-removal approval, original CAS and release claim/reconciliation,
      final empty named parent preserved. Exact immutable release fact
      acknowledges original name/revision, not metadata deletion or
      replacement; replay needs no driver/layout, never touches later
      attachment. Original strict-CAS SDK retained, no new schema/ledger.
      Grouped offline lifecycle 1/1, Store Sandbox 4/4, Host lifecycle 1/1;
      strict affected Clippy, durable library/minimal checks, full CLI
      and required/unsupported-mode smoke, scoped fmt/diff passed.
      **Authenticated Git Sandbox source setter landed:** existing sealed
      stdio sandbox_sources_set, explicit exact local Actor-owner startup
      and private existing attachment/replacement or named-removal approval.
      Only ref/name/typed opaque SourceRef/policy/rev or explicit null;
      no path/version/mode/recovery selectors. Actual host verifies original
      owner, holds reread approval lease and independent layout exclusions,
      original Store/driver owners do all effects. Exact shared materialization
      RPC request namespace, explicit offline original-ID recovery only;
      release retains original CAS/factual replay. No new resource owner,
      schema, ledger or source-bearing execution.
      Grouped actual stdio 3/3, offline lifecycle 1/1, headless 8/8;
      strict CLI Clippy, durable library/minimal checks, full CLI and
      required/recovery-only flag smoke, scoped fmt/diff passed. See handoff
      for corrected fixture ownership and boxed approval variants.
      **Live native source Actor/Turn admission landed:** one owner-local
      exact source scope under actual independent live policy; explicit
      Manage/CAS open, original journal activation, original queue claim
      and Input prepare/accept/dispatch, exact original dispatch proof at
      Turn begin, current context/model/tool admission, original native
      effect guard and factual terminal settlement. Configured Git host
      builds original native Agent and scope; original HostAgentActivation
      runs it. Owner Input without participant evidence corroborated by
      immutable original dispatch/Run anchor and segment, no new ledger.
      Real Git-backed Actor test proves read/write, held/revoked pending
      queue, no ambient activation, fresh restart grant, original queued
      Turn settlement and close. Grouped Agent lifecycle 3/3, Sandbox
      4/4, Host native/Process 2/2; strict affected Clippy and durable/minimal
      all-target checks green. Broader Actor 44/46: legacy queue-revision
      and shared-session Agent permission failures at unchanged sites;
      parent not independently re-run, qualification unresolved (see handoff).
      **Actor qualification corrective checkpoint:** historical pre-native
      binary reproduces both legacy failures. Ordinary Pending queue
      publication advances exact Agent rev once, optional compatibility
      CAS in original transaction; no revision change on replay/refusal,
      no staged Send/construction double increment. Shared fixture
      grants explicit Agent ACL independently of Session participation.
      Own local disclosure seal no longer duplicates inherited history;
      use same actual inherited-base selector for projection/provenance/
      sealing, preserve original immutable grants and exact Turn lookup.
      Final Actor 46/46, History 4/4, Store queue 8/8, staged plan 1/1,
      checkpoint 4/4, Agent lifecycle 3/3, stable child 2/2; strict affected
      Clippy, durable/minimal targets, full all-feature executable and
      actual version smoke, scoped fmt/diff passed. Prior timeout resolved.
      Historical binary reproduction is not an exact parent rebuild.
      **Production native source-root stdio landed:** exact saved local
      Agent, existing private v1 Git approval directory, explicit grant
      CAS and separate held-open CAS, original current provider and
      native-only tool selection. Reuse the actual qualified source
      execution scope/SessionHandle/Actor owner and stdio transport;
      no ambient tool builder or duplicate Runtime, no additional
      resource mutations or automatic event/wake dispatch. Raw prompt
      capability and dispatch disabled at the original Actor transport
      gate. Real executable/private Git/SQLite/loopback provider test
      qualifies read/write and queued continuation across fresh
      approval/regrant at restart. Executable native root 1/1, headless 8/8, prior stdio 3/3,
      Agent lifecycle 3/3; strict CLI Clippy, durable/minimal all
      targets, full executable, actual version/help/required-flag
      smoke and scoped fmt/diff passed.
      **Live native source withdrawal landed:** permanent monotonic
      request closes new actual policy admission immediately, original
      read guards retained through factual effects; async revoke drains.
      Exact configured local stdio owner only, no selector, signals
      current cooperative cancellation via the original verified Actor
      Turn rather than dropping future. Live withdrawal, stop evidence,
      durable grant mutation and reconciliation remain distinct.
      Connection ID/budget gate now precedes concurrent reader effects.
      Actual executable regression covers active provider cancellation,
      duplicate/malformed refusal, pending next Input, original
      unverified attempt and unchanged durable grant/Git state.
      Executable native RPC 1/1, headless 8/8, Actor 46/46,
      Agent lifecycle 3/3, Host native/Process 2/2, prior stdio 3/3,
      Core journal 14/14; strict affected Clippy, durable/minimal
      targets, full executable/version and scoped fmt/diff passed.
      **Pre-tool restart/native artifact recovery landed:** exact
      direct original owner Input/Run without any tool intent can
      reopen exclusive Store and use existing interrupted Turn recovery;
      paid attempt remains unknown, no queue replay. Unclaimed tool
      intent still held. Driver/Sandbox-specific original recovery checks
      exact selection before recorded proof or release replay. Actual
      offline native-source-recover with private approval in original
      artifact root, exact complete named ordinary checkout custody,
      actual registered driver and original artifact records. Independent
      host permanent quiescence/exclusion required; wrong identity
      refuses, exact owned stage only cleaned, no history/hazard rewrite.
      Real executable crash and native artifact recovery regression.
      Actual executable recovery 1/1, Host native/Process 2/2,
      Sandbox 4/4, source barrier 1/1, Actor 46/46, Agent lifecycle
      3/3; strict affected Clippy, durable/minimal targets, full CLI
      version/help, scoped fmt/diff passed. No whole-Run clearance.
      **Whole-Run native reconciliation landed:** original terminal
      attached direct native Git Run, prior exclusive owner/Store scope,
      current exact Manager/Project/Session writer and original Input.
      Every ToolStarted must have its matching complete native claim,
      exact original versioned binding digest and positive release.
      Partial/unknown/unclaimed/unsupported intent refuses. Original
      Run hazard only + exact Input/queue disposition in one owner
      transaction; Inspected is not success/retry, already-settled
      Input supports missed queue ACK. Original payload unlink after
      terminal fact, exact replay idempotent. No source barrier release,
      provider/Turn rewrite, grant, or generic note-based acknowledgement.
      Actual offline native-turn-reconcile; real process crash after
      read/write, partial-proof refusal, per-effect cleanup, exact Run
      reconciliation and next pending Input only.
      Executable recovery 1/1, Agent lifecycle 3/3, Sandbox 4/4,
      Input admission 2/2, Store queue 8/8 and recovery 2/2, Host
      native/Process 2/2, Actor 46/46; strict affected Clippy,
      durable/minimal targets, full executable/version/help,
      scoped fmt/diff passed. Signed `a747e78`; independent locked/offline
      all-feature executable native_source_rpc rerun 1/1 (19.82s).
      **Original native no-start proof landed:** fixed claim-gated native
      route contract in exact registered binding, original source journal
      stamps optional ToolStarted route fact; reject caller-supplied proof.
      Never infer no-start from missing claim alone. Exact original
      marked intent with no contradictory custody/success can attach on
      owner restart and reconcile through the same whole-Run owner;
      actual claims still require original positive release. Unmarked and
      unsupported intents refuse. Report no-start count separately; no
      physical release fabrication, history rewrite or new grant. Real
      Actor task loss after intent acknowledgement before read/write
      worker, actual exclusive Store restart, immutable Turn and queue
      disposition, exact replay and fresh-granted later Input only.
      Agent lifecycle 3/3, executable recovery 1/1, Host native/Process
      2/2, core journal 14/14, router 3/3, SQLite/JSONL provenance 1/1 each;
      strict affected Clippy, durable/minimal checks, actual version/help
      and scoped fmt/diff passed.
      **Exact linked Git native recovery landed:** existing offline effect
      recovery accepts v2 independently approved private standard linked
      worktree backing; v1 ordinary layout unchanged. Canonical root,
      common/worktree admin and objects identity pins, bounded single-link
      mapping/backlink checks before Git discovery, disjoint private
      backing; original driver binding/source stamp still authoritative.
      Hold independent approval and original driver/capability pins
      through settlement, reopen only existing original artifact namespace,
      exact cleanup/release and terminal replay without old backing.
      Actual executable ordinary+linked grouped lifecycle, altered marker/
      wrong ID/substituted backing refusal, original artifact custody and
      both Git indexes preserved, whole-Run and next Input unchanged.
      Executable 1/1 (both layouts), Host native/Process 2/2, strict
      affected Clippy, durable/minimal all-target checks, version/help
      and scoped fmt/diff passed.
      **Exact Process receipt custody and independent recovery fence:**
      source lease binds to actual PipeProcess receipt or captured launch
      failure; no foreign terminal or failure-kind proof. Original
      driver-supplied RAII proof held through release/deferred quiescence,
      factual Process terminal remains independent. Trusted registered
      local Process recovery uses exact original effect/source/site/driver/
      Sandbox selector before any replay or inspection, no new owner or
      ledger. Refusal preserves Unknown, no broad Run clearance. Grouped
      real fixed-child regression covers wrong receipt, actual no-spawn,
      missing driver, proof refusal/held release, lost awaiter and replay.
      Host native/Process 2/2, adapter Process 29/29, aw-process 8/8,
      actual executable recovery 1/1, strict affected Clippy, durable/minimal
      checks, version/help and scoped fmt/diff passed.
      **Source-aware foreground Process Job dispatch:** registered
      physical driver + independent live source decision compose the original
      Agent, source scope and root-free one-node Job runtime. Fixed
      `ProcessSourceV1` route, original journal/source claim/fenced spawn,
      original Job-owned stream writers and terminal, held independent
      proof and post-journal release; no ambient fallback or new owner.
      Actual Actor regression exercises reviewed fixed child and strict
      containment refusal/no-spawn, original stream/result/claim settlement,
      revision and queue preservation, runtime retirement and reopen.
      Actor lifecycle 3/3, adapter 259/259, shared Process 8/8.
      Host native/Process 2/2, executable recovery 1/1, strict affected
      Clippy, durable/minimal checks, version/help and scoped fmt/diff passed.
      **Exact original Process Run/Input/queue reconciliation:** shared
      bounded scan and original logical recovery transaction; native-only
      API/CLI stays narrow. Fixed original local `ProcessSourceV1` admits
      positive claim-gated no-start (never absent-claim alone) and exact
      claimed positive release; actual quiescence must first be consumed
      by the original per-effect owner. Reject legacy/unmarked Process,
      mixed routes and unsupported intents. Real Actor losses at original
      intent ACK and after actual reaped Process, quiescence + ToolFinished
      ACK before release; exact Store restart, release refusal/recovery,
      immutable Turn, hazard/Input/queue Inspected, idempotent replay and
      later pending Input only. Actor lifecycle 3/3 passed.
      Host native/Process 2/2, native executable 1/1, Store Sandbox
      owner 4/4, strict affected Clippy, durable/minimal checks, version/help
      and scoped fmt/diff passed.
      **Mixed direct native/Process source execution:** shared fixed-tool
      adapter composition, one original Agent/registry/approval and Git
      execution fence, one foreground Process constructor, serial source
      tool ceiling; no ambient root or fallback. Exact marked native and
      Process bindings may coexist, with original per-effect custody and
      post-journal release. Shared whole-Run recovery consumes each exact
      release or positive original no-start; native-only API stays narrow.
      Real mixed Actor regression: native read + actual reviewed Process +
      interrupted unclaimed write, original releases preserved, exact
      Store reopen/replay/queue retirement and later pending Input only.
      Refuse missing grant, batch and second shell. Actor lifecycle 3/3,
      adapter 259/259 passed.
      Host native/Process 2/2, Store Sandbox owner 4/4, native executable
      1/1, strict affected Clippy, durable/minimal checks, version/help and
      scoped fmt/diff passed.
      **Foreground native Graph node source custody:** original marked
      constructor, actual sealed RunningNode/JobManager transition identity,
      combined constructor+peer authority, serial root-free Git native Graph.
      Exact per-node source claim/quiescence/terminal release in the original
      owner; full named source set, no ambient fallback or extra ledger.
      Bounded startup/Run reconciliation counts original releases/no-start
      by coordinate, exact positive recorded-proof recovery, immutable history
      and original Input/queue disposition. Grouped Actor loss before claims,
      after actual Graph results, and actual callback loss after physical
      write/positive proof (foreign transition refused). Actor lifecycle 3/3,
      adapter 259/259, binding authority 6/6, Host 2/2, standalone Store 4/4,
      native executable 1/1, strict affected Clippy, durable/minimal checks,
      CLI version/help and scoped fmt/diff passed. Explicit Store batch/durable
      dependency avoids relying on combined feature unification. Wider
      shared Job suite 88/89: unchanged 64-Process-node compact status fixture
      exceeds 8 KiB (8,458 bytes); assertion retained, parent not independently
      executed, correction open. 363 passed, one failed overall.
      **Compact Job status correction + exact native Graph physical
      recovery:** original shared 8 KiB bound preserved with exact compact
      affected-stream lists; direct result and typed receipt unchanged.
      Strengthened original worst-case assertion; shared Job 89/89,
      runner 4/4 passed. One original Store recovery transaction for direct
      and exact claimed Graph node; actual outer records plus exact native
      node recovery view, no fabricated receipts. Shared original Git/
      artifact inspector holds independent pins through positive proof/
      release/revision commit. Missing proof refuses recorded-only recovery;
      fresh claimed no-start denied; exact replay no inspection. Private
      native recovery approval v3 selects exact Graph node; v1/v2 stay direct.
      Real Stage fault + Graph write/Actor loss, wrong selector/physical
      identity refusal and actual owned artifact cleanup, unowned lookalike,
      original Turn/queue and index preserved; Actor lifecycle 3/3,
      adapter 259/259, Host 2/2 passed. Standalone Store
      4/4, actual executable 1/1 (v3 selector refuses direct tool),
      strict affected Clippy, durable/minimal checks, version/help and
      scoped fmt/diff passed. 362 selected tests green. Private offline
      recovery settlement preserves original live node callback boundary.
      **Registered foreground Process Graph:** distinct marked mixed
      constructor, one fixed registered shell route with native peers,
      same root-free original JobRuntime, direct Process still works.
      Exact sealed RunningNode+constructor/peer authority at claim and
      final launch, both grant watches, actual receipt/output owner,
      deferred proof and original node terminal persist release. Reuse
      same native route/approval and exact source driver, complete source
      barrier. Shared exact node view/recovery typed Process inspector,
      original no-start/positive release and one-shell Run reconciliation.
      Grouped real Actor cuts: direct Process then mixed Graph, strict
      containment refusal, actual reaped Process with missing independent
      proof and exact physical recovery, immutable history and queue.
      Actor lifecycle 3/3, adapter 259/259, binding authority 6/6,
      Host 2/2, standalone Store 4/4, shared Job 89/89, runner 4/4,
      actual native executable 1/1, strict affected Clippy,
      durable/minimal, version/help and scoped fmt/diff passed.
      368 selected tests green.
      **Provider-selected derived signing Env projection:** actual
      local encrypted provider and host-only typed fixed selection in
      registered direct/Graph Process route; reviewed driver opt-in,
      sealed runtime identity, original one-shot spawn/source/Actor/
      tool/provider/VaultGrant gate, no key or arbitrary target, no
      unprojected fallback. Real grouped Actor cuts: stale rotation,
      reserved-name collision and valid proof-bearing direct+mixed
      Graph, exact no-start/release/revision, original history and
      later pending Input; wrong registry refused. Actual Process
      authority required at Vault launch. Actor lifecycle 3/3,
      Host 2/2, Vault signing 2/2, Sandbox owner 4/4, adapter 259/259,
      native executable 1/1, strict affected Clippy, durable/minimal,
      actual version/help and scoped fmt/diff passed. 271 tests green.
      **Derived signing File/Socket projection:** Linux sealed anonymous
      non-executable memfd and already-connected one-shot socket with EOF,
      child-only inherited capability through existing SpawnOnce/Process
      owner. Parent CLOEXEC, exact descriptor retirement, no pathname
      artifacts/listener/extra lifetime. Explicit exact projection kind,
      driver opt-in, distinct HMAC domains, all reserved integration
      collision checks inside original provider/source/Actor launch
      fence. Existing Env unchanged. Real Process child consumption,
      refusal/cancel cleanup and grouped Actor direct/Graph file/socket,
      cross-kind/collision refusals, original no-start/releases/history/
      queue/index. Actor lifecycle 3/3, shared Process 9/9,
      Unix platform 14/14, Host 2/2, Vault signing 2/2, Sandbox 4/4,
      adapter 259/259, native executable 1/1, strict affected Clippy,
      durable/minimal and fmt/diff passed. 294 selected tests green.
      Obsolete Socket advertisement assertion updated for Linux only;
      actual cross-kind grant refusal separately qualified.
      **Protected source/Vault disclosure custody:** original source
      claims atomically seal exact immutable source and optional Vault
      fact edges into the existing disclosure DAG, with the independent
      whole-Session stable-actor ceiling before activation and claim.
      Existing Session/Agent/participant/draft owners and prior history
      grant shortcuts enforce it. Exact history forks and Event manifests
      inherit it; broader recipient audiences and legacy transfers/exports
      without manifest support refuse. Actual Vault projection selection
      committed before admission; mismatched or downgraded launch refuses.
      Old unmarked custody remains decodable, not retroactively qualified.
      Actual source Actor/fork and Host no-start tests plus manifest/Event
      authority-owner tests passed. History owner 5/5,
      Actor 3/3, Host Process 2/2, native executable 1/1, strict affected
      Clippy, durable/minimal and scoped fmt/diff passed. Full Store
      272/277: the same five documented legacy failures (closed Agent
      shell subscription, principal/remote-start downgrade, remote-view
      reconnect and downgrade); no new failure identified. 278 distinct
      selected tests green; full Store suite remains not green.
      **Live provider-owned Broker:** explicitly selected Broker grant
      (distinct from one-shot Socket), Linux connected anonymous duplex
      socket and bounded fixed-challenge request/reply in original
      finite Process wait. No listener/task/second lifetime; actual
      launch pins held until retirement, original cancellation probe
      rechecked before response. Non-cloneable original effect live-use
      fence, exact source/lease/ToolBinding/Actor/Store/provider/grant
      held through synchronous response. Shared proof derivation,
      reserved integration collision refusal, no arbitrary message/key
      export. Actual provider and Store revoke cuts, direct+Graph
      consumers, invalid request/cancel cleanup, existing independently
      authorized metadata redaction revalidated. 300 selected locked/offline tests passed;
      strict affected Clippy, durable/minimal all-target and scoped format/diff
      checks passed. Prior five known full-Store failures were not rerun.
      **Explicit source construction RPC:** serialized sandbox_create
      baseline/parent and captured checkpoint create/restore using
      configured independent private approvals and the original durable
      receipt/physical Git owners. Current Project scope, complete
      required sources, private taskless captured restore; no paths
      or authority supplied by RPC. Same original semantic namespace,
      replay before approval, no unresolved retry/rematerialization.
      Offline --agent-rpc-request only with explicit recover/abandon;
      existing CLI namespaces unchanged. Actual Git lifecycle through
      ports and transport duplicate/path/advertisement checks.
      Headless 8/8, native executable 1/1, recovery namespace 1/1,
      actual Git lifecycle 1/1; strict Clippy, durable/minimal,
      CLI help/parse gates and scoped fmt/diff passed. 11 selected tests.
      Prior five legacy Store failures not rerun.
      **Explicit physical retention reconciliation:** separate
      Actor-owner maintenance RPC for exact retired checkpoint and
      original closed Sandbox revision, independent private approval,
      current Project/writer, original source leases/provenance/driver
      release and bounded redacted projection. No automatic deletion.
      Writer observed before approval and rechecked at checkpoint claim;
      closed managed alias retained custody versus passive closed alias,
      existing child/live-anchor gates, canonical and directory-identity
      exclusion of current controller layout. Real Store writer refusal,
      actual managed-alias collision, exact Git maintenance/Unknown
      recovery and headless capability/parameter/duplicate checks.
      13 selected tests passed (headless 8, Store 2, Host Git 1, CLI Git 1,
      native executable 1); strict Clippy, durable/minimal, help/flag
      conflicts, scoped formatting/diff passed. Separate flattened
      retention approval group and fixture lifetime corrections verified.
      Real maintenance stdio subprocess qualification remains pending.
      **Quiescent registry maintenance:** existing staged typed leaves
      and exact registration handles, exclusive registry/Store owner,
      reader and native Arc/Weak user fences. Aggregate exact retained
      binding, checkpoint, managed recovery and unresolved construction
      schema pins; absent/incompatible definitions remain diagnostic/
      recovery-required. Current owner/process/SQL fence and unresolved
      source mutation custody exclude removal/replacement. First cut
      blocks any dependency-bearing SourceDriver replacement; no generic
      semantic compatibility declaration. Vault withdrawal preserves
      attached grants and actual encrypted key custody, with aggregate
      projection diagnostics and exact current authority revalidation.
      Core 102 passed/1 ignored; Store maintenance 1, checkpoint 4,
      Vault 2; Host Git 1, Process 2; Actor 3, native executable 1:
      116 distinct selected tests passed. Strict affected Clippy,
      durable/minimal and scoped fmt/diff passed; five prior historical
      full-Store failures not rerun.
      **Next:** production reviewed physical Process containment/dispatch,
      bound-Agent structural mutation and remaining actual host gates.
      Explicit dependency-bearing replacement compatibility, final atomic
      ExtensionRegistry and protected broader metadata/Views, manifest-
      preserving transfer/export and independent source/provider share policy remain. Raw file/env and arbitrary secret exposure
      remain held; live broker is bounded fixed-challenge only. No arbitrary shell CLI
      or strong Unix backend promoted; nonstandard/overlapping linked
      layouts and unmarked legacy native/Process no-start remain held.
      Separately qualify legacy participant
      Agent ACL/access_generation dispatch. No generic
      shared/forked/participant or non-native execution promotion.
      Complete source-aware Job/Process/extension containment/dispatch, broader Run-hazard reconciliation,
      general multi-source Process policy, other baseline effects, unmarked/ambiguous scratch and
      non-exact partial checkout cleanup, physical authority/production cleanup, bound-Agent mutation
      and other structural removal cases remain. Configured signing and Git approval loaders, local
      shell minimum, single-link native policy, redacted read/list and history/empty-Auto create +
      empty-baseline task-less restore, logical checkpoint retirement and configured signing
      activation/local binding RPCs, provider-authorized Vault entry metadata,
      independent source identity metadata and explicit local signing broker,
      offline Git capture/restore/exact
      recovery/abandonment and narrow offline checkpoint/closed Sandbox
      retention reconciliation, standalone Git derivation and retained-version
      baseline construction/recovery/abandonment and unused-Sandbox
      attachment/recovery/abandonment, replacement/recovery/continuation
      and exact named/final-source release, plus authenticated unused-Sandbox
      Git source setter are landed; do not repeat.
      Remaining Vault production Process dispatch, raw credential projection and broader/interactive broker contracts, broader provider-specific protected metadata/Views, general physical retention authority/Project retention-admin authorization,
      task-bearing/cross-Project restore, broader source capture/restore authority,
      ambiguous/non-exact or changed-binding cleanup and remaining Sandbox/Vault host RPCs held.
      See [November 27 comprehensive handoff](../../docs/AW-DL200-PHASE3-FULL-COMPREHENSIVE-HANDOFF-2026-11-27.md) §5 for next cuts, limitations and acceptance. Schema v60.
      Last independently verified published AW main `0f9807f4`, dotlink main
      `85ceccc1`; this next local checkpoint not yet published.
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
- [x] Phase 2: gap-free Event projection progress, authority-scoped EventStore affinity, same-store Event→Agent Input transaction boundary, source-first candidate-oblivious retryable auth-unavailable with external authority fence semantics, exact source-owner delivery cutover rev + legacy accepted-input provenance sidecars, subscription/admission race linearization and explicit cross-store refusal/rebaseline.
- [~] Phase 3: durable DriverKey + artifact-schema/recovery vocabulary, staged SourceDriver leaf, v55 named binding persistence/startup compatibility, Git clean/dirty capture/private retention/inspection/gitlinks and isolated Prepared restore, v56 immutable checkpoint/source storage and trusted history-only replay, v57 indexed construction receipt/Prepared source custody and unresolved dependency resolution, plus Store-local complete-set source mutation barriers, receipt-bound checkpoint capture preparation, exact restart inspection/recovery, evidence-fenced atomic checkpoint publication/replay, receipt-bound non-destructive restore preparation/inspection, atomic held Agent/Sandbox restore publication, v58 independent SandboxGrant evidence/source preflight, typed SandboxDriver/fenced host source-effect handoff, original PipeProcess source-aware host composition, direct ToolStarted/ToolFinished-linked custody/recovery, pinned native single-source route/deferred post-journal settlement, concrete Git materialization resolution, root-free native registration, live native source approval, explicit Actor host selection, quiescent metadata-only binding mutation, checkpoint retirement/retained-anchor reconciliation and source-bearing Sandbox logical close/retained-layout fencing and trusted closed-Sandbox materialization reconciliation and exact managed Git restore-materialization release/original Applied restore provenance and metadata-only fresh baseline restore/empty Auto capture and unpublished capture/restore candidate reconciliation and shared custom initial history ACL construction and captured-source restore initial-task custody/publication and task-only baseline restore/recovery and fenced Git checkpoint capture/restore/retention reconciliation, managed closed-Sandbox retirement, effectful versioned baseline restore host composition, v59 Agent VaultGrant references and staged VaultProvider leaf with actual local signing broker and fenced derived Process proof projection, plus interrupted source-effect custody recovery and worker-owned native/finite Process quiescence are live; remaining work is production source-root activation and broader live source-aware execution/recovery, production approval provisioning, complete source-aware Job/Process dispatch and containment, broader Run-hazard reconciliation and production physical recovery proof acquisition, final-source removal/effectful structural mutation, remaining partial/scratch and production candidate/retention cleanup, effectful fresh baseline materialization, concrete provider fences, full contextual replay/preflight ordering, remaining Vault projections/dispatch and host RPCs (configured signing local activation and redacted list/set landed).
