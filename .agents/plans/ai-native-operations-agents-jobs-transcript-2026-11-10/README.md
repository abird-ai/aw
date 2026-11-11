# AI-native operation language, Agents, Jobs and transcript projection

Author: Prasanna Loganathar
Co-authors: ChatGPT GPT-6, ChatGPT GPT-5.6 Sol
Owner: @pvl

Status: **DESIGN RESOLVED / implementation not authorized** (2026-11-10).

This new follow-up design extends but does **not edit** the existing, sealed designs:
- AW `ai-native-job-control-wake-2026-11-10/` — prior Jobs/Control/wake plan;
- AW `dotlink-tool-port/` — current shared-tools convergence work;
- dotlink `dotlink-ai-native-filesystem-2026-10-31/` — DL-150 original AI-native filesystem/Graph/Job specification.

[DESIGN.md](DESIGN.md) is the complete normative **proposed vNext design**. [TODO.md](TODO.md) contains implementation phases and focused acceptance gates. [NEXT-AGENT.md](NEXT-AGENT.md) contains the starting prompt. The dotlink integration companion is `/home/pvl/spaces/abird/src/dotlink/.agents/plans/ai-native-operations-agents-jobs-transcript-2026-11-10/` and intentionally does not duplicate design semantics.

## Final model-native surface

~~~text
filesystem       ls find grep read write edit patch cp mv rm bread bwrite bedit
constructors     bash pwsh powershell batch agents
management       agents jobs
optional         team plan review_release + exact-granted MCP/extensions
~~~

Canonical **semantic operations**:

~~~text
agents.run, agents.read
jobs.list, jobs.get, jobs.output, jobs.ack, jobs.send, jobs.cancel,
jobs.subscribe
~~~

### One internal call language, one model-wire projection

~~~text
canonical SDK/Graph IR:  [operation, args]
canonical Graph node:   [operation, args, when?]
~~~

The tuple is **not** a second advertised provider syntax. The model sees flat leaf tools and one grouped model tool each for `agents` and `jobs`:

~~~javascript
read({path:"README.md"})
agents({op:"run", task:"Review the parser", background:true})
agents({op:"read", id:"a7"})
jobs({op:"get", id:"j42", wait_ms:10000})
~~~

Only currently permitted `op` variants are advertised; the trusted resolver validates each variant's actual args/effect/grants before execution. Do not advertise both grouped and per-op aliases. Provider ToolCall wire name/arguments/ID remain immutable original journal evidence; the effective resolved operation lives beside them in PreparedOperation. `batch` reuses the same leaf registry and never embeds a duplicated union of full nested tool schemas.

This projection is accepted only after comparing **actual serialized schema bytes, representative tokenizer cost, model selection/valid-first-call rate, complete model/tool rounds and result tokens** against DL-150. Fewer tool names alone is not an efficiency proof.

## Four conceptual services, one authoritative execution path

| Concept | Meaning | Authoritative owner |
| --- | --- | --- |
| Operation | One typed, exact-granted action | Original AW ToolPolicy/Envelope/TurnJournal plus trusted resolution |
| Graph | Immutable bounded composition of prepared leaves | Canonical shared `aw-tools::JobManager`/Graph |
| Job | One live execution or Graph-node projection | Existing Job/Process owner, no second runner |
| Agent | Durable conversation across Turns | Original AW Agent + exact existing SessionStore branch |

A root/delegated Agent uses the **same** Agent/TurnMachine/ToolRegistry/ToolPolicy/TurnJournal implementation and a narrowed Envelope. A public Agent ID binds exactly to a durable (Session, branch) pair; a fork receives a new ID, and `agents.run({id})` starts a new Turn/Job on that exact branch. Lost first-creation responses never create a second Agent/paid Turn; the identity is recoverable through its original authorized Job/Session receipt.

A top-level Agent Turn has a Job handle; a child Agent **inside** a Graph is addressed as `(Graph Job ID,node)`, not a competing independently managed Job. Its Session/Turn journal is still the only provider/effect evidence.

## Clear, minimal Job responsibilities

| Operation | Contract |
| --- | --- |
| `jobs.list` | Compact authorized Job IDs/kinds/status; no default title or bulk data |
| `jobs.get` | Status/progress, optional `wait_ms` for a meaningful state change |
| `jobs.output` | Raw-byte/typed retained output with optional `wait_ms` for bytes/EOF |
| `jobs.ack` | Explicit monotonic acknowledgment of retained delivered output |
| `jobs.send` | Exact-authorized interactive Agent Input or live Process stdin |
| `jobs.cancel` | Whole-Job cancellation intent; never fictional rollback |
| `jobs.subscribe` | Replace/clear this receiving Agent's future wake rules |

`get` never returns bulk data; `output` never ACKs; `agents.read` never reads raw Process spool or consumes Job output. Both `get` and `output` have bounded `wait_ms`; wait expiry is not an execution timeout. `title` is short optional audit metadata (1–160 UTF-8 bytes), never repeated in ordinary successful model results. `background` is **direct-call response disposition**, not a Graph node argument.

Distinct `bash`/`pwsh`/`powershell` interpreter identities retain separate exact launcher grants where available. DL-150's replay-protected retained-Graph restart is preserved at the legacy/advanced SDK boundary; the rare default model-facing `jobs.restart` operation is omitted unless measured use warrants an opt-in grant.

