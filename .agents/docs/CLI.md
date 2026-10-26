# CLI

## Canonical options

Use canonical options only. For example: aw --state-memory-only --provider-profile llama --model-id MODEL --provider-base-url URL --tools-mode off --output-format jsonl --output-print "Hello". The old --profile, --model, --base-url, --no-state, --print, --output, --tools, --tool-root and other deprecated long spellings are intentionally NOT recognized. No migration helper or compatibility switch is provided.

The same typed Clap Arg IDs, short flags, value parsers, environment names, static defaults, source-bound tool permissions and durable Session checks remain authoritative. Offline aw cli schema now reports schema v2 with supported canonical flags only (no before, old_aliases or deprecated_aliases). The compiled feature cut determines which command paths and options are available through --help. For independent RPC clients, use aw rpc --agent-stdio with canonical provider/state options.

This is a breaking source change; release/version decisions and unfinished qualification are tracked in [release readiness](../plans/release-readiness/TODO.md). Historical implementation is accepted at the [recorded Git checkpoint](../plans/accepted-history/TODO.md).


## Native terminal custody

- Linux Workspace palette `terminal-promote`: move the focused running shell
  into an independent broker, retaining its PID/parser after Workspace exit.
  Cancel outstanding docking offers first; use the printed, quoted attach command.
- `aw workspace attach INVITE`: one admitted native controller; F12 detaches.
- `aw workspace persistent stop INVITE`: explicit stop after detach; success
  requires observed exit of the exact shell, not all escaped descendants.
- `aw workspace persistent inspect DIRECTORY`: read-only custody record status;
  missing proof is UNKNOWN and never causes input or shell replay.
- `aw workspace persistent start ABSOLUTE_ROOT ABSOLUTE_BASH` continues to create
  a new broker-owned shell. Native docking still needs its original owner.

The broker creates no Agent/State/provider context. Migration is Linux trusted
local; actual different-UID runtime rejection needs a privileged runner and
remains unexecuted. See [native custody](../plans/workspace-views/NATIVE-PTY-CUSTODY.md).

---

## R12 — machine-readable stdout and stdio RPC

For one model turn without human terminal output:

    aw --output-format jsonl --output-print "Explain this code"

Full binary stdio RPC, with provider, tool, model and Session flags after the RPC subcommand:

    aw rpc --agent-stdio --provider-profile llama --model-id MODEL --provider-base-url URL --state-memory-only
    aw rpc --agent-stdio --provider-profile llama --model-id MODEL --state-backend jsonl --state-dir /private/aw --session-resume

JSONL-minimal binary, with existing global flags before the RPC subcommand:

    aw --provider-profile llama --model-id MODEL --provider-base-url URL --state-memory-only rpc --agent-stdio

RPC stdin is one version-1 JSON request per line with a unique ID and typed params. Stdout starts with a versioned hello/capabilities record and then emits accepted, streamed AgentEvent records and settled, or typed errors. Never assume acceptance means execution finished. Broken pipes can mean uncertain paid/tool effects: reconnect using the exact prior Session ID and inspect state/messages, never automatically replay. Active abort signals the original Agent cooperatively and never undoes already-dispatched effects.

Read-only methods include get_state, get_messages (bounded recent public turns), and available_models (no implicit network discovery). Prompt and named prompt_template route through the existing Agent and R11 pinned source trust. JSONL new_session/open_session and compact use existing owner-bound journal and R10 compaction, and stateless model/effort changes require the currently approved profile. Direct privileged trusted_bash/extension_control RPC methods are deliberately unsupported.

Opt-in --diagnostics-protocol belongs to Diagnostics & validation and writes only event type/frame size to stderr, not raw model prompts, HTTP wire or credentials. Rust and Python external examples: [client protocol README](../../examples/sdk-rpc-client/README.md).

---

## R11 — source-pinned project instructions, named skills, prompt templates and themes

Normal full and JSONL-minimal builds share one grouped --resources-* family. This is NOT automatic project trust; first explicitly approve the current exact source snapshot (without starting a provider or tools):

    aw -C /project/sub --resources-trust-project /project
    aw -C /project/sub --resources-project /project --output-print "Read approved project instructions"
    aw -C /project/sub --resources-project /project --resources-template review --resources-var language=rust --output-print "Check code"
    aw -C /project/sub --resources-project /project --resources-theme ocean --agent-skill review --output-print "Check code"
    aw --resources-revoke-project /project

The user-private configuration stores canonical root and exact SHA-256 hashes, never project-derived provider/tool authority. Sources include ancestor AGENTS.md/CLAUDE.md/AGENTS.override.md, root .agents/prompts/NAME.md, .agents/skills/NAME/SKILL.md and .agents/themes/NAME.json. Every newly admitted prompt rechecks exact current snapshots; changed/added/deleted/symlinked sources require reapproval. Templates expand only explicit input and NAME=VALUE variables as the User prompt; selected skills require explicit --agent-skill NAME or interactive /skill:NAME TASK. Native local Agent /prompt and /skill show approved resource names; /prompt:NAME TASK executes an approved named template; /theme NAME selects an approved semantic palette, while /theme auto|light|dark resets any named accent.

This phase deliberately admits project files only in the local single-user Agent, NOT browser/owner or Workspace execution. The existing full+minimal CLI /help categories now include Project resources & trust.

---

## R10 categorized provider attempts and Session context (2026-10-10)

Canonical options, full and JSONL-minimal unless noted:

- --provider-pre-dispatch-retries 0..2 (default 0), only trusted positive no-request-dispatch before any bytes; ordinary HTTP failures and partial streams never auto-retry.
- --context-compact-mode ask|off|auto (default ask), --context-keep-recent 16 (1..64), --context-trigger-turns 64 (2..256), --context-known-window-bytes optional, --context-auto-spend off by default. Auto requires both trusted known context metadata and explicit paid consent; otherwise asks. No auto if unresolved effects.
- --context-compact-now explicitly runs one paid, tool-free summary+append-only JSONL CAS for an already-bound exact provider/owner, then exits; in minimal interactive mode /compact does the same. Previous raw turns remain immutable in the JSONL Session log. Full SQLite uses the pre-existing explicit context prepare/generate/apply workflow (not this JSONL shortcut). After crash, a completed saved summary can apply without any second paid model request.

    aw --provider-profile llama --model-id MODEL --context-compact-mode ask --context-keep-recent 16
    aw --state-dir /my/private/state --session-resume --context-compact-now --context-keep-recent 8
    aw --state-backend jsonl --session-resume --context-compact-now
    aw --provider-pre-dispatch-retries 0 --output-print "Never silently retry ambiguous vendor errors"


---

## Latest tools CLI — one --tools-* namespace (2026-10-10)

Use --tools-mode off|read-only|workspace|trusted (OFF by default), --tools-allow, --tools-root and --tools-bash. Scheduling is model-directed by default: the model sets parallel:true on independent read or trusted Bash calls, and omits/sets false for dependent/serial work. Resource controls never create grants.

    aw --tools-mode read-only --tools-parallel model --tools-parallel-limit 16 --output-print "Inspect"
    aw --tools-mode trusted --tools-bash /absolute/path/bash --tools-parallel model --output-print "Run independent checks"
    aw --tools-mode trusted --tools-bash /absolute/path/bash --tools-parallel off --output-print "Run serially"
    aw --tools-parallel-limit 16 --tools-parallel-read-limit 8 --tools-parallel-bash-limit 2 --output-print "Constrain scheduling"

Native filesystem tools now comprise `ls`, `grep`, `read`, `bread` (read-only)
and `write`, `edit`, `patch`, `bwrite`, `bedit` (workspace/trusted).
`ls --pattern`-style tool arguments subsume the former model-facing `find`
tool; this is unrelated to workspace/UI Find. `write` now permits bounded
atomic create/replace, optional create_only and expected_sha256 preconditions,
and preserves unchanged files. Only explicitly marked
`parallel:true` read calls can overlap; mutations remain serialized and
cooperatively lock their workspace. Trusted Unix `--tools-bash` and Windows `--tools-powershell` select
an absolute trusted executable, not a sandbox. The shell tool supports
`dir`, `stdin`, `timeout_secs` and `max_output_bytes` bounded by AW.
Native Windows validation is pending.

Canonical options are --tools-mode, --tools-allow, --tools-root, --tools-bash, --tools-parallel, --tools-parallel-limit, --tools-parallel-read-limit and --tools-parallel-bash-limit. Optional specific ceilings inherit the shared limit; none can raise it. MCP, extension and subagent flags retain their own integration-specific namespaces. A model cannot enable disabled tools, gain credential/root authority, or turn a Bash Process effect into Read merely by requesting parallel:true.


---

Every read wave durably journals all intents before effects, rechecks account/source permission before dispatch and before output disclosure, holds results until completion and writes ToolFinished receipts in model proposal order. Failed/canceled waves disclose no partial results to the next model invocation and never auto-retry; completed but rejected writes/unknowns still require inspection. There are no detached Tokio tasks or implicit background workers.

## R7 versioned process extensions (2026-10-09)

Standard full and JSONL-minimal builds expose one typed CLI for independently compiled external extension tools/hooks, separate from MCP. A child process never starts from help, project files, inactive config or model text; explicit absolute host selection is mandatory.

    aw extension --extension-process box=/ABS/PATH/extension --output-json
    aw extension --extension-process box=/ABS/PATH/extension render box hello name=Alex
    aw --provider-profile llama --model-id MODEL --extension-process box=/ABS/PATH/extension --extension-allow box:echo --extension-hook box:before_prompt --output-print "Run extension"

The child uses versioned JSONL v1 over stdin/stdout; --extension-arg box=LITERAL is not a shell. Process startup clears environment and child stderr. The C11 and Rust executable SDK examples run independently of aw's linked APIs. Discovery lists metadata but creates **no** tool or hook grant. Host --extension-allow approves an exact advertised ext__box__TOOL with SourceKind::Extension Unknown effect; --extension-hook separately approves advertised before_prompt/before_tool/after_tool gates. Both use the current Agent and its journal-before-effect/no-replay safety. Hook vetoes and process crashes fail closed. The executable has the OS user's rights unless independently sandboxed; aw does **not** claim process OS confinement, arbitrary extension provider adapters or full aw plugin parity.

## R6 explicit MCP data commands (2026-10-09)

Both standard full and JSONL-minimal native binaries add typed commands for currently selected MCP endpoints. The prompts listing exposes required/optional argument names in JSON and plain terminal output. No server is started or contacted without --mcp-stdio/--mcp-http/--mcp-sse; --output-json belongs before the selected MCP data subcommand.

    aw mcp --mcp-http box=http://127.0.0.1:8000/mcp prompts --help
    aw mcp --mcp-http box=http://127.0.0.1:8000/mcp --output-json resources
    aw mcp --mcp-http box=http://127.0.0.1:8000/mcp templates
    aw mcp --mcp-http box=http://127.0.0.1:8000/mcp prompt box review path=src/lib.rs
    aw mcp --mcp-http box=http://127.0.0.1:8000/mcp read box test://docs/report
    aw mcp --mcp-http box=http://127.0.0.1:8000/mcp read-template box entry id=report
    aw mcp --mcp-http box=http://127.0.0.1:8000/mcp watch box --mcp-resource test://docs/report --mcp-seconds 3

Only a current host-selected --mcp-resource-allow box:test://docs/report registers the model tool mcp__box__resource_read; this uses R5 SourceKind::Mcp exact grants and Unknown-effect journal. Merely listing a resource or prompting the Agent to look it up grants no authority. Changed resource/tool revisions invalidate tool uses until reconnect/regrant. All remote text remains untrusted data, never an implicit instruction or context. No passive background watch or model action from notifications (only an explicitly granted active Agent keeps its negotiated source change stream), templates level-1 only, and no binary resource decoding.

## Feature cuts

