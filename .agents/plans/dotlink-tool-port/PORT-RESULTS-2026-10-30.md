# Dotlink to AW: exact port/adaptation inventory, 2026-10-30

Read-only dotlink source: sibling src/dotlink HEAD 70939fb. AW start: 11bea90.
Last common tooling baseline: dotlink 9c771d2; 16 intervening commits inspected.
AW retains its original durable Agent, exact grants, TurnJournal and
model-directed scheduling. Dotlink remains untouched.

## Complete native-tool accounting (full / adapted / partial / skipped)

| Native surface | Port status | Post-sync functionality now in AW | Remaining difference |
| --- | --- | --- | --- |
| `ls` | Fully adapted | Recursive glob/depth/kind/sort/limit, gitignore/hidden controls, bounded top-N discovery, all-cause incomplete JSON, pinned-root traversal | Dotlink discovery lives in its own MCP transport; AW retains original ToolPolicy |
| `grep` | Fully adapted | Literal/regex, glob, context/paging/files/count, bounded 32 MiB/4,096 scans, no match-index materialization, quoted unsafe path labels, all stop reasons, binary/ignore rejection | Dotlink's transport wire-size budget is enforced by its MCP host; AW uses Agent result ceiling |
| `read` | Fully shared algorithm | One sync bounded line-window/CRLF/numbering/cancel implementation used by both Unix and Windows roots | File authority remains with AW platform |
| `write` | Fully adapted | Atomic conditional create/replace, SHA-256 preconditions, no-op receipts, pinned path, metadata preservation | Dotlink and AW may use different host-level overwrite grants |
| `edit` | Fully shared planner / AW publication | 512 ordered edits / 64 paths; 64 MiB original+working cap, checked expansion, in-place shrinking replacements, exact match + ambiguity, bounded receipts | AW retains 48 KiB request and 1 MiB per-file ceilings; actual commit remains AW platform |
| `patch` | Fully shared grammar / AW publication | Shared Codex Add/Update, KMP hunk matching, mixed CRLF/LF/EOF, ambiguity rejection, 64 MiB aggregate planning, bounded receipts | Per-file atomic, not multi-file transaction; deletes/moves remain refused |
| `bread` | Partially compatible | Explicit base64/hex byte windows and `format=mcp` MIME-labeled encoded envelope | AW ToolOutput still lacks true typed MCP ContentBlock media |
| `bwrite` | Fully adapted | Atomic binary create/replace, optional create-only and digest precondition | Host grants and publication ownership remain AW |
| `bedit` | Fully shared algorithm / AW staging | SHA-256 original-relative bounded streaming splices, deletions/insertions, no-op checks, cancellation, Unix + Windows stage stream APIs | Native platform IO and commit logic remain host-owned |
| `bash` / `powershell` | Partially adapted | All native parameters, bounded stdin/time/output, Process owner, model-directed parallel hints and explicit trusted permissions | Dotlink Bubblewrap, durable async job fallback and MCP progress/Tasks not installed in AW |
| `job_start/list/get/output/ack/cancel` | Shared policy/interface only | Task quotas, retention limits, monotonic ACK, sealed spool writer leases and journal/host interface | No model-facing job tools or duplicate AW Task/Session owner introduced |
| Tool authorization / scheduling | Intentionally AW-owned | Existing ToolRegistry, ToolPolicy, exact source/site/effect grants, TurnJournal, serial mutation barriers and model-directed read/Process parallelism preserved | Not imported from dotlink |
| Task durability | Opt-in in shared library; AW remains durable by default | `async` and `durable` features selected in AW tool adapter; sync is the common default | Real durable job IO/storage remains with the original AW owner |
| Optional sandbox | Interface only | Optional `sandbox` feature exposes a host-supplied policy contract | No new Bubblewrap mount/network implementation or authorization |

## Fully ported into reusable synchronous algorithms and AW adapters

