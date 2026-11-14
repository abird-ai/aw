# Next agent — final exact-pin / commit preparation

Use `dotlink---l5` only for implementation/source work.

Canonical repos:

- AW: `/home/pvl/spaces/abird/src/aw`
- dotlink: `/home/pvl/spaces/abird/src/dotlink`

Do not use A1. Do not create another worktree. Never inspect or touch AW
`.agents/plans/ux/**`. Preserve unrelated work, especially dotlink
`docs/CHATGPT_PLUGIN.md` and the separate `dotlink-016-fixes` release worktree.

Do not commit, push, tag, release or deploy unless the user explicitly authorizes
that action.

## Authority and pushed checkpoints

Normative design authority remains:

`890e9a383fe2edc3e0bd0aa34a2b217f2724475c`

Current pushed baselines:

- AW implementation: `9e9ab043410241c3f797f760d93a1cfd9126fdd2`
- AW current pushed docs/handoff HEAD:
  `7800c8b832c927960dc6b2b69c0529e89a9999d3`
- dotlink D1: `3f59bb16b0262f6157351fd28b13167cc1b3e5db`
- dotlink current pushed HEAD:
  `01e13a0bf52dc7d6afb3da51a93f533d5aeee34f`

The current dirty AW/dotlink main worktrees contain the completed D2/D3/Phase-7
implementation. Source/tests/executed evidence outrank prose.

## Read first

1. both repos' `AGENTS.md`, plan index and TODO;
2. AW `.agents/docs/CURRENT_HANDOFF.md`;
3. AW unified README/DESIGN/TODO;
4. dotlink `.agents/docs/CURRENT_HANDOFF.md`;
5. live Git status/diff/tests.

## Runtime state

Runtime Checkpoints 0/A/B/C/D/E/F/G and dotlink Checkpoint 3 are green.

The final cross-repo review found and fixed replay-first ordering in three
dotlink remote-effect owners:

- cancel now claims before mutable revision precondition;
- Graph now claims before mutable Graph preparation/topology revalidation;
- shell now claims before current local shell-policy recheck.

Fresh dotlink post-fix qualification:

- **651 passed / 0 failed / 1 intentional ignored fixture**;
- all five integrations green;
- strict all-target/all-feature Clippy green;
- fmt/diff green.

AW final qualification includes strict changed-crate Clippy, reduced
stateless/minimal/no_std boundaries, 303/303 shared DL-150 tests and model-first
execution fixtures.

Live model-selection qualification used x2 inference-only ROCm Ollama
`qwen3.8:27b` with the exact current L5-generated 23-tool catalog:
**30/30 first-call selection/schema/task validity**, plus exact Bash Jobs
continuation 3/3 and Agent synchronous-result continuation 3/3.

## Only remaining finalization dependency

dotlink still pins the last reachable reviewed AW revision
`9e9ab043410241c3f797f760d93a1cfd9126fdd2`.

The current dirty AW tree contains the final shared
`aw_tools::remote_start::arguments_sha256` helper. dotlink temporarily mirrors
that helper locally because it cannot consume dirty/unpushed AW.

When and only when the user explicitly authorizes commit/push:

1. revalidate/final-review AW;
2. signed AW commit + push;
3. capture the exact reachable AW SHA;
4. repin dotlink's `aw-tools`, `aw-tools-fs-unix`, and `aw-tools-fs-win`;
5. replace the local digest mirror with
   `aw_tools::remote_start::arguments_sha256`;
6. regenerate dotlink `Cargo.lock`;
7. run one locked dotlink all-features/integrations + strict Clippy/fmt/diff;
8. commit/push dotlink only if explicitly authorized.

Never repin to dirty/unpushed AW, a sibling path, `file://`, or a vendored copy.

If commit/push remains unauthorized, stop after revalidation and report:
**runtime-qualified; exact-pin repository finalization pending authorization**.

Keep one model turn near ~30 minutes and pause at the exact pin checkpoint.
