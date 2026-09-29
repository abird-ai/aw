## R11 — project instructions are data, never permissions

Selecting or trusting source-pinned project instructions, named skills, named prompt templates or named semantic themes does NOT grant access to read/write/search/Bash/MCP/extension tools or any provider credential, remote executor or Session owner. These continue to require the existing exact --tools-* mode/actor/site/source/epoch grants and durable intent-before-effect receipts. Approved project content is loaded from the existing descriptor-relative no-follow scoped filesystem and only the exact current owner-approved SHA256 snapshot is presented to an Agent. Trust/revoke require explicit CLI actions and no provider or shell startup. The model never gets to approve its own file sources. Project data cannot change accounts/endpoint/tool grant by merely appearing in instruction text.

The shared R11 admission layer is only enabled for explicit local single-user Agent invocations; browser and Workspace resource trust remain separately unauthorized until an owner-authenticated grant protocol is designed. R14 will revisit all CLI flag families in one coherent taxonomy, without changing execution permissions. See [accepted history](../plans/accepted-history/TODO.md).

---

## Latest R9 follow-up — model-directed parallelism, no implicit tool grants

The standard full and JSONL-minimal Agent default to *model-directed* scheduling. The model sets optional parallel:true on independent calls to read, ls, find, grep and already-granted trusted Bash; false/omitted means execute in original order. aw never infers independence from a tool name, command text, or provider readOnlyHint. Exact actor/site/source/epoch grants, rooted filesystem policy and full-schema validation still apply. Only canonical --tools-* CLI settings determine host restrictions.

Host ceilings are resources, **not scheduling instructions**. --tools-parallel model (default) allows explicit model-selected waves, --tools-parallel off forces serial; --tools-parallel-limit 16 is the default shared ceiling and can be adjusted 1..128. Optional --tools-parallel-read-limit and --tools-parallel-bash-limit further constrain a specific class but inherit the shared ceiling when omitted. Tool mode remains OFF by default. Trusted Bash is available only after --tools-mode trusted/explicit grants and an absolute --tools-bash executable; parallel Bash remains Process, can modify shared files concurrently, and uncertain effects always require inspection/no replay. Writes and other Process/Unknown effects are barriers.

The existing shared no_std+alloc TurnMachine handles exact, bounded Read and Bash waves; standard Agent uses no detached wave tasks. Every effect requires journaled intent before dispatch and durable factual original-order receipts before post-tool hooks and provider disclosure; cancellation/failure drops in-flight future work and blocks partial replay. Model batches now permit up to 128 distinct calls under the existing hard 1MiB context/trace budget, with fair bounded per-call output truncation explicitly visible and journaled. Read [accepted history](../plans/accepted-history/TODO.md).

---

## R9 — parallel exact-read waves and deterministic receipt ordering (2026-10-10)

Host-authenticated, currently granted exact Read tool calls are now processed in contiguous bounded waves (2–4), with --parallel-reads 1 forcing ordinary sequential execution. The pure no_std+alloc TurnMachine in aw-agent-kernel enforces read-wave state, all ToolStarted intents before any effect, current source-grant revalidation, and all ToolFinished receipts before the next mutation or model call. The default full and JSONL-minimal CLIs share the exact same std Agent. No second tool executor/Agent, detached Tokio tasks, implicit OS permissions or background polling.

Write, Process and Unknown (including MCP and process-extension advisory readOnlyHint) remain serialized barriers. All original batch schema/grant/context checks happen before any effect; results are held and disclosed in model-call order, regardless of execution finish order. Cancellation or execution error drops the whole in-request Future wave, closes acknowledged intents with Interrupted receipts where possible, and never auto-retries uncertain external work. A source revoked mid-flight cannot leak its result to a subsequent provider invocation. ToolPolicy.max_parallel_reads=4 by default; hosts can lower to 1..4 only, independent of source grants. See [accepted history](../plans/accepted-history/TODO.md).

# Phase 3 tool loop and built-in tools

## R7 source-bound process tools and gates (2026-10-09)

The standard full and JSONL-minimal native Agent now supports explicit --extension-process ID=/ABS/EXEC, literal --extension-arg ID=ARG, --extension-allow ID:TOOL, and --extension-hook ID:before_prompt|before_tool|after_tool. Discovery only: aw extension --extension-process ... [--json] lists advertised metadata without grants or provider calls. A selected prompt template can be rendered as untrusted text using aw extension ... render ID NAME KEY=VALUE. No extension starts on aw --help or an ordinary model-only run.

