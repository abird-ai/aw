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
      Agent lookup reuses indexed `job_exposure_current` with current-authority recheck. A private
      branch-bound `agent_owner` seam now owns legacy revision reads + zero-work in-transaction
      `created`/`changed` markers with no new persistence. Wired exact paths: create/fork, Turn
      admission, durable result + attachment settlement, changed-only subscription, delegated
      parent/child publication with immutable parent lineage, stable close request, exact stable
      settlement. Recovery deduplicates exact changed Agent keys in its already-cold transaction and
      marks each once before commit; hot paths remain allocation-free. Committed Input now marks the
      Agent only on exact `Prepared -> Accepted`; proposal, dispatch/settlement and ActorQueue staging
      remain revision-neutral at this layer. Legacy Session title/participation are explicitly backing
      compatibility state, not branch-selected or broadcast Agent mutations; participation only seeds
      the first direct Agent ACL conservatively. Legacy revision remains `MAX(audit.seq)+1` until
      exclusive activation; no audit rows were added. Next: perform the one exclusive Agent-state+ACL
      seed/activation and atomically switch Agent-facing revision/title/access reads+writes away from
      Session compatibility state. Add shared cursor/projection-head mechanics only when an actual
      Phase-1 consumer needs them.
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
- [x] R6 comprehensive handoff + copy-ready next-agent prompt created under `.agents/docs/`
      and wired as the canonical entrypoint from AW/dotlink CURRENT_HANDOFF.
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
