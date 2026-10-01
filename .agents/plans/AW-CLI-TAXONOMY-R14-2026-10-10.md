# R14 — COMPLETE canonical CLI taxonomy, grouped help, and hidden compatibility aliases

Date: 2026-10-10. **R14.1–R14.4 IMPLEMENTED in the live local aw tree**, original long-name removal **R14.5 DEFERRED** pending a separately authorized future major-release migration. This is the specific user-requested refactor of ALL existing CLI flag names AND help categories, not another provisional checklist. Baseline R13 commit fbfd57eb5f7b2eb2debdf99665944446995d512d. Full source version 0.15.0, SQLite v33, original phase-68 tag unchanged. No Phase69, skipped Phase64/65 indexing/LSP or push.

Read original frozen historical FULL-COMPREHENSIVE-HANDOFF-2026-10-09.md in full when exact original reasoning is needed, then current [START_HERE.md](START_HERE.md). The R13 local-certification versus global-parity distinctions remain valid and are not overruled by this CLI refactor. The original R14 planning specification is preserved verbatim below as dated initial planning scope.

## R14.1 — actual complete typed Clap inventory (implemented)

The R14 implementation enumerates the **real original Clap::CommandFactory tree**, including every flattened Args group and all recursively nested top commands. The actual compiled full cut contains **1,685 long-option command-path occurrences across 239 command paths** and 17 semantic help categories, with **228 original distinct long spellings**. Every occurrence is accounted for. The built-in full CLI migrates **1,026** legacy long-name occurrences to canonical names; the JSONL-minimal cut has **107 flags over 23 paths** and **57** migrated alias occurrences; stateless has **97 flags over 23 paths** and **54** migrated occurrences. There are **276 unique (previous, canonical, category) mappings**, including unchanged already-canonical options and scope-sensitive old names. Two original double-underscore test-only flags remain hidden/internal.

The independent complete *generated* migration reference is R14-CLI-FLAG-MIGRATION-2026-10-10.md, containing 276 unique old→new mappings, selected short/env metadata, each category, all 239 full command paths and both reduced feature-cut command paths. For every individual one of the 1,685 full option occurrences, the offline **aw cli schema** emits deterministic JSON with exact command path, source Arg ID, original/new long spelling, hidden aliases, short, env-variable NAME (never secret value), literal defaults, allowed values, visibility, requiredness and description. Avoid duplicating a 630KB JSON blob in committed docs. the removed generate_r14_cli_docs helper deterministically regenerates and validates the document from actual full, JSONL-minimal and stateless compiled binaries; its --check fails on drift.

## R14.2 — single canonical projection of typed source (implemented)

The new shared source src/cli_taxonomy.rs is deliberately a **pure Clap metadata transformation**, not a second interpreter of argv or a second provider/Agent: it recursively applies ONE canonical prefix and semantic help heading to existing original Clap Arg IDs across all nested commands using Clap Command::mut_args/mut_subcommands. It is invoked by **both** src/main_full.rs and src/main_minimal.rs through the same cli_taxonomy::parse::<Cli>(), using the original derived Clap FromArgMatches and CommandFactory. Existing value parsers, defaults, typed conflict groups, config/env precedence, model/account/Session/tool/source authority, single existing Agent, feature cuts and short aliases remain the exact original implementations. No privilege grant is created by renaming.

Where a long name changes, the previous name becomes a **hidden alias of the same Arg**. There is no parallel old+new field which could smuggle two unrelated grants; the native and actual executable tests reject conflicting old+new nonrepeatable selectors. Even deprecated old options under deeply nested commands preserve their original argument ID and parser/value semantics. The canonical categories cover Provider/model; Account/auth/security; Configuration; Agent/skills; Project resources; MCP; Extensions; Tools; Context; Session/state; Subagents/planner/guardian; Workspace/domain/machine/fleet; Automation/routing; Network/devices; Web/server; UI/TUI/output; and Diagnostics. Generic old names such as --timeout or --listen are explicitly scoped by top command. All 17 categories appear as group headings where relevant, with unrelated groups absent from the smaller feature cuts.

R13's one-shot minimal --print command noun disambiguation now uses the same normalized derived Clap command tree, with both canonical --output-print/--output-format and hidden --print/--output aliases. The primitive typed flag-arity normalization remains a narrow safe bridge; R14 does not create another command parser. User short options (-p, -m, -C, -w, -a) preserve original meanings. Both full and minimal existing MCP, external extension and R12 RPC paths remain executable with unchanged model/tool grant/cancel/saved-Session semantics. Minimal/stateless hide the unsupported Workspace UI choice from displayed --ui-mode possible values while explicit legacy workspace requests still receive the previous clear runtime denial. No hidden network, project-file activation, cost spending, credential handling or automatic tool replay is permitted.

