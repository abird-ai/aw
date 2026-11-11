# Final design: AI-native operation language, Agents, Jobs and transcript projection

Author: Prasanna Loganathar
Co-authors: ChatGPT GPT-6, ChatGPT GPT-5.6 Sol
Owner: @pvl

Status: **DESIGN FINAL, 2026-11-10. Implementation is not yet authorized.**

This is a new follow-up design. It preserves the prior
`ai-native-job-control-wake-2026-11-10/` plan unchanged and records the final merged
API plus filtered durable Agent transcript access.

## 0. Authority order, scope and constraints

Implementation must still begin from live source/tests. This design does not claim
the APIs already exist.

Preserve:

- one AW Agent / TurnMachine / ToolRegistry / ToolPolicy / TurnJournal;
- Runtime, Actor, Message, Envelope as the four runtime primitives;
- Next/Defer as the hook verbs;
- shared native filesystem/Graph/Job semantics in `aw-tools`;
- secured filesystem backends in `aw-tools-fs-unix` / `aw-tools-fs-win`;
- AW SessionStore/history as the durable Agent transcript owner;
- AW Process custody and dotlink's product-specific launcher/sandbox;
- exact actor/source/site/epoch/effect grants and current writer ownership;
- fail-closed unknown effects and no automatic replay of ambiguous effects;
- raw-byte Job output ownership/ACK and transport replay semantics;
- no_std/minimal/stateless feature boundaries.

Do not introduce:

- a second Agent implementation for children;
- a second Job/Graph manager;
- a transcript database beside SessionStore;
- a global event bus;
- arbitrary Graph loops/dynamic dataflow;
- nested Graph/Job control nodes in vNext;
- ambient QuickJS/Python OS/network authority;
- implicit parent transcript access solely because of lineage;
- hidden reasoning/provider-private continuation disclosure.

## 1. Model-native design principles

### 1.1 One semantic operation language

Canonical **semantic** operations are stable names such as `read`, `agents.run`, `agents.read` and `jobs.get`. The shared typed call IR and Graph syntax are compact:

~~~text
[operation, args]
[operation, args, when?]   # Graph only
~~~

For example:

~~~javascript
["read", {"path":"README.md"}]
["agents.run", {"task":"Review cancellation"}]
["jobs.get", {"id":"j42", "wait_ms":10000}]
~~~

**The tuple is an internal/SDK/Graph representation, not a second model-visible call syntax.** A provider that calls named tools sees authorized flat leaf tools plus two grouped resource tools:

~~~javascript
read({path:"README.md"})
agents({op:"run", task:"Review cancellation"})
agents({op:"read", id:"a7"})
jobs({op:"get", id:"j42"})
~~~

One model-visible tool name per resource family. The host resolves grouped calls to exactly one trusted semantic operation before policy/preparation. A model is never advertised both `jobs.get` and `jobs({op:"get"})` (nor both forms of `agents.run`) in the same catalog. There is no public `tools.call` model tool; that generic tuple form belongs only to a host-mediated SDK.

The advertised grouped schemas are **effective-grant filtered**, not permanently broad. Advertise only authorized operation values, use short JSON Schema property descriptions, and validate exact per-operation required/excluded arguments at the trusted resolver. Do not embed a duplicated copy of every leaf tool's schema inside `batch`. The schema should make choosing `agents.run` vs `agents.read`, and `jobs.get` vs `jobs.send`, nearly mechanical. Unsupported op/field pairs fail before any effect, including when a provider only supports a compact flat envelope rather than a discriminator-aware union.

Grouped names are an optimization hypothesis, not proof of token efficiency. Measure generated **serialized schema bytes and tokenizer estimates**, representative tool-selection/schema-valid-call rates, end-to-end model/tool rounds, and returned-result tokens versus the DL-150 baseline. Use the grouped form as the default vNext candidate; before cutover, if it materially degrades model reliability or total token cost, revisit **this** design and choose one smaller measured projection. Do not advertise simultaneous aliases or extra operation variants merely for compatibility.

### Provider provenance is immutable

The original provider ToolCall (wire name, validated original arguments, call ID and provider correlation identity) is immutable factual evidence. A separately host-owned PreparedOperation records the trusted resolved semantic operation/effect, normalized **execution** arguments, source/epoch/site, permissions and any authorized revision. Never rewrite the original provider call in the transcript or journal to make it look as if it originally used a canonical semantic name.

Keep the original and effective calls linked by the same call ID and source-bound revision evidence, exactly as AW's existing ToolRevised/ToolStarted journal contracts require. Historical transcript/tool filtering may select the trusted resolved name only when that mapping was durably recorded or proven by a versioned compatibility adapter; it must never infer authority from untrusted `op` text alone. Model-wire and SDK paths converge on one prepared executor, not additional policy/Agent engines.

### 1.2 Flat when atomic, namespaced when resource-oriented

Keep atomic primitives short:

~~~text
read
write
edit
bash
batch
~~~

Use namespaces only when several operations act on one durable/live resource:

~~~text
agents.run
agents.read

jobs.get
jobs.output
jobs.send
...
~~~

This minimizes advertised names without turning unrelated atomic operations into
verbose `fs.read` / `process.bash` namespaces.

### 1.3 One prepared path

Each invocation uses the original AW admission and factual journal path; the resolver is a trusted preparation step, not another execution engine:

~~~text
Provider wire ToolCall (immutable) / typed SDK operation
  -> strict schema and trusted source/operation resolution
  -> PreparedOperation (original + effective identity, ExecutionArgs, metadata)
  -> exact ToolPolicy + Envelope + operation/resource/owner checks
  -> optional narrowing dynamic policy, full re-resolution after revision
  -> original intent/attempt journal and current binding recheck
  -> original AW Agent/Process or shared aw-tools executor
  -> factual receipt + current disclosure authority + compact projection
~~~

A grouped model name alone confers no authority. Nested Graph leaves use the same resolver, including conditional nodes that might eventually skip.

Preserve DL-150's original six primary outcomes, more/incomplete modifiers, atomic discovery records, native mutation commit classifications, captured Graph semantic ceilings, replay ownership and compact result rendering. The sealed original remains normative, not a specification to duplicate in a second implementation. See Section 11 for the outer Graph Control admission rule.

## 2. Final core surface

### 2.1 Native filesystem operations

Preserve the 13 canonical DL-150 operations:

~~~text
ls find grep read bread
write edit patch cp mv rm bwrite bedit
~~~

`ls` remains shallow, `find` recursive, `grep` content search.

All canonical native leaf operations become Graph-eligible once typed retained
results support `bread(format=mcp)`.

### 2.2 Constructors

Canonical direct constructors:

~~~javascript
["bash", {
  command, dir?, stdin?, timeout_secs?, max_output_bytes?,
  writes?, title?, background?, interactive?, wake?
}]

["pwsh", {
  command, dir?, stdin?, timeout_secs?, max_output_bytes?,
  writes?, title?, background?, interactive?, wake?
}]

