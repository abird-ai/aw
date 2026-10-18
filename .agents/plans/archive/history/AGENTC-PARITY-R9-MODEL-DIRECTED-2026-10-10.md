# R9 follow-up — model-directed tools and consistent --tools-* CLI

**Date:** 2026-10-10. **Track:** unnumbered R9 follow-up, user-authorized revision after [R9](AGENTC-PARITY-R9-PARALLEL-TOOLS-2026-10-10.md). **Base:** c2be78737881042dacd253ec7c98fae80a6d95f5. **Historical authority:** FULL-COMPREHENSIVE-HANDOFF-2026-10-09.md, frozen with 12 annexes. Version 0.15.0, SQLite v33, Phase68 tag and skipped Phases64/65 unchanged.

## User direction and architecture

1. Default to **model-selected concurrency**, not unconditional parallel Read or serial Bash. The model sets optional boolean parallel:true on each independent read, ls, find, grep or Bash call. Omitted/false means ordered. --tools-parallel model is default; --tools-parallel off enforces global serial. Model text/arguments never grant tools or reclassify Process as Read.
2. Tool flags canonically share the **--tools-*** prefix: --tools-mode, --tools-allow, --tools-root, --tools-bash, --tools-parallel, --tools-parallel-limit, and optional --tools-parallel-read-limit / --tools-parallel-bash-limit. Prior --tools, --tool-allow, --tool-root, --bash and --parallel-reads are hidden backward-compatible aliases. MCP/extension/subagent namespaces remain separate integrations.
3. Default shared resource ceiling is **16**, not a command to execute 16 effects. --tools-parallel-limit 1..128 controls it. Read/Bash subceilings are OPTIONAL limits, inherited from the shared ceiling when omitted and never able to widen it. CLI rejects 0 or more than 128 before any provider call. Hosted SDK can additionally constrain its policy.
4. Bash remains an explicitly granted **ToolEffect::Process** with OS-user rights. It is available only under --tools-mode trusted and an absolute --tools-bash executable; the model may choose adjacent commands to overlap using parallel:true, or mark them false for ordered dependencies. Concurrent Bash can race on files; user/model can force serial. The host enforces explicit owner trust, immutable effect classification, bounded execution and truthful intent/receipt recovery without command-text heuristics or arbitrary host-parallel interdiction.
5. The shared no_std+alloc TurnMachine now implements Read/Bash wave intents/effects/receipts, preserving exact host actor/site/source/epoch grants, journal-before-effect, current account validation and original-order receipts before model disclosure or post-tool hooks. Unknown/Write and other Process effects remain barriers. Aborted uncertain Bash effects require reconciliation and can never auto-replay. std Agent still owns the only provider/tool execution loop and uses bounded nonspawned FuturesUnordered; embedded host remains explicitly host-driven.
6. The old 4-read and 16-call hard ceilings are revised. MAX_BATCH now equals the already existing 128-call turn ceiling. Context/trace memory remains at 1 MiB; actual outputs are explicitly apportioned and truncated at UTF-8 boundaries when needed. The durable ToolOutput is_error and truncated flags and public output marker reflect that truncation. Model schemas and original grants still govern every call; no implicit context-summary or account migration.

## Canonical CLI examples

    aw --tools-mode read-only --tools-parallel model --tools-parallel-limit 16 \
       --print "Inspect independent files"

    aw --tools-mode trusted --tools-bash /absolute/path/to/bash \
       --tools-parallel model --print "Run independent shell checks"

    aw --tools-mode trusted --tools-bash /absolute/path/to/bash \
       --tools-parallel off --print "Make ordered changes"

    aw --tools-mode trusted --tools-parallel-limit 16 \
       --tools-parallel-read-limit 8 --tools-parallel-bash-limit 2 \
       --tools-bash /absolute/path/to/bash --print "Run independently where appropriate"

Tools remain OFF by default: --tools-mode off. A parallel input only requests scheduling of an already authorized tool.

