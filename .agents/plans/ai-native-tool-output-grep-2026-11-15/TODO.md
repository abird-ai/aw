# AI-native Job output + grep

Base AW: `62b8da6babfa729d6bbb9c21e75ac89c228b22b1`
Source: [DESIGN.md](DESIGN.md). Dotlink dependent plan will reference this AW-first work.
Merged implementation: `0d2b4e3` — `Build configurable toolset and harden shared Job control`.
Review base before the signed follow-up: `7d09d5f`.

## Committed

- [x] `0d2b4e3`: shared output ownership/range primitives, exact delivered-byte
  proof, multi-consumer lease primitives, unknown-dominant Graph status,
  current Actor read gate, atomic finite ACK/Finish commit, shared grep
  context/parser/engine parity and compact Job projection are merged on
  `main`.
- [x] `be6d48db552fd44b9dd85364cd8734454109ce68` — signed shared-runtime
  checkpoint containing the event-driven Job waits, delivery/owner/Finish
  hardening, journal-uncertainty retention/publication fixes, typed receipt
  updates and the commit-readiness regressions recorded below. It is published
  on AW `main` as the parent of `403eb84`, and dotlink D0 consumed it in signed
  commit `d0a3922`. Historical checkpoint headings below retain their original
  “uncommitted” wording as chronology, not current Git status.
- [x] `26a8c788d6dc7e0a7fe16995f8057fb332440978` — signed D1 shared follow-up
  fixing strict delivered-ACK replay after independent owner compaction while
  preserving the prohibition on new ACK advances into compacted-away bytes.
- [x] `beef1bc2d2f37b5660ebe928d368bca37368e608` — signed/published
  writer-quiescence follow-up. Hidden terminal retirement, automatic capacity
  reclamation and runtime live-work reporting now refuse to cross an unfinished
  retained output writer.

## Current status

- [x] A. Shared output ownership: contiguous delivered prefix, exact scoped terminal Finish and group all-or-none preflight; unchanged known ACK/replay semantics.
- [x] B. Shared bounded byte-tail and signed-offset range/snapshot primitives, typed result; no extra spool/backend.
- [x] C. Shared event-driven, lost-wakeup-safe Job settlement/output observation; non-async builds preserved; no additional JobManager or scheduler.
- [x] D. AW finite get/read/ack projection, current payload/Control authority,
  512/1024 aggregate tails, compact receipts, Job/Graph node/root semantics and
  event-driven waits are qualified in the shared and live adapter tests below.
  - [x] **Security gate:** default/explicit `jobs_get.tail` independently
    rechecks the current `jobs_read` binding and live Actor Envelope.
  - [x] **Output delivery gate:** only raw bytes represented by a successfully
    validated finite projection advance contiguous delivery proof.
  - [x] Live direct/Graph read/ACK/revoke/owner qualification is recorded in
    the merged checkpoints below.
- [x] E. Shared grep symmetric/asymmetric context up to 128 in schema/request/search, exact file/dir, output budgets and direct/Graph conformance.
- [x] F. Catalog/description limits and unified tests for shared direct/Graph surface; no new generic registry or hidden model-facing `jobs_*` leaves.
- [x] G. The reviewed shared checkpoints `be6d48d`, `26a8c78`, and
  writer-quiescence follow-up `beef1bc` are published; the current uncommitted
  dotlink tree consumes `beef1bc`. D1 shared output and D2 grep convergence are complete there. The
  first D3 owner-policy checkpoint also removed dotlink's private native-Task
  output-owner hash: standalone shell planning now declares
  `task_result_eligible=true` and uses shared `task_result_owner(id,1)`, while
  Graph plans remain Task-ineligible. Dotlink D3 also now retires old durable
  shell request owners when the hosted namespace/execution authority changes,
  using its existing bounded retirement queue; ordinary mutations stay
  fail-closed without shell retirement. Final D3 audit proves Busy retirement
  retry, positive-delivery monotonicity under late revocation, and serialized
  runtime replacement/cleanup failure blocking. Optional shared consumer leases
  remain unadopted because no distinct authenticated output audience exists.
  Dotlink D1–D3 are complete. D4 independent security review is also complete:
  one host-only stale-guard Busy-discard leak was fixed by carrying guard
  revocation through dotlink's bounded cleanup queue in discard mode; no AW
  source change was needed. Host all-feature tests/lint are green. Dotlink
  repo-wide Markdown and all Nix check derivations are green; the hermetic Nix
  test fixture supplies pinned Bash explicitly. Native full Bubblewrap plus all
  seven targeted GPU/KVM/device-security cases are green. Dotlink D1–D4 are
  complete; only separately authorized publication/signing/release actions remain.

## Working checkpoint — 2026-11-15 (uncommitted)

- Shared A/B primitives implemented: exact contiguous delivered-prefix proof; single `OutputPosition::Absolute/Tail` bounded spool reader with returned absolute offset; terminal-only `finish_output_for_owner` preflights all owned outputs before logical ACK; aggregate Process tail uses a fair half-split with unused-byte redistribution. Focused two-owner/512/1024/out-of-order/terminal/idempotent tests pass.
- Shared grep E implemented: `context` symmetric fallback plus canonical `before`/`after` overrides, each 0..128, shared schema/parser/engine and direct AW adapter. Existing exact-file vs recursive directory backend remains; focused direct-file and negative-boundary tests pass.
- AW D partially wired: `jobs_get.tail` default 512 **combined**, maximum 1024 combined, status-only `tail:0`, current `jobs_read` binding and producer-payload checks; `jobs_read` signed retained offsets, `jobs_ack({ref})` terminal owner Finish, Graph root/no child spill. One targeted API schema test and the unchanged AW adapter suite pass. **Do not claim end-to-end new Job flows qualified without a live runtime test.**
- Execution still pending: lost-wakeup-safe event-driven wait C (existing finite get uses a 10ms loop and read waits are not yet supported); **D security/delivery gates above**, full live get/read/ACK + Graph/revoke/replay tests; F catalog descriptions; G grouped AW checkpoint and reviewed Git pin. The current AW adapter is a provisional implementation and is not approved for independent rollout. Dotlink dependent implementation intentionally not started before AW is qualified.
- Executed: `cargo test --offline --locked -p aw-tools --all-features --lib` **307 passed**; `AW_BASH="$(command -v bash)" cargo test --offline --locked -p aw-tools-adapter --all-features` **229 passed** (before the final schema-only fixture addition); focused `compact_get_read_ack_models...` passes; `cargo clippy --offline --locked -p aw-tools -p aw-tools-adapter --all-targets --all-features -- -D warnings` passes after fixes; formatting and `git diff --check` pass.
- No commit/push, no dependency pin change, no worktree move. Existing AW UX/storage/user-authorship changes and dotlink operational changes remain independent.

