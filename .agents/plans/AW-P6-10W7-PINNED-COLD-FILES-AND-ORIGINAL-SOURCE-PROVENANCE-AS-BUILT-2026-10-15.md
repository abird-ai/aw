# aw — AW-P6-10W7 original cold filesystem anchors and source evidence (15 October 2026)

**Status:** W7 implemented locally; original immutable cold State remains non-runnable. **No permission to activate.**
**Verified source starting point:** `fbd0c421816fa107620e08fb685f74828387ace3` (signed UX v1.5 forward-only correction, with all original source files restored); original W6 `88c2d6c9c1888cd47d476e7338a0698f9940f034`. Actual source and signed HEAD must be checked again before further work. Original correct author/signing key preserved.

## 1. Scope, design, and explicit invariants

AW-P6-10W7 continues W1–W6's **internal-only non-executing effect-inspection PREPARE** over the original v33 SQLite journal. There are still exactly FOUR Runtime primitives (`Runtime`, `Actor`, `Message`, `Envelope`) and TWO Hook verbs (`Next`, `Defer`). Original `Agent`, `TurnMachine`, `ToolPolicy`, `ToolRegistry`, immutable `TurnJournal` and owner/Host custody remain sole execution boundaries. No second executor, duplicated receipt ledger, schema change, API promotion, restored-State runnable activation, copied writer grant, Process resurrection, automatic provider/Tool/OS effect retry, automatic Queue claim or invoice settlement. `aw` 0.15.0, SQLite v33, JSONL v1 unchanged. E6 parked, Phase69 unauthorized, phases64/65 intentionally skipped. UX plan v1.4 and additive v1.5 separate; the other agent exclusively owns `.agents/plans/ux/` and its staged entries and was not accessed.

## 2. Actual W7 source changes

### Stable bounded original file identity during read inspection

`crates/aw-store/src/cold_effect_reconciliation.rs` adds private Linux `ColdFilesystemAnchors`, `AnchoredColdFile` and `ColdFileIdentity`: retain opened descriptors for the original cold State directory, `sessions.sqlite3` and `.aw-cold-restore-pending` quarantine marker. Each anchor records original Linux dev/inode/size, mode/uid/gid, mtime and ctime (seconds and nanoseconds), and verifies both the still-held original descriptor and a freshly platform-validated re-open of the **same path**. The wrapper first calls existing R/S/T `verify_cold_for_inspection`, anchors files, performs original W6 SQLite DEFERRED read-only snapshot, compares pinned original identity **before** and **after** the original post-read archive/AEAD/two-quarantine verification, and rejects any mismatched identity. This detects same-byte inode replacement and many in-place writes that a pure content check would not detect. It does NOT pretend to be an unbreakable system-wide filesystem transaction against a malicious privileged local OS user.

`crates/aw-store/src/composite_staging.rs` exposes the original quarantine filename as a **crate-private constant**, not a public API, so the detector uses the existing original marker spelling rather than another shadow name. Existing original cold restore/SessionStore dual denial logic was not changed.

### Exact transaction-level original SQL denial and source evidence

`collect_single_snapshot` now requires the exact original published `store_id`, strict original decimal `restore_generation`, and original `cold_restore_quarantine` identity all **within the same pinned v33 SQLite read transaction** as original T's counts and W2–W4 exact evidence. A copied Store ID or SQL marker with the correct number of risks is rejected. All returned classifications still have `runnable=false`, `permits_original_effect_replay=false`, and require independent human origin and cross-clone exclusion.

Original Queue/Actor evidence preserves exact immutable sender, recipient site/target, message and Turn IDs, plus the original claimed Queue owner Process ID, Store generation, recipient Actor and writer epoch when present. Missing claim, mismatched writer, missing owner Process or zero generation fail closed. Original Actor Control owner Process/generation remains optional when historical facts lack it, but contradictory half-present/invalid evidence is denied. The inspection treats original Actor and Source labels as historical data, **never live identity or grants**.

Original Tool anomalous journal evidence now retains exact original `ToolSourceEvidence` (Host/MCP/Extension source kind, ID and epoch, if journaled) and original execution site. Sources are validated with the original `ToolSourceEvidence::validate` predicate. Legacy missing source remains explicitly None; no source is fabricated. A `ToolFinished` without a corresponding `ToolStarted` remains a **mismatched inspection obligation**, not a fake start or a zero-effect success. Contradictory per-call sites/source attribution fail closed.

### Original frozen handoff checkers, forward-only exemptions

The Oct9/Oct11 SHA integrity scripts explicitly classify W1–W7 new plans and the user-approved standalone UX v1.5 plan as **POST-snapshot authorized additions**. The Oct9/Oct11 historical frozen master and source-manifest bytes are not regenerated or edited. No independent UX-owned subtree is scanned or modified by this change; the removed generate_aw_p6_handoff_20261011 helper excludes forward files from its old Oct11 snapshot inventory. the removed test_comprehensive_handoff_20261009 helper extends only its original approved-new-plan set. Keep the exact exclusions up to date for future forward as-builts.

## 3. Native negative-first investigations, actual observed validations

