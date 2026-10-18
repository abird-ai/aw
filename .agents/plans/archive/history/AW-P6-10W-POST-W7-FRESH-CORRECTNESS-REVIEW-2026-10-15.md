# AW-P6 — Fresh correctness and architecture review after W7 (15 October 2026)

**Status:** Actual live source reviewed, one native-reproduced correctness bug fixed, redundant private inspection routes removed; remaining blockers catalogued below. Review began at signed HEAD `f8be63d2ad6ad7a32e305b77be80ea55aa39faa1`. Original author/signature and existing history must be preserved. The other agent exclusively owns `.agents/plans/ux/`, never inspect or stage it.

## Absolute project invariants

There are exactly FOUR Runtime primitives: Runtime, Actor, Message, Envelope, and exactly TWO Hook verbs: Next, Defer. One original Agent, TurnMachine, ToolPolicy, ToolRegistry and immutable TurnJournal remain execution/settlement authority. Original Domain/Workspace/Session/branch/Turn, WriterEpoch, Actor generation, Process incarnation, SourceKind/ID/epoch/site and provider payer/credential custody remain hard requirements. SQLite v33, JSONL v1, aw 0.15.0. Phase69 unauthorized; phases64/65 skipped; E6 parked. **No runnable restored-State activation**, model self-grants, automatic paid/Tool/OS/Queue replay or zero-cost settlement.

## Last 10 commits: independent source review

| Commit | Scope | Result |
| --- | --- | --- |
| `f8be63d` W7 | Original filesystem file/inode/marker anchors; Tool/Actor/Process provenance | Real S/T cold native inspections and same-byte SQLite/marker replacement tests pass. Root directory metadata also detects tested transient same-file swap-back. Not a root-host filesystem certificate. |
| `fbd0c42` UX tree repair | Forward restore of previous accidental candidate deletions | Original repository tree restored; signed commit should be preserved. |
| `cd0a297` UX v1.5 | New Herdr-inspired additive design | **HIGH process finding:** intermediate signed commit deleted 1,488 unrelated tracked files. Restored by next commit, but breaks bisectability/historical checkout. NEVER rewrite signed history silently; enforce whole candidate-tree changed-path allowlist in commit automation. |
| `88c2d6c` W6 | Single SQLite snapshot for original T risk and exact W2–W4 identities | True two-connection WAL race test verifies same-snapshot counts/IDs. |
| `5106d0c` W5 | Initial read snapshot matching previous external risk counts | Superseded by W6's more coherent in-transaction original T inspection. |
| `ba41a1e` W4 | Execution leases/recovery hints/retries | Retains original keys, Process holder, generation, no execution. |
| `200fdbf` W3 | Tool/Queue/Actor original identities | Preserves uncertain claimed/accepted Queue, original historical actor context. Older T count-only Tool matching defect found in this review. |
| `a7fbf78` W2 | Provider attempt/Run/hazard original IDs | Correct identity inspection, not external billing proof. |
| `5cc2f67` W1 | Fourteen unresolved-effect classification categories | Appropriate non-executing model; obsolete private wrappers cleaned up in this review. |
| `0b8b4ca` Oct14 master | Complete historical handoff/checkers | Frozen source checks intact; forward doc allowlists currently updated per phase. |

Supporting S/T/U/V modules inspected: S leaves restored State doubly quarantined; T source-independent inspection and original review request; U original Ed25519 device-key evidence only; V transactionally reverified *non-runnable* dry run. Neither U/V establishes independent live human presence, cross-clone nonce uniqueness, or activation authority.

## REVIEW-01 — HIGH, confirmed and repaired: fake same-ID Tool completion

Prior original T `inspect_original_cold_risks_on_connection` matched `ToolStarted` and `ToolFinished` by only (Run, call.id) counts. In synthetic original SQLite evidence, same call ID with a **different Tool effect** led to **zero outstanding risk**. This contradicted the original immutable TurnJournal intent-to-terminal matching rule and could mislead a future U/V review of copied SQL.