## Implementation and acceptance matrix

- Portable kernel: MAX_BATCH/MAX_READ_WAVE both safely bounded to MAX_CALLS=128, default16; generic WaveKind::Read and WaveKind::Bash with no_std+alloc effect-aware intent/receipt machine and Process reconciliation. E1 core-only still no alloc or OS dependencies.
- Native hosted Agent and tools: ToolPolicy default model-driven, shared/optional ceilings, exact source trust for Bash, optional parallel boolean in read/ls/find/grep/Bash schemas only; write/edit unchanged. Dynamic per-call result budget retains bounded 1 MiB context and explicit receipt and public marker.
- Native parallel tests: real 16-read overlap, Bash Process overlap with ordered write barrier, model serial vs host serial, failed Process ambiguity and no replay, 16 large model-selected results truncated to fit context, existing cancellation/journal/source-revocation tests.
- Compiled standard JSONL-minimal and default full CLI: independent mock SSE model dispatches 16 separate read calls and two actual trusted Bash subprocesses using finite coordination in disposable workspace files, also exercises host/model serial and optional resource constraints. Real provider result ordering, tool identifiers and process exit status checked.
- Retain R5/R6 MCP, R7 process extensions, R8 actual executed zero-import wasm32v1-none host, E1 core-only WASM, JSONL resume and native PTYs. Run full/minimal Rust, Clippy -D warnings, docs, reference and historical checksum tests.
- Freeze original October-9 handoff and its 12 annexes; this is an authorized successor, not a rewrite. No version/schema/Phase69/tag change or push.

**Not claimed:** A model's belief that commands are independent is not a verified race-free dependency graph, and parallel trusted Bash retains the possibility of concurrent file mutations. This is NOT OS sandboxing, live API/security certification, pinned Rust 1.98.1 or native libc-free ELF proof.

## Verification ledger

**Verified against the live uncommitted R9 follow-up before local checkpoint:**

- **Actual native Agent/tool scheduling:** portable no_std kernel **12/12**; builtin scoped tools/Bash schema **8/8**; real hosted Agent concurrent/model-directed adversarial fixture **21/21**. These include 16 overlapping reads, 2 model-selected Bash Process calls + ordered write barrier, host and model serial overrides, failed Process unknown-effect inspection/no replay, 16×128 KiB synthetic outputs explicitly truncated within the unchanged 1MiB trace, original-order factual receipts before hooks, revocation and cancellation with no detached wave tasks. Hosted-vs-embedded R8 exact transition parity **2/2**.
- **Actual freshly compiled binaries, independently:** standard full **11/11** and JSONL-minimal **11/11** against isolated fake SSE model. Both exercise --tools-* canonical CLI and hidden legacy alias behavior, 16 read calls in ONE model response, no forced overlap without model parallel:true, host --tools-parallel off, optional stricter per-class limits, and two **real separately spawned trusted Bash subprocesses** that would fail if run serially. Returned tool_call_id order and bounded typed outputs were checked. Both freshly rebuilt profiles also passed inherited R6 MCP resources/notifications **32/32** and R7 independent C/Rust process extensions **21/21** each.
- **Native primary CLI/SDK:** minimal CLI **88/88**, full CLI **260/260** plus shared full library **108/108** using exported Nix trusted AW_GIT/AW_BASH paths. Previously existing R5a/R5b modern/legacy MCP, JSONL resume and PTYs are independently tested in the repository but no live vendor is certified.
- **Embedded invariant:** independently linked E1 core-only consumer WASM has **zero imports**; E3 no_std+alloc Agent WASM linked and **actually executed** a fake provider→tool→journal→answer turn in Node with zero imports, using installed rustc **1.95.0**, not pinned toolchain 1.98.1. The embedded host remains explicitly sequential, but reuses the one shared portable TurnMachine now able to handle Bash Process wave receipts as well.
- **Structural/security-source and compatibility:** R2 original canonical selector checker **50**, R8 portability checker **207**, updated R9 source checker **187**. Frozen original October-09 master still **10,684 lines / 12 exact verbatim annexes**; historical inventory/hash checker **701**, with only this R9 follow-up successor added. Source reference profile **passed**, report **validation/host/20261009T225915Z-3b4e86151d40/report.json**; that report explicitly makes no claim of Rust runtime execution (native and WASM tests above are independent).
- **Final compiler/warnings/docs:** whole-workspace/all-target Cargo check and Clippy -D warnings passed, Rust doctests and focused rustfmt/diff checks independently verified. No blanket monolithic workspace/all-target test, release/Nix cross-platform build, paid production provider, arbitrary native libc-free ELF certification or independent security audit claimed.
- The CLI **--tools-parallel-limit=16 is a resource ceiling, NOT a command to launch 16 tools**. The model requests parallel:true per call only under already-granted tools. Trusted Bash has the OS user's rights and may race with other commands; no sandbox/race freedom guarantee. Write/edit/unknown remote effects do not inherit concurrency. There is no automatic replay of unknown/Process effects after interruption.