["powershell", {
  command, dir?, stdin?, timeout_secs?, max_output_bytes?,
  writes?, title?, background?, interactive?, wake?
}]

["batch", {
  ops, title?, background?, wake?
}]

["agents.run", {
  task, id?, title?, background?, interactive?, wake?
}]
~~~

`pwsh` and `powershell` are distinct prepared interpreters, not aliases. Advertise each only when the exact interpreter is available and authorized on that platform. An older spelling may remain accepted at the legacy wire boundary, but cannot redirect to a different interpreter without explicit grant.

### 2.3 Resource operations

~~~text
agents.run
agents.read

jobs.list
jobs.get
jobs.output
jobs.ack
jobs.send
jobs.cancel
jobs.subscribe
~~~

No default `jobs.start`, `jobs.restart`, `agents.send`, `job_wait`, `notify`,
`set_notify` or separate model-facing watch handle.

## 3. One Agent implementation

"Subagent" is a relationship, not an implementation type.

~~~text
                         Agent
                           |
              +------------+------------+
              |                         |
             root                    delegated
              |                         |
           Session                    Session
              |                         |
            Actor                      Actor
              |                         |
      same TurnMachine          same TurnMachine
      same ToolRegistry         same ToolRegistry type
      same ToolPolicy           narrowed grants
      same TurnJournal          same journal API
~~~

Delegation code should only:

1. allocate/validate parent lineage and depth;
2. reserve child budget/capacity;
3. choose a host-authorized route/model/account;
4. narrow Envelope/tool/resource authority;
5. bind/create the durable Session/branch;
6. instantiate/rebuild the same Agent runtime;
7. run the same controller/journal path.

Do not introduce `SubagentAgent`, TeamAgent or PlannerAgent executors.

Team candidates and planned workers are ordinary Agents under additional host
topology/review policy.

## 4. Agent and Job identity

### 4.1 Agent = one exact durable conversation branch

An Agent ID is a stable opaque host-issued handle for one owned **(Session ID, exact branch ID)** binding in the existing AW SessionStore, not a second Agent/transcript database or a mutable alias to a branch named `main`. Reuse the durable Session/branch identity; any handle encoding must be deterministic or recoverable through the same authoritative store, not a separate mutable mapping.

A branch fork creates a **new Agent identity**. Renaming an existing branch preserves its ID. Running an existing Agent never silently switches branches. All references require current principal, parent/lineage, store, branch, writer epoch, source, provider/account and effective budget verification; a guessed Agent ID is not authority.

`agents.run` without `id` creates a durable Session/branch binding and starts its first Turn. With `id` it starts one **new Turn** on that exact existing branch after current continuation authority is verified. The old Turn/Job remains historical, never reopened or restarted.

~~~text
agents.run(new task)        -> Agent a7, Turn 1, top-level Job j42
agents.run(id=a7, new task) -> same Agent a7, Turn 2, new Job j57
~~~

Child Agents remain ordinary narrowed AW Agent/Session instances using the same original TurnMachine/TurnJournal; a read-only session can independently grant bounded Agent delegation without FS Write or arbitrary shell permission.

### 4.2 Job = one execution, not a second Agent/Graph journal

A top-level Agent Turn, shell command or Graph run exposes one Job execution handle. AW's existing Agent/TurnJournal owns model calls, paid/effect evidence and durable history; shared aw-tools JobManager owns the Graph/Node lifecycle and retained output. When an Agent runs **inside** a Graph, its Turn is projected through that Graph's existing node coordinate—not an additional independently managed public Job, competing Graph manager or duplicate provider journal.

~~~text
top-level Agent         j42
Graph Agent node        (j42, node=2)  # child Session separately authoritative
~~~

Node-scoped `jobs.get/output/send/subscribe` use the parent Graph ID and coordinate. A future `agents.run({id})` follow-up creates a fresh top-level Job/Turn, without changing the settled Graph node. Any Graph involving the same existing Agent branch in concurrent nodes must reject conflicting writer ownership before the roots start; Graph scheduling hints never create authority to run simultaneous branch writers.

### 4.3 Creation receipts, loss and recovery

Before any paid provider effect, fresh `agents.run` must durably bind its new Agent/branch identity, initial Turn and reserved Job/replay admission under one recoverable request identity. The original complete normalized **invocation envelope**, including disposition/title/wake, is checked on redelivery; same identity and same request reattaches, a changed request fails closed, and no second Agent or Turn is started.

A fresh Agent's first successful result **includes its Agent ID even for synchronous completion**; it is the continuation handle, not redundant request echo. A background response includes minimal Agent ID, Job ID and state. When a synchronous answer fits, publish the answer **once**, with the short Agent handle; do not force a second `agents.read` or duplicate wake for the delivered result.

A newly created Agent's Job is recoverably exposed/reserved **before** the first Turn may execute, including when optimistic synchronous execution is expected. During Job retention an authorized `jobs.list/get` can recover the associated Agent ID after a lost response. After Job expiry, the original authorized Session/branch discovery remains the long-lived Agent identity source. Do not invent a new public Agent listing merely to duplicate AW's existing Session discovery or claim a dotlink Job survives process replacement.

A lost or ambiguous creation without a reconstructible original receipt fails closed—never re-runs model inference or creates a replacement Agent to make a missing handle reappear. Current continuation/disclosure permission is revalidated on every identity projection.

## 5. Invocation side semantics

Constructor arguments partition into:

~~~text
ExecutionArgs
  actual operation semantics
  interactive belongs here

Metadata
  title

Disposition
  background

Subscription
  wake
~~~

### 5.1 title

- optional;
- 1..160 UTF-8 bytes;
- short human/audit intent only;
- no `desc`, `description` or `label` alias;
- never permission, proof, scheduling authority or semantic execution identity;
- may appear in bounded `jobs.list/get`, not routine success output.

### 5.2 background

`background=true` means **return the admitted Job immediately**.

False/omitted means optimistically wait and return the direct result when compact;
if handoff becomes necessary, expose the **same Job**.

It never means shell `&`, does not respawn work and does not change operation
effect.

Graph nodes never accept `background`: the enclosing Graph Job already owns
asynchronous scheduling.

### 5.3 Replay identity

Distinguish **execution equivalence** from **transport redelivery identity**:

~~~text
Execution fingerprint: semantic operation + normalized ExecutionArgs.
Redelivery fingerprint: complete admitted invocation envelope, including
                        title, background and wake configuration.
~~~

A different title/background/wake must not produce a different *side-effecting*
execution for the same request ID, nor may it silently mutate the existing
creation's subscriptions or return-disposition contract. It is a mismatched
redelivery, which fails closed. A matching duplicate returns the original
factual outcome/handle. The first committed metadata and wake rule win;
subsequent authorized changes use `jobs.subscribe` and cannot reinterpret
the original creation.

The immutable provider ToolCall and any separately approved argument revision
remain explicit journal provenance. The semantic hash/replay ledger must never
derive authority from untrusted metadata or omit actual execution arguments;
preserve DL-150's original Graph request hashing and protected transport
replay/Task owner boundaries.