## Unknown-dominant Graph checkpoint — 2026-11-15 (uncommitted)

- **Shared outcome contract implemented:** `status::aggregate` returns `unknown` when **any** child is `unknown` (even with `ok`, `fail`, or `mixed` children). `mixed` now denotes only heterogeneous known/settled outcomes. `ok + fail = mixed`; `ok + unknown = unknown`. `skip`, `more`, and `incomplete` remain orthogonal.
- **Truthful known counts preserved:** shared Graph-result summary renders known successes and failures alongside `unknown=N` when the overall result is unknown, rather than discarding settled facts.
- **Root Quick Finish tightened:** `JobManager::finish_output_for_owner` checks exact per-node terminal outcomes and unresolved journal ambiguity rather than trusting an aggregate label. Unknown root refuses `jobs_ack({ref})` before mutating any owner ACK/lease; unknown node refuses its own Finish. Explicit Finish for an independently known node remains allowed when no global journal uncertainty applies. Known `mixed` Graphs remain Finish-eligible.
- **Tests executed:** two new live shared-manager Graph tests (known+unknown root no-release, independently scoped known-node release, known+fail root Finish/idempotence); updated status and exported conformance vectors; full `aw-tools --all-features --lib` **325 passed**, `aw-tools-adapter --all-features --lib` **234 passed**, conformance fixture tests **3 passed**, minimal/read-only compilation, strict affected-crate Clippy and fmt all green.
- **Still open:** output owner granularity, source/Actor Envelope disclosure, exact bytes-as-delivered ACK accounting, lost-wakeup-safe `jobs_get` completion waits and `jobs_read` byte waits; don't promote dotlink or claim whole Job/grep plan finished.
- No commit/push, version/pin change, new worktree or unrelated UX edits.

## Two-phase delivery-proof checkpoint — 2026-11-15 (uncommitted)

- **A/D partial correctness landed:** shared `JobManager::output_page_for_owner_at` and `process_tail_for_owner` are now side-effect-free **owner-authorized raw reads**. Neither read nor a default/explicit `jobs_get.tail` advances `OwnerAck.delivered_through` or `delivered_once`.
- **New shared host hook:** `JobManager::record_delivered_output(id, coordinate, stream, owner, raw_start, raw_count)`. Trusted host attests the exact raw prefix **represented in the successfully projected response**; it validates owner, release state, range overflow and retained bounds, and advances only contiguous `delivered_through`. A later out-of-order page never credits an unread gap. The hook does **not** ACK, release, create leases or issue disclosure authority. The host must never pass a model-supplied range unverified as a delivery mark.
- **AW finite `jobs_read` wired:** bounded text/UTF-8-lossy and base64 renderer returns both model text and the actual number of **raw bytes represented** (not the bytes initially fetched). It records the credit only after `ToolOutput::validate()`, under fresh `jobs_read` binding + producer payload checks. A projection failure, budget failure or revoked binding yields no credit. A synthetic Graph-root result is not a retained node stream and never earns an owner mark.
- **Delivery semantics boundary:** this is a trusted **model/SDK response-projection** attestation, not a claim that the transport proved the client physically received the bytes; loss/replay and MCP Task acknowledgement remain host responsibilities. `jobs_ack({ref})` is still separately authorized explicit Finish, allowed without delivery proof when terminal and settled.
- **Live qualification:** canonical `Batch` Graph handoff + exact node `jobs_read` of a 2 KiB native result under a 512-byte model budget, then rejected ACK at `returned_offset + 1`, accepted ACK at `returned_offset`. A further **successfully projected out-of-order** page does not bridge the unread gap; the next contiguous page extends the ACKable prefix. Additional checks: lossy UTF-8 expansion and base64 count exactly the surviving raw prefix; zero-length metadata-only page gives zero credit; owner tail peeks do not credit even offset 0; out-of-order credited reads don't bridge gaps.
- **Executed green:** `cargo test --offline --locked -p aw-tools --all-features --lib` **325 passed**, `AW_BASH="$(command -v bash)" cargo test --offline --locked -p aw-tools-adapter --all-features --lib` **237 passed**, `aw-tools --features read-only --lib` **110 passed**, minimal `--no-default-features` check, strict affected-crate Clippy (`-D warnings`), scoped rustfmt and `git diff --check`.
- **Still open / do not accept whole plan:** `job_output_owner` is Job-bound: independent callers share that owner today. Exact independent **consumer lease** scope, whole-caller quick Finish, live Actor Envelope/ToolStepGate disclosure, stale-owner/authority races, physical response replay/Tasks and completion-only finite `jobs_get` / byte-aware `jobs_read` waits require separate changes. Do not repin dotlink yet.
- No commit/push, version/pin/worktree change, edits to reserved AW UX subtree or unrelated repositories.

## Scoped multi-consumer lease checkpoint — 2026-11-15 (uncommitted)

- **Shared optional identity:** `JobManager::consumer_output_owner(id, scope:[u8;32])` derives a separate host-scoped lease by hashing the immutable Job binding and a host-authenticated, stable consumer scope. Zero scope, non-existent Job and owner collisions are fail-closed. Derivation **does not authorize reading or enrol the lease**.
- **Shared atomic enrolment:** `JobManager::enroll_output_consumer(id, node?, view?, owner)` preflights all currently selected streams (exposure, valid node/view, physical release, owner capacity) before any state update. New owners start with zero delivered and ACK watermarks; repeat enrollment is unchanged. Cap is 16 owners per stream. Released streams cannot be resurrected; streams created later require explicit enrollment. The existing Job/Task owner derivations and retention policies remain intact.
- **AW host decision:** finite Job namespaces are single-Agent audience (enforced by `JobRuntime::current_host` → `JobHost::bind_execution_owner`) and retained producer payload is independently Actor/node-binding protected. AW continues using its **stable Job-audience lease**, which spans turns of that one authorized Agent; it does not create unnecessary per-turn model consumer leases. Added a live AW handoff test rejecting another Agent's namespace takeover and another Actor's payload grant.
- **Qualified isolation:** two distinct consumer scopes cannot share delivered/ACK proof; explicit producer Finish cannot release output while either independently enrolled consumer still owns it. Atomic multi-stream enrollment rejects a full 16-owner stream without partially enrolling the other. Idempotent repeats and post-release no-resurrection verified. No new registry/JobManager, automatic grant, or cross-Job lease.
- **Executed green:** `aw-tools --all-features --lib` **327 passed**, `aw-tools-adapter --all-features --lib` **238 passed**, read-only `aw-tools --lib` **110 passed**, conformance **3 passed**, minimal `--no-default-features` build, strict affected-crate Clippy `-D warnings`, scoped Rust formatting and `git diff --check`.
- **Still open:** host integration of multi-consumer leases in dotlink/MCP/Task contexts (including enrollment before possible producer release), current Actor Envelope/ToolStepGate and ACK Control revocation, exact transport delivery/replay, and `jobs_get` completion-only / `jobs_read` byte-aware waits. This checkpoint does **not** complete the broader Job/grep rollout or dotlink repin.
- No commits, pushes, version/pin/worktree changes, or reserved UX edits.