## R14.3 — help, offline machine-readable schema and migration discoverability (implemented)

- Standard aw --help and all nested aw ... --help paths render semantic headings with canonical long flag names; inherited legacy long aliases are hidden in human help but still accepted.
- **aw cli schema** on full/minimal/stateless exports complete source-derived JSON with every compiled command path/Arg ID and old→new mapping. This command never opens a provider, tools, MCP child, extensions, config migration, Session owner, or network endpoint. It is a real first-class offline read-only subcommand.
- generated documentation lists ALL old→canonical mappings and paths from compiled binaries and makes reproduction easy.
- The historical text within this plan remains as the original design checklist, not as evidence that R14 has not yet been implemented.

## R14.4 — executable conformance and security regression (implemented)

New the removed agentc_r14_cli_taxonomy_smoke helper uses freshly compiled real full, JSONL-minimal and stateless binaries, isolated private HOME/state/config, local mock OpenAI SSE provider, actual rooted file read, and executable read-only help across **every command path**. It refuses alias/name collisions across **all 1,685 option occurrences**, verifies category prefixes/headings, typed schema/hidden old aliases, original short/env/static defaults, no provider/child/state startup from all 239 full and 23/23 small-cut help commands, root/nested legacy/new flag equivalence, explicit provider/profile env override, denied old+new double model selectors, tool permission preservation, and the old R12/R13 prompt/subcommand regression. The canonical and legacy flags produce identical ACTUAL model+scoped-read/turn/receipt traces, not just string matches.

Native cli_taxonomy unit tests verify canonical/legacy parse to the same Arg ID, conflicting nonrepeatable choices reject, static env/default/short values are untouched, and contextual names cannot collide. The complete inherited R13 real executable certification matrix is rerun against each new cut: full **17/17** local mock scenarios, minimal **17/17**, stateless **6/6** actual checks. R13 documented global agentc 0.6.0 reference platform/hardware/firmware work remains OPEN and is not falsely certified by this CLI change.

R14 local implementation adds Cargo Clap string-support (for owned dynamically produced canonical names) without a second flags registry or vendor dependency; only one pure metadata normalization module owns mapping rules. Nothing in its transformation invokes model/provider/tool network API, reads user secrets, or changes execution policies. Rust help generation and schema are offline. Tools and docs are source-locked through existing validation_gate.py reference/native steps and historical handoff integrity checking, and a new source-only the removed test_cli_taxonomy_r14 helper (source checks never substitute for runtime proof).

### R14 final portability hardening — bounded full-tree parser stack

The full CLI has 239 deeply nested command paths; directly constructing the original Clap tree previously overflowed a small Rust test-thread stack. R14 therefore constructs/parses the **same** typed full Clap tree (not the Agent) on a bounded 32 MiB-stack worker and returns the fully typed Cli to the calling thread; offline aw cli schema likewise uses that bounded stack. The JSONL-minimal and stateless small trees continue parsing directly, without extra thread creation. A new native full test calls parse and schema from a 1 MiB outer thread and verifies the exact 1,685-flag/239-path inventory, so this is an actual nested-stack regression proof, not merely a build check. The full and reduced native executables also passed the complete old/new alias/source grant and nested help checks after the change.

The already/manual .github/workflows/r13-native-smoke.yml now includes full, minimal, and stateless R14 real-binary native CLI smoke steps for all six declared runner labels with per-host receipts. **Those jobs have not run**, and are NOT Windows/macOS/ARM certification; actual platform proof remains open as described by R13.

## R14.5 — explicit future major release only (not yet approved)

Removing old hidden aliases is **not** part of R14. A separate future user-approved major release should communicate removed forms, offer dry-run migration, prove native CI/compatibility and avoid breaking existing scripts. All old names continue to work in 0.15.0.

## R14 complete local verification ledger (2026-10-10)

R14.1–R14.4 are implemented, with R14.5 deliberately deferred until a separately approved breaking major release. This is a local native CLI certification, not global original agentc/cross-platform/firmware parity. All receipts are from independently built real binaries or separately described source-only checks.