One typed Clap tree owns root/nested help, feature-gated roles and dispatch. Consult `aw cli schema` or command-specific `--help` for the exact supported options; copied historical spellings do not imply aliases.



## Lightweight local build

The local, Rust-idiomatic minimal feature is a separate compile-time closure. It retains the shared Agent loop, provider/TUI/tool code and uses the existing single-owner JSONL SessionCursor/TurnJournal instead of SQLite:

```sh
cargo build --locked --no-default-features --features minimal -p aw-cli --bin aw
aw --model-id MODEL --provider-profile PROFILE --output-print "prompt"  # after building this binary
aw --session-list
aw --session-resume
aw --session-id js_EXACT_ID
aw --state-memory-only                              # deliberately memory-only
```

Bare `--no-default-features` remains memory-only. The full default remains unchanged. Cargo features are additive, so `--no-default-features` is required alongside `--features minimal` to omit the full product. In the lightweight build, `--state-dir` changes the JSONL root; `/new` creates a new file-backed Session, `/resume ID` switches after compatibility checks, `/sessions` lists, `/title TITLE` changes title, and `/acknowledge INSPECTION_NOTE` records explicit inspection of unresolved effects. `--state-memory-only` cannot be combined with saved-Session selection.

Reduced builds omit full-product services; they still use the same Agent and current provider/tool protocols. An API-key adapter does not imply consumer subscription sign-in. Further parity or platform claims require their own tests.

### Native Anthropic API key setup (R3a)

A built-in `anthropic` profile selects the native Anthropic Messages API. Provide `ANTHROPIC_API_KEY` through your environment or an explicitly versioned SecretStore-backed provider account in the full build. Then run `aw --provider-profile anthropic --model-id MODEL --output-print "Hello"`. The first-run wizard also offers Claude API (Anthropic). Explicit `--provider-base-url` override clears the default credential reference for safety; supply `--auth-api-key-env ANTHROPIC_API_KEY` again to retain the intended account. Supported thinking levels use native effort/adaptive mode; model-specific legacy budget thinking and subscription OAuth are separate capabilities. Tool grants are still explicitly controlled with `--tools-mode` and optional `--tools-allow`, and no implicit model/account reroute is performed.

### Native Gemini API key and model-scoped request path (R3b)

The built-in `gemini` profile uses Google AI Studio `GEMINI_API_KEY` as an explicitly named environment key reference; full-build `aw provider-account create --provider-kind gemini` may instead use the existing versioned SecretStore. Use `aw --provider-profile gemini --model-id gemini-3.8-flash --output-print "Hello"` to call native `/v1beta/models/<model>:streamGenerateContent?alt=sse` with `x-goog-api-key` (never Bearer or URL key). Deliberate `--provider-base-url` changes clear a default key reference: provide `--auth-api-key-env GEMINI_API_KEY` again only after confirming the new destination. The first-run wizard offers Gemini API (Google). Tool outputs require explicit current `--tools-mode` and optional `--tools-allow`; signed Gemini function-call continuation is deliberately not stored in ordinary Session JSONL. Only GenerateContent native text/tool interaction is implemented; model-dependent legacy budgets, Interactions API, Vertex credentials, production account compatibility and billing are outside R3b.

### R4 shared provider/model catalog (2026-10-09)

Both std full and std JSONL-minimal builds expose the same typed aw model namespace:

```sh
aw model list --provider-profile gemini               # offline; no API call
aw model show gemini-3.8-flash --provider-profile gemini
aw model refresh --provider-profile anthropic         # explicit network
aw model list --provider-profile gemini --model-refresh      # explicit network
aw model price set gemini-3.8-flash --provider-profile gemini --model-input-usd-per-mtok 0.5 --model-output-usd-per-mtok 3
aw model price estimate gemini-3.8-flash --provider-profile gemini --model-input-tokens 1000 --model-output-tokens 1000 --output-json
aw model price remove gemini-3.8-flash --provider-profile gemini
```

Bundled reference IDs are informational and only shown on their official built-in endpoints. Configured models, discovered model metadata and user-priced overrides have explicit source labels. Listing a model does not authorize account/model execution or verify model availability, price, quota or invoice. Provider discovery is bounded and accepts no model-provider fallback: Anthropic after_id, Gemini pageToken, and OpenAI/Ollama native endpoints share one cache source with first-run onboarding. A fresh cache expires after 24h; stale/offline catalog remains readable, only explicit refresh initiates network. Metadata and user price cache files are private, salt-credential-scoped, endpoint-bound and atomically written. Changed/missing selected API keys cannot inherit cached models or rates; no unsalted API-key hash or raw credential is persisted. Full OAuth plan discovery remains through its separate supported account authority. No background polling or hidden key refresh occurs.

### R5a explicit MCP tools (2026-10-09)

Both standard full and JSONL-minimal aw binaries now support MCP tool calls through the single Agent executor and the official rmcp Rust SDK. MCP servers are **never** started or discovered without explicit flags. Preview advertised names with aw mcp first, without a model, grant or tool call:

    aw mcp --mcp-stdio files=/absolute/path/to/server --mcp-arg files=/absolute/path/to/script
    aw mcp --mcp-http files=https://mcp.example.com/mcp --mcp-auth-env files=FILES_MCP_TOKEN

Then explicitly grant one server tool when starting a native single-user Agent:

    aw --provider-profile ollama --model-id MODEL --mcp-stdio files=/absolute/path/to/server --mcp-allow files:read --output-print "Read that resource"
    aw --provider-profile llama --model-id MODEL --mcp-http files=http://127.0.0.1:8000/mcp --mcp-allow files:read --output-print "Read"

Names are namespaced as mcp__files__read, never inherited from built-in read grants. --mcp-arg files=ARG is a repeatable literal argument, not a shell; --mcp-pass-env files=ENV_NAME forwards an explicitly allowed environment variable to the child. The child starts with a clear environment. --mcp-auth-env and --mcp-ca-file reference explicit HTTP bearer and optional bounded HTTPS PEM trust bundle, respectively. External URL userinfo/query/fragment, remote HTTP without TLS, unknown grants and incomplete JSON Schema are refused. All remote effect hints are untrusted/Unknown, and tool arguments are validated locally before dispatch. No automatic restart/tool replay on uncertain external effects.