## Current Actor Envelope and Control gate checkpoint — 2026-11-15 (uncommitted)

- **Missing boundary corrected:** `ToolContext` now carries an optional live, host-owned `AgentStepGate`. The real `AgentSessionController::shared_gate()` clones current Actor/Envelope/writer-fence authority, **not** an allow-bit snapshot. Custom Agent step gates fail closed if they cannot supply a live shared handle. Both original Agent dispatch paths propagate the gate; SDK/host calls without a controlled Actor retain their existing ToolPolicy requirement and do not acquire a fabricated Envelope.
- **Independent read authority:** `JobRuntime::payload_read_authorized` checks the exact current `jobs_read` ToolBindingAuthority binding **and** the same Actor's live Envelope for its source/effect/site. A current `jobs_get` grant alone does not authorize any retained bytes. `jobs_get` with implicit tail remains status-readable but renders `tail=denied`; explicit nonzero tail denies. `tail=0` reports status without output, and Graph roots never expose child streams. Tail projection is buffered until a final live recheck, rather than temporarily inserting sensitive bytes into the result string.
- **Read and ACK boundary:** finite `jobs_read` checks read+producer payload authority before reading and again after whole response validation, including synthetic Graph-root projections; only the successful final **physical stream** page can mint its bounded delivery proof. Finite `jobs_ack` checks the **current `jobs_ack` Control** grant, original producer payload authority and live Actor Envelope, then rechecks Control immediately before the shared manager's mutating ACK/Finish. The existing owner/Task/replay state engine remains unchanged.
- **Host context wiring:** SDK/CLI/MCP/auxiliary host-created `ToolContext` fixtures explicitly pass `live_step_gate: None`; live Agent dispatch shares the actual gate. No new tool, policy engine, serialized grant, scheduler or Job runtime.
- **Executed green:** `cargo test --offline --locked -p aw-tools --all-features --lib` **327 passed**; `AW_BASH="$(command -v bash)" cargo test --offline --locked -p aw-tools-adapter --all-features --lib` **240 passed**; `cargo test --offline --locked -p aw-agent-core --lib --jobs 1` **88 passed / 1 ignored**; targeted real `AgentSessionController` exact Envelope, finite read/ACK live revocation and background Process-tail denial tests all passed; strict `cargo clippy -p aw-agent-core -p aw-tools -p aw-tools-adapter --all-targets --all-features -- -D warnings` green; `aw-mcp`, minimal CLI, and full `aw-cli --lib` checks green. The first broad core/CLI compile attempts were externally terminated during dependency compilation, without Rust diagnostics; bounded low-parallelism reruns succeeded. Scoped formatting and diff checks passed.
- **Unresolved:** live cross-tool gate is queried at actual projection/ACK preflight, but exact atomic commit vs concurrent Actor Envelope revocation remains a distinct authority-linearization gate. Other `jobs_send/cancel/subscribe` control paths need equivalent review; no claim they were altered. `jobs_get` completion-only waits and `jobs_read` byte waits remain unchanged and pending. No dotlink dependency repin or external publish.
- No commit, push, version/pin change, new worktree or modification to reserved AW UX plans.

## Atomic finite-ACK Control commit checkpoint — 2026-11-15 (uncommitted)

- **Exact lock composition:** shared `ToolBindingAuthority::with_revalidated_binding` already serializes registry/ToolPolicy revocation. The controlled `AgentSessionController` now also exposes object-safe `AgentStepGate::with_authorized_tool_commit(name, source, effect, &mut action)`: it verifies the writer fence and holds the existing Actor runtime/Envelope mutex across one **synchronous, no-await** mutation. Default custom gates fail closed rather than falling back to check-then-act. `ToolContext::with_peer_tool_commit` projects the callback without granting capabilities or allowing repeat execution.
- **AW finite `jobs_ack`:** `JobRuntime::with_control_commit(ctx, "jobs_ack", action)` holds the **current exact ToolBindingAuthority guard**, then the live Actor gate, then commits the original `JobManager::acknowledge_delivered_output` or `finish_output_for_owner` effect. Lock order: ToolBindingAuthority → Actor runtime → JobManager. Neither raw output rendering nor replay/task callbacks await inside this transaction; no new lock, scheduler, journal or policy source.
- **Outcome correctness:** revocation wins first → Denied, with **no ACK effect**; an already-admitted commit wins first → exactly one ACK/Finish before the revocation publishes. If a nonconforming custom gate reports failure **after** invoking its action, return `Transport("Job Control outcome uncertain; inspect before retry")`, not a false no-effect denial and not an automatic retry.
- **Qualified tests:** deterministic real Actor release/commit ordering and rejected stale/incorrect effect, ToolPolicy revocation/commit ordering, committed-once despite faulty post-effect host gate, plus live finite ACK/Finish. Full `aw-tools --all-features --lib` **327 passed**; `aw-agent-core --lib` **90 passed, 1 ignored**; `aw-tools-adapter --all-features --lib` **242 passed**; strict affected-crate Clippy, minimal `aw-tools` check, MCP and full CLI library checks, and fmt/diff checks all green.
- **Scope boundary:** atomic relative to ToolPolicy publication and the authenticated Actor Envelope **held by the controller**. External writer-epoch fencing, original producer binding/source revocation, multi-owner/Task replay transport delivery and the other Control verbs have distinct checks; do **not** label those atomic or release-qualified. `jobs_get` completion-only and `jobs_read` byte-aware event waits remain next.
- No commit, push, version/pin change, new worktree, dotlink source edit or reserved AW UX edit.

## Event-driven finite Job waits — 2026-11-16 (uncommitted)