The independent aw-extensions crate speaks a strict version-1 bounded JSONL process protocol. Tool names are ext__ID__TOOL with host-minted SourceKind::Extension epoch and current actor/site; all effects are Unknown and pass the **same** Agent ToolPolicy, per-call schema validation, intent-before-effect journal, cancellation and no-replay policy. A discovered tool is never implicitly granted. Only explicit host-approved, advertised before/after gates participate in the existing ordered Extension hook loop. The remote process cannot mutate credentials, provider/route, effects, actor or grants through its data payload; unsolicited response fields fail closed. A timeout/crash/malformed response poisons the connection, hides its tools from model-visible capability discovery, blocks future dispatch, and never automatically replays a possibly completed effect.

A child process starts only from an explicit native single-user owner selection, with cleared environment, absolute executable (no PATH/shell), bounded arguments/messages/deadlines and kill-on-drop. **This is not an OS sandbox:** the selected executable still has the launching OS user's privileges. Web/multiuser process execution is closed; host trust/revocation remains distinct from advertised metadata. See [accepted history](../plans/accepted-history/TODO.md), [C example](../../examples/process-extension-c/README.md), and [Rust example](../../examples/process-extension-rust/README.md).


## R6 MCP text data and change notifications (2026-10-09)

The std full and std JSONL-minimal CLI share the official rmcp client with R5a/R5b. Explicit commands aw mcp --mcp-http box=URL prompts|resources|templates discover source-labeled data only; aw mcp --mcp-http box=URL prompt box NAME KEY=VALUE..., read box URI, or read-template box NAME KEY=VALUE... fetch only an exact current advertised text prompt/resource. --json gives machine-readable arrays. Opt-in aw mcp --mcp-http box=URL watch box [--resource URI] --seconds 1..30 observes current notifications, with 2026 subscriptions/listen and 2025/2024 legacy subscribe/unsubscribe. It never calls a model or tools in response.

The additional run flag --mcp-resource-allow SERVER:URI registers a model-visible mcp__SERVER__resource_read only for those currently advertised URIs, max 16 host-selected URI grants, using the existing SourceKind::Mcp actor/site/epoch capability, ToolEffect::Unknown and journal-before-effect path. Remote read-only hints remain untrusted. An active explicitly granted modern Agent listens to negotiated change events with a reserved HTTP slot; old servers subscribe to approved URI updates when supported. The stream stops when the source connection is dropped; no idle daemon or passive startup watch exists. Tool/result access is refused if list_changed/update revisions make the cached capability stale. Server names, resource IDs and remote prompts never grant new permissions or become system instructions by discovery alone.

Bounds: eight cursor-checked pages, 128 advertised data entries, 16 KiB descriptor, 128 KiB text response, 32 message parts, no binary/media, no unsafe URI traversal, no automatic remote URI HTTP fetch. URI templates support conservative level-1 {variable} expansion; advanced RFC 6570 operators, interactive MRTR, live owner/browser and embedded no_std transport remain later work. See [accepted history](../plans/accepted-history/TODO.md).


**Source implementation; Rust compilation, actual tool binaries and Nix checks remain unrun in this authoring environment.** Source tests and actual-binary host harnesses are included. Tools default to **off**. Nothing installs or starts an agent tool without an explicit host grant.

## R1 exact-source capability grants (2026-10-09)

New aw-agent-kernel portable core-only capability predicates are shared with hosted ToolRegistry/ToolPolicy. Existing named ToolPolicy::permit grants remain valid **only for trusted-host registered tools**. MCP and extension descriptors are separate sources: registration alone creates no authority; an exact host-issued source-ID, epoch, actor, execution-site and effect grant is required for visibility and just-before-effect dispatch. A remote same-named tool does not inherit a trusted-host name grant. Unknown external effect is conservative, not a synonym for Read. The registered ToolSpec/schema/effect is cached once for the whole invocation and cannot be rewritten by a later spec() call.

R1 does **not** implement MCP transport, runtime plugin loading or new filesystem operations. It only supplies the shared provenance/authorization boundary those later features will consume. See [accepted history](../plans/accepted-history/TODO.md). All previous journal-before-effect, root, account, cancellation and recovery guarantees below remain.