## 6. Jobs: one live-execution control plane

### 6.1 list

~~~text
jobs.list
~~~

Returns authorized exposed Jobs only, newest-first, bounded pagination, compact
`id kind status` by default. `title` is omitted from ordinary model list/get rows,
but may be shown in an explicit detail query or the UI/audit. A durable Agent ID
may be included when needed for continuation/recovery and currently authorized.

No broad process/Session enumeration.

### 6.2 get

~~~text
jobs.get(id, node?, wait_ms?)
~~~

Status, lifecycle and compact progress **only**. When `node` is absent return whole-Graph/Job status; when specified return the exact node. Never bulk stdout/media, Agent transcript, hidden reasoning, implicit output ACK or transcript field duplication.

With `wait_ms`, subscribe to the **current status revision before taking the initial snapshot** (or equivalent atomic generation check) and return immediately if terminal, otherwise on the first meaningful lifecycle/progress change or expiry. On expiry, return the current factual status, not a fabricated execution timeout; unrelated internal implementation noise is not a model-visible change.

### 6.3 output

~~~text
jobs.output(id, node?, stream?, offset?, limit?, encoding?, wait_ms?)
~~~

Outbound execution data only:
- Shell streams: `stdout`, `stderr` and `result` (compact terminal metadata, **not** duplicate stdout).
- Native/Agent nodes: `result` (bounded typed content, not a durable Agent transcript).
- Shell defaults to stdout; native/Agent defaults to result; explicit invalid stream/node combinations fail instead of guessing.
- A multi-node Graph requires `node` to select an output/ACK stream. A one-node Job may omit it.

The `offset` and `limit` select a positive bounded **raw retained-byte** window; text may be lossy for arbitrary process bytes, base64 or typed content preserves exact source bytes. The next raw offset is independent of encoded presentation. No ACK or cleanup is implied.

With `wait_ms`, return immediately when requested bytes exist, otherwise wait for bytes becoming available at that offset, definitive stream seal/EOF, or the bounded wait expiry. A sealed empty stream is an honest EOF, not an OutputMissing forever; expiry does not alter execution timeout or Job state. Use the same lost-wakeup-safe revision check as get. Reads never hold an execution/Actor lock while waiting.

Retained `Agent result` is only that Turn's bounded **public answer and factual outcome**, not its historical conversation, provider-private continuation or original model/tool stream. A synchronous Agent answer is returned once from `agents.run`; an asynchronous/Graph node may be read through `jobs.output` while retained, and later through `agents.read` under its different owner/authorization when needed.

### 6.4 ack

Monotonic idempotent release of bytes actually delivered to this exact output
owner. Never clears another replay/native MCP Task claim.

### 6.5 send

Inbound live input only.

Agent:

~~~javascript
["jobs.send", {
  "id":"j42",
  "text":"Also inspect the symlink race",
  "at":"step"
}]
~~~

Shell:

~~~javascript
["jobs.send", {"id":"j43", "text":"yes\n"}]
["jobs.send", {"id":"j43", "eof":true}]
~~~

Agent target:

- requires active interactive Agent Turn;
- `text` nonempty;
- `at` required;
- uses existing ActorMessage::Input / Session writer path;
- no prompt rewrite or mutation of an already paid/signed request.

Shell target:

- requires retained interactive stdin;
- `text` XOR `eof`;
- rejects `at`;
- requires exact current Process/input/sandbox authority;
- successful send proves host write/flush only, not application consumption.

### 6.6 cancel

Whole Job only initially.

Idempotent cancellation intent, propagating to authorized descendants/process nodes.
Never claims rollback of already committed effects or successful process kill without
real custody proof.

### 6.7 subscribe

Atomically replace or clear **this authenticated receiving Agent's** future
wake rule for one Job or Graph node. No separate public subscription identity
or immediate notification; `wake:{}` clears the rule. A terminal Job's
previous event is never retroactively re-fired merely by subscribing.

Only an authenticated AW parent/recipient with current `agent.wake` authority
can register a model wake. Standalone dotlink keeps passive events/get/output
and rejects/hides wake where no real AW parent binding exists.

### 6.8 Retained-Graph restart: optional advanced surface

The default vNext model schema omits `jobs.restart` to avoid expanding the
common tool-choice surface. This is an intentional **presentation** decision,
not a removal of DL-150's `job_restart` guarantee. Preserve the legacy
operation and host/SDK capability during migration; make an explicitly granted
advanced `jobs.restart` projection available only if real tasks justify it.

A restart requires one retained terminal immutable Graph, a *new* Job and
request identity, complete current node/schema/tool/authority/budget
revalidation and replay protection before any effect. It is not Agent-Turn
continuation and never silently retries an unknown execution. Do not build
a second restart or Job engine.

## 7. Durable Agent read / transcript projection

This is the deliberate vNext extension beyond DL-150. A durable Agent persists across Turns; a Job represents one live execution and may expire. Do not overload Job output with historical conversation.

~~~text
jobs.get     = bounded live execution status
jobs.output  = retained execution stdout/stderr/result (raw-byte cursor + ACK)
agents.read  = sanitized committed durable Agent history (no Job ACK)
~~~

The authoritative source is AW's existing SessionStore/branch lineage and immutable Turn/effect receipts. No second transcript cache/database and no reconstruction by rerunning a model.

### 7.1 Canonical API, filter and pagination

Use **one optional `filter` parameter**, not `include` plus a separate top-level `tools` argument. Two common presets and one compact custom form cover both normal delegation and diagnostics:

~~~javascript
agents({op:"read", id:"a7"})                          // default
agents({op:"read", id:"a7", filter:"default"})        // explicit equivalent
agents({op:"read", id:"a7", filter:"none"})           // unfiltered authorized public view

agents({op:"read", id:"a7", limit:2, filter:{
  result:true,
  tool_calls:true,
  tool_outputs:true,
  tools:["read","grep"]
}})

// Exclude tool outputs, keep all other authorized categories
agents({op:"read", id:"a7", filter:{
  tool_outputs:true, exclude:true
}})

// Exclude only Bash calls and outputs, keep everything else
agents({op:"read", id:"a7", filter:{
  tool_calls:true, tool_outputs:true,
  tools:["bash"], exclude:true
}})
~~~

The exact input type (illustrative; the model sees only one `filter` property):

~~~text
filter? = "default" | "none" | {
  result?: boolean,
  messages?: boolean,
  tool_calls?: boolean,
  tool_outputs?: boolean,
  tools?: string[],
  exclude?: boolean

}
~~~

