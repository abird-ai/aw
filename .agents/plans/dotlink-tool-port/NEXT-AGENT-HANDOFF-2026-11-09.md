# Next-agent handoff pointer — 2026-11-09

Author: @pvl
Co-author: ChatGPT
Owner: @pvl

The full cross-repo handoff is intentionally single-sourced in the sibling
dotlink repository:

```text
/home/pvl/spaces/abird/src/dotlink/.agents/plans/
dotlink-ai-native-filesystem-2026-10-31/
FULL-COMPREHENSIVE-HANDOFF-2026-11-09.md
```

Read it before the older `NEXT-AGENT-HANDOFF-2026-10-31.md`; the 2026-11-09
handoff supersedes that older architecture/pin state.

Exact reviewed checkpoints:

```text
AW      4a23e92a8f516971bb232b98e4fc000554bcc4ba
dotlink b31683c79a8ffd7c5540ac9388e607d8af6c7ffa
```

GitHub `abird-ai/aw` `main` was directly verified at `4a23e92...`. Dotlink pins
all shared crates to that exact remote Git revision. Dotlink `b31683c` is still
one commit ahead of its configured `origin/main`; do not push without explicit
user authorization.

After the full handoff, use this AW authority order:

1. current source/tests at `4a23e92`;
2. `SHARED-AW-TOOLS-CONVERGENCE-2026-10-31.md`;
3. current status block in `TODO.md`;
4. older dated review/handoff files as provenance only.

The final architecture is already implemented: `aw-tools` is canonical shared
semantics/Graph/Job runtime, `aw-tools-adapter` is AW-specific authority/process/
projection, secured backends are reusable, and AW retains one original Agent,
ToolPolicy, TurnJournal and Process path. Do not recreate `aw-tools-core` or a
second JobManager.