## R2a bounded coding search (2026-10-09)

The std aw-tools backend now supplies three additional **read-only** model tools under explicit tool grants: ls (alphabetically sorted directory entries), find (globbed file/path walk) and grep (regex/literal file content search with optional case folding, glob filtering and line context). The implementation reuses regex, globset and ignore from the ripgrep/fd Rust ecosystem, **not** a new regex/Gitignore parser or an ambient subprocess walker. Every listing and file read still goes through the descriptor-rooted aw-platform scoped filesystem. Scans are on demand, bounded, cancellable and unindexed; .gitignore patterns are loaded through scoped Root::read and include ancestor rules. Binary/non-UTF8 and unsupported/symlink/hardlink entries are not opened as search content.

The explicit optional CLI --tool-allow grep,find selects a **subset** of tools already authorized by --tools read-only|workspace|trusted. It cannot widen a read-only grant or activate Off. Previously approved child-Agent tool grants remain read or read/write/edit only; no automatic subagent expansion. External rg/fd processes and Windows trusted shells are **not** implemented here: the std in-process libraries are the default, and acceleration would need a separate justified trust/benchmark review. A future no_std host will supply file enumeration and bytes while portable regex-automata evaluates patterns; the current R2a aw-tools code remains std-based. Details: [accepted history](../plans/accepted-history/TODO.md).

## R5a MCP external tool boundaries (2026-10-09)

The new aw-mcp standard Rust adapter uses rmcp 3.5.0 to connect only explicit stdio/Streamable HTTP servers in native single-user CLI. R1 core-only SourceKind::Mcp, exact actor/site/source epoch and ToolPolicy gates remain the sole tool authority. Merely listing a tool or claiming readOnlyHint does not grant it. aw mcp prints names without any model or tool execution; --mcp-allow SERVER:TOOL grants one currently advertised name. Each tool is cached with its validated JSON Schema (max 64KiB, bounded depth and no remote refs), classified Unknown, and checked again just-before-effect. The standard Agent journals intent and refuses unsafe automatic replay on ambiguous failures; tool results are bounded to text/structured JSON, non-text media requiring later adaptation.

Model/Agent process does not choose MCP command, HTTP endpoint, bearer env, CA trust, executor site or tool grant. Stdio is an expressly selected host process, not an OS sandbox, and always clears environment except explicit --mcp-pass-env entries; child exits when connection drops. Only explicit HTTPS with configured trust or loopback HTTP accepted, with no proxies/redirects. Web/owner principal isolation remains closed. Old 2024-11-05 HTTP+SSE transport, MCP resources/prompts and list_changed are still R5b/R6. See [accepted history](../plans/accepted-history/TODO.md).

## R5b deprecated HTTP+SSE MCP tool source (2026-10-09)

The explicit --mcp-sse SERVER=URL selects deprecated MCP 2024-11-05 two-endpoint GET SSE + POST, unlike modern --mcp-http. The same official rmcp protocol and the R1 exact SourceKind::Mcp source/actor/site/epoch grant predicates authorize tools, not remote readOnlyHint. Untrusted endpoint announcements are checked for exact origin before sending a bearer or POST; a legacy session query is opaque same-origin routing, not a transferable credential. Each external effect remains journaled and never automatically replayed after uncertain failure. No automatic fallback from Streamable HTTP, default server startup or old transport dependency in the no_std kernel. R6 MCP prompts/resources/notifications remain separate. See [accepted history](../plans/accepted-history/TODO.md).

## Tool modes

| Flag | Advertised capabilities | Important boundary |
|---|---|---|
| `--tools off` | None | Existing conversation-only path. No file root opened or shell spawned. |
| `--tools read-only` | `read` | Reads bounded UTF-8 regular files under the configured root. |
| `--tools workspace` | `read`, `write`, `edit` | New-file creation and exact-match replacement, no implicit shell. |
| `--tools trusted` | All four including `bash` | Shell has the launching OS user's authority. **Not a sandbox.** |

The root defaults to `-C` or an explicit `--tool-root`. Tool execution occurs on the owner hosting `aw-tools`, not automatically on the viewing browser's device or the model's endpoint. Web tools additionally require `--allow-web-tools`. A future remote runner must supply its own grants/auth; changing the UI's current worktree or account cannot rebind a running call.

## Complete proposals, then execute

