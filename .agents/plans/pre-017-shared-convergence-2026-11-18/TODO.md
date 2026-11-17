# Pre-0.17 shared convergence

Owner: @pvl
Reference range: dotlink `84cd9b7..3317429` (55 commits; before v0.17.0 release commit)
Canonical design: [README.md](README.md)
Commit accounting: [AUDIT.md](AUDIT.md)

## A. Freeze scope and invariants

- [x] Define cutoff immediately before the v0.17.0 release commit; account for
      `88d7b71`, `75ff9fe`, `3317429` here and defer the release commit +
      all later v0.17.x work.
- [x] Preserve current one shared AW JobManager/runtime, exact authority,
      delivery/ACK, replay and Toolset architecture.
- [x] Classify all 55 pre-v0.17.0-tag commits in `AUDIT.md`.
- [x] Revalidate exact AW/dotlink refs and dirty/staged ownership before source
      implementation begins: AW local `30dc55a`, published `7e6fe20`; dotlink
      `HEAD == origin/main == 5381729`; existing unrelated AW docs preserved.

## B. Shared scoped receipt/result metadata

- [x] Extend `ReceiptProfile` with operation/category scope supporting all,
      category selection, explicit include and explicit exclude.
- [x] Add stable semantic `ReceiptCategory` mapping for Filesystem, Process,
      Batch, Jobs and Agent.
- [x] Keep scope allocation-free/`Copy`: fixed category bits + `u64`
      operation include/exclude masks; exhaustive OperationId mapping.
- [x] Add typed `ReceiptMetadata` projection. Shared code accepts host-supplied
      UTC Unix seconds and owns no clock.
- [x] Define `ts` as first-finalization time of the represented result/
      observation: immutable result/Task replay reuses the original timestamp,
      while each freshly rendered live observation gets the current timestamp.
      Independent event timestamps remain separate future typed facts.
- [x] Apply the same profile selection to compact typed receipts and arbitrary
      result shapes without parsing free-form payloads.
- [x] Preserve payload bytes/JSON/media exactly; metadata is a sidecar.
- [x] Keep result budget accounting exact when a compact textual receipt renders
      the selected metadata.
- [x] `model_instructions()` remains opt-in and generated only when
      `checkpoint_minutes` is configured.
- [x] Focused shared tests: all/category/include/exclude precedence, exclude
      wins, effective-empty scope, timestamp enabled/disabled, missing host
      timestamp, compact budget edge and exhaustive operation mapping.
- [ ] Product/MCP tests for raw/structured/media payload identity and exact
      replay-vs-fresh-observation timestamp behavior remain in section F after
      dotlink consumes the reviewed shared pin.

## C. Shared Job read-page profile

- [x] Add `JobProfile.read_page_default_bytes` and
      `JobProfile.read_page_max_bytes`.
- [x] Shared default read page is 256 KiB.
- [x] Add shared engine hard maximum independent of product defaults.
- [x] Set shared Job read-page hard maximum to 8 MiB; default/max are 256 KiB
      unless the embedding product explicitly widens max.
- [x] Validate `1 <= default <= max <= hard_max`.
- [x] `describe_operation(JobsRead)` advertises exact default/max and existing
      wait ceiling from one profile.
- [x] Runtime omitted `limit` uses default; explicit over-max rejects.
- [x] Negative-offset snapshots use the same configured page budget.
- [x] Actual outer serialized/model budget narrows the returned page truthfully
      without changing source offsets, EOF facts, delivery proof or ACK scope.
- [x] Decouple finite `jobs_read` from the unrelated 64 KiB
      `MAX_TASK_OUTPUT_PAGE` constant.
- [x] Focused shared tests: default 256 KiB, narrowed/widened max, tiny outer
      envelope, text/base64 expansion, tail snapshot, byte/EOF wait and ACK
      delivery proof.

## D. AW product integration

- [x] AW keeps receipt timestamps/checkpoint guidance disabled by default.
- [x] AW `jobs_read` consumes the shared 256 KiB default and configured read max.
- [x] Add/choose an AW profile/config path for Job read maximum; do not conflate
      it with filesystem/search result defaults.