- Omitted / `"default"`: **latest committed public result only** (result category, one Turn by default), minimal tokens and no tool evidence.
- `"none"`: **no optional projection/category/tool-name selection filter**. Request all categories currently permitted by the *receiving caller's current* disclosure policy, in the same sanitized public projection. It does **not** mean "no results" or raw unredacted private transcript; system/hidden/provider-private data and denied sensitive tool contents remain excluded. It does not turn off the host byte/Turn limits or change the default one-Turn window.
- Custom object: one exact **matching predicate**. A category matches when its boolean is `true`; omitted/false means not matched. `tools` optionally restricts matching within selected `tool_calls`/`tool_outputs` to bounded unique exact semantic names. Reject `tools` without at least one true tool category, and reject a custom object with no true category—even if `exclude:true`; use `"none"` to request the unfiltered authorized view.
- Omitted/`exclude:false`: **include matching items only**. This is the existing default; there is no implicit `result` in a custom object. An explicitly selected category without its disclosure grant is denied, not silently omitted.
- `exclude:true`: **exclude matching items from the entire currently authorized public view**. A true category with no `tools` list removes that category; a true tool category with `tools` removes only records of those operations, keeping other records in that category. Other, nonmatched authorized categories remain. Excluding a category the caller cannot read does **not** require that category's disclosure grant. There is no invert-then-invert or double-negation rule.
- `"none"` and inverted filtering both operate only on currently permitted public content. They cannot disable mandatory security redaction, expose inaccessible categories or bypass byte/Turn limits. If no category is visible, fail closed; do not describe a restricted view as an original unredacted transcript. Revalidate every disclosed field/resource before rendering.
- The `filter` is solely a **view selector**: it never grants history, tool output, raw retained Job bytes or continuation permissions. `agents.run` and `jobs.output` have no such filter.

Other arguments:

| Name | Meaning |
| --- | --- |
| `id` | Required opaque durable Agent ID bound to one exact Session/branch |
| `filter` | Optional preset or custom predicate; `exclude:true` removes matches instead of including them; default `"default"` |
| `turn` | Optional exact immutable committed Turn ID |
| `before` | Optional exclusive older-Turn cursor, only when `turn` omitted |
| `limit` | Number of selected Turns (default 1, host-capped, initial target 16), positive |
| `byte` | Optional UTF-8 byte position within one exact `turn`'s deterministic authorized rendered projection; valid only with `turn` |

Return up to `limit` visible committed Turns before `before`, ordered oldest-to-newest. When older visible Turns remain, return `more before=N` with the next exclusive older-Turn cursor. Exact `turn` returns only that Turn; `turn` and `before` are mutually exclusive. If a bounded page cannot include a selected Turn's full projection, expose its exact Turn ID and `more turn=T byte=B` rather than silently dropping content or returning a non-advancing cursor. Resume with the **same exact `id` and `filter`**, plus `turn=T,byte=B`; byte offset refers to the canonical UTF-8 serialized *sanitized* projection, not private database bytes. Changing `filter`, Agent branch or effective disclosure revision invalidates continuation assumptions; restart at byte zero or reject a provably stale cursor.

A large Turn must not make its result permanently inaccessible. Render a contiguous bounded prefix, split only at valid UTF-8 boundaries, place exact text data and any continuation/control marker in separate content blocks when necessary, and never pretend an incomplete JSON object/record is an entire one. The host can impose a fixed response-byte limit without advertising it as routine model input. If even the minimum data unit cannot fit after transport framing, fail with an actionable limit rather than issuing an empty `more` cursor.

The source Turn is immutable; every page is nevertheless reauthorized under current actor/branch disclosure authority. Do not invent a server-side transcript snapshot engine. As with DL-150 live cursors, different policy/projection settings between calls require a fresh read; the caller must not mix incompatible offset windows.

`agents.read` never reissues a paid model attempt, does not grant continuation or steering, and never acknowledges/consumes retained Job output.

### 7.2 Orthogonal projection categories, with deduplicated output

The host renders a compact deterministic sequence of authorized Turn records, using field names only where needed for clarity. Data projected in one category is not repeated in another **within the same response**.

| Category | Contents |
| --- | --- |
| `result` | Turn ID, outcome, final public answer, bounded public error and needs-inspection marker; default |
| `messages` | Public user/assistant conversational text **including the final assistant answer**, excluding Tool/System/host context/private continuation |
| `tool_calls` | Original factual call ID, source tool wire name, trusted resolved semantic name where recorded, bounded authorized arguments, effect and terminal/denied status |
| `tool_outputs` | Factual recorded outputs/status/errors/truncation tied to original call ID and resolved name; historical text/typed summary only, not Job spool bytes |

`messages` must behave like ordinary public conversation history, not an unexpectedly partial list that omits the final answer. When both `result` and `messages` are requested, the final answer appears **once** under `messages`; the `result` portion supplies only the outcome/Turn ID/error/reconciliation metadata. With `result` alone, it includes the complete bounded final answer (subject to exact byte pagination). Do not duplicate Tool-role output as a `messages` entry, nor tool arguments in both messages and tool_calls.

A `tool_outputs` request does not require `tool_calls`: it includes only the minimum call ID/name to correlate output. Stored output may already have been intentionally bounded/truncated; preserve its original factual truncation marker. If exact historical shell bytes are no longer retained, do not claim the transcript contains them.

The existing `Turn` record stores prompt/answer/context messages/tool records. Construct public projections by selecting **allowlisted fields from authoritative Turn and ToolRecord evidence**, not by serializing `Turn` or replaying all stored context and deleting a few fields. Original assistant tool-call proposals may contain provider-private correlation material and tool results may contain secrets. Preserve the factual original/effective call distinction and do not report an invented normalized call as the original provider proposal.

### 7.3 Exact tool filtering

Within a custom filter, `tools:["read","agents.run"]` restricts **matches** inside categories selected by `tool_calls:true` and/or `tool_outputs:true`, using verified resolved semantic operation identity. Without `exclude`, only matching records are included. With `exclude:true`, those matching records are removed while other currently authorized records in those categories remain. A selected tool category without `tools` matches all records in that category. No separate top-level `tools` parameter is advertised; modern records retain the original provider wire identity and trusted resolved operation separately.

For historical records, translate only through an exact versioned, trusted legacy registration mapping. If the resolution identity cannot be established, preserve the recorded wire name as such; never parse untrusted JSON and relabel it as an authorized semantic operation. Tool-name filtering is selection only, not disclosure authority. No glob/regex/history indexing engine is needed in vNext.

### 7.4 Active Turns and history-compaction boundaries

`agents.read` is **not** an in-flight provider trace. The existing SessionStore branch reader currently refuses active/unreconciled runs. For initial vNext, specify **fail closed with a short busy/unreconciled result** while that exact branch is active; direct callers to `jobs.get/output` for live progress. Do not build a concurrent uncommitted-history reader merely for convenience.

A committed-history reader must correctly distinguish the complete authoritative branch history from the potentially compacted *model-context tail* (`HistoryState`). If a requested older Turn is unavailable from a verified immutable SessionStore/journal record, return an explicit `incomplete=compacted`/unavailable-history diagnostic; never fabricate it from a summary, quietly skip it, or claim the latest active-context tail is full retained history. Support only existing safe indexed reads initially; don't introduce a second long-term transcript store or full export for a normal result-only query.

