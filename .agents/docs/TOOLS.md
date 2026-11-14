# Current AI-native filesystem tool surface — DL-150 (2026-10-31)

Source of truth: dotlink's DL-150 approved specification, applied to AW
with `aw-tools` as the shared canonical engine and `aw-tools-adapter` as
the Agent ToolPolicy/TurnJournal/Process adapter. Dotlink itself is unchanged.

- Off (default): no model tools.
- Read-only: ls, find, grep, read, bread.
- RW: all read tools plus write, edit, patch, cp, mv, rm, bwrite, bedit.
- Full: RW plus explicit unsandboxed bash or pwsh (Windows) grant.
- Old workspace/trusted CLI mode spellings remain accepted.
- All tool visibility intersects selected-tool limits, source/actor grants
  and the original bound Agent Envelope; no feature enables authority.

ls inspects one directory; find locates names/types/metadata recursively;
grep scans contents with a filename glob. Meta columns are opt-in.
Time names mtime, ctime, btime are precise, with unavailable reported
as -; hidden and gitignore default false. Results are compact lines,
more offset and incomplete coverage markers are independent and
explicit, and writes return ok/unchanged.
Edit uses path and compact edits=[[old,new,all?],...] tuples against one
original snapshot; overlaps are denied before the single publication.
Legacy multi-file and singular edit shapes remain AW input-only adapters. Read uses canonical line/lines/byte UTF-8 continuation; old
offset/limit is input-only compatibility. AW currently represents
partial numbered=false reads as JSON data/status rather than native
multiple MCP content blocks. The persistent child-session SQLite mode
continues to store workspace even when the CLI displays rw.
Patch advertises strict Codex Update/Add only; legacy best_effort is
input-only. Cp copies bounded directory trees by default without an advertised
recursive flag. Exact cp/mv/rm avoid
shell-style globs, destination inference, overwrite or fallback. Native
Unix/Windows lifecycle adapters reuse pinned root, mutation guards and
private staged publication; macOS uses no-replace exclusive rename when
supported. Runtime Windows/macOS and native sandbox isolation are
separate acceptance gates.

Build presets: `aw-tools` read-only, rw, full; the sealed Graph compiler,
shared JobManager and advisory shell write-scope validation are present. AW
registers `batch`, `job_start`, `job_*` and direct trusted shells through one
live `JobRuntime` bound to its original outer journal and `PipeProcess` owner.
Specialized read/write/
binary/lifecycle/async/durable/sandbox. Feature selection never mints
a model grant. DOTLINK ADOPTION IS A LATER SEPARATE PHASE.

Detailed AW source/verification:
.agents/plans/dotlink-tool-port/AI-NATIVE-FS-IMPLEMENTATION-2026-10-31.md

---

# Historical native tool surface — shared toolset follow-up (2026-10-30)

Tool algorithms shared with dotlink historically lived in `aw-tools-core`, now renamed to `crates/aw-tools`: synchronous
core by default, AW-selected async+durable host features, and an optional
sandbox policy interface. AW's original Agent, ToolRegistry, exact ToolPolicy,
TurnMachine, durable TurnJournal and model-directed parallel waves remain
unchanged. Pure shared code cannot mint a tool grant, launch a durable job,
read a credential or bypass the scoped filesystem backend.

The post-sync tool update raises bounded exact edits to 512 operations /
64 files with a 64 MiB aggregate original+working budget and 48 KiB AW
argument envelope; moves patch KMP/EOF/CRLF matching, streaming binary
splices and cross-platform read windows into the shared crate; and uses
bounded preflight receipts, a 4,096-file candidate/32 MiB content search
budget and exhaustive incomplete-reasons metadata.

AW retains synchronous native shell and durable Agent/Session/Process
ownership. Dotlink's background job_* API and JobManager are **not**
implicitly registered in AW: their opt-in task budget, acknowledgement,
output-spool and host-ownership contracts are shared, but an AW adapter must
first use its existing exact owner/journal and no-replay semantics. A sandbox
feature provides no OS isolation until a host-approved sandbox adapter exists.
True typed MCP media remains separate from native encoded JSON.