| Capability | Source/implementation | AW disposition |
| --- | --- | --- |
| Linear matching | Dotlink mcp/tools/matching.rs KMP | Shared aw-tools-core/matching.rs; matcher never collects all positions |
| Patch grammar/hunks | Dotlink patch + AW's existing Codex semantics | Shared aw-tools-core/patch.rs, bounded files/hunks, repeated-context ambiguity, EOF, mixed LF/CRLF, Add/Update; AW native Tool adapter remains authority |
| In-place text edit | Dotlink reduced-copy literal compaction | Shared text.rs, checked expansion/match counts; native edit uses shrinking in-place, unique-match checks, ordered evolving file buffers |
| Multi-file edit budget | Dotlink mutation planner, up to 512 edits/64 files | Shared aggregate 64 MiB original+working cap; AW adapter keeps its 48 KiB argument and 1 MiB per-file guard; bounded incoming arrays |
| Mutation receipts | Dotlink preflight / bounded JSON, incomplete metadata | Shared MutationReceipt preflights before staging, tracks accurate changed-file count and omitted details; AW's journal still owns effect status |
| Streaming binary edits | Dotlink bounded SHA-256 source streaming/splices | Shared binary.rs; Unix and Windows platform stage_stream_update, exact-source hash recheck before commit, cancellation, partial/no-op handling, 32 original-relative ranges |
| Read windows | Dotlink offset budgets and honest bytes | Shared read.rs; one synchronous implementation used by Unix+Windows; 32 MiB skip budget, CRLF/numbering, control rejection, cancellation |
| Binary/text distinction | Dotlink binary-control classification | Shared text.rs; AW read/search/edit/patch retain own exact grants |
| Search work accounting | Dotlink cumulative bytes/files, sorted candidates | Shared search WorkBudget; AW grep uses 32 MiB/4096-file budget, 4096 retained candidate cap, no large match-position Vec |
| Search incomplete causes | Dotlink incomplete_reasons array | Shared Incomplete; AW ls/grep expose all causes, retaining compatibility first cause |
| Safe search labels | Dotlink bounded readable file labels | Shared content_path_label quotes ambiguous paths, ordinary paths unchanged |
| Native file staging | Dotlink mutation hardening, AW stronger scoped FS | AW Unix+Windows retain pinned, no-follow, owner/mode/DACL/xattr, per-file atomic publication, SHA/byte snapshot verification and cooperative cross-process locks |
| Tool feature separation | Dotlink sync and optional async/durable use cases | Shared default sync; AW opts into async/durable, does not create another executor |
| Existing AW tool surface | ls, grep, read, write, edit, patch, bread, bwrite, bedit | All nine remain registered through original ToolRegistry/ToolPolicy, no new default grants |
| Existing AW shell | bash/powershell parameters, trusted boundary | AW Process host retained, optional parallel waves still model-directed; no behavioral permission widening |

## Partially adapted, with a deliberate remaining integration

| Capability | Ported portion | Not yet implemented / why |
| --- | --- | --- |
| Dotlink durable jobs | Shared task budgets/states, exact-source generation fences, bounded host-supplied file spools, ACK high-water, sealed writer leases, and host journal/settlement traits | AW has no new native job_start, job_list, job_get, job_output, job_ack, job_cancel; must first map to AW Session/Turn/Process ownership, durable CAS, source revocation and no-replay |
| Task/process ownership | TaskDisposition and DurableTaskHost; AW original durable effect journal maintained | Dotlink JobManager's persistent session store, replay-key mapping, sweepers, response-window-to-job fallback not duplicated |
| Bounded spool | Shared 64 MiB/stream and 512 MiB/session, 32 active/256 retained/4096 hidden caps; host-opened file spool with quota and exact paging, integrated opt-in with AW PipeProcess | No persistent restart-recovery job registry or model-facing job_* endpoints; AW Process/Session remain owners |
| Optional sandbox | Shared validated host SandboxPolicy plus Linux FD-pinned Bubblewrap executable plan and RAII child | Not a new AW shell tool; l5 Bubblewrap runtime failed mounting /proc (EPERM), so native sandbox runtime not certified |
| Native typed media | Shared digest-verified MediaPacket, explicit TypedMediaHost permission gate, AW bread encoded JSON | True MCP ContentBlock image/audio/blob output needs an Agent/provider typed-media contract, not a shell/library flag |
| Shared filesystem backend | Sync algorithms are shared, Unix/Windows stage streaming + no-follow retained | Dotlink secure_fs/staging and AW aw-platform are not yet linked to one host backend crate; porting dotlink's lock authority wholesale would duplicate existing AW guards |
| Shared declarative tool schema | Edit and binary arrays are bounded during deserialization; shared limits used | Host-facing ToolSpec is still AW native, dotlink generates rmcp schemas; unifying schemas requires preserving both native parallel hint and MCP transport metadata |
| Portable host tests | Unix platform and Windows GNU cross-target build | Native Windows NTFS ACL/Job Objects/macOS xattr, sandbox runtime, and live dotlink-as-consumer tests are separate gates |

