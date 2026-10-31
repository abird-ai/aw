# NEXT AGENT HANDOFF — AW tools core DL-150 parity review and implementation

Author: @pvl
Co-author: ChatGPT GPT-6
Owner: @pvl
Prepared: 2026-10-31
Status: handoff after signed coordinator-owned AW tooling commit

## Start here

Use dotlink-l5 connector only and operate in /home/pvl/spaces/abird/src/aw. Dotlink at /home/pvl/spaces/abird/src/dotlink is a read-only source of truth. Inspect the current AW HEAD, git status and actual source before taking any action. Never assume an older handoff or test result is current.

Read in this order:
1. AGENTS.md, .agents/plans/README.md and selected dotlink-tool-port/TODO.md.
2. .agents/plans/dotlink-tool-port/reference/DL150-SEALED-c34ea5a-README.md (complete sealed design).
3. .agents/plans/dotlink-tool-port/reference/DL150-SEALED-c34ea5a-TODO.md (complete roadmap).
4. .agents/plans/dotlink-tool-port/PARITY-AUDIT-2026-10-31.md (latest independent parity findings and priorities).
5. .agents/plans/dotlink-tool-port/DL150-AW-CORE-REFRESH-2026-10-31.md and CORE-ENGINE-RESUME-2026-10-31.md (architecture/decisions, older evidence).
6. Actual Rust source, tests and Cargo manifests; these always outrank prose.

Revalidate the latest dotlink commits after c34ea5a before modifying any surface. If the canonical spec changed, snapshot/diff the new plan and reconcile the AW-owned parity record first. Never silently change public semantics or substitute a different design.

## Current architecture and source locations

- aw-tools-core: crates/aw-tools-core/src; canonical schemas, filters, discovery, UTF-8 read paging, exact snapshot edits, secured filesystem operation orchestration, mutation outcomes, media, quotas, pure batch graph and Job transition projection.
- aw-tools-fs-unix / aw-tools-fs-win: crates/aw-tools-fs-{unix,win}/src/scoped_fs.rs and scoped_fs/port.rs + port/lifecycle.rs; the ONLY secured Unix/Windows filesystem backends.
- aw-platform-unix / aw-platform-win: preserve original OS-wide services; reexport the shared secured FS backend.
- aw-tools: crates/aw-tools/src; thin native tool/Agent ToolRegistry/ToolPolicy integration, optional compatibility parsers, Process owner bridge.
- Original Agent, immutable TurnJournal, Session and Process authority: crates/aw-agent-core/src, crates/aw-agent-kernel/src, crates/aw-store/src, src/agent_runtime.rs.
- The only model-visible core filesystem tools are ls, find, grep, read, bread, write, bwrite, edit, bedit, patch, cp, mv, rm. Shell bash / pwsh / powershell is a separate trusted Process grant.
- Cargo read-only/rw/full selects compiled code only. It must never mint actor/source/site/effect grants or widen shell authority.
- Pure Job(Graph) and batch parser are NOT model-registered, by design, until the original AW journal can durably claim each node before execution.

Preserve low-allocation Rust design: synchronous core by default; zero-copy/RAII scoped resources; reuse existing original locks and host journals; no duplicate backend, process scheduler or mutation engine. Favor simpler source-level invariants over broad code generation or premature abstractions.

## Your task: review, fix and implement, not just plan

Perform a NEW read-only code/spec parity audit independent of the prior report. Review current AW HEAD including the last tooling commit and recent dotlink spec commits. Inspect actual tool schemas and every shared operation plus native Unix/Windows implementation. Reproduce each confirmed correctness, regression, authorization, race or status mismatch in a focused failing test before patching whenever possible. Fix bugs at the lowest shared layer. Run review-fix-review until the scoped pass has no reproducible remaining issue; do not ask to continue for routine phases.

Priority 0 — security and correctness:
- Implement sealed final-symlink content semantics safely. Read/bread/write/bwrite/edit/bedit/patch Update must support symlink-to-regular-target only after lexical protected-path check, resolved-target reauthorization, pinned no-follow intermediate validation, target identity check, atomic replacement of the target while preserving the symlink, dangling-link and cross-root denial. No raw canonicalize+follow shortcut or weaker second OS backend. Add adversarial link-race tests.
- Enforce per-descendant protected/denied rules for recursive cp/mv/rm and resolved namespace ancestry/alias semantics on case-insensitive filesystems. Retain no-replace publication and failure-before-visibility.
- Investigate partial namespace commit and rollback/tombstone cleanup under cancellation, I/O failure and restart; report known fail vs mixed vs unknown exactly, never auto-replay an uncertain mutation.
- Bind pure Job(Graph) to the ONE original AW Session/TurnJournal/Process owner. Per-node journal claim BEFORE effect, durable terminal BEFORE dependents; source/actor/site/epoch/turn/call grants rechecked at admission and launch; runner reattach, no-launch proof, cancellation and revocation; one ID namespace, output/ACK/retention and explicit protected restart. Do not publish batch/job_* unless these properties are proven; never add a second journal or executor.