- **One shared observation source:** `JobManager::subscribe_changes(id)` returns a bounded Tokio watch receiver for that exposed Job (behind `aw-tools/async`). Existing manager lifecycle transitions, stream attachment, retained bytes, truncation/failure, EOF and output release publish to the same receiver. No polling, second scheduler, background worker, unbounded event log or separately mutable execution state. Minimal/non-async `aw-tools` still compiles.
- **Get semantics:** finite `jobs_get.wait_ms > 0` subscribes **before** checking authoritative state; it waits only for the requested root Job or node to settle, or the absolute wait deadline. Interim output, unrelated Graph nodes, `rev` changes and control revisions only cause a recheck, never early completion. `wait_ms=0` returns current state. Invalid node refs deny promptly. Observer cancellation terminates only the wait.
- **Read semantics:** finite `jobs_read.wait_ms=0` is nonblocking. Positive waits at nonnegative absolute stream offsets return on new retained bytes beyond that offset, writer EOF, truncation, Job settlement or deadline. Streams may attach after subscription without missed wakeups. `offset=-N` remains an immediate newest-byte snapshot; positive wait with a negative offset is denied. Synthesized Graph-root results use terminal-only observation (not stream readiness). Existing owner/Actor disclosure checks are repeated after waiting, and raw reads are still side-effect-free until final response projection; a wake never creates ACK proof.
- **Live regression:** background Process prints stdout before exit (`jobs_get` still waits for terminal), different positive get deadlines, Graph node-1 settles while sibling-2 is running (targeted get returns; root waits), bytes then EOF (`jobs_read` returns each correctly), immediate negative peek, canceled independent observers that do not terminate the Process. Shared manager notification test covers late attachment, append, EOF, settle and no implicit delivery/ACK.
- **Executed:** `cargo test --offline --locked -p aw-tools --all-features --lib` **328 passed**, `AW_BASH="$(command -v bash)" cargo test --offline --locked -p aw-tools-adapter --all-features --lib` **246 passed**, read-only shared tests **110 passed**, `--no-default-features` build, strict affected-crate Clippy `-D warnings`, scoped fmt/diff checks and MCP/minimal/full CLI host compilation green.
- **Remaining separate:** full dotlink adoption/repin and native MCP transport/task wait qualification, policy revocation/transport-delivery gates beyond the qualified AW finite paths, and public release checks. Do not treat this as a dotlink consumer rollout.
- No commit, push, dependency pin, new worktree or reserved UX subtree changes.

## Node-scoped byte-wait correctness — 2026-11-16 (uncommitted)

- **AW finite `jobs_read` follow-up:** `JobsTool::wait_for_output` used Job-root terminality when an exact Graph node's stream was missing or unsealed. A skipped/failed node could consequently wait for an unrelated still-running sibling. Now inspect the **requested coordinate's terminal state** after stream readiness; a settled node returns from the wait even if it never attached a result stream. The subsequent raw page correctly reports unavailable output instead of fabricating bytes or creating delivery/ACK proof.
- **Live regression:** three-node background Graph: node 1 `bash false`, node 2 conditional `read` skipped on `1.ok`, node 3 independent slow Bash. `jobs_get(n:2)` settles while root runs; `jobs_read(n:2,wait_ms=1200)` returns the missing-stream error before a 500 ms deadline rather than waiting for node 3. Root later settles normally.
- **Executed:** focused live test passed; `AW_BASH="$(command -v bash)" cargo test --offline --locked -p aw-tools-adapter --all-features --lib` **247 passed**; strict `cargo clippy --offline --locked -p aw-tools-adapter --all-targets --all-features -- -D warnings`, fmt/diff checks green. Dotlink's pre-pin adapter has the matching node-local readiness fix and independently qualified live tests.
- No new observer, scheduler, JobManager, policy grant, ACK side effect, worktree or dependency repin. Still requires the reviewed AW commit + dependent dotlink pin and MCP Task/replay qualification; no commit or push.

## Pre-pin wait and disclosure race qualification — 2026-11-16 (uncommitted)

- **Late stream attachment:** live two-node Graph starts a dependent `read` before its result stream exists; `jobs_read(wait_ms=1500)` subscribes first, wakes on result attachment/write and returns the exact bytes. Delivery proof permits subsequent explicit ACK but the read itself leaves `through=0` and `acknowledged=false`.
- **Revocation during wait:** a controllable live test gate revokes the `jobs_read` permission while a Process runs, before its stdout arrives. The real adapter rejects the waiting read on wakeup; no secret content or delivered-byte credit is returned. ACK beyond zero remains denied. Both tests exercise real `JobRuntime`, `JobsTool`, `Batch/JobShell` and shared spools; the gate itself is test-controlled, not a separate production authority implementation.
- **Qualification:** `aw-tools --all-features --lib` **328 passed**, `aw-tools-adapter --all-features --lib` **249 passed**, shared `read-only --lib` **110 passed**; minimal non-async build, strict affected-crate Clippy `-D warnings`, scoped rustfmt/diff checks and MCP/minimal/full CLI host checks all green.
- **Boundary:** tests prove live wake and payload denial at adapter projection, not guaranteed physical MCP delivery or atomic producer-policy revocation. Existing shared notifications/owner semantics unchanged; dotlink still uses prior AW pin. **No new AW Git commit or authorized dependency repin** yet; do not call D0 complete.

## Quota-truncation wake checkpoint — 2026-11-16 (uncommitted)

- **Shared read readiness fixed:** `OutputWriter::discard_if_full(seen)` can mark a running Process stream truncated without appending retained bytes. It previously skipped the per-Job watch notifier, leaving finite `jobs_read(wait_ms>0)` asleep despite its `summary.truncated` wake condition. Emit a change only on the first `false → true` truncation transition; later discarded chunks still update exact source-byte totals without one wake per chunk.
- **Red/green qualification:** shared JobManager test saturated an 8-byte spool, subscribed at the end of retained bytes, observed `discard_if_full(3)` set `truncated` without EOF/new bytes, and **failed** on the missing notification before the fix. Now passes; subsequent discard preserves `total_bytes=13` without another wake; no read or notification grants ACK proof.
- **Executed green:** `aw-tools --all-features --lib` **329 passed**, `aw-tools-adapter --all-features --lib` **249 passed**, read-only shared tests **110 passed**; minimal non-async shared build, strict affected-crate Clippy `-D warnings`, formatting/diff, MCP and minimal/full CLI compilation. One combined test command was externally terminated before adapter execution; isolated rerun completed successfully.
- **Dependency boundary:** the fix uses the existing bounded Job watch and existing `OutputWriter`, not a new scheduler, shared owner, result log or stream policy. Dotlink remains on its prior AW Git pin and must not claim this behavior there before the reviewed shared-runtime commit/repin. No commit, push, version or dependency change.

