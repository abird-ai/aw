# Phase 61 — Persistent explicit skill registry

**Status:** complete; implementation committed/tagged and validated
**Implementation checkpoint:** `5175003a37c891a3adcc1017a72eefc3ff1cafbe` (`phase-61`)
**Version:** 0.15.0
**SQLite schema:** v32 unchanged
**Authority expansion:** none

## Goal

Build a persistent local skill catalog on top of Phase 60 without weakening its central rule:

> Installed is not active.

The registry makes reusable skill definitions convenient. A skill is still loaded only when the user explicitly selects it for the current run.

## Registry contract

The normal aw config may contain:

```toml
[skills.review]
path = "/home/me/skills/review.md"
description = "Review Rust invariants"
```

Registry limits:

- at most 128 entries;
- Phase-60 portable skill names (1..64 ASCII bytes);
- one source path per entry;
- optional description at most 512 bytes without control characters;
- relative config paths resolve against the config file directory.

Registry metadata is secret-free.

## Activation

The Phase-60 run flag now supports both forms:

```text
--skill review
--skill review=/one/run/source.md
```

`--skill review` resolves only the named registry entry.

`--skill review=PATH` remains the exact one-run source form.

Both flow through the same Phase-60 admission checks:

- maximum 16 active skills;
- maximum 64 KiB each;
- UTF-8 only;
- unique active names;
- unique exact active source paths;
- 128 KiB combined system/instructions/skills envelope.

Merely registering a skill never adds it to model context.

## Management UX

Full aw exposes one typed resource namespace:

```text
aw skill list
aw skill show NAME
aw skill add NAME PATH [--description TEXT] [--replace]
aw skill remove NAME
```

All commands accept the standard explicit `--config PATH` override.

`add` validates the actual file with the same Phase-60 loader before persisting it and stores an absolute canonical path. Existing names require explicit `--replace`.

The existing shared Agent `[Skills]` header is the runtime activation view; it continues to show only active skills, not every installed registry entry.

The native Settings reducer remains presentation-only and does not own skill-catalog mutation. This avoids making `UiConfig` a second resource/config authority.

## No discovery

Phase 61 still does not scan:

- the current repository;
- `SKILL.md`;
- `AGENTS.md`;
- config-adjacent directories;
- a default skills directory;
- render-time filesystem state.

Registry entries exist only because the user explicitly created them.

## Saved Session semantics

Activation still changes the exact system instruction envelope.

Existing Session binding rules therefore continue to reject a changed active skill set when resuming a saved Session. Registry edits do not mutate historical Session bindings or running Agents.

## Non-goals

Phase 61 does not add:

- automatic/default active skills;
- dependency graphs or packages;
- remote install/update;
- executable extensions;
- browser registry management;
- model-selected skills;
- schema v33;
- Phase 62 work.

## Validation target

Phase 61 must prove:

1. config registry bounds/name/path/description validation;
2. config-relative paths resolve deterministically;
3. `aw skill list/show/add/remove` use the canonical config authority;
4. adding validates the real skill file before persistence;
5. overwrite requires `--replace`;
6. `--skill NAME` resolves only a registered entry;
7. missing registered names fail before provider work;
8. `NAME=PATH` remains compatible;
9. registration alone does not activate anything;
10. runtime skill provenance and provider-path privacy remain Phase-60-correct;
11. full/minimal builds remain warning-clean;
12. schema stays v32.

## Next gate

Complete validation, commit/tag `phase-61`, then continue to the explicitly authorized Phase 62.

Do not infer any skipped Phase 64/65 implementation.
