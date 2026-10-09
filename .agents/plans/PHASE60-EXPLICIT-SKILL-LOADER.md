# Phase 60 — Explicit named skill loader

**Status:** complete; implementation committed/tagged; handoff consolidation in progress
**Implementation checkpoint:** `9e526b9f81a663f124adbf2fddd9cbba023d01c9` (`phase-60`)
**Version:** 0.15.0
**SQLite schema:** v32 unchanged
**Authority expansion:** none

## Goal

Make aw's existing `[Skills]` projection represent real runtime-loaded skills instead of remaining permanently empty.

A Phase-60 skill is a **named, explicitly selected UTF-8 instruction resource**. It is not a tool, executable plugin, extension process, permission grant, filesystem-discovery convention, or autonomous capability.

## User contract

Repeatable run flag:

```text
--skill NAME=PATH
```

Examples:

```text
aw --skill rust-review=/home/me/skills/rust-review.md
aw --skill docs=./skills/docs.md --skill release=./skills/release.md
```

Only flags explicitly supplied for the current run are loaded.

There is no:

- implicit `SKILL.md` discovery;
- project-directory scanning;
- config-directory scanning;
- automatic `AGENTS.md` promotion;
- skill activation merely because a file exists;
- model-selected skill loading;
- executable skill code.

## Bounds

The loader enforces:

- at most **16** selected skills;
- portable skill names, **1..64 ASCII bytes**, using letters, digits, `.`, `_`, and `-`;
- unique skill names;
- unique exact source paths within one run;
- UTF-8 content only;
- at most **64 KiB per skill**;
- the existing **128 KiB total system/instructions/skills envelope** remains authoritative.

Invalid or duplicate specs fail before provider dispatch.

## Provider/context semantics

Selected skill text is appended to the same system-instruction envelope as explicit `--instructions`.

The provider sees:

```text
Explicit user-selected skill "NAME":
<skill contents>
```

The local source path is **not** embedded in the provider-bound prompt.

The source path remains local provenance only.

Skills do not alter:

- tool grants;
- provider account;
- payer;
- execution site;
- filesystem root;
- routing;
- subagent permissions;
- Workspace authority.

## Runtime provenance

The Phase-59 `LoadedSkillResource` becomes live runtime state.

For each successfully admitted skill it records:

- skill name;
- exact selected source path;
- byte count.

Shared Agent chrome renders:

```text
[Skills]
  NAME · PATH · N B
```

Full Agent, Inline, Fullscreen, Scrollback and Workspace all continue to consume the same `LoadedResourceSummary`.

Resource fingerprints already include the skill vector, so skill activation participates in Inline/Workspace presentation identity without a new cache mechanism.

## Saved Session truthfulness

Current Session bindings persist the flattened system prompt, not first-class skill provenance.

Therefore Phase 60 does **not** invent or reconstruct skill names/paths on resume.

A saved Session retains the effective skill instructions because they are part of its saved system prompt, but its reconstructed resource header cannot truthfully claim the original skill files. A future binding-format change may preserve that provenance explicitly.

No SQLite migration is required in this phase.

## Architecture

One shared `src/skills.rs` module owns:

- `SkillSpec` parsing;
- portable-name validation;
- selection count and duplicate validation;
- bounded file loading;
- UTF-8 validation;
- provider-bound skill instruction framing.

Full and minimal Agent entrypoints use the same module.

The TUI resource module remains presentation metadata only and does not perform filesystem I/O.

## Explicit non-goals

Phase 60 does not add:

- skill auto-discovery;
- project/local config inference;
- persisted skill registry;
- skill marketplace/install/update;
- executable scripts/WASM/processes;
- tool definitions from skills;
- dynamic loading after a turn starts;
- model-selected skills;
- browser skill management UI;
- remote skill synchronization;
- schema v33;
- provider/tool/governance authority;
- Phase 61 work.

## Validation target

Phase 60 must prove:

1. full and minimal CLI accept repeatable `--skill NAME=PATH`;
2. malformed names/specs fail before provider dispatch;
3. duplicate names and duplicate exact paths fail;
4. non-UTF-8 and oversized skill files fail;
5. total system + instructions + skills remains bounded by 128 KiB;
6. skill source paths never enter provider-bound system text;
7. exact selected skill name/path/bytes appear in local `[Skills]`;
8. no render-time filesystem discovery is introduced;
9. full/minimal warning-denied builds stay green;
10. an actual-binary PTY smoke proves loading/display alone makes no provider request, then inspects one explicit local-fixture prompt to verify provider-envelope semantics;
11. schema remains v32;
12. historical Phase-59 resource behavior remains green.


## Implementation evidence before tag

- Phase 60 checker: **62/62**;
- full native `aw`: **251/251**;
- minimal/no-default warning-denied native `aw`: **80/80** plus inherited library/provider tests;
- actual-binary Phase 60 skill PTY: **3/3**;
- Phase 59 loaded-resource PTY: **3/3**;
- actual-binary unified CLI help smoke: green;
- Phase 36 CLI checker: **88/88**;
- Phase 59 resource checker: **74/74**;
- post-Phase59 UX checker: **62/62**;
- whole-workspace/all-target compile: green;
- whole-workspace/all-target Clippy `-D warnings`: green;
- doctests/rustdoc warnings-as-errors: green;
- static validation: **1017 files / 1000 authored Rust test attributes / 738 links / 0 errors / 0 warnings**.

The complete source-bound reference profile is intentionally rerun after the numbered commit/tag and handoff metadata are consolidated.

## Next gate

User review / explicit next task.

Do not infer Phase 61 automatically.
