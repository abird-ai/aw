# Next agent — unified Jobs + Agent runtime finalized

Use `dotlink---l5` only for source work.

Canonical repos:

- AW: `/home/pvl/spaces/abird/src/aw`
- dotlink: `/home/pvl/spaces/abird/src/dotlink`

Do not use A1. Do not create another worktree. Never inspect or touch AW
`.agents/plans/ux/**`. Preserve unrelated dotlink `docs/CHATGPT_PLUGIN.md` and
the separate `dotlink-016-fixes` release worktree.

## Authority / final commits

Normative design authority:

`890e9a383fe2edc3e0bd0aa34a2b217f2724475c`

Final shared/runtime AW implementation:

`0a6c6a0bea07a3886c4dc8be4f423eec3ea3c134`

Final dotlink unified bridge:

`c90a6603ccef0bbf626ce10d95bedf59877747b8`

dotlink's `aw-tools`, `aw-tools-fs-unix`, and `aw-tools-fs-win` all pin the
exact AW implementation SHA. No sibling path or temporary digest implementation
remains.

## Status

Runtime Checkpoints 0/A/B/C/D/E/F/G and dotlink Checkpoint 3 are complete.

Final dotlink locked qualification against the exact AW pin:

- all-target/all-feature check green;
- 651 active tests passed / 1 intentional ignored fixture;
- integrations 1/1, 8/8, 1/1, 12/12, 4/4;
- strict all-target/all-feature Clippy green;
- fmt/diff green.

Final replay-first review guarantees exact Start redelivery reaches replay claim
before mutable Graph preparation, shell-policy checks, or cancel revision checks.

Live model-selection qualification used x2 inference-only ROCm Ollama
`qwen3.8:27b` with the exact AW 23-tool catalog: 30/30 first-call
selection/schema/task validity, plus Bash Jobs continuation 3/3 and Agent
synchronous-result continuation 3/3.

## Next work

There is no remaining unified-runtime implementation task. Revalidate live Git
state before any future change. Release/tag/deploy or native platform
certification are separate scopes and require explicit authorization.
