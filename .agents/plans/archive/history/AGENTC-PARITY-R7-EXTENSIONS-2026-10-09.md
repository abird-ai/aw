# R7 — process extensions, typed hooks and C/Rust examples

Date: 2026-10-09. Status: implemented and runtime-validated; unnumbered R7 checkpoint ready to commit. Unnumbered R7 of the one-way agentc → aw parity roadmap (A7). Based on R6 commit 53dec0e. Normal/full aw stays std-first; version 0.15.0, SQLite v33, phase-68 tag unchanged.

**Historical authority:** FULL-COMPREHENSIVE-HANDOFF-2026-10-09.md is frozen byte-for-byte, including original R0–R13 roadmap and Phase 0–68 annexes. This file is a dated successor. Underlying design: [Rust-native master](AGENTC-RUST-NATIVE-PARITY-MASTER-PLAN-2026-10-09.md), §8; [embedded tiers](AW-EMBEDDED-NOSTD-LIBC-FREE-ARCHITECTURE-2026-10-09.md).

## Design and user contract

R7 implements a **single Agent** with independently compiled process tools and host-approved lifecycle gates. It reuses aw-agent-core Extension, Tool, ToolRegistry, ToolPolicy, SourceKind::Extension (from portable E1 kernel) and existing durable journal-before-effect. No second Agent, provider, credential store, tool loop or embedded runtime.

- Explicit --extension-process ID=/ABS/EXEC starts an absolute executable only in native single-user Agent or an explicit inspection command; up to 8 selected. --extension-arg ID=ARG passes at most 16 bounded literal arguments. Never run shell/PATH, auto-discover project files, auto-install/load libraries, or start a background extension on aw --help. Child environment is cleared, stderr suppressed and process kill-on-drop. **This is not a filesystem/network OS sandbox**: independently executed code still has the launching OS user's rights. Only select trusted executables or sandbox them externally.
- Versioned JSONL process protocol v1 over stdio: host requests and peer responses have exact version, monotonic ID, bounded UTF-8 newline frame and typed payload. Every request is serialized. ID mismatch, EOF, invalid output, crash, cancellation, deadline, oversized response or unexpected schema poisons that process; no automatic retry/reconnect or effect replay. Handshake deadline 5s, per operation 20s, frame 256 KiB, manifest 128 KiB, tool JSON Schema 64 KiB with local references only, tool input 64 KiB, tool output 128 KiB.
- Data-only describe returns name, revision, up to 32 tool descriptions, up to 3 supported hooks and optional prompt templates. Manifest revision is informational, never an authorization token. Tool names/duplicates/schema bounds are checked before admission. No unknown effect/read-only hint is trusted.
- Host --extension-allow ID:TOOL (opt-in, repeatable) admits only a currently advertised, locally JSON-Schema-validated tool named ext__ID__TOOL, SourceKind::Extension and random host-issued nonzero epoch, current actor/site and Unknown effect. Complete selected registry collision/capacity preflight happens before registering anything. The shared Agent journals unknown external effects before calling the child; unselected tools cannot acquire grants.
- Host --extension-hook ID:before_prompt|before_tool|after_tool separately authorizes an advertised gate, using the existing ordered and timeout-bounded Extension trait. An advertising extension without explicit hook approval does NOT run a hook. Hooks receive bounded public prompt/tool data only, not bearer tokens, private provider continuation or Session/SQL handles. Responses must contain only an allow boolean; cannot change identity, provider, route, tool schema/effect or grant. Pre-effect veto fails before action. After-effect veto cannot undo or silently replay its previous effect.
- Host can revoke an exact registered Extension source/epoch using the existing trusted Agent revocation API; further model-visible and effect grants disappear. There is no live source rebind, silent upgrade or plugin-created grant. Host extension-hook changes require explicit Agent recreation and new source epoch.
- The typed aw extension command inspects an explicitly chosen program and its metadata without an Agent, execution grant or provider. aw extension ... render ID TEMPLATE VARIABLE=VALUE obtains bounded untrusted text **only on explicit request**, never inserts a system message or starts a model.