The provider supplies complete typed `ToolCall` records. OpenAI-style fragmented argument strings are assembled by index before JSON parsing; Ollama uses native argument objects. No partial streamed function/argument fragment executes. Truncated/unclosed calls, unknown tools, missing grants, duplicate call IDs, malformed arguments and whole-batch limits fail before the first effect in that batch.

The native Agent validates a proposed batch, runs `before_tool` extension gates, rechecks the caller/account grant, executes sequentially, captures bounded results, and runs `after_tool` gates before disclosing results back to the model. Tool errors with known results are paired as error messages so a model can explain/recover. A follow-up provider round receives the exact assistant tool-call message and all matching result IDs. Subsequent prompts use only completed valid context traces; no result is silently attached to the wrong call.

Ollama's provider-private continuation is temporarily retained only within the same invocation sequence when needed; it is not displayed/serialized as hidden reasoning or passed to another account/model on the next prompt. Protocol support is not a claim that every configured model supports tools.

Defaults: 8 model rounds, 32 tool executions total, 16 calls in one batch, 64 KiB arguments per call, 128 KiB result, 1 MiB trace/context caps and a 30-second per-tool timeout inside the overall request timeout. Trusted embeddings can choose bounded `ToolPolicy` settings. Registration alone grants no user permission; actor grants are explicit. The registry holds at most 32 tools and bounds aggregate schema size.

## File operations

`read`: `{ "path": "src/main.rs", "start_line": 1, "max_lines": 200 }`. It returns numbered lines; max 1000 lines, bounded output, only files up to 1 MiB and valid UTF-8. Larger/binary content is rejected or explicitly truncated. Unknown fields are rejected.

`write`: `{ "path": "new.txt", "content": "text\n" }`. Creates **new files only**, at most 64 KiB content. Existing targets are never overwritten. Parent directories must exist. Failed writes can leave a partial new file; uncertainty is preserved, not automatically retried.

`edit`: `{ "path": "src/main.rs", "old_text": "one exact old span", "new_text": "replacement" }`. Requires exactly one nonempty old match. Replacement data is bounded; zero/multiple matches are errors. Writes through a unique temporary file, checks the observed source again, and atomically replaces it. No fuzzy patching. Does not promise full ACL/xattr preservation or safe coordination with malicious/noncooperating writers.

On Linux, directory traversal uses held directory descriptors and no-follow component/leaf opens. Clean relative paths only; symlinks, absolute paths, parent traversal, empty path components, special files and multiply-linked targets are refused. This hardens ordinary scoped operations but is **not tenant isolation** against malicious same-UID code, root, mount changes, or an external race after a preimage check. Multiple aw processes do not share an OS-wide workspace lock. Use an isolated worktree/runner and external coordination for concurrent writers.

## Trusted shell

`bash`: `{ "command": "cargo test" }`. Requires `trusted`, an absolute configured shell executable, and the actor's grant. Nix package builds provide their store bash path; Cargo users can set `AW_BASH` or `--bash /absolute/path/to/bash`. The model cannot choose the shell, environment or cwd through arguments.

The process uses `--noprofile --norc`, closed stdin, cleared environment with a small configured PATH/HOME/LANG set, stdout/stderr pipes, bounded capture and a new Unix process group. Cancellation/timeout uses best-effort process-group cleanup and kill-on-drop. Descendants that escape their process group or act externally cannot be guaranteed stopped; completed effects are not rolled back. This is a **noninteractive process tool**, not a human PTY terminal tab.

Normal nonzero exit is a ToolOutput error and can be explained. Timeout, dropped future, mutation I/O errors and failed post-tool gates may leave effects. Environment cleanup is not credential/file/network confinement; do not enable this for an untrusted browser participant or model task merely because a key is absent from the child's environment.

## Unknown effects and continuation

An interrupted/incomplete/failed turn that attempted a mutating/process tool sets `needs_reconciliation`. The Agent blocks new prompts until the trusted host explicitly acknowledges inspecting effects. `/new` or reset does not clear that gate. CLI `/acknowledge` and the browser's confirmed inspection action clear it without rollback or replay. `/retry` refuses any unsuccessful attempt containing tool executions. Acknowledgement is a user assertion, not an automated proof that the workspace is safe.

In explicit --no-state mode, restart loses in-memory records and independent effect inspection is required. The default Phase 3 store now journals intents before effects and preserves unresolved hazards across restart; see STATE.md. Neither mode claims exactly-once external execution.