The source/client supports modern MCP 2026-07-28, initialized Streamable HTTP MCP 2025-11-25 and explicit legacy HTTP+SSE. MCP data commands share the same grants; browser/owner multi-user MCP is deliberately rejected pending separate grants. No implicit MCP authority restored from saved Session context.

### R5b opt-in legacy MCP 2024 HTTP+SSE (2026-10-09)

Both full and std JSONL-minimal native binaries now support the deprecated MCP 2024-11-05 two-endpoint SSE transport. It is an explicit choice with --mcp-sse, not an automatic fallback from --mcp-http. The user supplies the SSE GET URL; the server must announce a same-origin POST URL in its first endpoint event. MCP discovery, exact grants, bounded JSON Schema and tool effects use the R5a official rmcp client and one Agent/journal.

    aw mcp --mcp-sse old=http://127.0.0.1:8000/sse
    aw --provider-profile ollama --model-id MODEL --mcp-sse old=http://127.0.0.1:8000/sse --mcp-allow old:read --output-print "Read via legacy MCP"

Remote servers require verified HTTPS; --mcp-auth-env old=ENV_NAME references an explicit bearer, and --mcp-ca-file old=/ABS/CA.pem supplies an optional bounded HTTPS trust bundle. The selected bearer reaches only the original SSE origin and a POST endpoint passing exact scheme/hostname/port matching. Server readOnlyHint creates no permission. Malformed/missing endpoints and stream failures return errors, with no reconnect/uncertain-effect replay. No no_std dependency is added.

## Current native mode grammar (Phase 54 foundation + post-Phase59 command hardening)

Application mode and Agent presentation are independent:

```text
aw [--ui-mode agent|workspace] [--tui-mode scrollback|inline|fullscreen] [RUN OPTIONS] [PROMPT...]

aw workspace                 # open full native Workspace
aw w                         # alias for aw workspace
aw workspace open NAME       # explicit named Workspace
aw workspace list|create|show|archive|writer-* ...
```

`--ui-mode` defaults to `agent`. `--tui-mode` applies only to Agent mode and defaults from `[ui].tui_mode`, whose default is `inline`.

Agent TUI modes:

- `scrollback` — append-oriented terminal history with one persistent bounded inline viewport;
- `inline` — aw owns/redraws the visible primary screen; no alternate screen;
- `fullscreen` — same Agent renderer on the alternate screen.

Workspace is a distinct multi-pane fullscreen application, not `TuiMode::Fullscreen`. Interactive presentation switching is now one typed command: `/tui scrollback|inline|fullscreen|workspace`. `/tui` with no value reports the current Agent presentation plus that usage. `/workspace` remains the direct shortcut for `/tui workspace`. The old standalone `/scrollback`, `/inline`, `/fullscreen`, and `/agent` slash commands are no longer registered. Inside Workspace, `/minimal` is the one return-to-Agent command and preserves the configured Agent `tui_mode` instead of changing it. The persisted `[ui].tui_mode` remains only the three Agent physical presentations; `workspace` is a `TuiTarget` application destination, not a fourth persisted Agent renderer mode.

The minimal build accepts `--ui-mode=agent` and all three Agent TUI modes; `--ui-mode=workspace` fails explicitly because that build omits the Workspace application.


## Phase 61 / 66 skill registry and portable bundles

The persistent local catalog is managed through one typed resource surface:

```text
aw skill list
aw skill show NAME
aw skill add NAME PATH [--skill-description TEXT] [--skill-replace]
aw skill remove NAME
aw skill export NAME OUTPUT [--skill-replace]
aw skill verify BUNDLE
aw skill install BUNDLE [--skill-replace]    # `import` alias
aw skill update BUNDLE
```

Registry/install presence never activates a skill. Runtime activation remains explicit with `--agent-skill NAME` for a registered entry or `--agent-skill NAME=PATH` for a one-run source. Phase-66 bundles contain UTF-8 instruction text only, carry a deterministic SHA-256 integrity digest, install to content-addressed managed local files, and do not contain scripts/WASM/tool grants/provider credentials. The digest is integrity metadata, not a publisher signature. Post-Phase68 hardening resolves relative explicit config paths once before managed installation and serializes no-replace bundle export. All native/web/skill config mutations now share one locked three-way merge authority: unrelated concurrent changes are preserved and stale same-field edits conflict instead of being silently overwritten.

## Phase 67 governed schedules

```text
aw schedule define NAME --schedule-binding BINDING --schedule-every-seconds N \
                 [--schedule-delay-seconds N] [--schedule-overlap serial|skip] [INPUT...]
aw schedule list
aw schedule show SCHEDULE
aw schedule pause SCHEDULE --schedule-expected-generation N
aw schedule resume SCHEDULE --schedule-expected-generation N
aw schedule occurrences SCHEDULE
aw schedule run-due [normal provider/tool/target options]
```

Schedules are bounded interval triggers over an already-authorized **active caller-owned user-scoped task pipeline binding**. `run-due` claims/admit/runs at most one occurrence per invocation and reuses the existing pipeline executor. Claims use durable store-process identities, survive owner restart, and reconcile an already-committed run by the reserved deterministic `sched:<schedule>:<occurrence>` request ID. Manual pipeline runs cannot use the `sched:` namespace. Pause prevents an unadmitted orphan claim from running; resume restarts cadence when its due time was missed. Long active downtime coalesces to at most one catch-up occurrence, so there is no silent burst backfill. No cron parser, arbitrary scheduled prompt, shell/webhook trigger, model-authored schedule, Session-scoped schedule or hidden scheduler/provider loop exists.

## Phase 60 explicit named skills

Skills are explicit named UTF-8 instruction resources, not tools or executable plugins. Select them per run with repeatable `--agent-skill NAME=PATH`. aw loads only the exact paths named by the user, permits at most 16 skills, bounds each file to 64 KiB, rejects duplicate names/exact paths, and keeps the existing 128 KiB combined system/instructions/skills ceiling. No `SKILL.md`, project, config-directory or render-time discovery occurs.