## Empty terminal-output Finish checkpoint — 2026-11-17 (uncommitted)

- **Correctness fix:** shared `finish_output_for_owner` now treats a settled, known Job/node with no retained streams and no explicit `view` as an idempotent `changed=false, released=true` Finish. Previously, an all-skipped/output-free scope failed `OutputMissing` despite having nothing to release. Explicitly naming a nonexistent stream still fails; unsettled `Busy`, unknown outcomes and ambiguous journals still reject before the empty-stream case.
- **Red/green and live test:** a claimed no-output Job refuses Finish while unsettled, then settles `Skip` without ever attaching a stream. The original terminal Finish test failed `OutputMissing`; after the fix, node/root Finish and repeat Finish return unchanged/released. A live three-node Graph demonstrates `jobs_ack({ref:n:2})` succeeds for a settled skipped node while a sibling continues running; a named missing `result` view still denies.
- **Qualified gates:** `aw-tools --all-features --lib` **330 passed**, `aw-tools-adapter --all-features --lib` **249 passed**, read-only shared **110 passed**, minimal shared build, strict affected-crate Clippy `-D warnings`, formatting/diff, MCP and minimal/full CLI compilation. The last added pre-settlement assertion was rerun as a focused pass.
- **Scope:** no owner lease, delivery proof, replay, Process writer or model schema change. This and the shared event-driven waits remain uncommitted until the AW coordinator's reviewed signed checkpoint; dotlink is **not** repinned.

## Post-release owner ACK replay checkpoint — 2026-11-17 (uncommitted)

- **Fixed owner bypass:** `JobManager::acknowledge_delivered_output` and trusted `acknowledge_output` previously returned `released=true` before validating owner/offset when the physical spool had been discarded. They now share a small post-release check: the owner must exist, have a prior acknowledged watermark, and the replay's `through` must be at most that watermark. No foreign owner or newly enlarged offset can be accepted as an idempotent replay.
- **Exact replay retained:** matching owner offsets in `[0, through]` return `changed=false, released=true` after compaction. Explicit terminal `finish_output_for_owner` also preserves owner-only replay without requiring prior read credit; the already released bytes and ACK history are not modified.
- **Red/green tests:** test failed on the original foreign-owner `Ok(released=true)`; passes after fix, covering both ACK variants, unauthorized/oversized offsets, same-owner replay and stable zero spool usage. Additional live known-mixed Graph Finish regression passes for same owner vs stranger. Shared optional consumer-lease hash domain separator now uses a **Rust escaped** NUL instead of an embedded binary source-file NUL (same runtime bytes); consumer scope test passes.
- **Qualification:** full `aw-tools --all-features --lib` **331 passed**, `aw-tools-adapter --all-features --lib` **249 passed**, read-only shared **110 passed**, minimal shared build, strict affected-crate Clippy, rustfmt/diff, MCP/minimal/full CLI checks green. Domain-separator cleanup and Finish replay assertions received targeted reruns afterward; no altered hash or new authorization source.
- **Still open:** verified signed AW shared-runtime commit and dotlink dependency adoption, physical MCP transport receipt, remote consumer leases and independent Task/replay proofs. No commit, push, version bump or repin.

## Post-truncation append notification checkpoint — 2026-11-17 (uncommitted)

- Shared `OutputWriter::append` used to publish a Job-watch change for **every** nonempty source chunk, including writes retaining zero bytes after immutable truncation. Under high output volume, every chunk could wake all finite `jobs_read` observers despite no new readable bytes or readiness transition.
- Notify only when an append retains at least one byte **or** the stream first becomes truncated. Subsequent discarded appends still account for every source byte; no new retained output, ACK proof, scheduler, thread or queue. The separate `discard_if_full` first-truncation notification remains unchanged.
- **Red/green:** full 8-byte memory spool, append one overflow byte, then sixteen `append(b"later")` calls. Previously a reader woke repeatedly; now only the initial retained bytes and first truncation signal. `total_bytes=89`, `retained_bytes=8`, writer still open and unauthorized ACK still denied. Both focused append and discard tests pass.
- **Qualified:** `aw-tools --all-features --lib` **332 passed**, adapter **249 passed**, read-only **110 passed**; minimal non-async, strict affected-crate Clippy `-D warnings`, rustfmt/diff, MCP and minimal/full CLI builds green. This is an AW pending-pin optimization, not present in dotlink's committed AW dependency.
- No commit, push, version bump, worktree or dependency repin.

## Failed-output drain notification checkpoint — 2026-11-17 (uncommitted)

- **Shared writer hot path:** after an output write fails, the Process must keep draining and counting every source byte. The existing `OutputWriter::observe_after_failure` published a Job-watch change **for each** later drained chunk even with no new retained bytes or new readiness. It now publishes only the first `truncated=false → true` transition. Source totals remain exact, without a second worker/scheduler, stored event log, or repeated wakeups for discarded chunks.
- **Failure visibility preserved:** `OutputWriter::append` now also signals on an underlying write error that retains no bytes; that failure may occur before a separate `mark_failed`/EOF publication. First failed-drain truncation and final writer EOF still notify; neither creates delivery or ACK proof.
- **Red/green tests:** a failed open writer with 3 retained bytes counted sixteen 5-byte drained chunks. The regression initially failed on repeated notifications; it now observes one first-truncation wake, none for the next 15 chunks, `retained=3, total=83`, EOF wake and denied unproven ACK. A separate internal-spool-failure test verifies an immediate failed append wakes before EOF without producing data or delivery credit.
- **Qualification:** `aw-tools --all-features --lib` **334 passed**, `aw-tools-adapter --all-features --lib` **249 passed**, minimal/non-async shared build and strict affected-crate Clippy `-D warnings`, MCP and minimal/full CLI builds, rustfmt/diff checks green. Read-only shared tests previously passed **110** (unchanged surface). One combined validation command was externally terminated before full CLI/fmt; those gates were rerun individually and passed.
- **Cross-repo boundary:** this is pending AW shared-watcher source, not present in dotlink's pinned AW revision. D0 signed reviewed AW commit, then dotlink repin/host qualification; no commit, push, version change or new worktree.

## Unknown-journal mismatch watch checkpoint — 2026-11-17 (uncommitted)