First added real R/S/T/owner-backed W7 cold fixtures: build an actual original SQLite Store, enqueue original encrypted future Queue Input, capture+publish complete original composite, then cold-restore into a genuinely new owner-private, doubly quarantined State. The initial negative-first tests could not compile before the new test-injection hook and identity guard; after implementation the tests verified source naming strictness, source-independent restore, no runnable activation and rejection of **same-byte replacement** of (a) quarantine marker and (b) SQLite database during the snapshot-to-postcheck gap. Scoped test fixtures exist only under native ephemeral temp directories, with no commercial key or billing.

Added original claimed Queue native Process owner/generation/epoch projection assertions and synthetic negative Queue evidence for absent owner, incorrect writer epoch, missing claim. Extended the actual original T unmatched Tool fixture to assert both factual anomalous Tool identities and preserved original Host source/site, with no missing-start fabrication; a first stricter finished-before-start rejection regressed legitimate risk inspection and was corrected before full validation. W6 count+exact-identity WAL test remains green. Original Store SQL-marker/store-ID tampering is rejected even if generation and risk counts agree.

**Native validation completed at this checkpoint:**

| Gate | Measured outcome |
| --- | --- |
| W7 focused negative native tests | 4/4 PASS (real cold restore, same-content marker/SQLite replacement, Queue owner/WriterEpoch mismatch) |
| Original T unpaired Tool / W6 WAL | PASS (real historical malformed Tool evidence; competing second SQLite connection) |
| Full Store library | **152/152 PASS** |
| Store integrations | context_flow 29, resources 23, store 88, composite_design 7, custody_inventory 9: **156/156 PASS** |
| Complete SQLite Store | **308/308 PASS** across all six targets |
| Original CLI | library **169**, binary **274**, mock HTTP-provider **6** = **449/449 PASS** |
| Minimal CLI | library **25**, binary **98**, HTTP-provider **6**, plus build check PASS (unrelated pre-existing dead-code warning) |
| Stateless CLI (`--no-default-features`) | library **25**, binary **95**, HTTP-provider **6**, plus build check PASS (same warning) |
| Portable kernel | **31/31 PASS**, separate `--no-default-features --features alloc` build PASS |
| Whole workspace | `cargo check --locked --workspace --all-targets --quiet` PASS; strict `cargo clippy --locked --workspace --all-targets -- -D warnings` PASS |
| Original signed native transport | local Actor RPC **9/9**, live Control **6/6**, signed localhost paired QUIC **9/9** |
| Other compiled CLI native smokes | R12 headless **20/20**, R9 model-selected parallelism **11/11**, R14 canonical CLI full **11/11** |
| Original portable consumers | E3 actual **zero-import wasm32v1-none** executed in Node; E4 native x86-64 libc/CRT-free debug and release executables both ran; E5 separate C11 debug/release executables each passed **710 native assertions**, 11 ABI exports |
| Source/handoff | All **nine** Oct09/Oct11/Oct12/Oct14 read-only historical + latest SHA/checker commands PASS after adding explicit forward-only exclusion/approved-plan entries. Frozen handoff/master/manifests unchanged. |

The native checks use Rust 1.95.0 on Linux x86-64 with original local SSE mock provider, real local OS subprocess/SQLite/QUIC and independent C11 execution. They are NOT commercial provider billing settlement, hostile-public-Internet certification, multi-machine authority uniqueness, signed five-platform release, pinned Rust 1.98.1 release validation or bare-metal E6 firmware. E4 **debug** additionally executed explicit negative and panic-abort controls; the **release** invocation ran the positive libc/CRT-free path and did not report the debug-only negative controls. None of the new W7 read-only behavior is an executable activation path.

## 4. Unresolved boundaries; no activation

The original post-read filesystem check is deliberately **not** a global linearizable filesystem+SQLite snapshot. A privileged local writer able to swap all paths between descriptor probes, fake trusted public source manifests or compromise the original machine/root is outside this proof. SQLite's original v33 read snapshot is stable under concurrent WAL, but real OS crash, power loss, mutable external API charges and off-host clone authority remain unproven. SourceKind/ID/epoch/site in *historical facts* does not authorize new provider or Tool calls. Claiming the same original Process ID or copied WriterEpoch is never a current grant. Copied U signatures and V dry-run records do not prove independently initiating-human presence, nonce uniqueness or source-owner authorization. Original R/S/T dual guards continue denying `SessionStore::open`/Queue/Agent/Tool execution. No per-effect disposition write has been created in W7; original hazards and uncertain charges stay intact.

## 5. Recommended next scoped W8

Start from actual live signed HEAD. Design/implement a **private owner-only, non-executing, source-grounded reconciliation review request** using original append-only v33 audit, with exact original Run/Provider/Tool/Queue/Actor/Process provenance from W7, and repeat archive+SQL authority checks **inside `BEGIN IMMEDIATE`** before any immutable audit write. Negative-first tests must distinguish KEEP QUARANTINED from ABANDON AS A SEPARATE FUTURE PROPOSAL; neither may mutate original source effect state, mark charges zero or allow replay. If independent real initiating-human attestation and global cross-clone uniqueness are unavailable, keep review only and explicitly block runnable activation. Do not add Phase69, UI/UX changes, secret exposure, OS signal or paid provider retry. Stage only reviewed owned files in an isolated index, preserve every opaque unrelated shared-index entry, SSH-verify and expected-parent CAS; no push/tag/release.
