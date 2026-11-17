# Pre-0.17 release-line audit

Reference: dotlink `84cd9b7cbda48e327e15781c771e162fb8820ae7..331742987fbcc82ff3f00fbd4ff8943f4e5fc8e6`
(55 commits, immediately before the v0.17.0 release commit).

This table accounts for every commit in the first convergence wave. “Done” means
current `main` has the same intent or a stronger reviewed equivalent; it does not
mean the old patch is byte-identical. “Superseded” means importing the historical
implementation would regress the current shared AW architecture.

Status:

- **PORT-SHARED** — implement reusable semantics in AW first.
- **PORT-DOTLINK** — dotlink product behavior after the reviewed AW pin.
- **VERIFY** — current code appears equivalent/newer; re-run focused evidence.
- **SUPERSEDED** — current shared design intentionally replaces the old mechanism.
- **RELEASE-ONLY** — version/changelog/release-only; never source-port.

| Commit | Release-line change | Current disposition / gap |
| --- | --- | --- |
| `6a13963` | Cap synchronous hosted shell wait below the OpenAI request window and reserve terminal delivery time. | **VERIFY.** Current dotlink has the authenticated remaining-window model, fixed hosted lifetime and delivery reserve. Reverify no execution timeout is shortened. |
| `425b025` | Release bookkeeping for 0.14.4. | **RELEASE-ONLY.** |
| `3a2bf98` | Handoff still-running shell at 30 s and return a self-describing Job handle. | **SUPERSEDED.** Current shell creates one durable execution and transport chooses sync/Task/compatibility exposure; current shared Job refs/receipts are newer. Preserve no-rerun identity, not the 30 s implementation. |
| `5ae178b` | Release bookkeeping for 0.14.5. | **RELEASE-ONLY.** |
| `960185d` | Assert the platform-specific shell name in advertised schema tests. | **VERIFY.** Current Bash/Pwsh/PowerShell registration and Toolset metadata are platform-aware. |
| `c5d9d15` | Add `DOTLINK_VERBOSITY`. | **VERIFY.** Present on current main with single parse owner and CLI precedence. |
| `eb5639c` | Release bookkeeping for 0.14.7. | **RELEASE-ONLY.** |
| `094059e` | NixOS service passes `DOTLINK_VERBOSITY`. | **VERIFY.** Covered by the operational port and module tests. |
| `4d10cc7` | Release bookkeeping for 0.14.8. | **RELEASE-ONLY.** |
| `4f78a26` | Config schema v11 and environment forms for settings. | **VERIFY.** Current main has explicit v9→v10→v11→v12 migration and CLI/env/profile precedence. |
| `122c534` | Release bookkeeping for 0.15.0. | **RELEASE-ONLY.** |
| `be03aa7` | Add `--no-serialize`. | **VERIFY.** Current main supports it and lets explicit CLI suppress malformed lower-priority env. |
| `6d57489` | Release bookkeeping for 0.15.1. | **RELEASE-ONLY.** |
| `d798158` | Let grep target one explicit file path. | **VERIFY.** Shared AW grep now handles exact file and directory targets through one parser/engine; direct + Graph parity already qualified. |
| `5b65c62` | Release bookkeeping for 0.15.2. | **RELEASE-ONLY.** |
| `7c5734f` | Make each module setting single-owned and fail closed on malformed environment values. | **VERIFY.** Current operational port is stronger: explicit CLI skips only its overridden env setting while malformed active settings still fail. |
| `9006657` | Release bookkeeping for 0.15.3. | **RELEASE-ONLY.** |
| `7dc862d` | Config schema v12, discovered cache paths and strict fields. | **VERIFY.** Current config version is 12 and nested profile structs are strict. |
| `fe02e12` | NixOS `settings` becomes the only declarative config surface. | **VERIFY.** Current module retains the single-settings design. |
| `23f5b44` | Stage active Nix config privately and restore deployment provisioning. | **VERIFY.** Current systemd staging is fail-closed; no stale staged config fallback. |
| `ea0b9e4` | Refuse ambiguous Nix/config selection instead of switching configs. | **VERIFY.** Current profile/config selection fails closed on ambiguity. |
| `3970148` | Document schema v12 and full migration chain. | **VERIFY/DOC.** Current docs should remain aligned with live v12 until the deferred v0.17 mount/config-v13 wave. |
| `032af01` | Resolve `DOTLINK_CONFIG_FILE` explicitly and report deprecated `DOTLINK_CONFIG`. | **VERIFY.** Current startup has canonical file override + alias handling. |
| `2e26e3b` | Add response `ts` and restore a bounded synchronous shell hold. | **PORT-SHARED + PORT-DOTLINK.** Reintroduce the useful clock through scoped `ReceiptProfile` / typed result metadata, not ad-hoc fields. Dotlink selects Process/Batch/Jobs + 30-minute guidance. Hosted hold itself is already current; verify it. |
| `e251f12` | Let one Job poll wait for progress or completion. | **SUPERSEDED/VERIFY.** Shared AW event-driven `jobs_get` / byte-EOF-aware `jobs_read` waits are newer and avoid polling duplication. |
| `80b12b2` | Consolidate Job wait units/predicate/clamping. | **VERIFY.** Current immutable `JobProfile` owns wait ceilings; transport may only narrow them. |
| `c7bd69a` | Stamp immutable process completion results once instead of re-stamping them on replay; live status/receipt/output pages remain render-time observations. | **PORT-SHARED / VERIFY.** Preserve this distinction: an immutable terminal/Task result reuses its original receipt timestamp for byte-identical replay, while each newly rendered live Jobs observation gets a fresh host timestamp. |
| `fc03329` | Bound a Job poll even without a transport window. | **VERIFY.** Shared get/read waits have finite profile ceilings independently of transport. |
| `42cf414` | Document response-clock provenance. | **SUPERSEDED/DOC.** New docs define `ts` as response-finalization time from the embedding host and no shared clock. |
| `16173df` | Prove stored native Task result replay identity. | **VERIFY.** Current Task/result replay and owner retention are stronger; rerun exact-result replay tests after the new profile changes. |
| `07f27c8` | Release bookkeeping for 0.16.3. | **RELEASE-ONLY.** |
| `75c11e8` | Advertise the exact limits enforced by runtime state. | **VERIFY.** B6 Toolset already centralizes dynamic metadata. Extend that same invariant to new read-page default/max. |
| `6d8e4cc` | Release bookkeeping for 0.16.4. | **RELEASE-ONLY.** |
| `4e0b36b` | Test distinguishes a separate execution's clock from exact native-Task replay identity. | **VERIFY.** Exact replay must preserve the original result timestamp; a distinct execution may have a different timestamp. Add explicit regressions for both cases. |
| `c204c6f` | Release bookkeeping for 0.16.5. | **RELEASE-ONLY.** |
| `b351381` | Raise grep before/after context to 128 and report narrowing. | **VERIFY.** Shared AW schema/parser/runtime already enforce 0..=128 on direct + Graph paths and reject 129. |
| `9ff6e3a` | Emit MCP progress while a Job wait blocks. | **VERIFY.** Current generic progress sink covers Job waits and obeys absolute deadlines. |
| `693b090` | One MCP progress implementation for all tools rather than per-tool workers. | **VERIFY.** Current main uses the shared bounded progress helper. |
| `9f5b221` | Regression: a lock-blocked tool still reports progress. | **VERIFY.** Retain as an end-to-end operational gate. |
| `88b678c` | Omitted shell/Job output page defaults to 256 KiB rather than the whole profile ceiling. | **PORT-SHARED + PORT-DOTLINK.** Add shared `JobProfile.read_page_default_bytes/read_page_max_bytes` with 256 KiB default. Dotlink maps max to `max_output_bytes` and separately restores omitted synchronous shell output to `min(256 KiB,max_output_bytes)`. |
| `ba49c5a` | Advise reading a Job from its end by default. | **SUPERSEDED.** Current `jobs_get` aggregate tail + signed negative `jobs_read` offsets provide the intent more directly. Keep compact guidance, not old arithmetic/manual API. |
| `4d60e27` | Add tail-by-byte Job output reading and consolidate Job tools. | **SUPERSEDED.** Shared AW now owns signed retained-byte offsets, finite waits and output-owner proof. Do not restore `job_output`. |
| `1d68d25` | Default `jobs_get` tail to five lines, cap sixteen. | **SUPERSEDED.** Current shared contract uses 512 default / 1024 max aggregate raw bytes with redistribution. |
| `a2706fd` | Change default tail from five lines to four. | **SUPERSEDED.** Same reason. |
| `fe5f14f` | Rustfmt-only assertion cleanup. | **RELEASE-ONLY / no semantic port.** |
| `81e1c05` | Group output-page request fields and omit empty stream tails. | **VERIFY/SUPERSEDED.** Current shared request types prevent positional confusion and aggregate tail projection avoids legacy empty line-tail arrays. Verify no empty synthetic stream tail is emitted. |
| `a9e04d1` | Make tail truthful about snapshot, wait and fragment boundaries. | **VERIFY.** Current D1 shared tail/range semantics are snapshot-based, absolute-offset truthful and do not grant ACK credit. |
| `2684777` | Keep Job limits together and close guidance drift. | **PORT-SHARED.** Extend the existing `JobProfile` rather than reintroducing release-local constants. New read-page default/max must drive schema + runtime from the same profile. |
| `e331015` | Release bookkeeping for 0.16.6. | **RELEASE-ONLY.** |
| `c24a40f` | One explicit-background implementation and one failure shape. | **VERIFY.** Current main already has one `background` path and shared durable execution; no old duplicate `job_start` path should return. |
| `b93b14e` | Simplify background receipt return path. | **VERIFY/SUPERSEDED.** Current shared disposition/receipt flow is newer; verify background does not fork execution or failure semantics. |
| `b9ef787` | Release bookkeeping for 0.16.7. | **RELEASE-ONLY.** |
| `88d7b71` | Advertise a 35-minute model turn checkpoint using response timestamps. | **PORT-DOTLINK / SUPERSEDED VALUE.** Keep the feature through scoped ReceiptProfile/model instructions, but dotlink selects **30 minutes**, not the historical 35. AW itself remains off by default. |
| `75ff9fe` | Replace public `job_start` with `background:true` on shell. | **VERIFY.** Current main already exposes `background:true` and explicitly rejects/omits legacy `job_start`; no source port needed. |
| `3317429` | Collapse shell foreground/background onto one durable execution entry point and truthful Job surface. | **VERIFY.** Current main already has one durable execution with sync/native-Task/compatibility projection; reverify no second runner or failure shape exists. |