**Negative first:** native test `review_cannot_clear_original_tool_risk_with_mismatched_same_id_terminal` failed on `wrong_effect` before remediation.

**As-built fix:** T now validates event kind/payload, original ToolCall envelope, optional ToolSourceEvidence (kind/ID/epoch), genuine running start/no output, terminal non-running/non-denied finish, exact call arguments/name/effect/site equality, and original receipt ordering. Contradictory pair fails closed. Legitimate terminal pair remains nonhazardous; orphan ToolFinished stays an unresolved mismatch, never a fabricated started effect. Additional tests cover wrong arguments, wrong site, nonterminal status, reversed order, malformed kind, invalid source epoch and a legitimate original pair.

No old journal rows were rewritten, no second ledger or ToolRegistry implementation introduced, and no restored State opened.

## REVIEW-02 — MEDIUM, repaired: weaker private W1–W4 inspection entrypoints

Four obsolete `prepare_exact_*` wrappers plus the W1 standalone preparation wrapper survived after the stronger W6/W7 unified verification. No current callers, but future code could accidentally integrate them and bypass W7 file anchoring. Removed wrappers, retained one `prepare_consistent_cold_effect_snapshot` entry, original T inspection and typed shared extractors. Fault-injection entrypoints now exist only in `cfg(test)`; production path calls private implementations with inert hooks. This reduces rather than expands available paths.

## REVIEW-03 — MEDIUM, bounded filesystem proof

W7 captures open file descriptors and original dev/inode/uid/gid/mode/size/mtime/ctime for cold directory, SQLite and quarantine marker, with checks around original source/AEAD inspection. The SQLite connection itself is opened by pathname, not through the pinned descriptor/VFS. That is **not a formal malicious-root or adversarial namespace proof**, even though source-independent original validation and descriptor metadata guard work for the tested honest-host/private-Linux scenarios.

A new *real original restored-State* negative test swaps SQLite with identical bytes then swaps the original inode back **before final checks**; the in-flight review is rejected and original cold State remains quarantined. Future W8 must not treat the inspection as a grant. A formal root-host threat model may require stronger Linux fd-bound SQLite and mount namespace policy; avoid premature unsafe VFS rewrites.

## REVIEW-04 — HIGH future-activation prerequisite, unresolved by design

U/V consent binds signed device transcript, source archive manifest SHA and **aggregate risk counts**. A copied original manifest is not a signed globally exclusive provenance certificate, and a signature from a copied local device does not attest current initiating-human presence or live cross-host uniqueness. The original paid provider invoice cannot be inferred from SQL terminal counts. Before *any* runnable restore, require exact per-effect source-grounded reconciliation, separate human-origin authentication and globally unique crash-safe fresh authority, plus first-open no-replay certification. V remains dry-run only.

## REVIEW-05 — MEDIUM further correctness/maintainability

- The W7 Tool and Actor projections keep historical SourceKind/ID/epoch/site, Queue claim Process generation and WriterEpoch as **evidence only**. No old source/actor/process should become a fresh grant.
- Original T/U/V and W7 reopen SQL and reevaluate source/AEAD at multiple points. This is intentionally fail-closed but costlier on large 512-MiB bounded archived State. Future W8 can share one verified typed snapshot only with all original security checks retained; never cache stale authorization.
- Old Oct9/Oct11 source checks require a repetitive explicit exemption for each new forward plan. Prefer a separately versioned, append-only **current** manifest/registry while keeping historical archives immutable.
- Large implementation units remain maintenance risks: `src/resources_cli.rs` (~6.5k), `src/main_full.rs` (~6k), `src/actor_owner.rs` (~5.6k), `crates/aw-store/src/lib.rs` (~5.5k), `src/workspace/ui.rs` (~4.8k), `crates/aw-store/src/composite_staging.rs` (~4.7k), `src/web.rs` (~4.5k). Extract narrow typed pure helpers only when they reduce coupling; do not add parallel state/execution services or undertake a blanket rewrite.
- The independently maintained native Provider/MCP target cache has stale `rustls` build scripts whose ELF interpreter path points to deleted Nix glibc 2.42-84; standalone Cargo tests could not start that old build-script binary. This is a **verified local build artifact/configuration issue**, not established Rust source regression. Isolate and rebuild that target cache later; do not trash concurrently shared Cargo caches.
- UX v1.5 Herdr/Pi style remains DESIGN-ONLY and belongs to the separate UX agent. Existing Agent prompt-band and Tool-tone tests pass, while some Workspace panes still use bordered layouts. Preserve v1.4 and do not access UX-owned plan subtree.