- Full actual compiled CLI: 1,685 long-option occurrences on 239 recursive command paths, 1,026 migrated previous long names kept as hidden aliases and 17 meaningful grouped help categories. JSONL-minimal: 107/23/57/13 respectively. Stateless: 97/23/54/12. There are 276 distinct old→canonical/category mappings, including all nested paths. Two hidden internal double-underscore failure-injection flags keep their original spelling.
- Complete real-binary R14 exhaustive help/alias/provider/tool security suites PASSED: full 11/11 scenarios over ALL 239 paths, JSONL-minimal 12/12 across 23 paths, stateless 12/12 across 23 paths. Assertions include offline pure help/schema, no unwanted provider/config/Session/extension startup, no scope/alias collisions, environment and original short/default/value equivalence, denial of conflicting old/new model selectors, correct nested session export output-file versus output-format, actual old/new identical fake-provider+scoped-read Tool receipts, disabled-tool refusal, and command-noun prompt text not confused with a privileged subcommand.
- Native Rust after stack portability guard: full CLI 266/266, minimal CLI 95/95, stateless CLI 92/92, shared full library 117/117. Three dedicated typed taxonomy unit tests per cut check ID/alias collisions, env/default/short preservation and contextual canonical names. One additional full native 1MiB OUTER thread proof PASSED: it builds/parses the complete 239-path full CLI and exports all 1,685 original typed flags using a dedicated 32MiB bounded-stack worker, without moving the Agent or any tools. Minimal/stateless continue direct parsing with no extra parse thread.
- Inherited R13 real-binary mock-provider conformance reran against the updated R14 binaries: full 17/17 scenario groups, minimal 17/17, stateless one grouped fixture with 6/6 executable checks. Original 19 global R13 missing platform/firmware/provider/security/reference agentc 0.6 requirements remain OPEN.
- Compiler/portable gates: workspace Cargo --locked --all-targets CHECK passed; whole-workspace Clippy --all-targets -- -D warnings passed; Rust doctests with RUSTDOCFLAGS=-D warnings passed; scoped Rustfmt of cli_taxonomy.rs and Git whitespace check passed. No blanket cargo fmt --all success is claimed: historic prior-source formatting drift exists. The monolithic workspace/all-target Rust test gate did not obtain a complete connector receipt during R13 and is not fabricated as passed by R14.
- Separate linked no_std proof: E1 wasm32v1-none portable consumer zero imports, E3 shared no_std+alloc embedded Agent zero imports and ACTUALLY executed a fake model→tool→journal turn under Node. Installed rustc is 1.95.0, not pinned 1.98.1; there is no native freestanding ELF/C ABI/firmware proof.
- R14 source-only checker 197 assertions; frozen original October-09 historical handoff integrity 707 (original master 10,684 lines, 12 byte-exact verbatim annexes). Source-only reference validation passed under validation/host/20261010T053202Z-44d1d748b386/report.json; that report does NOT claim Rust/native execution. Actual independent Rust/CLI/WASM results above are separate.
- Deterministic generated 698-line markdown migration reference under .agents/docs/R14-CLI-FLAG-MIGRATION-2026-10-10.md passed --check against freshly rebuilt full/minimal/stateless source-locked binaries. For every flag on every nested path, the new offline aw cli schema JSON preserves original typed IDs, old/new long, shorts, env-variable NAMES only, literal defaults, allowed values, requiredness and help category.
- Warm Linux debug --version median over 20 repeated starts: R13 full 12.199ms versus R14 full 16.808ms; R13 minimal 3.982ms versus R14 minimal 4.167ms. The full ~4.6ms startup overhead is an honest dynamic complete Clap-schema/large-stack compatibility trade-off. This is NOT a release/cold-cache/Windows/macOS/provider benchmark.
- Existing MANUAL read-only six-host native GitHub workflow now includes separate full/minimal/stateless R14 actual binary CLI smoke and retained per-platform receipts, but has NOT RUN. No native Linux arm64/Windows/macOS/RISC-V certification is inferred from its source YAML or from local Linux mock testing. No user data secrets or raw vendor traces are exported.
- No provider/tool permission change, no paid compaction or replay change, no version (0.15.0)/SQLite schema (v33) bump, no numbered Phase69, no revival of intentionally skipped Phases64/65, no historical phase-68 tag change, no release signing, no Git push.

**Conclusion:** Locally executable R14.1–R14.4 COMPLETE; old aliases preserved. R14.5 breaking removal of those aliases and wider global R13 cross-target certification remain separately OPEN.

---

# Historical user-requested R14 original planning snapshot (retained verbatim)

# R14 — Comprehensive CLI taxonomy, help, and flag migration

Date: 2026-10-10. Status: USER-REQUESTED FUTURE PHASE — not yet implemented in full. Add to the unnumbered R-roadmap after R13 or move earlier when chosen. The user explicitly requested that ALL aw CLI flags be classified into clean categories in both their names and help output. R9 normalized tools; R10 starts grouped help. This is a durable backlog item, not a numbered Phase69.