## Net new source work from this 55-commit wave

The audit reduces the first wave to three implementation deltas:

1. **Shared scoped result metadata / dotlink timestamps**
   - evolve `ReceiptProfile` to select all/category/include/exclude;
   - typed metadata sidecar with host-supplied response `ts`;
   - dotlink selects Process + Batch + Jobs, checkpoint 30 minutes;
   - MCP preserves payload and attaches namespaced `_meta` + model-visible receipt
     block where needed.

2. **Shared `jobs_read` page profile**
   - `read_page_default_bytes = 256 KiB`;
   - configurable `read_page_max_bytes`;
   - schema/runtime use the same immutable profile;
   - AW decouples finite Job paging from the 64 KiB native Task page constant and
     qualifies the Agent result ceiling needed to realize 256 KiB when context
     permits.

3. **Dotlink synchronous shell omitted-output default**
   - omitted `max_output_bytes` becomes `min(256 KiB, configured ceiling)`;
   - explicit caller value still clamps to the configured ceiling.

Everything else is a revalidation of current behavior, an explicit shared
supersession, or release-only history.

## Deferred release work

Do not include any of these in this wave:

- the `v0.17.0` release commit and later v0.17.x release-line work;
- generic explicit shell mounts / config v13 from v0.17.2;
- signal identity, stop provenance and lifecycle logging after v0.17.3;
- version/tag/release/deployment changes.