### 7.5 Disclosure authority and sensitive records

`agents.read` has `Effect::Read` but not implicit transcript disclosure privilege. Require:
- exact `agents_read` source/site/epoch/actor operation grant;
- current **exact** Session/branch owner, writer epoch, lineage and current host disclosure policy;
- a host-owned projection ceiling such as `Result` / `Messages` / `ToolCalls` / `ToolOutputs`; explicitly **included** filter categories require grants, while `exclude:true` only subtracts matches from currently authorized content and does not require grants for data it removes; `filter:"none"` projects only currently authorized categories;
- applicable current source/content disclosure fences where tool arguments or outputs could reveal revoked/denied or otherwise private data.

Creating a child, having delegated it, reading its Job status, or being its parent does **not** grant full transcript access. In particular a user's or parent's permission to see a final reviewed result does not automatically grant past raw tool arguments, private shell outputs, secrets or protected filesystem contents.

Explicitly deny provider-private reasoning/continuation, system prompt, hidden context blocks, provider/account/payer/credential handles, raw Session encryption internals, hidden policy/review payloads and unrelated branch content regardless of `filter`. Public User/Assistant text also needs current disclosure checks; merely having Role::User is not a release token.

A failed/deferred/redacted disclosure is never reported as a complete original transcript. The native journal stays immutable; projection is the only authorized public view. For vNext, per-category grants are sufficient. Do not create a new general field-policy language; if a caller needs only tool names but is not permitted arguments, it cannot request `tool_calls` in full and a future small explicit projection can be reviewed separately.

### 7.6 Token-efficiency contract

Default exactly `filter="default"` (or omitted) and `limit=1`: select the latest committed public result, with no repeated task, titles, trace, output fields, model identity, usage or other metadata. `filter:"none"` is explicit and may cost substantially more output tokens, but always remains host byte-bounded and disclosure-limited.

For the usual parent/child flow, a foreground `agents.run` that returns a complete result does **not** need an additional `agents.read` call. A background/Graph child emits a compact handle/settlement event and then the authorized parent may read precisely the latest result. Only request `messages` or tool evidence when genuinely needed.

Apply DL-150's atomic-record and compact-outcome philosophy to every projection: one copy of each field, explicit incomplete/truncation markers, positive advancing cursors, no duplicate per-entry verbose JSON envelopes, no unneeded host policy ceilings in default tool schemas. Add focused coverage for one oversized final answer, one oversized historical output, compaction and active-reader denial; avoid exhaustive per-field fixture permutations.

## 8. Cross-Agent communication

### Default parent/child flow

The default communication topology is intentionally small and follows AW's
original Actor authority:

~~~text
parent -> owned live child:    jobs.send (Control-effect tool, Actor Input)
child  -> owning parent:       factual Notify / attention event
parent's own wake rule:        Notify -> separately authorized parent Input
~~~

A child does not gain permission to steer a parent or sibling by knowing its
Agent/Job ID or by emitting a Notify. The parent may inspect an explicitly
authorized child result using `agents.read` or `jobs.output` under their
**different** disclosure/retention rules.

### Optional peer communication is deferred

Independent peer/sibling/child-to-ancestor `jobs.send` is **not** a prerequisite
for initial vNext Agent nodes, wake or delegation. If later workloads require
it, the host may opt into an explicit directed communication topology with
separately verified recipient/lineage/generation, consent, writer lease,
rate/turn budget and no escalation/cycles. The default model cannot address
arbitrary Agents or use `jobs.send` to circumvent the parent-owned Notify
boundary. This avoids a universal messaging router or extra model tools.

## 9. Wake and notification semantics

A factual **event** is committed by its authoritative Job/Process/Agent
execution owner. A **subscription** is a parent-owned rule mapping that event
to a requested wake boundary. A **wake** is a separately authorized Actor Input
to the receiver; it is never equivalent to delivering a passive Notify, and it
never itself proves a new paid model Turn executed.

~~~text
wake: {
  settled?:   "now" | "step" | "turn",
  attention?: "now" | "step" | "turn"
}
~~~

- `settled`: exactly one factual terminal event for success, failure, cancelled or unknown execution (including shell Process and Graph node settlement).
- `attention`: an explicit, bounded, authenticated child Agent request. Arbitrary stdout/stderr/tool output/MCP text cannot claim attention.
- No automatic paid wake for progress ticks, stdout arrival, transcript append or ToolRecord changes. Observation remains possible via get/output/host UI.
- `wake` may be supplied at creation, or changed later with `jobs.subscribe({id,node?,wake})`. One parent-owned rule per source scope; replacement is atomic, `wake:{}` clears, and subscribing after settlement is never retroactive.
- Graph-level subscriptions observe Graph settlement; node-level subscriptions observe that node. No implicit recursive fanout to unrelated sibling/ancestor Agents.
- Creating a wake rule requires an authenticated parent Agent destination and current grant; standalone dotlink **does not advertise a functioning wake** without a real AW host binding.

### at — one scheduling vocabulary

Always relative to the **receiving Agent**, not to the Job producing the event:

| at | Active recipient | Idle recipient |
| --- | --- | --- |
| `now` | Request earliest safe interruption/redirect after current evidence is safely journaled; never mid-publication or with lost paid provenance | Next admitted Turn |
| `step` | Finish the current model invocation **and all its proposed tool-call receipts**, then redirect before any subsequent model Step | Next admitted Turn |
| `turn` | Let the current Turn complete normally; then admit a new Turn | Next admitted Turn |

AW's Step includes the complete proposed tool batch; `BeforeToolBatch` is **not** equivalent to `step`. Steering must never inject user text into an already provider-signed/persisted model message. The admitted steer/wake Input starts a **new, separately journaled Turn** after safely interrupting/settling the prior Turn (`now`/`step`), or after its natural completion (`turn`). Preserve original paid and tool outcomes or mark reconciliation required; no auto-retry of an uncertain attempt.

An exact requested active Step/Turn already passed fails as too late; no silent retarget to a later one. A relative request to an idle Agent maps to its next authorized Turn. `jobs.send` to a shell rejects `at` because Process stdin is not Agent steering.

### Liveness: interactive child versus waiting parent

A synchronous parent cannot answer a child Agent's attention request while its own model Tool batch is still blocked awaiting that child. This must be resolved **by the existing Job handoff** rather than a second scheduler, unjournaled nested model call or an indefinite wait:

- If a Graph has an interactive Agent node whose **only permitted controller is its invoking Agent**, the host **exposes and returns the same Graph Job handle immediately after admission**, before the child can block on that parent's input. This safe handoff applies even when the constructor used optimistic foreground default; the opt-in interactive node makes continued foreground waiting nonviable. The handle result completes the original tool call/Step through the normal journal, allowing the parent to act through later controlled inputs.
- If a separately authorized independent controller can service that child, the caller may continue ordinary optimistic foreground waiting. If the controller later disappears or all eligible controllers are blocked, hand off before waiting further or settle the child with a truthful unavailable/timeout status; never leave a parent-child wait cycle.
- A child `attention` may be queued while its parent is concluding the original Tool batch, but processing it must respect the parent's safe boundary and current grant. A wake is admitted as a separate Input, not a mid-stream ToolCall.
- The same rule applies to a standalone `agents.run` whose authorized interactive child needs the launching parent as its only controller. No global Runtime mailbox lock may be held while waiting for child/approval.