## Verification

Rust source tests cover registry/grants, complete context pairs, batch denial, timeouts/reconciliation, Linux traversal/symlinks/hardlinks, exact writes and shell behavior. the removed phase2_smoke helper --binary ...` must exercise a **real compiled** binary against clearly fake model endpoints, then check actual temporary files. `--self-test` only tests the Python fixture and must not be reported as Rust tool validation.


## Phase 3 durable effect guard

With default persistence, `TurnJournal` commits a tool-start receipt before the effect and a result record afterward. Failed/interrupted possible writes/processes remain inspection-required across owner restart. `/new`, reset, fork or checkpoint cannot erase same-root hazards in this store. `--no-state` explicitly disables that durable protection; historical memory-only limitations then still apply. Do not read earlier phase descriptions as a claim that the default now loses all effects on restart.

Persistence does not sandbox bash or other native tools. Root coordination uses the exact canonical tool root within one store, not a global lock across databases/nested roots/external processes. Current tool mode and grants are always reconstructed from current host configuration, never a stored/imported tool call. Acknowledgement records human inspection; it does not undo files or replay uncertain work. See [state](STATE.md).


## Phase 4 integration note

The optional native workspace uses the same current account/tool grants and persistent owner described above. The underlying Phase3 SQLite schema and browser account/transport behavior are unchanged. Human file views and layout changes do not enter provider context, start another loop, restore credentials from history or grant permission. Source0.5.0 adds a separate pure layout SDK and native rendering; no new remote hosting, browser key store, inference delegation, Git or embedded PTY is implied. See [WORKSPACE.md](WORKSPACE.md) and [BUILD-STATUS.md](BUILD-STATUS.md).


## Phase 42 child file-tool isolation

`--subagents` grants the model the bounded delegation tool; it does not grant local file tools. `--subagent-tools off|read-only|workspace` is a separate host choice. `off` preserves Phase-41 model-only children. `read-only` advertises only `read`; `workspace` advertises `read`, `write`, `edit`. `trusted` is rejected for child Agents in Phase 42. The model cannot choose this mode.

For a non-off child, aw requires a clean non-bare Git parent checkout, creates a detached managed worktree under the private aw state directory at the exact committed HEAD, and roots child file tools inside that isolated binding. The child never receives `bash`, `subagent`, or Git-management tools. The existing file-tool no-follow/path bounds remain unchanged. Worktree isolation separates cooperating aw file effects; it is not an OS sandbox against malicious same-UID code. See [accepted history](../plans/accepted-history/TODO.md).


## Phase 44 recursive tool inheritance

When `--subagent-max-depth` is greater than the current child depth, a child Agent may receive the same bounded `subagent` tool. The model still cannot select depth, budgets, account/model, routing, file-tool mode or worktree. Descendants inherit the root host's `off`, `read-only` or `workspace` file-tool mode unchanged (equal authority, never broader); `trusted`/bash remains unavailable. At the configured maximum depth the `subagent` Tool is absent from the child's advertised registry.


## Phase 45 routing does not grant tools

`--subagent-route` can change only the already-authorized provider account/model used for a child. It never changes the host-selected child file-tool mode, root, worktree, shell authority or recursive depth. A routed descendant therefore still receives exactly the same `off`, `read-only` or `workspace` tool grant inherited from the root configuration; `trusted`/bash remains forbidden. The model-facing `subagent` JSON has no route/account/model/candidate fields.


## Phase 46 team tool is topology, not authority

The root-only `team` tool accepts only `task` plus optional `title`. A host-selected immutable 2–4 member template decides candidate roles/instructions and `all_required` join. Each candidate is executed through the existing subagent Service, so the team cannot grant new provider/model, routing, file-tool, worktree, recursion, budget or shell authority. Candidate member text is untrusted evidence. Recursive child Agents do not receive the `team` tool in Phase 46.


## Phase 49 planner output is not tool authority

The root `plan` tool can request a bounded generative task proposal, but planner output is data only. Task roles are semantic labels and cannot grant files, shell, worktree, route/account/model, depth or budget authority. Each planned task is executed through the already configured subagent Service, so its effective local tools remain exactly the host-selected `off`, `read-only` or `workspace` grant and `trusted`/bash remains forbidden for these children. The planner provider invocation itself is tool-free.