See `crates/aw-tools/README.md` and
.agents/plans/dotlink-tool-port/PORT-RESULTS-2026-10-30.md for exact
fully/adapted/partial/skipped inventory and validation gates.

---

# Current native tool surface — shared AW/dotlink semantics

AW retains its original Agent/TurnMachine/ToolRegistry/ToolPolicy, exact
actor/source/site/epoch grants and durable intent-before-effect journal. The
filesystem/Graph/Job semantics live in reusable `aw-tools`; AW-specific
projection, compatibility, Process custody and host policy live in
`aw-tools-adapter`. There is no second Agent, journal, authority source or
process supervisor.

| `--tools-mode` | Host-granted built-ins |
| --- | --- |
| `off` (default) | none |
| `read-only` | `ls`, `find`, `grep`, `read`, `bread` |
| `rw` (`workspace` accepted as input compatibility) | read-only + `write`, `edit`, `patch`, `cp`, `mv`, `rm`, `bwrite`, `bedit` |
| `full` (`trusted` accepted as input compatibility) | rw + explicitly prepared trusted `bash` (Unix) or `pwsh`/Windows PowerShell identity (Windows) |

`--tools-allow` only narrows that set. A selected Job-backed producer may also expose the bounded `job_*` controls required to observe/cancel/restart that same execution; this does not add another node/tool capability. Native subagents retain their existing
narrower host-selected subsets; no saved Session can widen current host tool
policy. Canonical filesystem arguments are the shared schemas below; AW may
accept explicitly documented legacy fields only at its adapter/deserialization
boundary.

| Tool | Canonical argument contract |
| --- | --- |
| `ls` | `path?,sort?,meta?,hidden?,gitignore?,offset?,limit?,parallel?` |
| `find` | `path?,glob?,regex?,depth?,kind?,hidden?,gitignore?,mtime?,ctime?,btime?,sort?,meta?,offset?,limit?,parallel?` |
| `grep` | `pattern,path?,glob?,depth?,literal?,ignore_case?,mode?,context?,hidden?,gitignore?,offset?,limit?,parallel?` |
| `read` | `path,line?,lines?,byte?,numbered?,parallel?` |
| `write` | `path,content` |
| `edit` | `path,edits:[[old,new,all?],...]` |
| `patch` | `patch` |
| `cp` / `mv` | `from,to` |
| `rm` | `paths,recursive?`; advertised only when the host prepared durable exact mutation-recovery storage |
| `bread` | `path,offset?,limit?,format=mcp|base64|hex?,parallel?` |
| `bwrite` | `path,data,encoding=base64|hex?` |
| `bedit` | `path,edits:[{offset,length?,data?,encoding?}],sha256?` |
| `batch` / `job_start` | `ops:[[tool,args,when?],...]` |
| `job_list/get/output/ack/cancel/restart` | one shared live Job ID namespace; `job_output`/`job_ack` select node/stream as required |
| `bash` / `pwsh` | `command,dir?,stdin?,timeout_secs?,max_output_bytes?,parallel?,writes?` |

`LocalToolConcurrency` is the single AW host snapshot for both top-level Agent
waves and nested Graphs. `--tools-parallel off` therefore serializes Graph work
as well as direct calls. SDK narrowing is transactional and is refused while a
live Job still depends on the previous policy. `LocalToolLimits` similarly feeds
advertised schemas, validators, direct execution, Graph NodePlans, retained
native result reservations and shell stream limits from one host configuration.
AW defaults remain deliberately smaller than the shared engine maxima; the
shared maxima exist so another bounded host such as dotlink can reuse the same
renderer/spool/Graph implementation without semantic narrowing. Session scope
replacement and tool resource policy are separate lifecycle interfaces:
`InvocationScopeLifecycle` never becomes a generic policy hook, while
`ToolPolicyLifecycle` validates/applies dynamic scheduler narrowing. A configured
Job producer is admitted only when the aggregate spool can retain all exact
result reservations for one maximum-size Graph. The same preflight is applied by
public JobRuntime construction and again before a shell interpreter is published,
so no valid Graph later degrades into quota-driven `skip` outcomes.

Filesystem semantics:

- Secured roots are descriptor/handle pinned and component-wise no-follow with
  no cross-mount/reparse traversal. Existing **final content symlinks** are the
  narrow exception: the lexical name is authorized first, the resolved regular
  target is independently re-routed/re-authorized, and mutations replace the
  resolved target while preserving the symlink. `cp`/`mv`/`rm` operate on a
  final symlink object and never its target.
- Ordinary user regular-file hardlinks are supported. Single-link integrity is
  retained only for private control/spool artifacts where identity ownership
  requires it.
- Discovery/search use canonical reusable `/` model paths, reversible quoting
  for ambiguous UTF-8 labels, deterministic ordering and atomic rendered result
  units. Invalid-UTF8 Unix names are omitted with `incomplete path_encoding`
  rather than exposed through a hidden byte-path language.
- `.gitignore` loading is rooted at the requested tree and bounded per file and
  cumulatively. Unreadable, malformed, truncated or over-budget ignore state
  preserves safely filtered results and reports `incomplete gitignore`; it
  never silently widens authority.
- `read` preserves source line endings and exact continuation cursors. Text
  tools reject binary/control-containing content; arbitrary bytes use binary
  tools. Mid-read I/O faults remain error-marked and never masquerade as normal
  pagination.
- Canonical `write`/`bwrite` are write-authority operations: after atomic
  publication they return `ok` and do not perform an unauthorized equality read
  merely to claim `unchanged`. AW-only legacy conditional/SHA adapters retain
  their explicitly granted preimage behavior at the compatibility boundary.
- Edit/patch/binary/lifecycle mutation plans are bounded and staged before
  publication. Existing access protection is preserved; new parent suffixes are
  staged with the publication unit; known partial multi-root/multi-file commits
  are reported factually and uncertain effects are never automatically replayed.
- Recursive lifecycle work is bounded, symlink-no-follow and checked against
  protected/excluded descendants. `rm` is advertised only when the host supplies
  durable exact mutation-recovery storage; it durably reserves every requested
  tombstone before the first detach, revalidates identity before each rename and
  retains published cleanup records for restart reconciliation. Volatile custody
  protects exact Stage/Backup names only for the live process and cannot promise
  restart cleanup. Crash-clean staging therefore requires durable private state.
  User files merely resembling an internal prefix remain ordinary visible files.
- Unix cooperating writers use sorted descriptor-rooted mutation locking;
  Windows uses the matching invariant namespace/admission protocol. Concurrent
  reads do not take mutation locks. Opaque shell/external writers remain outside
  this cooperative transaction contract.
- `batch`, `job_start`, all `job_*` controls and direct trusted shells share one
  live `JobRuntime`/`JobManager`. NodePlans capture semantic budgets, live grant
  generations, runtime identity and conservative footprints before admission.
  The outer AW TurnJournal is the durable no-replay fence; internal Graph state
  is intentionally not resurrected after process loss.
- Windows trusted PowerShell execution remains owned by AW's existing contained
  Process/Job-Object path; this is process-tree custody, **not** an arbitrary
  filesystem/network sandbox.

**Compatibility boundary:** `bread(format=mcp)` is accepted, but native AW
currently transports a MIME-labeled base64 JSON envelope
(`transport:"encoded_json"`), **not** dotlink's typed MCP image/audio/blob.
The Agent's text-only `ToolOutput` must be deliberately extended before
claiming true typed-media parity. Trusted Bash/PowerShell is explicitly
**unsandboxed** and remains a Process effect; dotlink's Bubblewrap isolation
would require a separately granted execution mode. Windows GNU cross-target `aw-platform-win` and `aw-tools`
compilation (including tests) passes; Windows/macOS **runtime** validation
remains outstanding. The full Windows CLI cross-build additionally needs a
MinGW C compiler for the bundled SQLite dependency.

See `crates/aw-tools/README.md` and
`.agents/plans/dotlink-tool-port/` for implementation/validation.

---

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

Write, Process and Unknown (including MCP and process-extension advisory readOnlyHint) remain serialized barriers. All original batch schema/grant/context checks happen before any effect; results are held and disclosed in model-call order, regardless of execution finish order. Cancellation or execution error drops the whole in-request Future wave, closes acknowledged intents with Interrupted receipts where possible, and never auto-retries uncertain external work. A source revoked mid-flight cannot leak its result to a subsequent provider invocation. **Historical R9 value:** `ToolPolicy.max_parallel_reads` was 4 with a 1..4 host range at this checkpoint; the current live policy is the later R9 follow-up above (default 16, bounded 1..128). See [accepted history](../plans/accepted-history/TODO.md).

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