### Fast completion, handoff and shell settlement

At creation, validate/register the parent-bound wake rule **atomically with the admitted Job identity before any root/runner can start or settle**.

Only after factual settlement does the source generate a terminal event:
1. Shell exit, timeout, cancellation or unknown OS stop is resolved by the existing Process owner; stderr/stdout writer leases become quiescent and retained Job/Graph terminal checkpoint is committed. A mere OS-exit notification is not final.
2. Agent completion comes from its original Session/TurnJournal; Graph completion comes from the canonical JobManager/node status. No second settlement database.
3. The host resolves synchronous result delivery versus asynchronous handoff. If the original initiating model call already received the complete result, suppress the duplicate parent wake. If the Job was handed off, deliver once even if it completed before the creation response was received.
4. Revalidate the current receiving Agent owner/writer/lineage, explicit `agent.wake` grant, parent budget and subscription revision before preparing a source-labeled Actor Input. Deduplicate the **wake admission request** by source event ID + subscription revision + recipient, not by invented exactly-once model execution.
5. Admit/acknowledge through existing AW Notify/Input/owner facts, distinguishing prepared, accepted, consumed, rejected and unknown. Unknown delivery never triggers a blind replayed paid Turn.

A synchronously completed Agent result is returned **once**, with its Agent ID when freshly created, and is not automatically re-read or re-woken. A parent's wake input contains only compact `job id / node / event / terminal status`, not repeated command/title/stdout/private Agent answer. Output remains behind `jobs.output`; durable public history remains behind `agents.read`.

Revocation/absent parent can prevent wake delivery, but cannot erase factual source settlement. A bare dotlink Job session has no implied AW Agent; a new dotlink process cannot revive an old Job/wake grant. Progress/UI updates remain passive, coalescible observations; a missed event is recovered by authorized status/output query, not by forging execution.

## 10. Graph semantics

Graph is an immutable bounded execution DAG with DL-150's exact
`[operation,args,when?]` grammar, 1-based coordinates, implicit sequential
dependency, explicit independent roots and deterministic success/exception
predicates. Do not introduce nested graphs, post-admission node creation,
generalized dataflow expressions, model-produced loop bodies, or duplicate
Graph managers.

Every known node is prepared and fully authorized **before the first root
starts**, even if a conditional branch is likely to skip. The Graph captures
each node's semantic work and result ceilings, source/authority generation,
conservative conflict footprint and immutable execution arguments. A later
outer response/transport size change may cause **presentation-only** Job
handoff/paging, never change a node's `more`/`incomplete`, terminal outcome,
condition branch or permissions. Reuse DL-150's original status grammar and
ordering, including mixed/unknown and successful skips.

Graph node eligibility:
- all canonical native filesystem tools once `bread(format=mcp)` has typed retained result support;
- exactly host-registered and approved Bash, pwsh or powershell Process nodes;
- AW-hosted `agents.run` as one bounded child Turn node;
- no `batch`, `jobs.*` or `agents.read` nodes initially.
- `background` is direct-call **response disposition** only; any Graph node providing it fails validation, rather than silently treating it as a nested Job.

An Agent node captures task, exact branch/id (if an authorized existing child),
interactivity, narrowed root/delegation ceiling and any wake metadata at
preparation. The host runs the **original** AW Agent/Session/TurnJournal, not an
LLM executor inside aw-tools. An opted-in interactive node may accept separately
authorized Actor Input while running. That may change future Agent actions
but **never** its Graph node args, condition topology, grant ceiling or already
committed result. Node status is one factual checkpoint of that child Turn;
unknown provider/tool effects remain unknown and block automatic retry.

The default parent-child interactive waiting path uses the proactive handoff
rule in Section 9. A follow-up `agents.run({id,...})` creates another Turn/Job,
not an edit to a historical Graph result. Distinct Graph nodes cannot
simultaneously own the same child branch writer without explicit serialized
Graph dependencies and current owner admission.

The shared aw-tools Graph registry exposes only a host-neutral Node adapter
contract. AW supplies the child-Agent executor, current provider/account
custody, SessionStore and Actor owner. Standalone dotlink may consume shared
Graph/native/Process semantics but **must not** claim Agent-node/wake
capabilities without a real authenticated AW bridge.

## 11. Effects, Graph admission and resource permissions

Five trusted static effect classifications:

| Effect | Purpose | Examples |
| --- | --- | --- |
| Read | Observe data or state without mutating it | read, `jobs.get/output`, `agents.read` |
| Write | Mutate user/application state | write, edit, rm |
| Process | Execute a real OS process or paid/model Agent | bash, pwsh, powershell, `agents.run` |
| Control | Schedule/manage existing execution, retained output ownership or Actor Input/wake | `batch` outer admission, `jobs.ack/send/cancel/subscribe` |
| Unknown | Trusted registry cannot establish the effect | opaque external tool |

**Control is an effect category, not a fifth AW runtime primitive and not identical to ActorMessage::Control**: `jobs.send` to an Agent generates an authorized ActorMessage::Input. No Composite effect or blanket Graph grant.

A `batch`/Graph constructor has a separately granted **Control admission** effect for creating and managing a bounded Job, and **every leaf node is independently authorized** for its actual effect/resource/owner. A Graph consisting exclusively of Read leaves is permitted in a filesystem ReadOnly Session when the outer Graph Control admission and all Read nodes are independently granted; the outer grant cannot launder a conditional Write/Process node. Graph-wide `EffectSet` is a cheap *non-authoritative* diagnostic/scheduling summary, never a permission, persisted grant or replacement for per-node checks. All eligible nested operations are checked before roots and again at actual effect admission against captured ceilings/current revocation.

Grouped `agents({op,...})`/`jobs({op,...})` are not one general-purpose authority. A host-trusted operation registry resolves each permitted `op` to a distinct kernel-safe exact-grant identity (existing `job_get` etc where proven) and one of the five effects **before** ToolPolicy/Envelope/journal admission. The advertised op enum contains only current allowed variants. Preserve the original provider's call and current ToolBindingAuthority/TurnJournal per-call correlation; old AW `ToolSpec.effect` (one effect per tool) must be extended with a trusted per-invocation operation resolution rather than calling grouped `jobs` a single Read/Process/Unknown grant.