## Deliberately skipped / not applicable to AW native tools

| Commit or subsystem | Reason |
| --- | --- |
| 439098a OpenAI poll budget | Dotlink's OpenAI tunnel protocol only |
| ebddfc5 service-unconfigured stop | Dotlink server lifecycle, AW's Agent has separate owner |
| c1633b2, 28bcd5a, bb8a917 version-only releases | No native tool feature; version remains AW 0.15.0 |
| 3f38427, fe375df platform-specific fixes | Applied relevant portable algorithms/tests; dotlink's release gates remain separate |
| 08ca60d, aa8d817, 9e36979, 594b57e, 567ae18, 70939fb transport/profile/OAuth/replay internals | Native AW effects have exact actor/site/source grants and journal-before-effect; do not add a second credential selector, replay ledger or tunnel |
| Dotlink's 50-hits-per-file grep cap | AW intentionally supports up to its global requested 200 matches and reports partial pages; restoring 50 would reintroduce a reviewed correctness regression |
| Dotlink access/profile settings and OAuth/token/config rewrites | AW uses its own source-grant, account and credential model; not filesystem tools |
| Dotlink daemon, HTTP/stdin MCP progress and timeout-to-job response | AW does not expose the same transport; no hidden background command is started on tool-response timeout |

## Commit-by-commit verification

The preceding document SHARED-TOOLSET-2026-10-30.md enumerates every one
of the 16 dotlink commits individually. Applied tool-level work is concentrated
in 08ca60d (bounds), aa8d817 (process ownership review), 3f38427 /
fe375df (portability), b6aa9de (mutation refactor), f3309ad (borrowed
replacements/splices), 3a775e2 (KMP and jobs budgeting), 9e36979 and
594b57e (job/task lifecycle), 567ae18 / 70939fb (retention/replay rules).
All other infrastructure-only changes were inspected, not silently copied.

## Validation and limitations

- Shared core standalone: 23/23 synchronous-default tests and 30/30 all-feature tests pass.
- AW native tool adapters in disposable integration workspace: 63/63 unit
  and behavioral tests pass (includes 512 ordered edits across 64 files,
  streamed 4 MiB binary splice + SHA mismatch, LF/CRLF/EOF patch behavior,
  and multiple simultaneous incomplete search reasons).
- AW Unix platform in same integration workspace: 15/15 tests pass,
  including digest-verified staged stream update, stale-preimage refusal,
  atomic one-link creation, mode/ownership/xattr preservation.
- Windows GNU cross-target: aw-platform-win and aw-tools all targets check
  successfully against Rust 1.98.1 and matching Windows standard library.
  This is compilation, not native Windows runtime testing.
- Strict Clippy for shared core + aw-tools passes in integration workspace.
- Exact AW root Cargo.lock retains original pinned dependencies and adds only
  the shared crate and local dependency edges, with no version bump.
- **2026-10-31 update (supersedes earlier l5 cache blocker):** The
  original pinned Cargo.lock is reconciled without dependency-version changes,
  pruning only unused mime_guess/unicase records. Full AW workspace all-target
  compilation and individual original-lockfile native/core/CLI suites pass.
  The monolithic full-workspace test run did not return a terminal result, so
  it is not counted as a test-suite pass. The earlier disposable fixture
  remains ignored diagnostic material, not the final source of truth.
- Neither dotlink sources nor its staging/index/commits were changed.
- No user-visible background work was started. Native job tools, live MCP
  interoperability, native Windows/macOS runtime, typed media and sandboxed
  Process remain explicitly unverified/not fully ported.


### Live dotlink uncommitted task-spool hardening inspected

