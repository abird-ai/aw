# DL-200 — R6 simplification review + implementation queue

Base: `f035a00031b8f5879f4d77f3fd14bfe02b89305c`

Status: **R6 + SHARED PROCESS/RELEASE-CORRECTION PUBLISHED / PHASE 0B COMPLETE / PHASE 1 UNDERWAY**

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
- [~] **Phase 1 UNDERWAY** — private `StoreScope` reuses existing `store_id` + nonzero
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
      seam deliberately does not inspect finite Jobs. Native Host now retains the exact configured root
      JobRuntime; still open before the host shell is complete is only the orchestration of Actor Control +
      `close_owner` + terminal Store transition for root/default close, then live initial `task` dispatch,
      explicit logical Sandbox
      refs/`auto` policy owner, reviewed
      cross-Project redirect authority and staged `agent.inputs.*`. After that propagate provenance
      through new Event/Tool/Process/model facts, add only consumer-required snapshots/exposure rebuild,
      and defer reachability/flatten acceleration until a real consumer requires it.
- [ ] **Phase 2** — one EventScan for read/subscription/Handler with source-first
      proof/deny/unknown; final Input/Handler owners retain reauthorization, dedupe and
      acknowledgment; reuse projection/cursor/registry mechanics.
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