`--subagents` grants the model the canonical bounded `agent` constructor; it does not grant local file tools. `--subagent-tools off|read-only|workspace` is a separate host choice. `off` preserves model-only children. `read-only` advertises only `read`; `workspace` advertises `read`, `write`, `edit`. `trusted` is rejected for child Agents. The model cannot choose this mode.

For a non-off child, aw requires a clean non-bare Git parent checkout, creates a detached managed worktree under the private aw state directory at the exact committed HEAD, and roots child file tools inside that isolated binding. The child never receives `bash` or Git-management tools. Recursive children may receive only the same bounded canonical `agent` constructor when depth allows. The existing file-tool no-follow/path bounds remain unchanged. Worktree isolation separates cooperating aw file effects; it is not an OS sandbox against malicious same-UID code. See [accepted history](../plans/accepted-history/TODO.md).


## Phase 44 recursive tool inheritance

When `--subagent-max-depth` is greater than the current child depth, a child Agent may receive the same bounded canonical `agent` constructor. The model still cannot select depth, budgets, account/model, routing, file-tool mode or worktree. Descendants inherit the root host's `off`, `read-only` or `workspace` file-tool mode unchanged (equal authority, never broader); `trusted`/bash remains unavailable. At the configured maximum depth the `agent` constructor is absent from the child's advertised registry.


## Phase 45 routing does not grant tools

`--subagent-route` can change only the already-authorized provider account/model used for a child. It never changes the host-selected child file-tool mode, root, worktree, shell authority or recursive depth. A routed descendant therefore still receives exactly the same `off`, `read-only` or `workspace` tool grant inherited from the root configuration; `trusted`/bash remains forbidden. The model-facing `agent` JSON has no route/account/model/candidate fields.


## Historical team/planner/review model routes

Phases 46, 49 and 50 introduced `team`, `plan` and `review_release` model tools. The unified Jobs + Agent hard cutover removes those routes and their host adapters. Durable team/planner/review records remain readable for historical/admin workflows, but they no longer add model tools, provider rounds, tool authority or a second Agent orchestration surface. New model-directed child work uses only the canonical bounded `agent` constructor plus the shared Jobs/Graph controls.


### Shared toolset follow-up — 2026-10-30

- AW's supervised Process uses RAII stdout/stderr writer leases, seals reader
  registration on terminal settle, and requires completed writers before
  publishing a complete output receipt.
- Inert `ProcessOutputPage` provides bounded exact head/tail pages; missing
  middle bytes are explicit. Monotonic acknowledgement is permitted only for
  the contiguous bytes actually delivered, never an unseen suffix.
- `bread(format=mcp)` now carries SHA-256 and a conservative MIME source
  derived from bounded complete-file magic rather than an extension, but
  **remains encoded JSON**, not typed provider/MCP media.
- `aw-tools/sandbox` validates host-specified paths, symlink/reparse
  exclusions, nested writes and broad-root write opt-in. It cannot start or
  certify a sandbox; AW trusted shells remain unsandboxed.
- Validation of TS-N1..N5: 38 shared-core, 68 native-tool, 15 Unix platform
  tests; Windows GNU all-target compile and strict Clippy passed using the
  ignored l5 dependency-adjusted fixture. Original pinned full-workspace
  Cargo remains blocked by l5 registry/TLS, not reported as passing.

### TS-N6..N10 tooling continuation — 2026-10-31

- `aw-tools/task_fence`: exact host actor/source epoch/execution site/
  session/branch/call/effect and generation must match on every task
  operation. AW's host-only FencedProcessSpools enforces this for output
  summaries, pages and acknowledgements. Every terminal transition requires
  actual writer quiescence.
- Host-authorized PipeProcess may opt into `HostSpoolFiles` with pre-opened
  independent stdout/stderr files. The shared 64 MiB/stream and 512 MiB/session
  limits bound retained bytes, while all discarded bytes are still counted.
  Fast overflow accounting avoids blocking disk work after quota exhaustion.
  No second task manager, implicit background job or model-owned spool path.