On top of HEAD 70939fb, dotlink currently has substantial uncommitted job and
shell hardening. This work was not modified. Relevant invariants inspected:

- Seal every per-job output-writer registration before terminal publication;
  stop new output writes, wait for actual registered blocking spools to become
  idle, and avoid a lost last-writer notification. **Reusable core port:**
  aw-tools-core's opt-in SpoolWriters gate, RAII leases and synchronous
  bounded quiescence; stress tests. **Host integration pending:** AW's existing
  Task/Process owner would need to call it on its real output writer threads.
- Output spooling and quota accounting must distinguish retained bytes from
  delivered/acknowledged bytes, including partial release. **Core:** retained,
  delivered and acknowledged fields plus quotas. **Pending:** actual disk
  spool file leases, reader leases, cleanup/compaction and offset paging.
- Panicking runners become terminal internal errors only after writer
  quiescence. **Not integrated:** AW's Process supervisor has its own
  cancellation/unknown semantics; no second dotlink JobManager was added.
- Dotlink's Nix-store launcher trust, FUSE/mount restrictions, sandbox guard
  and HTTP/OpenAI replay state are context-specific. **Skipped:** they must not
  be treated as AW trusted-shell security guarantees.

This distinction is important: the shared library now provides portable
algorithms and contracts, **not** a production implementation of dotlink's
durable shell job system inside AW.


## TS-N6..N10 continuation — 2026-10-31

- **N6 exact durable fencing:** shared TaskFence binds source kind/epoch,
  actor, site, session/branch, original call and effect; rejects stale
  generations, revocation, unjournaled starts, conflicting settlement.
  AW FencedProcessSpools uses it for exact host-only pages, summaries and
  ACK; all terminal states require spool writer quiescence.
- **N7 durable output handles:** BoundedFileSpool writes the exact retained
  prefix to host-created regular files, tracks 64 MiB/stream and
  512 MiB/session quotas, compacts after sync and preserves historical ACK
  evidence. PipeProcess can opt in without model-selected spool paths.
  Exhausted quotas are accounted without scheduling disk writes per chunk.
- **N8 typed media handoff:** MediaPacket owns bounded complete bytes and
  signature-derived SHA-256 descriptor, verifies again before dispatch and
  requires explicit TypedMediaHost permission. No provider-native typed
  ContentBlock is currently advertised.
- **N9 sandbox execution boundary:** Linux BwrapPlan uses openat2 pinned
  workspace/runtime and executable file descriptors, no network by default,
  exact host-selected writable mounts, and a RAII BwrapChild. It is not
  registered as an AW tool. Actual l5 sandbox run reached bwrap but failed
  to mount /proc (Operation not permitted); native execution is blocked.
- **N10 original-lockfile closure:** Root all-targets workspace compilation,
  Linux/no_std and Windows GNU tool/core cross-compilation, strict Clippy and
  original-root native/CLI tests pass. The repository retains its original
  AW Agent/Journal/ToolPolicy as the only model execution authority.

**Still incomplete:** model-facing durable job_start/list/get/output/ack/
cancel with restart recovery and durable source epochs; native typed MCP
provider media; installation of a host-approved sandbox Process tool;
native Windows/macOS runtime tests; a terminal result from the full
workspace test suite. Dotlink remains read-only.

## Additional dotlink commits through abfc71d (2026-10-31)

The earlier 70939fb sixteen-commit baseline was followed by c44194f
(v0.14.2), 762dfa8 (portable CI) and abfc71d (Windows lexical pathname
admission and invariant UTF-16 lock names).

- Adapted shared spool writer sealing/retention, host-owned regular-file
  validation, Windows separate admission-before-path mutex namespaces,
  matching Windows uppercase UTF-16 digest and single cancel/deadline budget.
- Existing AW pinned roots, immutable TurnJournal, source/effect grants,
  and single Process owner retained.
- No dotlink OAuth, OpenAI replay, daemon, MCP progress or release metadata
  copied. No dotlink modifications.
- Windows GNU all-target compilation passes; native race interoperability
  and mounted sandbox execution remain separate unchecked gates.

See NEXT-FIVE-B-2026-10-31.md for the five executed phases.
