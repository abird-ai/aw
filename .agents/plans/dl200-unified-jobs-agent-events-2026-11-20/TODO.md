# DL-200 — R6 simplification review + implementation queue

Base: `f035a00031b8f5879f4d77f3fd14bfe02b89305c`

Status: **R6 SEALED / IMPLEMENTATION PROMOTED / PHASE 0 IN PROGRESS**

## Committed

- [x] `132bfd7` — published R1 DL-200 seal/implementation-plan baseline.
- [ ] R6 plan-only simplifications and ongoing Phase-0 source changes are uncommitted.
      No commit/push without separate authorization; Phase-0A focused evidence is historical,
      not the R6 Process rename/dotlink convergence gate.

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

- [ ] **Phase 0** — rename Rust `Tree`/`.tree` to `Occupancy`/`.occupancy`; **create
      `crates/aw-process`** as the only async Process + stdin/stdout/stderr pipe lifetime,
      wait/stop/drain owner; migrate AW and dotlink consumers, remove parallel guards,
      keep product output stores/wires, and qualify bounded sink failure + AW/dotlink parity.
- [ ] **Phase 0B** — reuse private replay/effect/ExternalEvidence helpers inside existing
      policy, guardian and remote Start owners; complete original-invocation, actor,
      disclosure and lifetime audit gates without a global replay database.
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
- [x] Phase-0A AW shared Process facts are implemented and focused-checked in the
      uncommitted working tree; Phase-0B source work from the parallel agent is preserved.
- [x] DL-200 dotlink Phase-0 consumer/projection work has not begun; unrelated dotlink
      source changes are not DL-200 implementation evidence.
- [x] No AW `.agents/plans/ux/**` content inspected or modified.
- [x] Sandbox/mount/config-v13 implementation remains frozen.
- [x] DL-200 R5 amendment/reseal recorded; R1–R4 retained as historical provenance.
- [x] R6 design written across README/SPEC/IMPLEMENTATION/TODO and explicitly sealed;
      Phase 0 remains in progress.
- [x] `IMPLEMENTATION.md` updated for R6; implementation remains separately promoted on
      2026-11-21 and proceeds under sealed R6.
- [x] R6 comprehensive handoff + copy-ready next-agent prompt created under `.agents/docs/`
      and wired as the canonical entrypoint from AW/dotlink CURRENT_HANDOFF.
- [ ] Accept Phase 0 only after its consolidated AW+dotlink behavior gate; Phase-0A focused
      checks and Phase-0B work-in-progress are not Phase-0 completion.
- [ ] Phase-0B review gaps: stable original replay vs writer/policy/parent changes,
      actual participant ToolPolicy/payer authority, compact replay tombstones, exact
      opaque/minimal withheld-replay response, current disclosure on cached receipts and
      indexed audit admission beyond former lifetime ceilings; validate together.
- [ ] Phase 1/4: exclusive Agent-state activation + exact indexed lookup, scalable live-authority-rechecked DisclosureManifest, store-local/gap-safe/path-reauthorized high-water `jobs_list` cursor pages with swap catch-up and race-free revision waits.
- [ ] Phase 2: gap-free Event projection progress, authority-scoped EventStore affinity, same-store Event→Agent Input transaction boundary, source-first candidate-oblivious retryable auth-unavailable with external authority fence semantics, exact source-owner delivery cutover rev + legacy accepted-input provenance sidecars, subscription/admission race linearization and explicit cross-store refusal/rebaseline.
- [ ] Phase 3: bounded/non-secret symmetric capture/restore Prepared recovery descriptors + exact driver-schema pinning across store-open/restart + immutable/held-through-commit external publication evidence + publication-transaction authority fencing.