## Wire protocol v1

Exact newline-separated JSON objects (illustrative):

    -> {"v":1,"id":1,"method":"describe","params":{}}
    <- {"v":1,"id":1,"ok":true,"result":{"protocol":1,"name":"example",
         "revision":"v1","tools":[{"name":"echo","description":"echo text",
         "input_schema":{"type":"object","properties":{"message":{"type":"string"}}}}],
         "hooks":["before_prompt","before_tool","after_tool"],
         "prompts":[{"name":"hello","description":"text","arguments":["name"]}]}}

    -> {"v":1,"id":2,"method":"tool.execute","params":{"name":"echo",
         "arguments":{"message":"hello"},"turn":1}}
    <- {"v":1,"id":2,"ok":true,"result":{"content":"hello","is_error":false}}

    -> {"v":1,"id":3,"method":"before_tool","params":{"turn":1,
         "tool":"ext__box__echo","id":"call_0","arguments":{},
         "effect":"unknown"}}
    <- {"v":1,"id":3,"ok":true,"result":{"allow":true}}

Supported methods: describe, tool.execute, before_prompt, before_tool, after_tool, prompt.render. Bounded result parsing uses exact recognized fields. A false/malformed response fails closed. There is no general arbitrary command execution, sampling request, root request, provider credential or dynamic Agent authority endpoint in this protocol.

## User workflows

    aw extension --extension-process box=/ABS/EXEC --json
    aw extension --extension-process box=/ABS/EXEC render box hello name=Alex

    aw --profile llama --model MODEL --extension-process box=/ABS/EXEC \
       --extension-allow box:echo \
       --extension-hook box:before_prompt \
       --extension-hook box:before_tool \
       --extension-hook box:after_tool --print "Use approved extension"

The flag --tools off affects host file tools, not an independently selected exact external tool grant. The web/owner and multiuser surfaces reject extension processes until a separate authenticated trust architecture is approved.

## Independent C and Rust process SDK examples

- [Rust executable](../../examples/process-extension-rust/src/main.rs) uses its own std/serde_json, with no aw Agent dependency, plus [Cargo package](../../examples/process-extension-rust/Cargo.toml). Build executable aw-process-extension-rust-example with cargo build --locked -p aw-process-extension-rust-example.
- [C11 executable](../../examples/process-extension-c/hello.c) uses only native stdio plus wire header. Build using cc -std=c11 -Wall -Wextra -Werror -O2. The small C fixture decodes only the host's fixed demonstration frame shape; **production C tools must use a proper bounded JSON parser**, not its substring matching. The C header is descriptive wire constants, NOT a binary ABI or any claim that agentc native C libraries load unchanged.
- Both consumers are independently compiled and run through real-binary fixture against a disposable fake OpenAI-model endpoint. The fixture separately exercises hostile Python children that send mismatched IDs, incorrect effect claims, invalid schemas, malformed/hugely oversized frames, duplicate names, unexpected hook/tool permission fields, crash/timeout, after-effect veto and environment-custody probes.

## Validation acceptance

1. Unit: cargo test --locked -p aw-extensions and aw-agent-core --test capability_sources. Exact Extension source kind/actor/site/epoch revocation must deny after revocation; legacy host name permits never widen to extension tools.
2. Rebuild independently: Rust process example, std JSONL-minimal aw and default full aw. Run independent C-compiler/Rust/mock-provider smoke against **each** freshly built aw: expected 21+ scenarios per build. A source checker does not count as binary proof.
3. Inherited R6/5a/5b MCP and old provider/JSONL/file/PTY binary fixtures; full/minimal native Rust, workspace compile, Clippy warnings denied, rustdoc/doctests, E1 core-only WASM zero imports, complete source-bound reference profile. Validate changed Rust formatting separately if inherited global rustfmt drift persists.
4. Update current focused docs and START_HERE; preserve original master and its 12 verbatim annexes/hash manifest. Immutable git tag phase-68 and skipped phase-64/65 remain untouched. Commit only after real verification and no push.

