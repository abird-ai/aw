# Next-agent handoff — finalized vNext AI-native operations + Jobs + Agents

Author: Prasanna Loganathar
Co-authors: ChatGPT GPT-6, ChatGPT GPT-5.6 Sol
Owner: @pvl

**Status:** design finalized; implementation not yet authorized.

## Source-of-truth order

1. Read AW and dotlink AGENTS.md.
2. Read each plan index/current TODO; do not touch AW .agents/plans/ux/.
3. Read this plan README.md, DESIGN.md and TODO.md, then dotlink companion.
4. Read sealed DL-150 + shared implementation and current AW shared-tools design.
5. Read live source/tests; live source outranks dated prose for implementation.

## Starting prompt

Continue the finalized cross-repository vNext design in:

- AW: /home/pvl/spaces/abird/src/aw
- dotlink: /home/pvl/spaces/abird/src/dotlink

Canonical design:
AW/.agents/plans/ai-native-job-control-wake-2026-11-10/DESIGN.md

Execution plan:
AW/.agents/plans/ai-native-job-control-wake-2026-11-10/TODO.md

Key finalized decisions:

- canonical call/IR is `[operation,args]`; Graph node is `[operation,args,when?]`;
- grouped object-wire `jobs({op,...})` / `agents({op,...})` is the only secondary form;
- semantic Agent operation is `agents.run`; "subagent" is a relationship, not another Agent runtime;
- root and delegated children use the same Agent/TurnMachine/ToolRegistry/ToolPolicy/TurnJournal implementation with narrowed bindings;
- one Agent Turn = one Job; durable Agent identity survives Job settlement;
- live interaction uses `jobs.send`; settled follow-up creates a fresh `agents.run(id,...)` Job;
- fresh Agent ID remains recoverable even when the Turn completes synchronously;
- direct bash/pwsh, batch and agents.run may use `background`; it is response disposition only and Graph nodes never carry it;
- constructor preparation partitions ExecutionArgs / title metadata / background disposition / wake subscription before semantic hashing;
- `batch` is top-level Graph construction and `jobs.*` is direct control-plane only initially; neither is a Graph node;
- jobs is the one live-execution control plane: list/get/output/ack/send/cancel/subscribe;
- jobs.output is outbound stdout/stderr/result; jobs.send is inbound input;
- default child->parent steering is attention/settled event -> parent-owned wake; direct peer/child->parent jobs.send requires explicit communication authority;
- add Effect::Control; no Composite grant; optional internal EffectSet only;
- PreparedOperation resolves exact operation/effect/resource authority before policy/journal/execution;
- wake is factual event -> authenticated parent-owned Input, only settled and attention initially, with now/step/turn;
- Graph remains immutable while authorized live Agent/Process nodes are controllable;
- typed retained results make bread Graph-native;
- QuickJS/Python/Rust use the same operation resolver and cannot bypass authority;
- team/plan/review remain optional governed workflows, not core primitives.

First action before source changes: do a fresh read-only live-source reconciliation against these decisions, especially ToolSpec/effect ABI, Envelope ReadOnly, JobManager/NodePlan, child Agent construction, Process stdin ownership, wake parent binding, output retention and dotlink replay/MCP Tasks.

Do not implement until explicitly authorized. When authorized, follow TODO phases, reuse existing runtimes/state machines, preserve dirty/unrelated work, and avoid overengineered evidence fixtures. No commit/push/version/release without separate authorization.