Authority: FULL-COMPREHENSIVE-HANDOFF-2026-10-09.md remains frozen; see [current R9](AGENTC-PARITY-R9-MODEL-DIRECTED-2026-10-10.md) and live source. Never break the existing CLI merely to imitate another tool.

## Design rules

1. A single typed Clap schema owns each argument, reused by full/minimal where appropriate. Canonical long flags get exactly one **category prefix**, and are placed under their category help heading. Durable nouns live in subcommands, not arbitrary new global flags.
2. Distinguish selection vs authority: naming a model, account, Session, tool, shell, MCP server, runtime or source is not permission to call it. No implicit network, hidden paid summary, or host permission changes.
3. Prefer reusable clap::Args with next_help_heading/help_heading, clear default/help values, kebab-case, and positive/negative forms consistent across modes.
4. Retain previously documented long names, short names, environment variables and config keys as hidden migration aliases. Never change -p, -m, -C, -w, existing --mode meanings or credential-custody semantics without explicit release approval. Old/new combinations must reject conflicting authority.
5. Full, JSONL-minimal and stateless compile cuts only advertise capabilities compiled into that cut. Help starts no provider/process/network work.
6. Inventory every original flag and nested command in main_full, main_minimal, model_cli, resources_cli, mcp, extensions, providers, subagents, owner/net/device, identity, UI/TUI, workspace, domain/fleet/machine, planner/guardian, auth/context/persistence and diagnostics.

## Taxonomy and proposed canonical prefixes

| Heading | Families | Responsibilities |
| --- | --- | --- |
| Provider & model | --provider-*, --model-* | profile, endpoint, model, reasoning, output limit, provider-dispatch retries |
| Account & security | --account-*, --auth-*, --security-* | explicit account/custody/secret refs, authorization, TLS and opt-ins |
| Tools & execution | --tools-* | tool mode/grants, root, Bash, model scheduling and optional safety ceilings |
| Context & summary | --context-* | compaction mode, retain tail, threshold, summary limit and paid auto consent |
| Session & state | --session-*, --state-* | Session ID/branch/resume/new/list/backend/fork, state directory |
| Extensions | --extension-* | process selection, exact grants, argv, hooks, bounded liveness |
| MCP | --mcp-* | stdio/HTTP/SSE servers, exact remote tool/data permission, notifications |
| Agent & delegation | --agent-*, --subagent-*, --planner-*, --guardian-* | agent mode, child budgets, routes, review |
| Interface & output | --ui-*, --tui-*, --output-* | theme, inline/fullscreen/scrollback, JSON/text, print |
| Workspace & target | --workspace-*, --domain-*, --machine-*, --fleet-* | worktree, local/remote executor, resource locations |
| Web & server | --web-*, --server-*, --device-*, --net-* | listeners, browser owner, remote trust and exposure |
| Diagnostics & validation | --diagnostics-*, --validation-* | redacted logs, performance, provenance, release audit |

Do not invent flags for decorative grouping or hide genuinely independent actions behind a vague generic mode. Tool flags use the canonical --tools-* family already adopted; other families can retain current aliases until migration.

## Delivery stages

R14.1: inventory/export complete Clap trees for full/minimal/stateless and nested commands; build before/after table of names, aliases, short flags, env/config precedence, defaults, trust boundaries and collisions.

R14.2: refactor shared typed Args groups, remove full/minimal boilerplate where practical and normalize category prefixes without changing permission/Session semantics. Preserve old aliases.

R14.3: implement compact grouped help for all feature cuts and nested nouns; deterministic headings/order, responsive terminal width and executable-free offline help. Add machine-readable help/completion guide if practical.

R14.4: independent actual-binary tests for complete categorized help, old/new flag equivalence, invalid values/conflicts, no network on help, Nix/Windows/macOS quoting, preservation of Session resumes/effect grants/provider selection; whole-workspace Clippy/rustdoc/reference, CLI/manpage/install/migration documentation.

R14.5: explicit later major-release removal of deprecated names only after approval and compatibility/migration communication.

## Definition of done

All flags, not only tools, categorized in names and help; nested commands covered; one coherent typed source with minimal/full parity, complete alias/security/feature gate tests, no silent changes in API authority, replay, persisted Sessions or default paid spend. The historic Phase68 tag, skipped numbered Phases64/65, version/schema remain unchanged.

R10 interim work adds immediate Provider & attempts, Tools & execution and Session & context help headings. R14 is the comprehensive explicit user-requested future cleanup; do not claim it finished with R10.