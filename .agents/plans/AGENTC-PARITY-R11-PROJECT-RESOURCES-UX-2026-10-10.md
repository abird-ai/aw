# R11 — Source-pinned project resources and native Agent UX

Date: 2026-10-10. Status: implemented R11, final verification in progress. Unnumbered one-way agentc parity phase, based on R10 commit 700e5a67b6b96dbd173b23f3c2793dfef3ea10c1. Source version 0.15.0 and SQLite schema v33 unchanged; phase-68 tag preserved. No push or numbered Phase69; skipped Phases64/65 remain excluded.

## Authority and reading order

The original full 2026-10-09 master remains byte-for-byte frozen, including all twelve verbatim historical annexes. Read [START_HERE](START_HERE.md), [R10](AGENTC-PARITY-R10-COMPACTION-RETRY-2026-10-10.md) and [R9 model-directed concurrency](AGENTC-PARITY-R9-MODEL-DIRECTED-2026-10-10.md) for immediate predecessor decisions.

The user explicitly requested a **future comprehensive CLI categories/help/naming migration**, not limited to new resource flags: [R14 CLI taxonomy](AW-CLI-TAXONOMY-R14-2026-10-10.md) is mandatory and still outstanding. In R11, all new project-resource flags use one typed, reusable --resources-* family and matching grouped help; R14 must later inventory and normalize every legacy flag and nested command while preserving aliases, permission contracts and full/minimal build parity. R12 headless and R13 certification remain next planned phases.

## Architectural goal and source authority

Use the existing single aw Agent, Session store, scoped filesystem and settings CAS, not duplicate their authority. Project data admission must require an explicit user-selected local root and an exact user-approved SHA256 source manifest. No ambient project scan or automatic activation of tools, providers, services, skills, themes, network, credentials or remote permissions.

Files:
- crates/aw-providers/src/config.rs: secret-free ProjectResourceTrust map of canonical source paths to exact SHA256 digests, with bounded names/path/roots and locked private config::mutate / save_changes 3-way CAS. Typed UiConfig.theme_accent optional hex RGB, old defaults unchanged.
- src/project_resources.rs: ONE shared full/minimal Catalog::scan/verify/trust/revoke. Reads use mature descriptor-relative, no-follow/hardlink-refusing WorkspaceFiles and aw_platform scoped_fs Root. Canonical root must be UTF-8 ancestor of current cwd. Strict bounded discovery: 96 files, 512KiB aggregate, 64KiB each, at most 24 ancestor directory levels. Root through cwd exact AGENTS.md, CLAUDE.md and AGENTS.override.md, plus explicitly selected root .agents/prompts/NAME.md, .agents/skills/NAME/SKILL.md and .agents/themes/NAME.json. Unexpected symlinks/incomplete directory listing/hash change cause refusal.
- src/resource_cli_args.rs: shared typed Clap group --resources-project ROOT, --resources-trust-project ROOT, --resources-revoke-project ROOT, --resources-template NAME, repeatable --resources-var NAME=VALUE and --resources-theme NAME. Trust/revoke are standalone human actions performed before any model/session/provider startup. A selected root's exact resource hashes must still match the config before and at each subsequent prompt admission. Unknown placeholders, future authority fields, implicit environment interpolation, duplicate/oversized template vars all rejected.
- src/lib.rs and src/main_minimal.rs, src/main_full.rs: same project engine and R10/R9 authenticated Agent paths. Root-to-leaf approved instructions appended as clearly labeled source text (not tool/provider/account grants). Named templates are expanded ONLY as user prompt content; bounded on-demand SKILL.md limited name/description frontmatter (max4KiB) admitted via explicit --skill NAME or /skill:NAME TASK. /prompt, /skill names from the existing unified slash registry; /prompt:NAME TASK uses approved named template through the same Agent, not a secondary interpreter. Full JSONL and SQLite saved Session resumes revalidate exact approved suffix once; omitting approval cannot launder persisted project instructions back into authority.
- src/tui/theme.rs and native Agent/Workspace renderers: typed NamedTheme, optional user-approved #RRGGBB accent and one palette_for UI composition across System/Light/Dark. /theme NAME and --resources-theme NAME may select approved native semantic palettes; builtin /theme auto|light|dark clears accent. No raw ANSI/JS/style execution from resource files, and browser project theme selection is not implicitly authorized.
- Existing interactive composer can seed last 128 user prompts from the current authorized Session history; no parallel plaintext history database is created.
- crates/aw-providers/src/provider/decode.rs: old native decoder test index16 was stale since R9 raised the shared batch cap to128; test now checks the actual shared limit.

## Native command examples

Approve once (source data only):
    aw -C /project/sub --resources-trust-project /project

Use in exact local host (tools still off unless separately granted):
    aw -C /project/sub --resources-project /project --resources-theme ocean --skill review --resources-template review --resources-var language=rust --print TASK

Revoke:
    aw --resources-revoke-project /project

Interactive local Agent:
    /prompt
    /prompt:review TASK
    /skill
    /skill:review TASK
    /theme ocean

Unapproved/changed source files, missing root or revoked metadata are denied BEFORE starting another model request. Provider credentials, execution site and tool grants continue to come only from their existing explicit control paths.

