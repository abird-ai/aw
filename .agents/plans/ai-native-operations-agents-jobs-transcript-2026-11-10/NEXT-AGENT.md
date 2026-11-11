# Next-agent handoff — AI-native operations / Agents / Jobs / transcript

Author: Prasanna Loganathar
Co-authors: ChatGPT GPT-6, ChatGPT GPT-5.6 Sol
Owner: @pvl

Status: **design reviewed and resolved; implementation not authorized** (2026-11-10).
The updated canonical [DESIGN.md](DESIGN.md) and [TODO.md](TODO.md) include the DL-150 token/model-first review corrections. No implementation, test, commit or push occurred as part of this design update.

## Read order

1. Read AW and dotlink `AGENTS.md`, then each `.agents/plans/README.md` / current TODO. Preserve coordinator-owned indexes and AW `.agents/plans/ux/` as reserved/opaque.
2. Read **this** new plan's `README.md`, complete `DESIGN.md`, and `TODO.md`, including the targeted acceptance criteria and migration decisions.
3. Read dotlink's vNext companion `ai-native-operations-agents-jobs-transcript-2026-11-10/README.md` and `TODO.md` (integration only).
4. Read sealed DL-150 `dotlink-ai-native-filesystem-2026-10-31/README.md` and `SHARED-IMPLEMENTATION.md`, and AW's current `dotlink-tool-port/SHARED-AW-TOOLS-CONVERGENCE-2026-10-31.md`.
5. Consult AW's `RUNTIME-FOUR-PRIMITIVES-AND-SERVICES-2026-10-11.md` and `RUNTIME-HOOKS-TRUST-AND-POLICY-2026-10-11.md` before editing effect, Agent, writer or disclosure boundaries.
6. Inspect **live** source/tests and dirty work. Code + actually executed test results outrank historical plan prose for what already exists.

## Successor prompt

Review then (only with explicit approval) implement the new AW↔dotlink vNext AI-native operation/Agent/Job design:

- AW repository: `/home/pvl/spaces/abird/src/aw`.
- dotlink repository: `/home/pvl/spaces/abird/src/dotlink`.
- Full new specification: AW `.agents/plans/ai-native-operations-agents-jobs-transcript-2026-11-10/DESIGN.md`.
- Phased work and acceptance: adjacent `TODO.md`. dotlink companion is an integration contract only.

Read all authority sources before source edits. **Do not change the sealed DL-150 or previous Jobs/wake plans** to implement this new proposal, and do not overwrite any unrelated source/plan working-tree changes.

Crucial finalized design decisions:

- 13 DL-150 native filesystem tool names and compact Graph grammar unchanged. Small flat leaf model tools; provider sees one granted grouped `agents({op})` and `jobs({op})` surface, while `[operation,args]` is the canonical internal/SDK/Graph IR, not a second advertised model format.
- Preserve original provider ToolCall/call ID/arguments and signed correlation in original AW journal. Store prepared resolved operation/effect separately. Strict exact per-op grants and filtered model op catalogs.
- Five effects Read/Write/Process/Control/Unknown. `batch` constructor has outer Control admission plus exact authorization of each Graph leaf; ReadOnly FS can still grant bounded read-only Graph and owned Job controls.
- Agent ID pins exact existing owned Session+branch; forks get new IDs. First new Agent+Turn+Job creation receipt is recoverable and replay-safe; no second Agent or transcript database.
- One original Agent/TurnMachine/ToolPolicy/TurnJournal. A Graph child Agent is the parent's Graph Job node, not another public Job manager. No concurrent same-branch writer.
- `agents.read` uses one `filter`: omitted/`"default"` returns latest public result; `"none"` returns all authorized sanitized categories; custom true category booleans/optional exact `tools` match a subset, and `exclude:true` instead subtracts that subset from authorized public content. No `include` list/top-level `tools`. Enforce current disclosure and no inverse permission bypass, one-Turn default, final-answer dedup, byte limits and Turn/byte pagination; handle active/unreconciled/compacted history truthfully.
- `jobs.get` is status, `jobs.output` is raw retained data, `jobs.ack` is separate consumption; common bounded `wait_ms`. No default verbose title/result echoes.
- Wake via parent-owned `settled/attention` rules and `now/step/turn` safe admission; a `step` includes model invocation plus tool receipts. An interactive child whose only controller is blocked on foreground Graph **proactively hands off the same Job**.
- Shell settlement events only after truthful Process/Job status and output writer quiescence. Standalone dotlink cannot invent AW parent wake or Agent history.
- `bread(format=mcp)` should be Graph-eligible via typed retained result; captured node status/more/incomplete/`when` decisions never change due to outer response budgets.
- Distinct authorized `pwsh` and `powershell` interpreters stay separate. DL-150's effectful replay-safe Graph restart remains at legacy/advanced boundary, not default model schema; general peer messaging is deferred.
- Measure **actual** provider schemas (bytes/tokens), first-valid-call accuracy, total model/tool rounds and returned tokens across representative workflows before public cutover. Do not overengineer fixtures or evidence pinning.

Implementation should reuse the shared `aw-tools` Graph/Job/secured FS crates, AW's existing exact-authorization/journal/SessionStore/Actor owners, and thin dotlink adapters. Keep no_std/minimal/stateless, current replay/MCP Task ownership and platform release-gate honesty. Revalidate and maintain strict failure, revocation, payment, media and output-ACK contracts.

Report exact modified paths, implementation-gate outcomes, unresolved native platform checks and any new design deviation **in this plan**, without rewriting historical sealed specs. Any commit, push, version, tag, release or deployment requires separate authorization.
