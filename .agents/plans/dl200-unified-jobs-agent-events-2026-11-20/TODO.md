# DL-200 — R6 simplification review + implementation queue

Base: `f035a00031b8f5879f4d77f3fd14bfe02b89305c`

Status: **R6 + SHARED PROCESS/RELEASE-CORRECTION AMENDMENT SEALED / IMPLEMENTATION PROMOTED / PHASE-0 CORRECTIVE IMPLEMENTATION GATE PENDING**

## Committed

- [x] `132bfd7` — published R1 DL-200 seal/implementation-plan baseline.
- [x] `7c3800c` — R6 architecture sealed.
- [x] AW shared Process runtime published through `7c16708`; AW main `59d67ba` adds the
      focused bounded-drain regression test only.
- [x] dotlink source `16475e4` + handoff/main `fb1b7b9` — repinned to AW `7c16708`, real
      `aw-process` consumer cutover complete, old shell Process owner deleted.

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

- [~] **Phase 0 shared cut accepted historically; corrective convergence pending** — `Occupancy`,
      `crates/aw-process`, AW+dotlink consumption, no parallel Process guard/runtime, compatibility
      `tree` projection and non-login Bash are live. The v0.17.7 Spawn Facility corrective gate is
      separately pending as recorded below; old executable evidence remains valid for unchanged
      semantics.
- [~] **dotlink-main release convergence + product-policy + shared-Process consumer — source implemented locally; publication/repin + platform qualification pending.** Last audited release is exact **v0.17.9
      `576d0a1`**; committed `81ef891..v0.17.9` diff SHA-256 `1aa632e3…`. Before the post-repin
      consumer cut, re-diff the then-current release/0.17 from that tag and classify any new delta by
      owner; never cherry-pick release runtime ownership.

      Implemented locally on main against the existing AW pin: one `RuntimeSurfacePolicy` from
      active transport constraints; 35m horizon - 10m report reserve = 25m/1500s bounded command
      cap; current-surface `jobs_get.next`; startup/server guidance from the same constants; effective
      cap in fresh execution authority; retained Job timeout immutable; AW receipt profile `ts=true`
      with `checkpoint_minutes=None`. `created` remains Job-age/`elapsed_ms`; the absolute
      `execution_deadline` is established at the canonical Running/Start checkpoint so queue,
      preparation and output admission do not consume execution budget. Advice counts down from the
      actual deadline only. No rendered-receipt parsing, duplicate canonical lifecycle, generic
      callback/plugin or AW turn policy.

      Consumer source is also implemented locally against the dirty AW checkout: dotlink's sole
      `aw_process::Process::spawn` is async, passes the canonical Running deadline + level-triggered
      Job cancellation through `SpawnControl`, and maps pre-spawn Cancelled/TimedOut to product
      cancellation/timeout with `process_not_started=true` and **no ProcessTerminal**. Shared
      settlement generalizes the old spawn-failure bit to the factual no-Process case without
      inventing a second runtime owner.

      Local evidence against local AW: shell **86/86**, Graph **13/13**, focused queued-lane
      cancellation and pre-spawn-timeout projection tests, canonical spawn-refusal settlement, full
      test-graph check, strict all-target/all-feature Clippy, fmt and diff-check green. The queued
      cancellation test uses the real process-global Spawn Lane and proves the command never execs.
      Validation used temporary local-path dependency substitution and restored Cargo.toml/Cargo.lock
      byte-for-byte; the committed manifest still intentionally pins published `7c16708` until AW
      publication/repin is authorized. Real Bubblewrap smoke was attempted with the opt-in enabled,
      but L5's outer tool sandbox rejects `bwrap`'s `/proc` mount (`Operation not permitted`), so that
      gate remains unavailable here rather than failed. Windows-native remains separately qualified.
- [x] **Phase 0B COMPLETE locally** — existing policy/guardian/Remote Start owners now carry
      original-invocation, actor, disclosure and lifetime replay correctness without a global replay
      database. Consumed AW policy rows compact only after a durable/effect recovery boundary into a
      <256-byte indexed tombstone while immutable audit retains full evidence; dotlink full Remote
      Start receipts compact after ordinary retention to payload-free never-retry tombstones rather
      than deleting the authorization identity. Unknown outcomes remain tombstones. Long-lived
      admission no longer depends on Session-lifetime audit/event scans, and live capacities remain
      explicitly scoped to active Turns/subscriptions/exposures rather than retained history.
- [ ] **Phase 1** — introduce `StoreScope` using existing `store_id` and
      `restore_generation` plus `StorePos(commit_log.seq)` for cursor/projection helpers;
      restore-stale old positions and finish Agent migration/history/disclosure without
      duplicate Session or revision ownership.
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
      Phase 0 remains in progress.
- [x] `IMPLEMENTATION.md` updated for R6; implementation remains separately promoted on
      2026-11-21 and proceeds under sealed R6.
- [x] R6 comprehensive handoff + copy-ready next-agent prompt created under `.agents/docs/`
      and wired as the canonical entrypoint from AW/dotlink CURRENT_HANDOFF.
- [~] Phase 0 shared convergence was accepted against dotlink v0.17.6, but v0.17.7 exposed one
      post-acceptance shared Process correctness gap: Linux `aw-process::Process::spawn` still
      forks from the requesting Tokio worker, so thread-scoped `PDEATHSIG` launchers can receive
      SIGKILL when that worker retires. Historical v0.17.6 evidence remains valid for unchanged
      semantics, but the focused v0.17.7 corrective gate below must pass before Phase 1.
- [~] Phase-0 shared Process/release-correction gate — **design SEALED / AW + DOTLINK CONSUMER SOURCE IMPLEMENTED LOCALLY 2026-11-21; publication/repin + real-Bubblewrap + Windows-native qualification pending**: one private logical Linux
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
      consumes that same execution deadline; pre-Running product queue time does not. Repin dotlink
      main and reuse the real Bubblewrap + Windows + stale-launch tests. No `spawn_blocking`/
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
      green. Remaining pre-Phase-1 blocker is now publication/repin plus platform qualification of
      the already-implemented consumer: real Bubblewrap on a host that permits its mounts, and
      Windows-native qualification.
- [ ] Phase 1/4: exclusive Agent-state activation + exact indexed lookup, scalable live-authority-rechecked DisclosureManifest, store-local/gap-safe/path-reauthorized high-water `jobs_list` cursor pages with swap catch-up and race-free revision waits.
- [ ] Phase 2: gap-free Event projection progress, authority-scoped EventStore affinity, same-store Event→Agent Input transaction boundary, source-first candidate-oblivious retryable auth-unavailable with external authority fence semantics, exact source-owner delivery cutover rev + legacy accepted-input provenance sidecars, subscription/admission race linearization and explicit cross-store refusal/rebaseline.
- [ ] Phase 3: bounded/non-secret symmetric capture/restore Prepared recovery descriptors + exact driver-schema pinning across store-open/restart + immutable/held-through-commit external publication evidence + publication-transaction authority fencing.