- **Shared lost notification:** when a persisted transition returns but its `transition_inflight` no longer matches the proposal, `JobManager::commit_transition` correctly fences the Job as journal-unknown; the branch previously did **not** publish to the bounded Job watch. It now signals alongside clearing in-flight state, matching the other existing unknown-journal branches. No journal outcome, permit, replay identity or effect is changed.
- **Red/green:** a reentrant test callback clears the matching proposal while the host journal callback is outside the manager lock. Its subsequent `claim` correctly returns `JournalUnknown` and the Job is ambiguous, but a previously subscribed watcher failed to observe the transition before the fix. Now the watch advances, with no claimed settlement or replay authority. This verifies **notification**, not immediate termination of finite settle-or-deadline waits: explicit unknown-state presentation/reconciliation is a separate owner/host policy boundary.
- **Executed:** `aw-tools --all-features --lib` **335 passed**, `aw-tools-adapter --all-features --lib` **249 passed**, read-only **110 passed**; minimal non-async build; strict affected-crate Clippy, format/diff checks, MCP/minimal/full CLI builds green.
- **Blocked for dotlink:** remains uncommitted until reviewed signed AW shared-runtime checkpoint; dotlink pin unchanged. No commit, push, release, new worktree or dependency repin.

## Journal-unknown finite Job status and wait checkpoint — 2026-11-17 (uncommitted)

- **Truthful shared status:** `Snapshot.journal_unknown` now carries the existing `Job::journal_ambiguous()` fact without changing `Lifecycle`, node settlement, permits, or replay. `Snapshot::status_label()` and Job/list/status headers show `unknown`, not ordinary `running`, when the durable journal is fenced. The shared Task projection already used this exact uncertainty source.
- **Typed receipts:** the shared `Receipt::JobStatus` renders `unknown ... journal=unknown`, never `settled` for an unresolved live journal. `Receipt::NodeStatus` retains independently known pending/running/terminal node facts and adds `journal=unknown` for the parent Journal fence. It does not overwrite a settled node's genuine outcome. Existing normal status receipts remain unchanged.
- **Finite waits:** AW `jobs_get(wait_ms>0)` subscribes first and returns early when the Job journal becomes unknown as an **observation boundary**, not as execution terminality. `jobs_read(wait_ms>0)` may return actual ready retained bytes; if the requested bytes remain unavailable after the journal fence, it fails promptly with a reconciliation conflict rather than waiting to deadline or fabricating EOF. `wait_ms=0` retains nonblocking raw read behavior, including current payload/Actor authorization and exact delivery accounting.
- **Red/green + live:** existing shared `unknown_claim_fences_job_and_keeps_global_permit_reserved` test failed when the Job remained labeled `running`; now asserts `unknown` while `Lifecycle::Running` and the active permit remain preserved. New typed Job/node receipt regression verifies status markers. Live Bash background Process test injects an `OutcomeUnknown` journal cancel via the original JobManager, then verifies both get and unreadable-stream read return within 350 ms despite `wait_ms=1200`, with no claim of Process settlement or output delivery.
- **Qualification:** `aw-tools --all-features --lib` **336 passed**, `aw-tools-adapter --all-features --lib` **250 passed**, shared read-only **110 passed**; minimal/no-default-features build, strict affected-crate Clippy `-D warnings`, formatting/diff, MCP/minimal/full CLI host compilation green. One grouped host check was externally terminated at full CLI; isolated rerun passed.
- **Limits:** journal-unknown is not a terminal action, resolved source authority, transport receipt, or successful recovery. This changes AW shared snapshot + finite tool receipt/read observation only; dependent dotlink is still on the older committed AW pin. No commit, push, version, dependency repin, or reserved UX modification.

## Journal-fenced output mutation checkpoint — 2026-11-17 (uncommitted)

- **Retention correctness:** shared `JobManager::acknowledge_delivered_output`, trusted `acknowledge_output`, and `compact_output_for_owner` could still physically release retained bytes after a Job's persisted transition became journal-unknown, bypassing the existing unknown guard on terminal `finish_output_for_owner`. This could erase retained evidence before host reconciliation.
- **One atomic shared guard:** all three use `with_known_output_mutation` holding the existing **manager → output** locks through journal preflight and the exact owner mutation/compaction, with no extra scheduler, owner ledger, lock or async transaction. `journal_ambiguous` returns `ReplayUnknown`; an unresolved `transition_inflight` returns `Busy`. A successful committed transition retains the prior exact-owner, contiguous delivered-prefix, idempotent and multi-consumer semantics. Read/producer drain and final projection remain separate.
- **Red/green regression:** claimed Process with a sealed six-byte stdout spool and valid owner delivery proof; an external Start persistence callback first attempts all three destructive calls while the journal transition is in flight (**Busy**, byte count unchanged), then returns `OutcomeUnknown`. Calls after the fence return `ReplayUnknown`, owner `through=0`, `acknowledged=false`, `released=false`, spool still holds all six bytes. The pre-fix code returned successful ACK/release and successful compaction; focused regression now passes.
- **Executed:** `aw-tools --all-features --lib` **337 passed**, `aw-tools-adapter --all-features --lib` **250 passed**, read-only shared **110 passed**, minimal no-default-features build, strict affected-crate Clippy `-D warnings`, format/diff, MCP/minimal/full CLI builds green. A combined gate was externally terminated after shared tests; isolated adapter/read-only/lint rerun passed.
- **Scope:** blocks **new destructive output ownership changes** once journal ambiguity or an in-flight persistence outcome exists; prior valid ACKs/cleanup cannot be retroactively undone. This is still uncommitted AW shared source, not adopted by dotlink's older AW pin. No commit, push, version, release, new worktree or repin.

## Journal-unknown terminal retention checkpoint — 2026-11-17 (uncommitted)