Resource permission remains independent of coarse effect:
- `jobs.send` shell = Control **plus** current original `process.input`/launcher/sandbox/process ownership and host intent journal (can execute arbitrary commands through stdin).
- `agents.read` = Read **plus** current category-specific transcript disclosure and owner/branch authority.
- `agents.run` = Process **plus** current child/model/provider/account/depth/resource budget.
- `jobs.subscribe` = Control **plus** current parent `agent.wake`, revalidated again upon event delivery.
- `jobs.cancel`/`jobs.ack` = Control **plus** exact Job ownership or output-retention owner; no filesystem Write/Process spawn implied.
- cp/edit = their original DL-150 source read and destination write/read-write requirements, not simplified by one coarse Write effect.

ReadOnly applies to filesystem/content mutation, **not** the full universe of Runtime Control or separately delegated model execution. Preserve all old denials where new explicit grants are absent, no broadening under default Off/narrow Envelope, and maintain Control as a non-read-wave scheduling barrier. Host emergency cancellation remains serviceable even while optional paid dynamic policy or Graph/child waits are suspended.

Optional local-rule/program/MCP/model reviewer pipelines may only deny, narrow or require approval **within** the exact host ceiling. They never mint tool/source/effect/site/credentials or convert an untrusted MCP readOnlyHint into trusted Read. Any changed arguments are fully re-resolved and reauthorized before journaled execution.

## 12. Read-only sessions

Do not implement ReadOnly as "only ToolEffect::Read is allowed" once orthogonal
management/delegation exists.

A valid session may be:

~~~text
filesystem Read       allowed
filesystem Write      denied

read-only Graph       allowed

jobs.get/output       allowed
jobs.cancel/ack       allowed if explicitly granted
jobs.send             denied

agents.read result    allowed
agents.read transcript/tool outputs
                      allowed only under projection ceiling

agents.run            optionally allowed under bounded delegation
bash                  denied
~~~

Preserve old denial behavior unless a new explicit grant exists.

## 13. Typed Graph result retention and immutable semantic ceilings

Remove the structural reason `bread(format=mcp)` remains direct-only. Reuse the existing shared direct executor's typed Body and media descriptor/verified packet contract. Generalize Graph retained result **presentation** to:

~~~text
Text
ExactRead
Bytes
Media
~~~

The immutable per-node Graph preparation retains its own original work/result-byte ceilings, output owners and status computation. **Never** change a native node's `more`, `incomplete` or terminal outcome because an outer Model/MCP response is smaller, a client asks for typed media, or handoff/pagination occurs later. That would change `when` branch decisions and the executed Graph—explicitly forbidden by DL-150. Node status is determined before rendering or outer output fitting.

For media, the retained descriptor records kind/MIME, original length, SHA-256, exact authorized bytes and supported host validation. MIME classification is not permission or proof of a fully safe codec. Provider/media adapters must validate current destination, supported content type, byte ceilings and disclosure independently before attaching. A typed-media failure never turns a committed native Read into an unexplained mutation outcome.

Adapters project MCP typed content, provider-supported image/audio/resource, QuickJS `Uint8Array`, Python `bytes`, or byte-exact base64 fallback. Use the same raw-byte `jobs.output` offset/`jobs.ack` ownership and replay/native Task leases; no duplicate serialized complete content in Job status, Graph summary, Agent historical transcript or shell result metadata.

Graph remains a fixed execution DAG, not a dynamic media/dataflow expression language. A node's result can be observed afterward but is not silently injected into another node's admitted arguments.

## 14. Dynamic policy

Optional host-selected evaluators may include:

~~~text
LocalRules
Program
QuickJS
Python
MCP
Model
Pipeline
Human
~~~

Input is an inert PreparedOperation / Graph summary plus current authenticated
context.

Output:

~~~text
allow
deny
narrow
ask
~~~

Never:

- new grant;
- new tool;
- new credential/provider;
- broader root/site/effect;
- authority minted from model text.

A narrowed proposal is fully re-resolved and reauthorized.

Policy timeout, malformed result, revocation or ambiguous paid reviewer attempt
fails closed.

Cheap deterministic rules should run before paid/model reviewers.

## 15. QuickJS, Python and Rust SDKs

Canonical generic interface:

~~~javascript
await tools.call(["agents.read", {
  id:"a7",
  filter:{tool_calls:true, tool_outputs:true}
}])
~~~

Typed sugar:

~~~javascript
await tools.read({path:"README.md"})
await tools.agents.run({...})
await tools.agents.read({id:"a7"})
await tools.jobs.send({...})
~~~

Python mirrors these names naturally.

Every wrapper calls the same resolver. `tools.call([op,args])` is not an escape
hatch.

No ambient script filesystem, subprocess, network, secrets or provider access unless
separately exposed as authorized operations.

A passive SDK `jobs.watch` may be sugar over Job revision observation only; it does
not create a second event system or model-facing tool.

## 16. Team, planner and review

Keep existing opt-in team / plan / guardian-release workflows intact.

They are higher-level governed orchestration over ordinary Agents and existing
review/evidence boundaries, not new runtime primitives.

They may use transcript projection only when their host policy explicitly grants it;
candidate evidence joins do not imply blanket Agent transcript access.

## 17. dotlink boundary

Standalone dotlink:

- consumes shared aw-tools through exact Git pin;
- exposes native filesystem/Graph/Job semantics;
- may expose grouped `jobs`;
- does not invent durable AW Agents or `agents.run/read`;
- does not advertise Agent wake without an authenticated AW parent;
- preserves Profile deny/sandbox/replay/MCP Task/output ownership semantics.

When hosted behind a real AW Agent bridge, dotlink may project shared factual Job
events and operations, but AW remains Agent/Session/transcript authority.

No transcript state is copied into dotlink merely to serve `agents.read`.

## 18. Compatibility, deliberate omissions and migration

Original DL-150/previous plans remain historical and untouched. This is an
explicit vNext semantic and model-wire migration, not a silent in-place rename.

1. Introduce trusted resolved OperationId/PreparedOperation and Effect::Control **without changing existing source grants or their interpretation by accident**. Keep original provider ToolCall as immutable journal evidence.
2. Validate grouped model projection vs DL-150's model-first schema and result budgets. Advertise one current authorized grouped name, not duplicate aliases.
3. Add typed retained `bread` result handling and per-node semantic ceiling fidelity.
4. Add the grouped `jobs` control plane and matching bounded observation semantics.
5. Expose exact branch-pinned `agents.run` with recoverable Agent/Job creation identity.
6. Add authorized `agents.read` on the current SessionStore, default result-only with precise byte and disclosure gates.
7. Add factual shell/Agent/Graph events and current-parent-bound wake, with fast-completion and blocked-parent handoff.
8. Add interactive Agent/Process input; introduce Agent Graph host nodes only in capable AW hosts.
9. Add host-mediated QuickJS/Python/Rust SDK projections; the generic tuple is **not** an extra model tool.
10. Cut over dotlink after exact shared Git pin, replay/Task/sandbox compatibility and model-quality gates.

Until an intentional and separately authorized cutover, retain old `job_*`,
`subagent` and interpreter identities in the appropriate legacy adapters. They
are compatibility pathways, **not extra advertised vNext names**. Distinct
prepared `bash`/`pwsh`/`powershell` identities stay available on capable
platforms with their independent host grants.