- A bounded `MediaPacket` rechecks complete bytes, SHA-256 and MIME signature
  before an explicit provider host can attach bytes. Native `bread` remains
  `transport:"encoded_json"`; this is **not** a provider-native ContentBlock.
- Linux `BwrapPlan` is an opt-in host-only executable plan using pinned
  workspace/runtime/binary FDs, a no-network default and RAII child ownership.
  It is not registered under AW's trusted shell. Native l5 sandbox smoke
  failed to mount /proc (EPERM), so no runtime sandbox certification is claimed.
- Real AW pinned Cargo.lock validated: 51 shared-core all-feature tests,
  72 native tool tests, 15 Unix platform, 70 Agent-core active tests,
  383 CLI binary and 226 CLI library active tests; workspace all-target
  compilation, strict Clippy, Windows GNU cross-compilation and no_std passed.
  The monolithic full-workspace test did not produce a terminal result.

### Windows mutation-admission interoperability — 2026-10-31

Windows native scoped mutations reserve a separate pathname admission
mutex before the path mutex and file resolution. Both mutex namespaces
use invariant Windows UTF-16 uppercase SHA-256 identities compatible with
current dotlink. One cancel/deadline budget is shared; permissions remain
with AW ToolPolicy. Cross-target compilation passes; native Windows
cross-process runtime verification is still pending.

### Fresh tool correctness pass — 2026-10-31

- Shared spool truncation is irreversible and quota is RAII-owned.
  Preflight requires distinct R/W regular file handles on both Unix
  and Windows. Quiescence and successful output are separate facts:
  Failed/Uncertain settlement is possible after persistence errors,
  but Completed still requires valid stored output.
- One bounded blocking writer per pipe uses 64KiB batches, with no
  batch allocation when the host has not enabled durable spooling.
- Grep marks oversized skipped files as partial results and prefers
  the matching source line over optional excessive context.
- Binary no-ops revalidate the complete source; patch additions use
  linear lookahead; pinned sandbox authorization survives pathname
  replacement and sandbox Drop reaps killed child processes.
- Source and tests: tooling plan REVIEW-FIX-2026-10-31.md.

### TS-N11..N20 shared tooling — 2026-10-31

- Task transitions require durable journal-first CAS. Unknown/panicking
  journal effects fence the local projection pending original-host
  inspection; no automatic replay.
- A versioned spool checkpoint verifies pinned file identity, exact SHA
  digest, source epoch, generation, immutable output bytes and historical
  ACKs before host-handle recovery. Quota is recharged exactly.
- Uncertain effects cannot be expired; retention cleanup requires an
  authenticated settled task and actual host disposal. Host-only bounded
  TaskOps is bridged to the original Process owner without new grants.
- Provider-declared typed media MIME/byte policy is explicit; current
  native bread output remains encoded JSON. Linux sandbox plans refuse
  nested user namespaces and expose checked launcher outcomes.
- Shared output-prefix accounting permanently prevents gap refills;
  platform execution and original Agent/TurnJournal remain authoritative.
- Exact implementation/test gates and remaining storage, native runtime
  and model-facing job integrations: tooling plan NEXT-TEN-2026-10-31.md.

### Post-N20 eight-cycle correctness audit — 2026-10-31

Checkpoint hashing uses positional reads without shared cursor effects;
snapshots reject unsettled tasks or Completed with failed output.
Ambiguous journal callbacks fence future operations, and CAS proposals
carry exact task-binding identity. Retention cleanup is exclusive within
the lease owner and persists confirmed-release state. Host media and
Process source/cancellation callback panics fail closed or return a
durable unknown-effect classification. Media attachment errors and panics
both preserve unknown-delivery semantics. Spool byte quotas transfer only
after StreamSummary commits. Regression/validation: tooling plan
REVIEW-FIX-N20-2026-10-31.md; native runtime gates remain open.


Post-N20 final eight-pass audit result: original Cargo.lock regression
suites pass (34 shared sync, 85 shared all-features, 81 native tools,
15 Unix, 70 active Agent, 383 CLI binary, 226 CLI library); workspace
all-targets compile, strict Clippy, Windows GNU cross-compile and
no_std+alloc compile pass. Native runtime and monolithic workspace-test
completion remain separate open gates. Source/evidence:
tooling plan REVIEW-FIX-N20-2026-10-31.md.


