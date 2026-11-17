# AI-native Job output and grep convergence

Author: Prasanna Loganathar
Co-authors: ChatGPT GPT-6
Owner: @pvl

**Current cross-repository context:** [AW–dotlink full comprehensive handoff (2026-11-17)](../../docs/AW-DOTLINK-TOOLS-FULL-COMPREHENSIVE-HANDOFF-2026-11-17.md). Includes the historical initial designs, complete implementation checkpoint index, exact Git/pin state, unresolved gates and next-agent prompt. Source and current plan TODO outrank dated handoff prose.

Implement the approved DL-150 model-first output workflow in canonical shared AW tools, **before** adapting dotlink. One `jobs_*` interface, existing Agent/ToolPolicy/TurnJournal and JobManager; no new execution, ownership or policy engine.

- **Design:** [DESIGN.md](DESIGN.md), approved 2026-11-15 decision superseding *only* conflicting get/read/ack and grep limits in the earlier [unified Jobs design](../ai-native-unified-jobs-agent-runtime-2026-11-11/DESIGN.md).
- **Builder prerequisite:** [shared Toolset factory](../ai-native-toolset-builder-2026-11-15/README.md) owns configurable schemas/defaults/descriptions/receipt opt-in; this plan still owns outstanding Job wait, output-delivery/ACK correctness and live-flow testing.
- **Execution:** [TODO.md](TODO.md). Checkpoint by coherent user-visible flows, not an individual microtest for each invariant.
- **Host:** AW `aw-tools-adapter` keeps exact current Actor/source/site/effect and output disclosure; dotlink is a later dependent consumer.
- **Existing behavior:** direct native filesystem, Process, Graph, Agent history and provider/tool authority remain as-is except explicitly specified extensions. No implicit `jobs_*` Graph leaves.

**Confirmed today:** AW grep accepts an exact file or a directory tree through `filesystem/direct.rs::grep_backend_path`; current shared `MAX_GREP_CONTEXT=8` and canonical Graph `grep.context` need extension to 128 and independent before/after.