- **Fixed expiry/eviction bypass:** `JobManager::expire_terminal` and automatic exposed-capacity `retention_releasable` had checked terminal lifecycle, in-flight transition, quiescent output and replay owner but **not** an unresolved journal fence. A terminal-looking, journal-unknown Job could therefore lose the history needed for reconciliation, even if it held no output bytes.
- **Shared fail-closed rule:** journal-unknown terminal Jobs are never explicit-expiry candidates and never automatic capacity-prune candidates. Other known safe terminal history may still be reclaimed; if the remaining slots are exclusively uncertain, new admission returns `Capacity` without removing the Job. Existing retention/clock and replay owner checks are unchanged; no new GC queue or state.
- **Red/green:** synthesized terminal Job with its original `Job::fence_journal_unknown()` fact was accepted by explicit expiry before the fix. After the fix, expiry returns `Busy`; pressure with one known terminal reclaims **only the known** candidate, and pressure with only the uncertain terminal returns `Capacity` while retaining its status. No replay record or output is fabricated.
- **Qualified:** `aw-tools --all-features --lib` **338 passed**, adapter **250 passed**, read-only shared **110 passed**, minimal/non-async build, strict affected-crate Clippy `-D warnings`, MCP/minimal/full CLI compilation, formatting and diff checks all green.
- **Follow-up separate:** review whether terminal-journal ambiguity needs extra protections at restart-source eligibility and host nonterminal-work summaries. This checkpoint changes expiry/pressure only. Dotlink remains on the prior committed AW pin; no commit, push, tag, release, version, dependency repin or new worktree.

## Journal-uncertainty lifecycle commit-readiness review — 2026-11-17 (uncommitted)

- **Fresh read-only review:** re-audited the pending AW shared runtime across journal uncertainty, retention, output ownership, ACK/Finish, event-driven waits, Toolset metadata, typed receipts and grep schema/parser/execution semantics. Existing owner delivery/ACK/Finish fences, watch subscribe-before-check behavior, receipt uncertainty markers, Toolset metadata-only hooks and grep context/paging contracts remain internally consistent; no additional correctness defect was found in those surfaces.
- **Genuine lifecycle defect fixed in the shared layer:** several lifecycle-only consumers still treated `Lifecycle::Terminal` or “all nodes locally Pending” as sufficient despite `Job::journal_ambiguous()`. A terminal journal-unknown Job could be accepted as a restart source and omitted from `has_nonterminal_jobs`; an `OutcomeUnknown` Claim could still satisfy `discard_unstarted` even though its maybe-committed effect intentionally retained the global permit; hidden terminal cleanup could likewise erase uncertain history. Restart now returns `ReplayUnknown`, host live-work summaries include journal ambiguity/in-flight transitions, and both optimistic cleanup paths fail closed while ambiguity remains.
- **Host cleanup ordering:** `JobHost` now commits shared-manager removal before deleting finite source/terminal event bookkeeping. If the manager refuses uncertain cleanup, the adapter retains its reconciliation metadata rather than partially cleaning host state first. No scheduler, owner model, replay ledger, output format or public tool surface changed.
- **Regression evidence:** the terminal-uncertainty test now proves live-work reporting, restart refusal with no new Job, expiry/capacity protection and hidden-terminal cleanup refusal. A new real `JournalError::OutcomeUnknown` Claim regression proves the Job cannot be discarded as “unstarted”, remains journal-unknown, and keeps its reserved global permit. Focused tests passed.
- **Full qualification:** shared Job-runtime feature set **200/200**, full `aw-tools --all-features --lib` **339/339**, read-only shared **110/110**, and `aw-tools-adapter --all-features --lib` **250/250**. Strict all-target/all-feature Clippy with `-D warnings` passed independently for both affected crates; `cargo fmt --check` and scoped `git diff --check` passed. Minimal `aw-tools --no-default-features`, `aw-mcp`, minimal `aw-cli --no-default-features --lib`, and full `aw-cli --lib` compilation all passed. Several earlier grouped compile/Clippy attempts were externally terminated without Rust diagnostics; isolated low-parallelism reruns completed green.
- **Boundary:** this closes the explicit restart/nonterminal-summary follow-up from the preceding retention checkpoint and also hardens the adjacent optimistic cleanup paths exposed by the same invariant. It does **not** constitute a signed AW checkpoint or authorize dotlink adoption. Dotlink remains pinned to `0a6c6a0`; no commit, push, tag, release, version change, dependency repin, new worktree or reserved UX edit was performed.

## Known-terminal publication and handoff review — 2026-11-17 (uncommitted)

- **Fresh continuation review:** traced every remaining terminal-shaped consumer reachable from the shared/adapter diff after the lifecycle fence: aggregate Graph rendering, synchronous Process/Graph inline completion, finite `result/settled` events, restart replay preflight, execution-scope resolution and optimistic hidden-Job retirement.
- **Shared publication fix:** `Snapshot::is_known_terminal()` centralizes the factual boundary `Lifecycle::Terminal && !journal_unknown`. `JobManager::render_graph_result` now returns `ReplayUnknown` before rendering any aggregate result from a journal-fenced terminal projection; finite Graph-root `jobs_read` maps that to a reconciliation conflict instead of a generic protocol failure.
- **Adapter fail-closed behavior:** finite terminal events commit only for a known terminal source; restart rejects journal-unknown source state before Graph planning/replay lookup; runner execution scopes mark journal uncertainty unresolved even if node summaries are otherwise known. Synchronous JobShell/Batch completion exposes and returns the same typed unknown handoff receipt rather than inline success when the terminal journal is uncertain.
- **No-lost-handoff ordering:** optimistic inline retirement now performs shared manager removal first and only then disarms `HandoffGuard`. Any cleanup refusal leaves the guard armed; Drop exposes the exact same Job. This complements the prior manager-first source/event cleanup ordering and prevents a fail-closed retention decision from turning an uncertain Job into unreachable state.
- **Regression evidence:** the terminal-journal shared test now also proves `is_known_terminal=false` and aggregate Graph rendering returns `ReplayUnknown`. New adapter regression creates a hidden running Job, forces inline retirement to fail, drops the still-armed guard and verifies that same Job becomes exposed. Existing normal terminal-event, inline Process and inline Graph tests remain green.
- **Qualified:** `aw-tools --all-features --lib` **339/339**, `aw-tools-adapter --all-features --lib` **251/251**, read-only shared **110/110**. Strict all-target/all-feature Clippy `-D warnings` passes independently for both affected crates; `cargo fmt --check` and scoped `git diff --check` pass. Minimal `aw-tools --no-default-features`, `aw-mcp`, minimal `aw-cli --no-default-features --lib`, and full `aw-cli --lib` all compile. One initial grouped full-suite attempt was externally terminated during shared compilation without a Rust diagnostic; isolated low-parallelism reruns completed green.
- **Boundary:** no new scheduler, recovery engine, owner ledger, event kind, public tool/schema or automatic reconciliation. Journal uncertainty remains owner-reconciled rather than guessed. Dotlink is still on `0a6c6a0`; no commit, push, dependency repin, release, new worktree or reserved UX edit.

## Strict delivered-ACK replay after owner compaction — 2026-11-17 (`26a8c78`)