### Sealed DL-150 current validation (2026-10-31)

Original pinned AW feature suites passed under the historical `aw-tools-core` name: read-only 41,
rw 64, full 120; aw-tools 96; Unix platform 15; Agent 70 active;
AW CLI binary 383 and library 226. Strict Clippy, workspace compilation,
Windows GNU cross-compilation, no_std+alloc and formatting passed.
The canonical shared Graph/Job types and cumulative progress types are reused
by the model-facing AW adapter. `batch`, `job_start`, `job_*` and direct trusted
shells all project one live JobRuntime; no alternate filesystem authority,
process supervisor or durable Graph database exists.


The sealed dotlink V2 shared Job specification at 8a3e7bb requires one
execution lifecycle and ID namespace for shell and batch, explicit
`job_restart`, and terminal-before-dependent ordering. AW now binds that
shared live-session JobManager to its original authority and `PipeProcess`
owner: `batch`, `job_start`, `job_*` and direct trusted shells share one
runtime. The outer AW tool journal is the durable replay/no-replay fence;
internal Graph state is intentionally not reconstructed after process loss.


Dotlink sealed a156142: V2 uses one canonical Job(Graph[Node]) execution
owner for standalone bash/pwsh/powershell, explicit `job_start` and `batch`.
AW implements this with one shared JobRuntime/JobManager and the existing
`PipeProcess` supervisor—no second executor or persistent Graph database.
Windows interpreter identity is derived from the canonical prepared executable,
so `pwsh` and `powershell` remain distinct advertised capabilities with no silent fallback.

## Latest shared engine validation — 2026-10-31

Source and test record: owned tooling plan CORE-ENGINE-RESUME-2026-10-31.md.
`aw-tools` is the reusable synchronous filesystem/Graph/Job semantic engine.
aw-tools-fs-unix/win provide the pinned/no-follow native backends.
`aw-tools-adapter` is the one AW Agent/ToolPolicy projection;
write/edit/copy/mv/rm all use shared operations with original locks.
Job(Graph) source-bound replay/no-launch proof is model-facing through
`batch`, `job_start`, `job_*` and direct one-ShellNode trusted shells. The
original AW outer TurnJournal and `PipeProcess` owner remain authoritative;
Job internals are bounded live-session state rather than a second durable database.

For cross-product differential tests, `aw-tools` has an opt-in `conformance`
feature containing only host-neutral versioned request/schema/Graph/status vectors.
It carries no AW Agent or dotlink policy types and is intended for dev/test consumers
that pin the exact same AW Git revision as the secured backend crates.

The secured Unix/Windows crates also expose host-neutral multi-root `CapabilitySet`
backends. They consume prepared allow roots, exclusions and shared private-artifact
registries but no product policy type; dotlink remains responsible for lexical and
resolved authorization and for reauthorizing a followed final-symlink target.


## Current c34ea5a sealed DL-150 parity checkpoint (2026-10-31)

Complete source-of-truth spec: owned dotlink-tool-port/reference/
DL150-SEALED-c34ea5a-README.md and TODO.md. Fresh implementation
parity review: dotlink-tool-port/PARITY-AUDIT-2026-10-31.md.
Handoff and copy-ready next agent prompt:
dotlink-tool-port/NEXT-AGENT-HANDOFF-2026-10-31.md.

Recent shared fixes include caller-root .gitignore isolation, bounded invalid
ignore as incomplete coverage, canonical slash-vs-basename glob, no
non-advancing discovery first page, unaddressable invalid-UTF8 filename
`path_encoding` reporting, atomic grep hit/context records, final-symlink dual
authorization, exact private-artifact recovery, recursive lifecycle exclusions,
and host-owned Job(Graph) execution. Shared safety maxima are host-neutral while
AW's `LocalToolLimits`/`LocalToolConcurrency` preserve smaller product defaults
and current-host policy. No new grant, process supervisor or database was added.
Provider-native data/control ContentBlocks and native Windows/macOS runtime gates
remain separate work.