Priority 1 — semantic correctness:
- Review grep logical-line primary-hit paging, full atomic hit/context groups and overlapping/coalesced context clipping across page boundaries. Offsets count primary hits only, not context. Reproduce and fix any duplicate/leaked context and non-advancing result pages. Keep exact path escaping.
- Verify bounded .gitignore semantics (nested anchored/negated patterns, malformed/unavailable/over-budget ignore files, no ancestors above caller-named root) and path_encoding warnings on invalid UTF-8 names.
- Honor RFC3339 exclusive time ranges, unavailable timestamps/sizes, and slash-versus-basename glob grammar; test Unix, macOS and Windows platform differences.
- Provider-native ContentBlocks for exact unnumbered read data plus independent status, and typed bread media, while preserving the existing text-only AW ToolOutput compatibility path.
- Complete shared compilation/runtime facets and literal tool schemas without legacy aliases; remove duplicate parser/receipt logic only when existing callers remain compatible.

Priority 2 — complete acceptance and dotlink portability:
- Native Windows NTFS ACL/reparse/symlink/move/case-only-race tests; macOS renameatx_np; native Bubblewrap isolation when namespaces supported; full integration, crash/power-loss/fault injection and no_std tests.
- Measure schema and response byte/token cost versus older JSON results. No silent truncation or output-loss.
- After AW is accepted, plan dotlink consumer of aw-tools-core and aw-tools-fs-{unix,win} without touching dotlink in this run. Compare all tool parameters, protections, budgets, task and transport adapters before retiring dotlink's implementation.

## Regression commands and required reporting

Use pinned offline Cargo.lock, at least 8 build jobs when appropriate:
- cargo test --offline --locked -p aw-tools-core --no-default-features --features read-only --lib
- cargo test --offline --locked -p aw-tools-core --no-default-features --features rw --lib
- cargo test --offline --locked -p aw-tools-core --all-features --lib
- AW_BASH="$(command -v bash)" cargo test --offline --locked -p aw-tools -p aw-tools-fs-unix -p aw-platform-unix -p aw-agent-core --lib
- cargo check --offline --locked --workspace --all-targets
- cargo clippy --offline --locked -p aw-tools-core --all-features --all-targets -- -D warnings
- cargo clippy --offline --locked -p aw-tools -p aw-tools-fs-unix -p aw-agent-core --all-targets -- -D warnings
- cargo check --offline --locked -p aw-agent-kernel --no-default-features --features alloc
- Windows GNU cross check with the repository's existing target/review-tool-integration/cross wrapper and separate CARGO_TARGET_DIR; never claim native Windows tests from cross compilation.
- AW_BASH and AW_GIT configured cargo test --offline --locked --bin aw and -p aw-cli --lib.
- cargo fmt --all --check && git diff --check.

Baseline recorded by the preceding agent (not proof of YOUR run):
51 read-only / 83 rw / 153 full core, 105 AW native tools, 10 Unix FS backend, 7 Unix platform, 71 Agent active, 383 CLI binary and 226 CLI library active tests; strict Clippy, full-workspace compile and Windows GNU cross passed. Run your own tests and update counts.

If a monolithic workspace suite or aw-store suite times out, record that exact limitation rather than claiming success.

## Ownership and Git rules

- Do NOT list, read, touch, stage, stash, reset, amend or commit .agents/plans/ux, including its index entries. Another agent owns it.
- Do NOT modify dotlink at all. Do not disable TLS verification, invent a remote version, or pull over local unpublished design commits.
- Work only in AW owned tooling crates, necessary original Agent integration files, and .agents/plans/dotlink-tool-port plus .agents/docs/TOOLS.md. Preserve unrelated uncommitted files.
- Do not edit coordinator-shared .agents/plans/TODO.md. Only update the owned subplan and give coordinator exact changed paths/results.
- Signed commits only when explicitly authorized. Never push, tag, publish, deploy or rewrite history without separate approval. Use an expected-HEAD check and atomic owned-path commit to avoid interleaving other agents' staging.
- Record each confirmed defect, proof, fix, tests and still-open gate in one concise current parity report, not multiple conflicting archives.

## What to return

Report findings in severity order, precise file/function and reproduction; fixes with tested evidence; updated parity matrix for every tool plus shared runtime; remaining gaps that cannot be safely completed. Provide a fresh next-agent handoff only if more work remains. Do not claim model-visible Job or full symlink support until live host integration proves it.

## Copy-ready starting prompt

Use the dotlink-l5 connector only. Work in /home/pvl/spaces/abird/src/aw; dotlink at ../dotlink is read-only. Start by reading AGENTS.md, the canonical dotlink spec snapshot at .agents/plans/dotlink-tool-port/reference/DL150-SEALED-c34ea5a-README.md and TODO.md, the latest PARITY-AUDIT-2026-10-31.md, and this handoff. Revalidate live AW HEAD and all newer dotlink commits. Do a fresh independent parity/security/correctness audit of aw-tools-core, the secured Unix/Windows FS backends, aw-tools, ToolPolicy/TurnJournal and pure Job(Graph). Reproduce bugs in tests, fix at the lowest shared layer, iterate review-fix-review without asking, and run the full pinned test/lint/cross-platform matrix. Prioritize safe final-symlink dual authorization, recursive lifecycle policy, exact grep context pagination and original-host durable Job integration; never introduce a second backend, journal, or executor. Preserve the reserved UX subtree, all other agents' staging, and dotlink. Report exact completed/partial/deferred parity and tests; commit only with explicit authorization, signed and atomic, and never push or release.