- [ ] Qualify the Agent/context/result hard ceiling needed to realize a 256 KiB
      Job page when context permits. Do not globally widen unrelated tool
      defaults merely to satisfy Jobs. Current 128 KiB Agent hard envelope
      truthfully narrows the otherwise-256 KiB request; raising it affects MCP,
      extensions and subagents and is intentionally deferred to the next slice.
- [x] Preserve Agent history cursor semantics and finite Process/Graph raw-byte
      semantics under the new page profile.
- [x] Re-run shared/adapter/read-only/minimal/no_std/strict-Clippy/downstream MCP
      and CLI gates.

## Shared implementation checkpoint — 2026-11-18

- [x] `aw-tools` scoped receipt metadata + Job-page profile implemented.
- [x] `aw-tools-adapter` unified `jobs_read` schema/validation/runtime consumes
      exact shared default/max; omitted-limit execution regression proves the
      configured profile rather than the old Task-page constant drives paging.
- [x] Qualification: `aw-tools` **342/342**, `aw-tools-adapter` **252/252**,
      read-only shared **111/111**, `aw-mcp` **18/18**, minimal aw-tools,
      downstream `aw-host` + `aw-cli --lib`, strict all-target/all-feature
      Clippy, rustfmt and `git diff --check` green.
- [ ] Next AW slice: design/qualify a Job-specific 256 KiB Agent presentation
      envelope without widening unrelated MCP/extensions/subagent defaults.

## E. Signed shared checkpoint / dotlink repin

- [x] Review exact AW owned diff and existing unrelated dirty docs/staging;
      only this plan/queue + `aw-tools`/adapter owned source enters the checkpoint.
- [x] Create signed AW implementation checkpoint.
- [ ] Publish AW only with separate explicit authorization.
- [ ] Repin dotlink `aw-tools`, `aw-tools-fs-unix`, `aw-tools-fs-win` and all lock
      records together to the exact published AW commit.
- [x] No temporary sibling/path dependencies.

## F. Dotlink product integration

- [ ] Build one shared Toolset with timestamp scope `Process | Batch | Jobs`.
- [ ] Set `checkpoint_minutes = Some(30)`.
- [ ] Append shared model instructions exactly once to MCP server instructions.
- [ ] Attach selected timestamp to `CallToolResult._meta` under a namespaced
      shared receipt key: `io.abird/receipt`.
- [ ] Ensure model visibility with a separate receipt content block when the
      timestamp is not already part of a compact typed receipt; never mutate
      raw/structured/media payloads.
- [ ] `jobs_read` pages receive the same selected response timestamp.
- [ ] Dotlink JobProfile read default =
      `min(256 KiB, configured max_output_bytes)` and max =
      `max_output_bytes`.
- [ ] Restore omitted synchronous shell `max_output_bytes` default to
      `min(256 KiB, configured max_output_bytes)`; explicit requests still clamp
      to the configured ceiling.
- [ ] Schema/descriptions derive the same default/max values as runtime.
- [ ] Preserve current hosted response-window, Task/replay, D1-D4 output-owner,
      guard-generation and B6 Toolset authority invariants.

## G. Pre-v0.17 equivalence qualification

- [ ] Configuration v11/v12 migration and strict decode.
- [ ] CLI/environment precedence including `DOTLINK_VERBOSITY`,
      `--no-serialize`, `--no-tunnel`, profile/config override handling.
- [ ] NixOS single-settings/staged config/ambiguity failures.
- [ ] Explicit-file grep + context 0..128 and current shared Graph parity.
- [ ] Authenticated response-window wait narrowing and generic MCP progress.
- [ ] Native Task replay/result identity.
- [ ] Current shared Jobs get/read/ACK behavior proves the old tail/page intent
      without restoring legacy line-tail/job_output APIs.
- [ ] Dotlink all-target/all-feature check, strict Clippy, full binary +
      integrations, fmt/diff, Nix and native Bubblewrap/security gates as
      available.
- [ ] Record any release behavior that is intentionally superseded rather than
      silently claiming byte-identical parity.

## H. Deferred

- [ ] v0.17.0 release commit and later v0.17.x work.
- [ ] v0.17.2 generic shell mounts/config v13.
- [ ] post-v0.17.3 Process signal identity / stop provenance / lifecycle log.
- [ ] Any v0.17.x version/tag/release work.

Do not begin H until A-G are complete.
