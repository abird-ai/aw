# Phase 59 — Typed loaded-resource provenance in shared Agent chrome

**Status:** complete; implementation committed/tagged and source-bound closeout validated
**Implementation checkpoint:** `8b7d307af32fe0a70c3a3f334a1cc9fb0bb2ab05` (`phase-59`)
**Version:** 0.15.0
**SQLite schema:** v32 unchanged
**Authority expansion:** none

## Goal

Make the shared Agent header report exactly the context resources the runtime actually admitted, with typed provenance captured at setup/admission time.

Phase 55 introduced the shared `[Context]` / `[Skills]` chrome, but `AgentHeader::standard()` still hard-coded `system prompt` and could not name explicit `--instructions` sources. The previous review explicitly forbade solving this by scanning project files or guessing skills.

Phase 59 closes that gap.

## Product contract

The Agent header is a projection of runtime truth, not discovery.

```text
[Context]
  system prompt · <origin> · <bytes> B
  instructions · <exact runtime source name> · <bytes> B
  import · artifact <id> · <N> sources
  compaction · artifact <id> · <N> sources

[Skills]
  none loaded
```

The exact visual list is bounded for presentation, but the typed summary retains all admitted resource metadata.

The header never displays system-prompt text, instruction-file contents, imported-context text, credentials, or provider-private state.

## Typed runtime model

New native TUI type:

```text
LoadedResourceSummary
├── context: Vec<LoadedContextResource>
│   ├── SystemPrompt { origin, bytes }
│   ├── InstructionFile { source, bytes }
│   ├── Import { artifact_id, source_count, bytes }
│   └── Compaction { artifact_id, source_count, bytes }
└── skills: Vec<LoadedSkillResource>
```

System-prompt origin is typed:

- built-in;
- config;
- saved Session;
- explicit `--system`.

Explicit instruction files use the exact path string that was actually passed to the runtime loader. Their contents are not retained in the UI metadata.

Durable ContextBlocks are projected from the Agent/history state by artifact ID, kind, source count and bounded metadata only.

## Skills boundary

Phase 59 does **not** invent a skill loader.

`LoadedSkillResource` exists as the typed future projection shape, but no filesystem file is converted into a loaded skill.

Until a real runtime skill-registration path exists:

```text
[Skills]
  none loaded
```

remains the correct output.

A future loader must explicitly supply skill name + loader/source provenance to `LoadedResourceSummary`; render code must not discover it.

## Runtime setup

Full and minimal Agent startup capture system-prompt origin while choosing the actual prompt.

When an explicit instruction file is successfully read and admitted, its exact source path and byte count are appended to the runtime resource summary.

This happens in the same setup path that already reads the instruction contents. No second file read or render-time I/O is introduced.

For a resumed legacy/saved Session, historical instruction-file paths are not reconstructable from the flattened saved system prompt. The truthful provenance is therefore:

```text
system prompt · saved Session
```

rather than invented filenames.

When `/resume` switches to another saved Session, the runtime resource baseline switches transactionally with the Agent binding and rolls back with the rest of the Host state if reactivation fails.

## Durable context

`Agent` and `Conversation` expose read-only `context_blocks()` slices so native presentation can project ContextBlock metadata without cloning the entire HistoryState/turn list.

The runtime baseline summary is combined with current durable ContextBlocks at projection time.

No resource metadata is written into new SQLite tables and no schema migration is required.

## Standalone Agent integration

`ComposerContext` carries the typed `LoadedResourceSummary`.

Owned Inline / Fullscreen render `AgentHeader::from_resources(...)` through the existing shared Agent chrome.

Inline’s presentation identity includes the resource fingerprint. If imported/compacted context changes while Session/branch stay the same, Inline re-shows the truthful header rather than reusing a stale presentation identity.

This affects presentation only; provider-bound context is still produced by Agent/Conversation.

## Scrollback correctness

Scrollback commits its initial Full header to terminal history.

Because committed terminal history cannot be rewritten, the presenter stores the last resource fingerprint.

If loaded resources later change, Scrollback inserts a compact **ResourcesOnly** Context/Skills update before continuing. It does not pretend the old committed header was rewritten.

No settled conversation row is replayed.

## Workspace integration

Workspace Snapshot carries the same `LoadedResourceSummary`.

The native owner merges the Host runtime baseline with whichever authoritative ContextBlocks are currently active:

- live Agent context blocks when an Agent exists;
- restored Session history context blocks when projecting a saved Agent before live Agent construction.

`workspace::agent_view` uses the same shared Agent chrome as standalone Agent.

The rich conversation render-cache key includes loaded resources. A context-resource change cannot reuse a rich transcript/header projection from the previous resource set.

## Presentation bounds

Shared Agent chrome renders at most eight rows per resource section before a compact omission marker.

This is a UI bound only. It does not silently drop provider context or mutate `LoadedResourceSummary`.

All display strings still pass through the existing terminal-safe text sanitizer.

## Explicit non-goals

Phase 59 does not add:

- a skill runtime/registry;
- render-time filesystem scanning;
- automatic AGENTS.md/skill discovery;
- model-context changes;
- new provider/tool permissions;
- new durable state or schema;
- resource synchronization;
- browser parity;
- secret/prompt-content display;
- Phase-60 work.

## Validation

Focused source/unit regressions prove:

- exact runtime instruction source names are retained;
- resource metadata never contains instruction contents;
- durable ContextBlock metadata changes resource fingerprint;
- Full and Embedded headers render the same resource truth;
- Inline presentation key changes when resources change;
- Workspace conversation cache key changes when resources change;
- Workspace Embedded header renders typed source metadata;
- minimal/no-default builds remain warning-clean;
- no `AgentHeader::standard()` static resource fiction remains;
- no render-time filesystem scan exists in resource/chrome modules.

Actual-binary PTY proof:

```text
the removed phase59_loaded_resources_smoke helper
```

launches Agent with explicit `--system` and `--instructions`, verifies the exact runtime source path/byte metadata, verifies Skills reports none loaded, verifies instruction contents are not rendered, and exits through the normal terminal lifecycle without dispatching a prompt.

## Final handoff/reference evidence

The complete source-bound reference profile contains **68 fixed steps** and passed during Phase 59 consolidation, including the Phase-59 checker, all inherited Phase 14–58/reference authorities, post-Phase58 hardening/decomposition checks, handoff integrity, static validation and patch-whitespace gate. The reference profile does not itself claim Rust execution; the Rust/PTY evidence above was run separately.

Initial Phase-59 consolidation receipt:

```text
validation/host/20261007T100742Z-4162c453665d/report.json
```

A final source-bound reference profile is rerun after all evidence and handoff self-hash fields are frozen. Its generated receipt path is intentionally not embedded back into this authority, avoiding a self-referential rewrite loop.

## Acceptance

Phase 59 is complete when:

1. the focused Phase-59 checker is green;
2. full native `aw` tests are green;
3. minimal/no-default warning-denied build is green;
4. whole-workspace/all-target compile + Clippy `-D warnings` are green;
5. doctests/rustdoc are green;
6. Workspace PTY and Phase-59 loaded-resource PTY smokes are green;
7. inherited Phase 55–58 and post-Phase58 checks remain green;
8. reference profile includes and passes the Phase-59 checker;
9. schema remains v32;
10. the implementation is committed and tagged `phase-59`.

## Next gate

User review / explicit next task.

Do not infer Phase 60 automatically.