## Explicitly deferred; NOT part of R7 completion claim

This is a functional **process tools/hook/prompt foundation**, not a certification of every agentc runtime extension. More hook phases (pre/post provider, session/model/UI events and transforms), process provider-dialect adapters, direct custom slash-command handlers and status-presentation providers, persistent install/enable/disable catalog, runtime hot reload, cross-platform native-runtime testing, code signing, and OS-level sandboxing remain scoped follow-ups/R11/R12. The existing in-process Rust Extension SDK remains trusted code. Embedded E2–E6 (portable no_std+alloc Agent and final libc-free host audit) remain open; R7 std process/I/O never imports into the E1 core-only kernel.

**Next planned phase:** R8 portable Agent transition kernel and host-driven embedded real turn, unless the user explicitly prioritizes deferred R2b or R7 enrichment.


## Closeout evidence (verified against live R7 worktree)

- Source-only R7 checker: **155** checks; October-9 full historic handoff/tag/inventory checker **694** checks; master remains **10,684 lines / 12 byte-exact annexes**.
- Native aw-extensions unit suite **2/2** and shared source-kind grant/revocation/liveness suite **8/8**; failures hide model-advertised external tools, block subsequent dispatch, and never regrant old source epochs.
- Independently rebuilt standard **JSONL-minimal** binary: **21/21** R7 actual-executable mock-provider/C11/Rust/adversarial child checks, **32/32** R6 MCP checks; JSONL Session restart/resume/no replay **6/6**; Phase2 file tools and Inline/Scrollback/Fullscreen PTY **3/3**. Minimal aw CLI native test suite **88/88**, additional small suites green.
- Independently rebuilt **default full** binary: **21/21** R7 external-process fixtures; **32/32** R6 MCP; CLI **260/260**, shared full library **108/108**, and other integration suites green.
- R5a modern MCP and R5b legacy SSE mock binary fixtures **11/11 each** (rechecked before final hardening), unchanged by additive R7.
- Whole-workspace all-target Cargo **check passed**, whole-workspace Clippy warnings-denied **passed**, workspace rustdoc/doctests warnings-denied **passed**, changed focused modules rustfmt check and git diff --check green.
- E1 portable kernel no_std core-only/alloc independent WASM consumer **zero imports** under installed rustc **1.95.0**. This is NOT an embedded full Agent or pinned rustc 1.98.1 claim.
- Reference profile source-only **passed** with report at validation/host/20261009T205847Z-3e485733a58a/report.json. It does not claim that Cargo/runtime/Nix was run by the reference profile itself.
- **Monolithic workspace all-target Rust test unverified**: the long cargo test --workspace --all-targets attempt ended in a connector-internal error; no product test failure or passing summary was available. Primary affected crates, full CLI, minimal CLI and mock binary integration suites all passed independently. Do not claim a monolithic workspace test pass.
- Not tested: live third-party processes/providers, real credentials, OS confinement, security audit, cross-compilation to Windows/macOS, optimized release/Nix builds.

**Final hardening after initial mock fixtures:** only local static schema refs permitted (dynamic/recursive/remote refs and incompatible drafts rejected); malformed hook/tool responses poison and kill the connection; the shared default Tool::is_available remains true for host/MCP tools while external process tools override it, so revoked/poisoned sources disappear from model capability lists and fail the pre-effect authorization recheck. No new persisted authority, schema change or release tag.

**Open scope:** broader provider adapters, command/status extensions, install/enable/disable registry, hot reload and full agentc extension parity. The R7 process tools/hooks/inert prompts foundation is ready; R8 portable embedded Agent lifecycle is next.