## Policy boundaries and deliberate deferrals

Only local single-user Agent project-resource admission is enabled in R11. Browser/owner/Workspace project data still needs a separately authenticated trust protocol; these contexts fail closed, not silent trust. Existing native named accent is not a claim of full browser CSS theme parity. This R11 slice does not introduce a new minimal credential vault; the minimal build retains explicit selected API-key environment references and existing HTTP binding/credential pinning, while SQLite/full retains current SecretStore/OAuth. No homemade plaintext JSONL secret storage, new OAuth/PKCE, project script execution, implicit LSP/indexing/watchers, daemon or permission expansion.

Root and source approval is checked before each newly admitted prompt. The existing provider/tool loop still controls cancellation/revocation during effects; resource scanning does not claim to revoke bytes already sent to a provider halfway through an in-flight stream. The user must explicitly trust the project instructions. No cross-platform release, real paid provider audit or independent security certification claimed.

## Verification ledger

**Verified in the live R11 worktree before local checkpoint:**

- Native shared project resource unit tests **3/3**: explicit SHA-256 approve/verify/revoke, deterministic root→leaf instruction ordering, recognized named SKILL.md limited frontmatter, typed bounded named templates and themes, symlink refusal, invalid path/oversized source, missing/duplicate template argument rejection. Separate native semantic UI palette test **1/1** verifies an approved RGB accent modifies only the semantic accent slot and rejects malformed colors.
- Independently compiled **default full and JSONL-minimal aw binaries**, each with a separate local fake OpenAI SSE provider: **R11 10/10 per build**, including trust without model/provider startup; zero project-derived tool grants; ancestor instructions, explicitly selected SKILL.md, named user prompt and typed named theme; actual durable JSONL Session resume without duplicated admitted instructions; omission of --resources-project, source edits or additions, explicit revocation, and symlink substitution all deny BEFORE calling the model. The test includes a real private config on disk and complete model call-count checks, not a source-only simulation.
- Existing explicit skill precedence remains intact: Phase60 actual compiled binary **3/3** and Phase61 local skill registry actual compiled binary **4/4**. Historical Phase60/61 source checker contracts were updated ONLY to account for the new explicit project-SKILL.md selection path and still require actual registered user-skill precedence/permission; neither project data nor skill registry permits background script execution. Existing TUI Inline/Scrollback/Fullscreen terminal-fixture **3/3** green; independent JSONL-minimal Session recovery **6/6** green on the correct minimal feature cut.
- Native provider library **80/80** after fixing the prior stale hardcoded 16-call decoder fixture to use the current R9 kernel-bound 128-call limit; native JSONL-minimal CLI **89/89**, full CLI **261/261**, and shared full library **111/111**, with Nix AW_GIT/AW_BASH paths for historical process fixture. These include newly integrated local project resource and theme cases.
- Whole-workspace Cargo check --locked --all-targets **passed**; whole-workspace Clippy --all-targets -- -D warnings **passed**; workspace Rust doctests with RUSTDOCFLAGS=-D warnings **passed**; focused independent/new resource, theme and config rustfmt --check and git diff --check **passed**. No single monolithic workspace/all-target Rust test run claimed.
- R11-specific source-only checker **165**; frozen Oct09 comprehensive master inventory/tag/hash **704**, retaining its original **10,684 lines, 12 verbatim annexes** and source hash 39f10f52494e8691a78c25e4963222f9f29532b69a3897bed38e77c49f1db467. SOURCE-ONLY reference profile **passed**, report validation/host/20261010T003752Z-fdef549847cb/report.json, expressly with no Cargo/runtime execution claim. Independent compiled CLI and native receipts above remain authoritative for runtime scope.
- Prior embedded E1 core-only WASM linked with **zero imports**, and E3 full host-driven no_std+alloc Agent WASM linked with **zero imports and executed** a real fake model→tool→journal→answer turn under Node using installed rustc **1.95.0**, not pinned 1.98.1. R11 did not add any OS/provider/native dependencies to the shared no_std kernel.

**Nonclaims / deferred R11 scope:** Production provider/paid auth testing, independent security penetration audit, native libc-free ELF/C ABI/firmware or pinned toolchain cross-release, full browser CSS named-theme rendering, web/owner/Workspace authenticated project trust, new persistent minimal secret vault or subscription OAuth/PKCE, file watcher/indexing/LSP and complete refactoring of all CLI flags are NOT claimed. The existing minimal selected API-key env refs and full SQLite SecretStore/OAuth remain the only credential custody. Explicit project content trust is rechecked at the boundary of each newly admitted prompt, not retroactively after vendor request bytes have been emitted.

R11 is ready for one unnumbered local checkpoint. Next original parity phase: R12 headless JSONL events/RPC; then R13 certification. The mandatory user-requested future **R14 CLI taxonomy** phase remains an explicit backlog item and must be preserved in every successor. Source version 0.15.0, SQLite v33, frozen original master and Phase68 tag unchanged; no Git push.

Next: R12 typed headless JSONL events and RPC; R13 certification; mandatory user-requested R14 comprehensive CLI taxonomy backlog remains open.
