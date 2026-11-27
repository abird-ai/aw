# Next agent — AW DL-200 R6 / Phase 3 (November 27, late)

Use **dotlink---l5 as the ONLY source-authoring connector**.

Continue DL-200 R6 / Phase 3 in:
- AW: `/home/pvl/spaces/abird/src/aw`
- dotlink: `/home/pvl/spaces/abird/src/dotlink`
- Frozen read-only release: `/home/pvl/spaces/abird/src/dotlink-017-mounts`

## 1. Acquire complete project context

Read in order, relative to AW:

1. `AGENTS.md`, `.agents/plans/README.md` and only the active DL-200 ownership line in `.agents/plans/TODO.md`.
2. `.agents/docs/AW-DL200-PHASE3-FULL-COMPREHENSIVE-HANDOFF-2026-11-27-LATE.md`: §§0–7 first; §8 preserves all nine subsequent signed checkpoints; Appendix A preserves the entire prior comprehensive handoff byte-for-byte, including its 16 earlier implementation checkpoints, original architecture, R1–R6 decisions, rejected designs and nested historical records; Appendix B preserves the exact signed active TODO. Use the original historical index to read relevant subsections, not all duplicated appendices into context at once.
3. `.agents/plans/dl200-unified-jobs-agent-events-2026-11-20/{README,SPEC,IMPLEMENTATION,TODO}.md`.
4. `.agents/docs/CURRENT_HANDOFF.md`, newest actual signed deltas and relevant original owner source/tests.

Sealed SPEC governs; live source and executed tests establish implementation truth. Do not reconstruct architecture from assumptions or repeat superseded Next statements.

## 2. Revalidate current state

Latest verified signed implementation: `38c4775e28308b779085b5230699bfab23cbcccc`.
Exact parent `ff12f14cbd429b6fc3c5713df3e3eaf7ec0c1c10`; tree `8c297d190777d2ba10719dc85fd4cb46e893ac56`.
AW last published/observed origin/main `0f9807f41456f9a29817de4a18fbf304da6417a5`; dotlink `85ceccc1088e201c98c994a46a68834440eeab32`; frozen `576d0a1a3fe0a5522a02d51f1d0598c3ddc160c2`.
Product 0.15.0, Store v62, Git derive recovery v4.

Revalidate actual newer documentation commit, HEAD/parent, owned dirty/staged files and any interrupted jobs. At this documentation cut the non-reserved worktree was clean; no new registry compatibility implementation started. Do not discard other agents' changes. No A1, no new worktree, no reserved `.agents/plans/ux/**` inspection/modification/staging, no frozen Sandbox/mount/config-v13 changes.

## 3. Continue implementation

Next substantive independent gap: **semantic compatibility for dependency-bearing SourceDriver replacement**. Read handoff §5, SPEC §§25.6–25.7, previous quiescent registry checkpoint `3c67319` and actual `crates/aw-store/src/source_compatibility.rs`, original binding/anchor/retained materialization/unresolved receipt/lease owners, `crates/aw-agent-core/src/{registry,source_driver,publication,sandbox_driver,vault_provider}.rs` and actual Git driver semantics/tests.

Reuse original staged immutable CAS publication and exclusive Store/quiescent implementation lifetime. Require explicit, actual validation of retained semantics (not matching schema ranges alone) before allowing replacement. Removal with retained dependencies remains refused; preserve original recovery identities, no receipt rewriting, no implicit effect, no unsafe active/hazard custody, no second registry or grant system. If compatibility cannot be proven, retain the specific refusal and move to another substantive independent gate. See §5 for remaining ExtensionRegistry composition, protected View/provider/manifest transfer, participant dispatch, other source constructors and later phases.

Do not repeat already-qualified direct/native/Process whole-Run, linked native artifact, mixed foreground Graph, derived Env/File/Socket/live Broker, actor-private disclosure, construction/retention RPC, quiescent registry retirement, held-Agent attach/replace/release/reattach, task-bearing captured/empty restore, explicit same-Domain cross-Project captured restore, first protected View leaf or v62 private manual context transfer. No physical containment or never-managed ambient first-source promotion: L5 has CapEff=0, NoNewPrivs=1, read-only cgroup; prior design/proof limitations are recorded. Never weaken these boundaries without new evidence. Phase 3 is underway, not complete.

## 4. Engineering and checkpoint discipline

Elegant, simple, clear, intuitive and composable Rust. Zero-cost abstractions where natural; optimize real hot paths when maintainable, simple first otherwise. Reuse original primitives; avoid over-granular evidence pinning, fixture proliferation and tiny cosmetic slices. Prepared-before-effect, exact authority and durable custody through original commit; inspect Unknown, never silently retry. Actual grouped locked/offline owner/integration tests, strict affected Clippy, relevant durable/minimal profile checks. Five historical Store failures documented; do not claim full suite green. L5 disk nearly full; avoid redundant build trees.

Limit each implementation turn to ~30 minutes, checkpoint the next coherent tested slice and pause. Update CURRENT_HANDOFF and active DL-200 TODO with actual results, limitations and next work; sign only owned paths, verify parent/tree/signature and non-reserved clean state. Never inspect another owner's reserved staging or change global queue ownership. No push, tag, release, publish, deploy or repin unless separately authorized.