## Actual native validation (review pass)

| Gate | Result |
| --- | --- |
| Full Store (six targets) | Final Store library **156/156** plus context_flow **29**, resources **23**, store **88**, composite_design **7**, custody_inventory **9**: **312/312 PASS** after Tool coherence fixes, inspection simplification and the transient SQLite swap-back regression. |
| Full CLI | library **169/169**, binary **274/274**, HTTP provider local mocks **6/6**: **449/449 PASS**. |
| Recent S/T/U/V/W7 | Original T **6/6**, U **4/4**, V **9/9**, W7 **3/3**, plus new targeted negative Tool tests and real transient filesystem swap regression PASS. |
| Kernel/core/tools | Kernel **31/31**, Agent core **57/57**, Tools **22/22** (requires AW_BASH/AW_GIT) PASS. |
| JSONL/session-format/platform | JSONL lib **5/5**, session format **3/3**, aw-platform-unix library compiled (0 unit tests). |
| Workspace | All-target check PASS; strict all-target Clippy PASS after fixes. |
| External transport/portability | Previous W7 actual local Actor RPC 9/9, live Control 6/6, signed localhost QUIC 9/9, R12 20/20, R9 11/11, R14 11/11, E3 WASM executed, E4 libc-free debug/release executed, E5 C11 debug/release 710 assertions each. **Not re-run specifically for this focused review**. |
| Source integrity | All **nine** Oct9/11/12/14 frozen/current SHA checks PASS with this forward-only review excluded from the historical source inventory; original archive/master/manifest bytes unchanged. |

Actual evidence: Linux x86-64 Rust 1.95.0, local native fixtures/mock paid providers. NOT production cloud billing/security, pinned Rust 1.98.1 five-platform certification, power-loss durability, real human attestation, cross-host uniqueness, or bare-metal E6.

## Prioritized follow-on plan

1. Keep REVIEW-01 terminal matching fix and REVIEW-02 one canonical inspection path under original native regression coverage; do not broaden Tool privileges.
2. Proceed to **AW-P6-10W8** as original-owner private non-executing per-effect reconciliation review *requests* with exact Session/Run/attempt/Tool/Queue/Actor/Process/SourceKind identities and re-verification **inside original BEGIN IMMEDIATE**, plus crash/race native tests. Neither 'keep quarantined' nor 'abandon as future proposal' may erase old billing/effect receipts, claim success, or grant execution.
3. Separately design independent initiating-human presence and cross-clone global grant arbitration; no runnable activation without both and first-open no-replay proof.
4. Later simplify repetitive forward source allowlists and oversized UI/CLI modules only in bounded owner-specific phases. UX v1.5 separately owned; E6 parked.

## Successor instruction

Verify actual signed HEAD and read this review plus original October 14 comprehensive handoff in bounded sections. Work only in reviewed owned paths; do not inspect, stage, reset, stash or commit `.agents/plans/ux/`. Begin AW-P6-10W8 with negative-first native tests. Preserve one original Agent and immutable TurnJournal, four primitives, two Hook verbs, schema v33, JSONL v1 and version 0.15.0. Do not activate restored State or auto-replay old effects. For any commit: PRIVATE GIT_INDEX_FILE initialized from exact parent, stage reviewed paths only, verify the **ENTIRE tree delta**, SSH-verify candidate, preserve unrelated shared staging as opaque data under exclusive index lock, expected-parent HEAD compare-and-swap. No push, tag, release, rebase or rewriting historical signed commits.