The provider-bound system envelope contains the selected **skill name and contents**, but never the local source path. The local `[Skills]` header shows `NAME · PATH · N B` provenance through the shared Phase-59 resource summary. Selecting a skill changes system instructions, so an existing saved Session will reject a changed skill set under the existing “saved system instructions differ; start a new session” binding rule.

## Phase 55 terminal exit semantics

Interactive Agent input uses a real bounded idle double-Ctrl+C contract in all three Agent TUI modes. With an idle/empty editor, the first Ctrl+C arms a one-second quit window and shows `Ctrl+C again to quit`; the second press inside that window exits cleanly through the typed run-loop outcome. A delayed second press re-arms instead of exiting. Ctrl+C while a model/tool turn is live remains cooperative cancellation, and embedded Workspace terminal Ctrl+C remains owned by the child PTY.

`/quit`, double Ctrl+C, normal errors, Rust panic unwinding and catchable Unix SIGHUP/SIGTERM/SIGQUIT all use the shared native terminal restore seam. This restores raw mode/cursor/paste/mouse/alternate-screen state best-effort before returning control. SIGKILL/power-loss restoration is not claimable.

Workspace keeps Alt+1..9 positional selection but no longer renders numbered tab/navigator prefixes or an `Explorer` pane title.

## Phase 57 Agent flow and Scrollback exit

`--tui-mode=inline` and `--tui-mode=scrollback` now use natural vertical flow: short Agent content is followed immediately by status/transient rows and the composer, with unused terminal rows below; after content fills the terminal the composer remains bottom-pinned while history scrolls above. Fullscreen retains full-screen ownership. Default status lives above the composer; custom BelowComposer rows remain opt-in.

In Scrollback mode `/quit` is committed visibly before the transient inline viewport is collapsed, so the invoking shell resumes immediately after aw's final committed line. Idle double Ctrl+C still exits without inventing a prompt. All terminal-mode restoration continues through the shared Phase-55 terminal lifecycle.

## 1. CLI principle

The CLI follows one rule:

> **The top level runs work. Resource nouns manage durable objects. Flags refine execution.**

The executable is always a single binary: `aw`.

### 1.1 Phase-36 parser architecture

The public CLI is one typed Clap tree. `Cli` owns the root run path plus `TopCommand`; resource modules expose reusable typed `Subcommand` enums and receive already parsed values. `attach`, `team`, and `device` are also root-tree nodes rather than separate argv reparsers.

Role-specific parser groups (`ProviderCliArgs`, `ToolCliArgs`, target/web/owner/open groups) compose into one internal `RunArgs`, so help stays relevant without forking runtime semantics. There is no `nested_argv`, `parse_canonical_args`, or manual `std::env::args_os()` command dispatch.

Help/parsing is side-effect free: it does not open the store, create/migrate state, read provider secrets, open listeners, start Git/PTY resources, or invoke a provider.

```text
aw [RUN OPTIONS] [PROMPT...]

aw workspace <VERB> ...
aw session   <VERB> ...
aw domain    <VERB> ...
aw machine   <VERB> ...
aw fleet     <VERB> ...
aw identity  <VERB> ...
aw context   <VERB> ...
aw auth      <VERB> ...
aw provider-account <VERB> ...
aw net       <VERB> ...

aw attach    [OPTIONS]
aw team      <VERB> ...
aw device    <VERB> ...        # advanced personal-inference operations
aw agent     [OPTIONS]
aw web       [OPTIONS]
```

Do not encode first-class resource operations as mode flags. For example:

```text
canonical:   aw workspace open aw-dev
not:         aw --workspace          # historical UI-mode spelling only
```

At the same time, `--workspace-id NAME` remains a valid **run-selection flag** for the default top-level agent action:

```text
aw --workspace-id aw-dev "fix the failing parser test"
```

That means “run this prompt in Workspace `aw-dev`”; it does not mean “open the Workspace UI.”

## 2. The default command is an immediate agent run

The most important path stays minimal:

```bash
aw
```

opens/continues the ordinary interactive agent using defaults.

Supplying positional text immediately creates/continues the applicable durable Session and submits it:

```bash
aw "fix the failing parser test"
aw fix the failing parser test
```

All remaining positional words are treated as the prompt after global/run-option parsing. If a prompt starts with a reserved subcommand or `-`, use conventional `--`:

```bash
aw -- workspace allocation is leaking memory
aw -- "-O3 causes this test to fail"
```

### 2.1 Default resolution

With no overrides, an immediate run resolves approximately:

```text
Domain       current/default Domain
Workspace    local
On           Workspace default/local Machine
Profile      user/default execution profile
Session      create or continue according to normal interactive behavior
Persistence  durable unless explicitly memory-only
```

`local` is the reserved default HostedWorkspace on every AwNode.

## 3. Run-selection flags

The canonical run flags are:

| Flag | Meaning |
|---|---|
| `-w, --workspace-id NAME` | Select the Workspace that owns the Session/run. |
| `--domain-id NAME` | Select a Domain when needed for disambiguation or deliberate context change. |
| `--workspace-target TARGET` | Select an implemented execution/view target. Remote targets still require their explicit current Machine/Domain/AwNet authority. |
| `-p, --provider-profile NAME` | Apply a saved execution profile. |
| `-m, --model-id MODEL` | Override the selected profile/default model. |
| `-a, --account-id ACCOUNT` | Override the permitted provider account. |
| `--tools-mode MODE` | Override the local filesystem/shell tool grant/profile for this run. |
| `--subagent-enabled` | Explicitly permit bounded durable child-Agent inference through the `subagent` tool. Separate from local file-tool grants. |
| `--subagent-tools MODE` | Host-selected child file tools: `off` (default), `read-only`, or `workspace`; non-off requires `--subagent-enabled` and an isolated clean Git worktree. `trusted` is rejected for children. |
| `--subagent-max-created N` | Root-turn child-allocation cap; default 32, hard max 32. |
| `--subagent-max-active N` | Root-turn concurrent child cap; default 4, hard max 4 and cannot exceed max-created. |
| `--subagent-max-model-attempts N` | Root-turn child provider-attempt cap; default 64, hard max 256. |
| `--subagent-max-depth N` | Recursive child depth; default 1 preserves non-recursive behavior, hard max 4. Descendants share the same root envelope. |
| `--subagent-route ROUTE` | Optional actor-owned immutable routing strategy for child workers; strategy role must be `worker`. No flag preserves exact parent-model inheritance. |
| `--subagent-route-revision REV` | Optional exact immutable child-route revision; requires `--subagent-route`. |
| `--subagent-team TEAM` | Optional actor-owned immutable bounded team template exposed as the root Agent `team` tool; requires `--subagent-enabled`. |
| `--subagent-team-revision REV` | Optional exact immutable team-template revision; requires `--subagent-team`. |
| `--subagent-deadline-secs N` | Root delegation deadline from first child allocation; default 3600, hard max 86400. |
| `--session-id ID` | Continue an existing Session instead of creating/selecting the ordinary current one. |
| `--session-branch ID_OR_NAME` | Select a branch within an existing Session. |
| `--output-print` | Run non-interactively and print the result. |
| `--output-json` | Request machine-readable output/events where supported. |
| `--state-memory-only` | Explicit memory-only compatibility/development mode; never the default. |

Examples:

```bash
aw --workspace-id aw-dev "fix the failing tests"
aw -w aw-dev --workspace-target pvl-x2 "inspect the ROCm failure"
aw -p coding "implement the parser"
aw -p coding -m qwen3.5 "implement the parser"
aw --session-id sess_123 "continue with the implementation"
aw --subagent-enabled "delegate bounded research when useful"
aw --subagent-enabled --subagent-tools workspace "delegate an isolated edit when useful"
aw --subagent-enabled --subagent-route worker-route "delegate to permitted worker targets"
aw --subagent-enabled --subagent-team candidate-team "use the bounded team when useful"
```

### 3.1 Profile semantics

A **profile** is a named bundle of ordinary execution defaults, for example:

```toml
[profiles.coding]
account = "personal"
model = "qwen3.5"
tools = "workspace"
thinking = "high"
```

A profile may later include routing/default inference behavior, but it is not an authenticated human identity and cannot silently impersonate another user.

Resolution order is:

```text
explicit CLI flag
    > selected profile
    > Workspace defaults
    > Domain defaults
    > built-in aw defaults
```

Provider account, payer, credential custody and inference executor remain independent of Workspace/Machine selection. `--workspace-target pvl-x2` never silently changes the model account or payer.

### 3.2 Durable shared provider accounts

`--account-id ID` remains explicit selection. If `ID` names a durable Phase-31 `provider-account`, aw requires the current stable user grant plus exact provider kind/model/executor/endpoint/credential version and resolves the secret through SecretStore immediately before dispatch. Failure is fail-closed; it does **not** fall back to a profile/environment account with the same textual ID.

Shared credentials are administered under `aw provider-account`; native rotating login/refresh state is administered under `aw auth`. Raw provider keys and refresh bearers are private-file material, not ordinary CLI-string arguments.

### 3.3 Explicit materialized-fact context

`aw context materialized-project` takes exact already-materialized fact IDs plus the current exact Session branch target and creates an immutable, non-generative candidate. It does not invoke a provider and does not mutate the target branch. `aw context apply` is a separate operation using existing write/disclosure/head/revision/writer-CAS rules.

Materialized remote facts are rendered as User-role derived context data, not privileged instructions. Their exact fact provenance and context-authorization policy ID/revision are inherited by descendants and revalidated immediately before provider use. Revocation blocks new use without deleting immutable prior history.

## 4. Resource commands use noun → verb → arguments

### 4.0 Implemented Phase 36 command surface