The old replay-protected retained-Graph `job_restart` remains supported in
legacy/advanced SDK form; `jobs.restart` can be selectively exposed for
measured advanced use, but is omitted from the default model catalog. This
preserves functionality without making an uncommon potentially effectful
restart an everyday model selection.

No default second scheduler, generic peer-message router, transcript index
service, active in-flight provider-history reader, replay tombstone database,
code-runner, additional planner engine or automatic cross-process Job
resurrection is introduced solely by this design. No source commit, push,
release or version bump is authorized by it.

### DL-150 normative compatibility contract

The new plan **inherits**, rather than copies, original DL-150 requirements:
13 orthogonal filesystem schemas, shallow ls vs recursive find, exact-path
mutation, no unrequested shell compatibility, six outcomes
`ok/unchanged/fail/mixed/unknown/skip`, separate `more`/`incomplete`,
complete-batch conditional preflight, deterministic dependency/Graph output,
captured native result ceilings, atomic mutation publication, truthful
unknown effect/replay records, cumulative progress, Job output/ACK raw-byte
ownership, runtime-session boundary and protected platform sandbox/denies.

Whenever vNext changes a model-facing schema, do so in **this plan** and its
new integration companion; do not rewrite DL-150's sealed original evidence.

## 19. Focused correctness and model-efficiency acceptance

These are compact contract gates, not a directive to build exhaustive field
fixtures, pin old snapshot hashes or duplicate existing test suites.

### Model-wire and provenance

- One advertised provider name per semantic operation family; authorized
  operation filtering, strict per-op fields and visibility; no duplicate
  `jobs.get` and `jobs({op:"get"})` advertisement.
- Generic SDK tuple, grouped model call and Graph node resolve to the same
  operation/effect and current authorization; **original provider ToolCall is
  preserved**, effective operation is separately evidenced, and call IDs stay
  correlated through deny/revise/execute/result paths.
- Existing leaf schemas, Graph compact tuples, status/result grammar and
  provider source identity are unchanged.

### Execution, ownership and recovery

- ReadOnly FS plus explicit Graph Control admission executes an all-Read Graph,
  while an unauthorized conditional Write/Process/Unknown node prevents any
  root start. A composite EffectSet never grants authority.
- New Agent ID always maps to one exact existing Session/branch; fork/switch
  cannot silently retarget it. Lost/redelivered creation never creates a
  second Agent or paid Turn; exact initial Agent ID is recoverable.
- Graph Agent nodes reuse the original Agent/TurnJournal, don't create a
  competing public Job; concurrent same-branch Turn writers are denied.
- An interactive child requiring only its waiting parent is proactively handed
  off on the **same** Graph Job before deadlock; the parent can then steer or
  inspect through normal Actor Input and Job controls. Process/Agent
  cancellation preserves any committed/unknown external effects.

### Transcript disclosure, bounds and semantics

- `agents.read` result-only/default-one Turn contains no hidden trace. `messages`
  contains complete permitted user/assistant conversational text; requesting
  result and messages together emits final answer once. Custom `filter` matches
  categories and optional exact tool operations; `exclude:true` subtracts matches
  from currently authorized public content without widening disclosure. Tool calls/outputs
  are independently selectable and correlated, with no duplicated arguments.
- One huge final answer or tool output pages with exact advancing `turn/byte`
  cursor rather than unmarked truncation. Identical `filter` and current projection authority are required for continuation. `before` Turn pagination remains
  copyable and non-advancing empty pages are errors. Current result ceilings
  apply separately from number of Turns.
- Active/unreconciled branch read fails closed; compacted context tail is not
  falsely advertised as complete historical SessionStore evidence.
- Forged or unauthorized Agent ID/branch/recipient; revoked sensitive tool
  args/outputs; system/private provider context; hidden reasoning, credentials
  and foreign branch data are never disclosed. Filtering uses verified
  resolved semantic name, never untrusted `op` text.
- A transcript read never triggers inference, Process execution or raw Job
  output ACK. `jobs.output` and `agents.read` remain different authorities
  and retention/byte contracts.

### Events and Graph/media output

- Fast shell settlement before constructor response, stdout/stderr writer
  quiescence, synchronous-result dedup, Graph node/whole-Graph wake,
  revoked/absent parent, attention authenticity and bounded wake Input
  admission are covered without duplicate model Turns or lost factual results.
- `get(wait_ms)` and `output(wait_ms)` use race-free subscribe/snapshot and
  truthful status/EOF/expiry, never execute/ACK or hold the Actor control path.
- Typed `bread(format=mcp)` in a Graph preserves exact data and outer
  response fitting **never** changes the original node's more/incomplete or
  dependency branch. Existing output/ACK/replay/Task ownership remains exact.

### DL-150 model-first efficiency gate

Before vNext advertised-tool cutover, compare **actual generated schemas**,
not just proposed example signatures:
1. Total exact serialized schema bytes and a representative tokenizer
   estimate, including effective-grant filtered catalogs and optional
   grouped Agent/Job variants.
2. A compact representative task set: read; read-only 3-node Graph;
   shell run/handoff/status/output; create Agent/receive result; historical
   result-only `agents.read`; filtered tool evidence; read-only `jobs.get`
   versus `jobs.cancel` tool selection.
3. Model/tool selection success and schema-valid first-call rate, full
   model/tool round count and returned result tokens **per workflow**; check
   concise failure paths and no unnecessary second Agent-read call.
4. Any material increase in model error rate or total token cost must be
   corrected or explicitly justified by new capability before schema
   migration. Do not expand schemas or test fixtures merely to produce
   more documentation artifacts.

Keep original AW no_std/minimal/stateless, shared native backends, AW
Agent/SessionStore and dotlink MCP/stdio/Task/OpenAI replay suites. Native
Windows/macOS/Bubblewrap execution must be run on capable hosts or remain
explicitly pending; source-only validation cannot be claimed as smoke evidence.
No duplicate Job/Agent/message/history owner is introduced.

## 20. Final decision record

The final model-native contract is:

~~~text
canonical call       [operation,args]
Graph node            [operation,args,when?]

Agent execution       agents.run
Agent history         agents.read
Job live control      jobs.*

live Agent input      jobs.send
live Job output       jobs.output
durable Agent result  agents.read default result

background            direct-call disposition only
title                 non-authoritative audit metadata
wake                  parent-owned event -> Input rule

effects               Read Write Process Control Unknown
Graph summary         non-authoritative EffectSet

transcript default    latest result only
transcript expansion  filter:"none" or custom category/tool-name filter; exclude:true inverts matches
~~~

The rationale is deliberately asymmetrical:

- Jobs own **live execution mechanics**.
- Agents own **durable conversational history**.
- Graphs own **immutable execution topology**.
- SessionStore owns **durable transcript facts**.
- ToolPolicy/Envelope/resources own **authority**.
- Adapters own **wire syntax**, not semantics.

This gives the model a small, predictable language while preserving exact security,
durability and audit boundaries.
