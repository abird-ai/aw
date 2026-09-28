# Session branching and summary parity — code audit

> **Phase 10 update (28 September 2026):** the source now implements the bounded explicit path. This document remains the dated Phase 9 design/audit; see [CONTEXT-FLOW-IMPLEMENTATION.md](CONTEXT-FLOW-IMPLEMENTATION.md) for actual source behavior, deliberate limits and build status.

**Reviewed:** 28 September 2026.  
**aw baseline:** `phase-9` / `df39129802b6a9e7da0101a92d56c0e5c3503174` / source 0.10.0.  
**Amendment status:** documentation and independent design fixtures only. No runtime or SQLite change; no Phase 10 completion claim.

## What the current aw source actually does

| Capability | Phase 9 |
|---|---|
| Immutable tree and named fork heads | Present in source. |
| View one selected ancestry | Present in source; checkpoint-aware. |
| Manual human summary/checkpoint | Present; caller supplies text, no LLM call. |
| Model-generated current-branch compaction | Not implemented. |
| Summarize branch being left while switching/forking | Not implemented. |
| Select several branches and generate a combined summary | Not implemented. |
| Import another branch's summary additively | No dedicated artifact/node/projection implemented. |
| Cross-user branch/source selection in team UI | Deliberately denied in Phase 9; owned sessions only. |
| Broad per-node/group derived-data policy | Target design, not shipped current protection. |

These are source findings, not compiled or end-to-end validated feature claims. See [state behavior](STATE.md), [team restrictions](TEAM-IDENTITY.md), and [new context-flow contract](CONTEXT-FLOW.md).

## Exact source references

- [crates/aw-store/src/lib.rs](../../crates/aw-store/src/lib.rs), lines 102–112: `tree` and `activate` read the authorized selected history; no summarizer is dispatched.
- [crates/aw-store/src/lib.rs](../../crates/aw-store/src/lib.rs), lines 114–138: Fork inserts a branch head reference; checkpoint persists supplied user text and advances the branch. Neither generates a model summary.
- [crates/aw-store/src/lib.rs](../../crates/aw-store/src/lib.rs), lines 225–238: `history_in` stops at the most recent checkpoint and returns the later attempts. It does not traverse siblings or record additive imports.
- [crates/aw-agent-core/src/conversation.rs](../../crates/aw-agent-core/src/conversation.rs), lines 130–158: `messages_for` uses the optional user-authored checkpoint plus completed turn context and the new prompt. There is no multi-source evidence selection here.
- [src/sessions.rs](../../src/sessions.rs), lines 39–53: Host branch/fork/checkpoint commands delegate directly to the store and restore the selected view.
- [src/web.rs](../../src/web.rs), lines 569–584: Phase 9 team mode preserves the owned-only open guard before changing its participant view.
- [crates/aw-store/migrations/001_initial.sql](../../crates/aw-store/migrations/001_initial.sql), lines 15–38: Persisted node kinds are root, prompt, turn and checkpoint; ancestry is single-parent and immutable. Adding a new node kind requires an actual migration.

The [accepted checkpoint](../plans/accepted-history/TODO.md) retains this design history in Git. Current source and executable tests determine implementation behavior.

The important missing distinction is **replacement versus addition**. Current checkpoint projection discards older active messages in favor of the supplied summary while retaining raw history on disk. Inserting a summary of branch B through this method would not preserve branch A's full pre-import active context. A dedicated ordered context-import block is required; a string concatenation workaround loses provenance, audience constraints and event ordering.

## What Pi provides

Pi's documented compaction summarizes older active context while retaining a recent span. Its tree-navigation summary instead carries work from the old path into the destination. `collectEntriesForBranchSummary` locates a common ancestor and gathers the departed path's suffix; structured summaries and source-entry linkage preserve a useful record. Pi exposes customization hooks, so arbitrary extension behavior is not ruled out by this comparison.

The aw proposal is not “the first ability to move context between branches.” It extends that interaction into an explicit multi-source, access/disclosure-aware import mechanism with cross-session sources, immutable coverage, per-requester model accounts and personal-device consent. Those broader semantics still need implementation.

Primary references actually inspected:

- [Pi compaction reference](https://github.com/earendil-works/pi/blob/main/packages/coding-agent/docs/compaction.md).
- [Pi branch-summarization implementation](https://github.com/earendil-works/pi/blob/main/packages/coding-agent/src/core/compaction/branch-summarization.ts), fetched blob `0e0408b50183e7e3d7d6c64c4d66cc64cfe62489`, lines 1–205. Branch/ref state can evolve; this identifies the source reviewed.

The old `badlogic/pi-mono` file fetch did not return readable content in this check; the primary `earendil-works/pi` repository supplied the inspected document/code. No model-quality, performance, compiler or source-format compatibility claim is inferred.

## Accepted direction for aw

Preserve zero-inference fork/switch, add optional single-branch carryover, and use the same summary/import primitive to select multiple permitted sources without navigating away. Keep summary generation separate from application, preserve source/destination history, and never infer that read permission includes disclosure to another audience or provider. Prefer owned-private sources first; cross-user sharing depends on the explicit release/revocation layer.

This is a documentation amendment to Phase 9 with a distinct design tag. The full-history Git bundle carries all prior phase tags unchanged. No new CLI command or endpoint is advertised as executable here.
