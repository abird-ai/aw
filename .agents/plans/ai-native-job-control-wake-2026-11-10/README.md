# AI-native Jobs, Control, wake and Agent execution

Author: Prasanna Loganathar
Co-authors: ChatGPT GPT-6, ChatGPT GPT-5.6 Sol
Owner: @pvl

Status: **DESIGN FINAL / implementation not yet authorized** (2026-11-10).
Base: AW main 4a23e92a8f516971bb232b98e4fc000554bcc4ba; dotlink main b31683c79a8ffd7c5540ac9388e607d8af6c7ffa.

This is the next cross-repository vNext design. It is separate from the sealed
DL-150 filesystem/Graph specification and the active AW shared-tools convergence
plan. It does not amend historical claims or authorize source implementation.

- [DESIGN.md](DESIGN.md): canonical finalized semantics.
- [TODO.md](TODO.md): implementation phases and gates; implementation remains open.
- [NEXT-AGENT.md](NEXT-AGENT.md): starting prompt and authority order.
- dotlink companion:
  /home/pvl/spaces/abird/src/dotlink/.agents/plans/ai-native-job-control-wake-2026-11-10/README.md.

## Final goal

Expose a small AI-native model surface and the same semantic operations through
QuickJS/Python/Rust, while retaining one canonical shared filesystem/Graph/Job
implementation and AW's existing Agent/Session/TurnJournal/Actor runtime.

~~~text
filesystem     ls find grep read bread write edit patch cp mv rm bwrite bedit
constructors   bash pwsh batch agents
management     jobs
optional       team plan review_release mcp__* ext__*

canonical IR   [operation, args]
graph node     [operation, args, when?]

agent op       agents.run
job ops        jobs.list|get|output|ack|send|cancel|subscribe

wire fallback  agents({op:"run", ...})
               jobs({op:"get", ...})

labels         title?       <=160 UTF-8 bytes; metadata only
disposition    background?  direct invocation only; never a Graph-node semantic
interaction    interactive? where supported
observation    get/output + optional wait_ms
wake           { settled?: at, attention?: at }
steering       at = now | step | turn
effects        Read | Write | Process | Control | Unknown
~~~

The canonical semantic/API form is the compact tuple. The object + `op` form is
the only secondary projection for transports requiring object-shaped tool
arguments. Do not add nested command objects or a third canonical spelling.

## Core design principles

1. **Model-native first.** Few names, compact arguments/results, one concept per
   tool/resource and no redundant aliases.
2. **One semantic operation language.** All callers resolve into
   `[operation,args]` and the same PreparedOperation path.
3. **One invariant, one owner.** aw-tools owns native filesystem/Graph/Job
   semantics; AW owns Agent/Session/TurnJournal/ToolPolicy/Actor; adapters own
   product authority, process custody, sandbox and transport.
4. **One Agent implementation.** Root, child, recursive child, team member and
   planned worker are the same Agent runtime with different parent/delegation
   bindings. "Subagent" is a relationship, not a second Agent type.
5. **Job = live execution; Agent = durable conversational identity.** One Agent
   Turn produces one Job. A later `agents.run(id=...)` is a new Turn/new Job.
6. **Graph is immutable.** Graph structure/initial args/edges/captured ceilings
   never change. Interactive Agent or Process nodes receive separately authorized
   live input without rewriting the Graph.
7. **Events are facts; wakes are inputs.** Factual settlement/attention is
   distinct from an authenticated parent-owned wake subscription.
8. **Exact grants and narrowing.** No model, script, manifest, title, reviewer or
   policy result can mint authority.
9. **Five effects, no Composite grant.** Composite Graphs may carry a
   non-authoritative EffectSet summary, but every leaf operation is authorized
   independently.
10. **Bounded and fail-closed.** Unknown effects are never automatically replayed;
    no duplicate Agent/Job/event engine or unbounded wake loop.
11. **Targeted validation.** Prefer implementation + focused contract tests +
    existing gates; avoid evidence/fixture bureaucracy.

## Final public decisions

| Area | Final decision |
| --- | --- |
| Canonical call | `[operation,args]`; Graph `[operation,args,when?]` |
| Object-wire fallback | grouped `jobs` / `agents` with one `op` field |
| Agents | public namespace `agents`; primary operation `agents.run` |
| Agent identity | durable Agent ID resolves the existing Session/lineage; root and children use the same Agent implementation |
| Job identity | one Job ID per live top-level run; Graph child = Job ID + node |
| Background | optional on direct constructors; response disposition only; omitted from Graph nodes |
| Batch | remains a first-class Graph constructor |
| Jobs | one control plane: list/get/output/ack/send/cancel/subscribe |
| Send | inbound live input; output is separate outbound stdout/stderr/result |
| Agent continuation | live Turn: `jobs.send`; settled Agent: fresh `agents.run(id,...)` |
| Wake | settled/attention only initially; parent-owned subscription; now/step/turn |
| Effects | add Control; no Composite authority |
| Composite analysis | internal non-authoritative EffectSet |
| Read-only | filesystem ReadOnly orthogonal to separately granted Control/Process/delegation |
| Operation identity | semantic `jobs.cancel`, `agents.run`; kernel-safe `jobs_cancel`, `agents_run` |
| Binary/media | all canonical native tools, including `bread`, become Graph-eligible through typed retained results |
| Dynamic policy | optional narrowing evaluator chain over PreparedOperation; never a grant source |
| SDKs | typed methods compile to the same compact canonical IR; generic `tools.call([op,args])` is permitted |
| Team/plan | remain host-opt-in orchestration workflows, not default primitives |
| Title | optional short audit metadata; stored, available in list/get, not echoed in routine results |

## Design lineage

~~~text
Initial discussion
  many job_* tools
  subagent as separate constructor concept
  bash no explicit background
  direct-only bread in Graph
  fixed per-tool effect
       |
       v
First ai-native synthesis
  one jobs namespace
  separate agents namespace proposed
  jobs.start considered for background
  Control effect + PreparedOperation
  immutable Graph + steerable Agent nodes
       |
       v
Original draft plan
  bash/batch/subagent + background
  jobs{op=list|get|...}
  Job = universal execution handle
  Session = durable child conversation
  wake/subscribe/wait_ms
  typed media Graph results
       |
       v
FINAL MERGED DESIGN
  agents.run replaces subagent as semantic operation
  root/child use one Agent implementation
  canonical [operation,args] everywhere
  op-object only wire fallback
  background retained only as direct-call disposition
  Graph nodes never carry background
  jobs is the sole live-execution control plane
  jobs.send inbound / jobs.output outbound
  settled Agent follow-up = new agents.run + new Job
  PreparedOperation + exact per-op capability
  internal EffectSet, never Composite authority
~~~

## Preserved sources and non-goals

The sealed DL-150 and current shared-tools plans remain preserved and historical.
This design does not introduce arbitrary Graph loops/dataflow, a second planner or
Agent runtime, universal crash-resumable workflows, automatic cross-machine Agent
execution, a multi-tenant Job ACL, a global notification bus, or ambient
QuickJS/Python OS authority.

Implementation, commit, push, version, tag, release and deployment all require
separate authorization.