## Wake is not a notification broadcast or a second model engine

~~~javascript
batch({
  background:true, ops:[/* fixed Graph */],
  wake:{settled:"turn"}
})

jobs({op:"subscribe", id:"j42", wake:{attention:"now"}})
~~~

Events are factual state projections. `wake` is a parent-owned rule for when a terminal/attention event may be converted to one separately authorized AW Actor Input. The only boundaries are `now`, `step` and `turn`, relative to the **receiving** Agent. One `step` includes the full model invocation and proposed tool batch receipts; steering always admits a new journaled Turn after a safe settle/interrupt, not hidden mutation of a provider-signed message.

If an interactive Agent node's only controller is the parent currently blocked on the foreground Graph, hand off the **same** Graph Job immediately after admission so the parent can act; never deadlock by keeping the only controller awaiting its child. A Shell Job emits `settled` only after factual Process classification, stdout/stderr writer quiescence and original Job/Graph checkpoint. Synchronous result already delivered => no duplicate wake. Without a real authenticated AW parent, standalone dotlink offers passive Jobs but **not** Agent wake.

## Five effects; static ceiling first

~~~text
Read | Write | Process | Control | Unknown
~~~

`Control` manages bounded execution/retention/Input/wake and **outer Graph admission**; every native/Agent/Process leaf retains its own exact effect, resource, source/site/epoch and current-owner grant. No Composite effect; an EffectSet summary is not authority. Read-only filesystem sessions may execute granted read-only Graphs and independently granted Job controls without acquiring file Write or arbitrary shell Process rights.

Optional dynamic local-rule, program, QuickJS/Python, MCP, model/pipeline or human review can allow only **within** the original static ceiling, deny, narrow or request approval. The original ToolPolicy, Envelope, typed hooks and journal remain the sole authority; no paid review can mint missing grants or silently replay uncertain effects.

## Durable Agent transcript: `agents.read`

Use **one optional `filter`**, not an `include` list plus a top-level `tools` selector:

~~~javascript
agents({op:"read", id:"a7"})                         // default result only
agents({op:"read", id:"a7", filter:"default"})       // explicit default
agents({op:"read", id:"a7", filter:"none"})          // all authorized public categories

agents({op:"read", id:"a7", filter:{
  result:true,
  tool_calls:true,
  tool_outputs:true,
  tools:["read","grep"]
}})

// Exclude tool outputs, retain other authorized categories
agents({op:"read", id:"a7", filter:{
  tool_outputs:true, exclude:true
}})
~~~

- **`"default"`**, or omitted: latest committed public result only, `limit=1`, minimum normal model tokens.
- **`"none"`**: no optional category/tool-name selection filtering; render all **currently authorized** public categories, subject to the same default Turn count and hard output-byte limits. Never disable security redaction or reveal provider-private data.
- **Custom object**: optional `result`, `messages`, `tool_calls`, `tool_outputs` booleans, optional exact-name `tools` subset and one optional inversion flag `exclude:true`. At least one category must be true; `tools` requires a true tool category.
- Without `exclude`, include only matches. With `exclude:true`, keep the full currently authorized public view **except** matching categories/records (e.g. omit all tool outputs, or only Bash tool outputs). Inversion is a selection rule, not a disclosure grant or permission to bypass mandatory redaction.

When result and messages are selected together, publish the final answer **once**. Historical Turn pagination remains `more before=N`; a large Turn's selected, sanitized projection continues via `more turn=T byte=B`, using the **same filter** and current disclosure authority.

Transcript disclosure requires independent current Agent/Session/branch and category/resource permissions. Parent lineage, running an Agent, or observing its Job does not grant its tool arguments/outputs. Never expose private reasoning/context, credentials, system prompts, or revoked sensitive material. `jobs.output` stays a distinct raw-byte/ACK operation, not an Agent transcript filter.

## DL-150 normative inheritance and design acceptance

Keep DL-150's 13 one-concept-per-tool filesystem schemas, exact paths, compact stable Graph tuple, fixed topology and captured per-node result/`more`/`incomplete` semantics, truthful mutation and aggregate `ok/unchanged/fail/mixed/unknown/skip`, cumulative progress, no output echo, raw-byte Job paging/ACK and replay protection. Outer transport fitting must **never** change a Graph node's dependency outcome. Graph `bread(format=mcp)` becomes eligible via a shared typed retained-result representation, not a lossy text workaround.

Reject invisible new daemons, alternate Agent/TurnMachine, extra Graph manager, universal event bus, generalized transcript store, generic peer router, code-runner or automatic cross-process Job resurrection. Optional team/plan/review remain governed higher-level workflows; direct parent-to-child steering and child-to-parent Notify are sufficient for the default model interface.

### Status and next step

The design-review gaps were incorporated **into this new plan only**, including provider provenance, grouped-schema cost, read-only Graph admission, interactive-child handoff, Agent branch/replay identity, transcript security/byte paging, distinct PowerShell, and intentional advanced restart/peer choices.

Implementation, code changes, commit, push, version, release and global plan queue promotion are **not** authorized by this documentation revision. Review [TODO.md](TODO.md) and [NEXT-AGENT.md](NEXT-AGENT.md) when starting the next approved implementation phase.
