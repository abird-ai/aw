# Pre-0.17 shared convergence

Author: Prasanna Loganathar
Co-authors: ChatGPT GPT-5.6 Sol
Owner: @pvl

Port the reusable behavior from dotlink's pre-0.17 release line into canonical
`aw-tools` / `aw-tools-adapter` first, then consume the reviewed shared pin from
dotlink `main`.

This is a semantic convergence plan, **not** a merge or a historical
cherry-pick sequence. Current AW/dotlink source and shared-runtime invariants
outrank old release implementation structure.

## Exact scope boundary

Reference merge base: dotlink `84cd9b7cbda48e327e15781c771e162fb8820ae7`.

This first wave covers the **55 commits before the v0.17.0 release commit**,
through `331742987fbcc82ff3f00fbd4ff8943f4e5fc8e6`.

It therefore also accounts for the three final pre-tag preparation commits:

- `88d7b71` — its 35-minute guidance is replaced by the shared ReceiptProfile
  mechanism with dotlink selecting 30 minutes;
- `75ff9fe` — `background: true` replaces `job_start`; already current;
- `3317429` — one background entry point; already current.

It deliberately excludes the `v0.17.0` release commit and every later 0.17.x
commit, including:

- the 0.17.2 generic mount layer
- the post-0.17.3 stop/signal provenance series

Current `main` may already contain newer equivalents of some excluded behavior;
that does not move those release commits into this wave.

## Goals

1. Account for every pre-v0.17 commit: port missing semantics, verify newer
   equivalents, or record why a historical implementation is intentionally
   superseded.
2. Keep reusable behavior in `aw-tools`; product policy remains in AW/dotlink.
3. Restore dotlink's useful pre-0.17 model ergonomics without restoring its old
   standalone Job/runtime implementation.
4. Publish one reviewed AW shared pin before dotlink consumes any new shared
   contract. No mixed/path dependency state.

## Result metadata / receipt profile

`ReceiptProfile` becomes the shared **result-metadata policy**, not merely a
compact-text receipt switch.

### Timestamp meaning

`ts` has exactly one meaning:

> UTC Unix seconds when the host first finalizes the represented result or
> observation receipt.

For an immutable terminal result/Task result, that timestamp is part of the
result receipt and exact replay reuses it. It must not be regenerated merely
because the same result is replayed; native Task/result equality and outer
exactly-once replay stay byte-identical.

For a live observation surface such as `jobs_list`, `jobs_get` or
`jobs_read`, each newly rendered observation is a new receipt and receives
the current host timestamp.

`ts` is not process start time, file mtime or output-byte production time. If
those independent facts are needed later, they get distinct typed names such as
`started_ts` / `settled_ts`.

### Selective scope

The shared profile supports:

- all operations;
- semantic categories;
- explicit operation include;
- explicit operation exclude.

Initial categories:

- `Filesystem`
- `Process` (`bash`, `pwsh`, `powershell`)
- `Batch`
- `Jobs` (list/get/read/ack/send/cancel/subscribe)
- `Agent`

Selection is deterministic:

1. selected categories form the base;
2. explicit include adds operations;
3. explicit exclude removes operations and wins.

Keep this zero-allocation and `Copy`: categories use a small fixed bitmask and
the current <=25 `OperationId` surface uses a `u64` operation mask for
include/exclude. Do not add heap sets or runtime callbacks to the Toolset.
Operation→category and operation→bit mapping must be exhaustive, so adding a new
`OperationId` cannot silently inherit an arbitrary receipt policy.

The profile remains immutable after `Toolset::build()`.

### Typed metadata

Add a small transport-neutral typed projection, e.g. `ReceiptMetadata`, returned
from the Toolset for an `OperationId` and host-supplied timestamp.

No shared clock/timer is introduced.

For compact typed receipts, the same metadata may be rendered as a footer within
the existing total result budget. For raw/structured/media results the payload
must remain unchanged; hosts attach the metadata beside it.

For MCP, dotlink should:

- attach `CallToolResult._meta["io.abird/receipt"] = {"ts":...}`; and
- preserve model visibility with a separate receipt text block when the
  timestamp is not already present in a compact typed receipt.

This keeps `jobs_read.data`, structured JSON and media bytes untouched.

### Product choices

AW default:

- timestamp metadata off;
- no checkpoint instruction.

Dotlink default:

- Unix timestamp enabled for `Process`, `Batch`, and `Jobs`;
- therefore `jobs_read` pages also carry response-time metadata;
- filesystem tools remain untimestamped initially, matching pre-0.17 behavior;
- `checkpoint_minutes = Some(30)`;
- append `Toolset::model_instructions()` to server instructions.

The checkpoint is advisory model guidance only. It never schedules a timer,
changes Job/process deadlines or becomes an AW-wide runtime policy.

## Job read page profile

The existing `JobProfile` lacks a read-page default/max, causing hosts to fall
back to unrelated result ceilings. Add:

- `read_page_default_bytes`
- `read_page_max_bytes`

Shared default:

- `read_page_default_bytes = 256 KiB`
- `read_page_max_bytes = 256 KiB` unless a product widens it explicitly
- shared hard maximum = **8 MiB**, independent of product defaults

Rules:

- omitted `jobs_read.limit` uses the configured default;
- explicit `limit` must fit the configured max;
- schema advertises exact default + maximum from the same immutable profile;
- actual serialized/model transport budget may narrow a page further at a
  truthful byte boundary; it never fabricates success or changes ACK credit;
- negative-offset end snapshots use the same page budget.

Dotlink:

- read-page default = `min(256 KiB, max_output_bytes)`;
- read-page max = configured `max_output_bytes` (already <=8 MiB);
- keep the existing profile/transport hard ceilings;
- restore the pre-0.17 **256 KiB omitted synchronous shell-output default**
  separately under the same dotlink `max_output_bytes` ceiling.

AW:

- `jobs_read` default = 256 KiB;
- max is product/profile configurable rather than borrowed from
  `MAX_TASK_OUTPUT_PAGE`;
- decouple Jobs read paging from the current 64 KiB native-Task page constant;
- preserve the outer Agent/context/result budget as the final presentation
  clamp;
- qualify the AW hard model-result ceiling needed to make the 256 KiB default
  realizable without widening unrelated filesystem/search defaults.

Do not silently turn every AW native tool result into 256 KiB. This phase changes
the Job page policy; other local-tool defaults remain independent.

## Pre-0.17 compatibility policy

The pre-0.17 release implementation is reference behavior, not architecture.

- Configuration/Nix/CLI/progress/grep behavior already ported to dotlink `main`
  is reverified rather than copied.
- Shared AW event-driven Job waits supersede release polling.
- Shared byte-tail/range/owner/ACK semantics supersede old line-tail and
  `job_output` contracts.
- B6 Toolset metadata supersedes release-local descriptor substitution.
- Response-clock behavior is reintroduced only through the new scoped
  `ReceiptProfile`, not old ad-hoc timestamp fields.
- Version/tag/changelog-only commits never enter source implementation commits.

See [AUDIT.md](AUDIT.md) for the complete 55-commit accounting.

## Cross-repository landing order

1. Implement/qualify shared ReceiptProfile scope + typed result metadata in AW.
2. Implement/qualify shared JobProfile read-page default/max in AW.
3. Update AW adapter Job schema/runtime to consume those exact shared settings.
4. Review Agent/context/result-budget interaction for the 256 KiB AW page.
5. Create one signed AW implementation checkpoint and publish it only with
   explicit authorization.
6. Repin all dotlink AW dependencies/lock entries together to that exact commit.
7. Dotlink enables Process/Batch/Jobs timestamps + 30-minute model guidance,
   adopts 256 KiB shell/read defaults, and attaches MCP result metadata without
   mutating payloads.
8. Run the full pre-v0.17 equivalence matrix plus current D1-D4/B6 regressions.
9. Only after this wave is complete, return to the deferred v0.17.x mount and
   stop/signal work.

No version bump, tag, release or deployment belongs to this plan.