No historical Phase69 tag, previously skipped Phase64/65 indexing/LSP, source version bump, schema migration, automatic provider/tool grant, or Git push.


## Final verification evidence (post-implementation, local checkpoint)

- Native shared portable TurnMachine **12/12**; builtin scoped read/search/Bash tools **8/8**; actual headless Agent tool-wave integration **21/21**, including model/host serial restrictions, sixteen simultaneous reads, two concurrent trusted Bash Process calls, ordered mutating barrier, no replay of failed Process calls and explicit truncation of sixteen worst-case outputs within the original 1MiB trace. Hosted vs embedded parity **2/2**.
- Standard full CLI unit **260/260**, shared library **108/108**, JSONL-minimal CLI **88/88**, all with the configured Nix AW_GIT/AW_BASH paths for external executable fixtures.
- Freshly independently compiled std-minimal AND full executable each passed **11/11** actual fake-OpenAI multi-tool scenarios (including two **real** Bash child processes coordinating through disposable files and 16 read calls); each also passed inherited MCP R6 **32/32** and independent extension R7 **21/21**.
- Actual minimal JSONL Session resume/no tool replay **6/6**, Phase2 scoped file tools and native Inline/Scrollback/Fullscreen PTYs **3/3**.
- Actual independent E3 linked WASM no_std+alloc fake-Agent request **executed in Node** and imported zero functions; independent E1 core-only WASM consumer also imported zero functions under installed rustc **1.95.0** (not pinned 1.98.1 or native ELF certification).
- Whole-workspace all-target Cargo check and Clippy with warnings denied passed. Workspace Rust doctests under RUSTDOCFLAGS=-D warnings, focused newly modified Rust rustfmt --check, git diff --check, Python/static source checker and full source-only reference validation passed. Final reference profile report: **validation/host/20261009T230330Z-65a3437b0a36/report.json**. The reference report explicitly does not claim cargo/runtime execution; those are independent gates listed above.
- Source-only R2 legacy selector checker **50**, R8 portability **207**, R9 current follow-up **187**. The immutable historical 2026-10-09 master stays **10,684 lines / 12 byte-exact annexes**, integrity checker **701 checks**. No schema/version, Phase69/tag changes, Phases64/65 indexing/LSP, hidden tool grants, automatic effect replay or Git push.
- Not claimed: monolithic workspace all-target Rust test run, deterministic release builds, pinned 1.98.1 target validation, native libc-free ELF/ARM/C ABI, real paid providers, full OS/sandboxing guarantee or independent security audit. A model's choice of parallel Bash cannot prove independence of commands; the host retains a serial override, bounded resource cap and conservative uncertain-effect handling.

The R9 follow-up is ready for an unnumbered local implementation checkpoint; next approved roadmap work is R10 bounded compaction/provider-attempt policy.