The current binary implements these primary resource verbs (use each namespace's `--help` for arguments/options):

```text
workspace  list create show archive writer-show writer-events writer-failover
session    list show tree
            # `aw session open ID` is a canonical top-level compatibility path

domain     list create show
           authority-show authority-configure authority-bundle authority-accept
           authority-grants authority-revoke-grant
           authority-election-begin authority-election-show authority-election-activate
           authority-replica-show authority-promotion-begin authority-promotion-show
           authority-promotion-activate authority-promote
           federate federations lease lease-check revoke-lease revoke-federation
           sync-export sync-import sync-inbox
           materialization-policy-create materialization-policies
           revoke-materialization-policy materialize materialized-facts

machine    list show register
fleet      list create show add plan runs dispatch-member run-update
identity   show device-create device-register device-revoke
           oidc-link oidc-revoke continuity-accept continuity-grants
           continuity-revoke bindings
auth       issue verify rotate revoke list
provider-account create list rotate grant grants revoke-grant versions revoke
oidc-provider create list rotate-secret revoke
oidc       login link
oauth-connector create list authorize refresh rotate-secret revoke
context    target materialized-project artifact apply pending discard-pending
net        status peers pair candidates room rendezvous punch relay serve connect
           machine-serve machine-connect
           authority-* identity-* workspace-* federation-* remote-session-*
```

`aw workspace open NAME`, `aw session open ID`, `aw attach`, `aw team`, `aw device`, `aw agent`, and `aw web` are first-class entries in the same root Clap tree. They do not reparse `std::env::args_os()` through private mini-parsers.

The examples below describe the desired shallow grammar. Where an example names an ergonomic verb not listed above (for example `workspace rename`, `machine labels`, `fleet remove`, or managed-AwNet `join`/`leave`), treat it as a **planned alias/product surface**, not as a currently shipped command.

Administrative/resource operations use explicit verbs. Do not overload arbitrary resource names as verbs.

### 4.1 Workspaces

```bash
aw workspace open NAME
aw workspace list
aw workspace ls                 # short alias for list
aw workspace create NAME
aw workspace show NAME
aw workspace rename OLD NEW
aw workspace archive NAME
aw workspace delete NAME        # only where lifecycle policy permits
```

`aw workspace open NAME` means **open the native/full Workspace UI** for that Workspace. It is deliberately distinct from:

```bash
aw --workspace-id NAME "prompt"
```

which runs work in that Workspace without implying the full Workspace UI.

Target selection composes with opening:

```bash
aw workspace open aw-dev --workspace-target pvl-x2
aw workspace open gpu-dev --workspace-target fleet
```

For a FleetWorkspace, changing `--workspace-target` changes the workspace lens/target without moving the Workspace authority itself.

### 4.2 Sessions

```bash
aw session open SESSION_ID
aw session list
aw session ls
aw session show SESSION_ID
aw session title SESSION_ID "Readable title"
aw session tree SESSION_ID
aw session recovery SESSION_ID
```

Normal users usually create Sessions by simply running `aw`; Session commands are primarily for navigation, inspection and recovery.

Phase-42 child execution resources are managed separately:

```bash
aw subagent list
aw subagent show SUBAGENT_ID
aw subagent budget SESSION_OR_CHILD_SESSION
aw subagent cleanup SUBAGENT_OR_BINDING_ID
aw subagent cleanup SUBAGENT_OR_BINDING_ID --subagent-discard
```

Phase-46 immutable team-template management:

```bash
aw team-template define FILE
aw team-template list
aw team-template show TEAM [--team-revision REV]
```

Cleanup is owner-only, never a model tool, refuses running children, retains dirty work unless `--subagent-discard` is explicit, and never deletes child Session/history.

Inside native Agent mode, `/title TITLE` renames only the current durable Session; `/title` shows it. It makes no model call. The reserved future `/title-generate` should be a separate explicit tool-free model operation, defaulting to the current Agent account/model unless Settings selects another already-authorized title model.

Durable retry/recovery should normally surface through interactive hints rather than require users to manipulate attempt IDs manually.

### 4.3 Domains

```bash
aw domain list
aw domain ls
aw domain create NAME
aw domain show NAME
aw domain link ...
aw domain unlink ...
```

Domains are ownership/administration/security authorities, not network tunnels. Domain federation relationships may use whatever authenticated AwNet route is currently available.

### 4.4 Machines

```bash
aw machine list
aw machine ls
aw machine show NAME_OR_ID
aw machine rename OLD NEW
aw machine labels NAME_OR_ID ...
```

Routine remote usage does **not** require a `machine connect` command. AwNet supplies connectivity and the Workspace `On:` target selects where views/execution run.

### 4.5 Fleets

```bash
aw fleet list
aw fleet ls
aw fleet create NAME
aw fleet show NAME
aw fleet add FLEET MACHINE
aw fleet remove FLEET MACHINE
```

A Fleet groups Machines. A FleetWorkspace is still a Workspace, so create/manage it under `workspace`, for example conceptually:

```bash
aw workspace create gpu-dev --fleet-id gpu-lab
```

Do not turn `fleet` into a second Workspace namespace.

## 5. Presentation/runtime commands

### 5.1 `aw web`

```bash
aw web
aw web --workspace-id aw-dev
aw web --workspace-id gpu-dev --workspace-target pvl-x2
```

`aw web` is a browser presentation of the same aw resources/runtime. It should attach to an appropriate already-running local runtime when possible or start the required local runtime explicitly; the browser does not become another canonical Session/database owner.

### 5.2 `aw agent`

```bash
aw agent
```

runs this installation as a long-lived AwNode/Machine Agent. It owns/serves its durable local Workspaces/Sessions/resources and participates in direct-peer/AwNet connections as configured.

Use `aw agent` for the long-lived owner role.

`aw agent` is still the **same binary and same native agent/runtime implementation**; it does not introduce another LLM engine or daemon package.

### 5.3 `aw attach`

```bash
aw attach
```

attaches another CLI/TUI presentation to an already-running local `aw agent`. It does not open the SQLite store independently or construct another canonical runtime.

Routine remote Machine browsing is performed through Workspace/AwNet targeting, not by teaching users to manually attach to remote sockets.

## 6. AwNet commands

All networking remains under one shallow `net` namespace. The current Phase 30 binary includes pairing, candidate/rendezvous/punch/relay/connect helpers, typed MachineLink operations, authority/Workspace/identity transport, federation sync and remote Session control:

```bash
aw net pair [CODE]
aw net status
aw net peers
aw net rendezvous ...
aw net serve ...
aw net relay ...
aw net connect ...
aw net remote-session-status ...
aw net workspace-handoff-sync ...
aw net identity-auth ...
```

Meaning:

- `pair` — create persistent direct AwNode identity trust; pairing alone grants no Workspace/Domain/user authority;
- `rendezvous` / `punch` / `relay` / `connect` — connectivity only, never policy authority;
- `machine-*`, `authority-*`, `workspace-*`, `identity-*`, `federation-*`, and `remote-session-*` — typed capability-specific operations whose authorization lives above transport;
- `serve` — run reusable self-hosted AwNet coordination; it does not become Domain authority;
- `status` / `peers` — inspect connectivity/peer identity without changing authority.

Managed-control convenience commands such as conceptual `aw net join` / `leave` remain planned product-surface aliases rather than current Phase 30 verbs.

These are roles of the same `aw` executable. There is no separate `awnetd`, relay binary or WireGuard daemon requirement.

## 6.1 Phase 37 first-run/native Settings onboarding

Bare interactive `aw` may start with no configured model. On a TTY, when there is no effective model and no immediate prompt/management action, Phase 37 opens a small Pi-like provider/model chooser before any model-bound Session is created.

The current guided choices after the Phase-37 design adaptation are deliberately small:

- **ChatGPT subscription** through the dedicated OpenAI Responses + Sign in with ChatGPT path;
- **Ollama Cloud** via `OLLAMA_API_KEY` environment reference;
- custom OpenAI-compatible **Chat Completions** endpoint;
- custom OpenAI-compatible **Responses API** endpoint;
- skip/cancel.

Local Ollama remains a supported built-in profile and explicit CLI/config target; it is no longer a separate first-screen provider card because the guided list above is the intentionally small product surface. Native Ollama and Ollama Cloud profiles default ordinary turns to `think=false` when `think` is omitted so reasoning-capable models return visible assistant content. `think=true` is an explicit opt-in only; provider-private `message.thinking` remains non-transcript continuation data and a thinking-only response with no visible content fails as a protocol error instead of settling a blank Chat turn.

Ordinary profile editing always requires an explicit model and never sends a provider request merely to save configuration. ChatGPT subscription setup is different only after the user explicitly chooses it: aw persists a stable installation host ID, performs dynamic OAuth registration with PKCE/nonce on loopback, stores access/refresh/ID tokens only in SecretStore, retrieves that account's permitted model catalog, and saves the selected model through the same `ProfileUpdate` / `apply_profile_update` authority.

Native `/settings` exposes the same provider editor through **Provider & model…**. Local single-user web Settings uses the same validated profile mutation/persistence authority and can initiate the same dedicated ChatGPT login flow. Raw secret values are never written to ordinary config; non-OAuth profiles persist only environment-variable names/references, while OAuth profile metadata contains only opaque connector/host references.

Changing provider/model later from native Settings intentionally starts a fresh model context by dropping the current live Agent/Session handle and stale explicit account selector before the next prompt. Saved history is retained durably but is not silently sent to the newly selected endpoint.

The Chat Completions adapter remains separate. ChatGPT-subscription OAuth is **never** passed through that adapter: it uses the dedicated Responses adapter with `store:false`, provider-private reasoning hidden from UI/journal, and same-provider opaque encrypted continuation only where needed inside an active tool loop.

## 7. Advanced personal-device inference

Keep the existing advanced namespace:

```bash
aw device key ...
aw device grant ...
aw device run ...
aw device receipts ...
```

This exists for the separately trusted personal-inference executor and credential-custody boundary. Normal users should eventually configure accounts/devices through higher-level UI/settings rather than manually operate these commands.

## 8. Help organization

`aw --help` should remain compact:

```text
aw — agentic workspace

Usage:
  aw [OPTIONS] [PROMPT...]
  aw <COMMAND>

Everyday:
  workspace    Open and manage Workspaces
  web          Open the browser Workspace
  attach       Attach to a running local aw Agent
  agent        Run this Machine as an AwNode

Resources:
  domain       Domains and federation
  session      Durable Sessions
  machine      Machines
  fleet        Fleets

Network:
  net          Pair, join and operate AwNet

Advanced:
  device       Personal inference executor and grants
```

Subcommand help stays similarly shallow:

```text
aw workspace --help
  open
  list (ls)
  create
  show
  rename
  archive
  delete

aw net --help
  pair
  join
  leave
  status
  peers
  serve
  relay
```

## 9. Design invariants

1. Bare `aw` remains the lowest-friction path.
2. Immediate prompt execution is a top-level default action, not hidden under `run`.
3. Resource management follows noun → verb → arguments.
4. A first-class resource is not represented primarily by a boolean mode flag.
5. `--workspace-id` selects a Workspace for a run; it does not open Workspace UI.
6. `aw workspace open` opens Workspace UI and does not itself imply a model call.
7. Explicit CLI flags override profile/workspace/domain defaults.
8. Workspace/Machine/Fleet selection never silently changes account, payer or credential custody.
9. `aw agent` and all AwNet roles remain part of the single `aw` binary.
10. The CLI exposes simple common actions first and keeps durable IDs/policy machinery available without requiring them for ordinary use.


### Saved Agent selection in fullscreen

`/resume SESSION [BRANCH]`, Session-tree clicks and command-palette Session selection reactivate the selected durable Agent in its own saved working directory/tool root/profile/model binding. They do not reinterpret that Session under the currently active Agent's root. Real binding changes still require an explicit new/import/transfer flow; ordinary caller-supplied resume under a different root continues to fail closed.


### Governed planner flags

The native owner can opt into Phase-49 planning only together with `--subagent-enabled`:

- `--subagent-planner-route ROUTE` — actor-owned immutable routing strategy; Phase 49 requires `role=planner` and `mode=fixed`.
- `--subagent-planner-route-revision REV` — optional exact immutable planner revision; requires the route selector.
- `--subagent-planner-role ROLE` — repeatable host-approved semantic task role. The planner may use only these role labels.
- `--subagent-planner-max-tasks N` — planner proposal bound, 1..6; default 4.

With planner enabled the root Agent gains a `plan` tool whose model arguments are only `task` and optional `title`. Planner and planned-child provider/model/account/routing/tool/worktree/depth/budget choices are not model arguments. Planned tasks execute sequentially as ordinary subagents, and dependencies do not automatically disclose predecessor answers.

### Guardian review flags and review decisions

The native owner may add the Phase-50 review gate only with the existing `--subagent-enabled` opt-in and at least one reviewed source (`--subagent-team` and/or `--subagent-planner-route`):

- `--subagent-guardian-route ROUTE` — actor-owned immutable routing strategy; Phase 50 requires `role=guardian` and `mode=fixed`.
- `--subagent-guardian-route-revision REV` — optional exact immutable guardian revision; requires the route selector.
- `--subagent-guardian-policy TEXT` — required bounded host-authored review policy. This is host configuration, not a model argument.

When enabled, joined team/planner evidence is reviewed before it is disclosed to the parent model. A guardian `pass` may release it immediately; `deny` and `require_evidence` do not. `needs_human` creates an explicit durable approval request.

Human review state is managed through the typed resource command:

```bash
aw review list
aw review show REVIEW_ID
aw review approve REVIEW_ID
aw review deny REVIEW_ID
```

Only the stable owning actor may decide the waiting approval. A later parent turn may use the root-only `review_release` tool after approval. Its model-facing schema contains only `review_id`; it cannot select policy, source, account/model, route, tools or budgets. Release remains bound to the original parent Session/branch and exact parent `InvocationIdentity`, so approval is not a context-transfer or provider-switch mechanism.