- **D1 integration exposed a shared replay defect:** dotlink switched its shared `jobs_read`/Through-ACK path from a temporary host `delivered_through` map to canonical `output_page_for_owner → record_delivered_output → acknowledge_delivered_output`. Its existing independent native MCP Task regression then reproduced a lost-response case: the Job/model owner had already received and ACKed stdout through byte 8, the Task owner legitimately compacted the physical retained prefix to byte 4, and replaying the same model ACK through 8 incorrectly returned `InvalidOutput`.
- **Root cause and shared fix:** strict `acknowledge_delivered_output` validated `through <= current retained bytes` before recognizing that the exact owner had already committed the same logical high-water mark. Reorder the checks so exact owner + recorded delivery proof are validated first; an already-acknowledged `through <= owner.through` is an idempotent no-op even after partial compaction. Only a **new** ACK advance is required to remain within the currently retained prefix. This matches the existing trusted-ACK invariant without weakening public delivery proof.
- **Regression upgraded at the source:** `owner_aware_compaction_requires_every_other_owner_to_release_the_tail` now records exact delivery for the Job owner, uses strict `acknowledge_delivered_output`, compacts to the Task owner's four-byte retained prefix, and proves replay of the Job owner's prior byte-8 ACK succeeds unchanged. The same test still proves the Task owner cannot newly acknowledge byte 5 after compaction and can release only its retained byte-4 prefix.
- **Qualified:** focused red/green regression passes; full `aw-tools --all-features --lib` **339/339** and minimal Job-runtime `batch,durable` **200/200** pass. Strict all-target/all-feature `aw-tools` Clippy with `-D warnings`, `cargo fmt --check`, and scoped `git diff --check` are green.
- **Cross-repo boundary:** this is a tiny shared semantic correction discovered by correct dotlink D1 delegation, signed in `26a8c78`. Do **not** restore a dotlink-side ACK exception or path-patch AW. Dotlink's D1 source migration remains pending remote availability plus the normal all-three-dependency repin. No version, release, new worktree or reserved UX edit was performed in this checkpoint.

## Tests and verification

- [x] Output+ACK/owner/Graph tests: exact prefix, multi-stream Finish, replay and limits.
- [x] Get/read/offset/status and authority tests, including live settle-only
  `jobs_get`, byte/EOF-aware finite `jobs_read` and negative-offset peeks.
- [x] Grep schema/parser/execution: 0/8/128/129 asymmetric, file/dir, caps/incomplete.
- [x] `cargo fmt --check` and strict targeted Clippy with all relevant features.
- [x] Targeted `aw-tools`, `aw-tools-adapter` and Agent-core tests plus
  minimal `aw-tools` build.
- [x] Reserved UX subtree untouched; no push/release performed.

## Post-merge review — 2026-11-16

- [x] Fresh review found no new authority/runtime split: retained output
  disclosure still requires independent current `jobs_read` + Actor authority,
  and finite ACK/Finish still linearizes through ToolBindingAuthority → Actor
  runtime → JobManager.
- [x] Current merged suites are green: `aw-tools` **327/327**,
  `aw-tools-adapter` **242/242**, Agent core **90 passed / 1 ignored**,
  minimal/read-only shared-tool checks and strict affected-crate Clippy.
- [x] Review identified the old 10 ms `jobs_get` revision polling and
  rejection of positive `jobs_read.wait_ms`. Resolved by the uncommitted
  event-driven checkpoint below; this is no longer an open semantic gap.

## Post-adoption AW merge review — 2026-11-17

- [x] Revalidated current published AW `a05d5c8` plus the shared pins
  `be6d48d` / `26a8c78` against the live JobManager, AW adapter and the
  current dotlink consumer. Event-driven finite waits, journal-unknown
  publication/retention fences, strict delivered-ACK replay after compaction,
  Task-owner eligibility and direct/Graph shared boundaries remain coherent.
- [x] Fresh current-HEAD baseline before this review: `aw-tools` **339/339**,
  `aw-tools-adapter` **251/251**, Agent core **90 passed / 1 ignored**,
  read-only shared **110/110**, minimal shared build, strict affected-crate
  all-target/all-feature Clippy, `aw-mcp`, `aw-host`, `aw-cli --lib` and
  rustfmt all green.
- [x] **New shared lifecycle defects reproduced red/green:** a Job could be
  locally terminal while an attached retained `OutputWriter` was still live.
  Both `discard_unexposed_terminal` and automatic exposed-capacity pruning
  could then remove its manager namespace (the latter when the live stream still
  retained zero bytes). That differed from `expire_terminal` and could leave a
  live writer capability detached from its Job record. Hidden retirement and
  automatic reclamation now both require every attached writer to be finished.
  Each focused regression fails on the prior code and passes after the
  shared-manager fix.
- [x] The fixes stay in canonical `aw-tools`; no AW/dotlink adapter special
  case, new owner ledger, scheduler, replay rule or public schema is added.
  Normal AW and dotlink settlement order already quiesces writers first, so the
  change is a fail-closed invariant for out-of-order/future hosts. Explicit
  retention expiry, automatic capacity pruning and optimistic hidden retirement
  now agree on writer quiescence.
- [x] The same writer-lifetime fact now participates in
  `has_nonterminal_jobs()`, because that predicate gates AW parallel-policy
  changes and Session scope replacement. A known terminal execution may still
  drain output, but the runtime remains live until its retained writers finish;
  this does not redefine `Snapshot::is_known_terminal()`.
- [x] Ready replay records intentionally remain fail-closed if a successful
  inline hidden Job is retired: a duplicate with no retained runtime Job becomes
  `Unknown`, never a redispatch. Task binding likewise is not itself a second
  retained-output lease; only explicitly attached/enrolled owners pin output.
- [x] **Final post-fix qualification:** `aw-tools --all-features --lib`
  **341/341**, `aw-tools-adapter --all-features --lib` **251/251**, Agent core
  **90 passed / 1 ignored**, read-only shared **110/110**, minimal
  `aw-tools --no-default-features`, strict affected-crate
  all-target/all-feature Clippy with `-D warnings`, `aw-mcp`, `aw-host`,
  `aw-cli --lib`, and rustfmt all green.
- [x] `beef1bc` is published on AW `main`; dotlink repinned all three shared
  dependencies/lock entries to that exact SHA and requalified its current D1–D4
  tree: all-feature binary **684 passed / 1 ignored**, integrations **36/36**,
  locked all-target/all-feature check, strict Clippy, fmt and diff checks green.
