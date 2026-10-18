# Phase 62 — Solid browser Agent parity

**Status:** complete; implementation committed/tagged and validated
**Implementation checkpoint:** `a30359c278100324eeb26868a02e00cb5ed84477` (`phase-62`)
**Version:** 0.15.0
**SQLite schema:** v32 unchanged
**Authority expansion:** none

## Goal

Bring the active local-single-user SolidJS Agent surface closer to the native Agent contract without moving authority into the browser or reviving the historical browser workbench.

Phase 62 is presentation parity, not a new execution engine.

## Runtime resource truth

The browser session snapshot gains a safe, content-free runtime resource projection:

- Context baseline labels authored during runtime setup;
- active Skill labels authored during runtime setup.

The browser combines those labels with the already-authoritative durable `context_blocks` in the snapshot.

It does not infer system/instruction/skill provenance from files, prompts or project contents.

The current local Solid root renders compact Context and Skills sections above conversation history.

## Presentation settings parity

The Solid root consumes the existing local settings API rather than creating browser-only settings.

It uses canonical `UiConfig` values for:

- tool-detail visibility;
- safe thinking-activity visibility;
- configured status-row item selection;
- theme remains on the existing shared resolver.

Saving Settings updates the live presentation projection without remounting the application.

## Tool and thinking behavior

Browser tool rows remain safe typed projections.

When tool details are hidden, the browser keeps a compact:

```text
tool NAME · STATUS · hidden
```

label rather than silently removing evidence of tool activity.

For an in-flight assistant turn with no visible answer yet, `show_thinking=true` may display only the safe activity label:

```text
Thinking…
```

Provider-private reasoning text is never exposed.

## Status projection

The browser projects only configured status items for which the current snapshot has authoritative data:

- model;
- profile/account label;
- Agent/session title;
- branch;
- tools count/off;
- Session ID;
- state.

Unavailable measurements are omitted.

In particular, browser TPS is **not fabricated** when the snapshot does not contain the native measured generation timing needed to calculate it.

## Scope

Phase 62 targets the current Solid root used by ordinary local-single-user `aw --web`.

The historical retained Phase-6 browser/workbench modules remain compatibility/reference surfaces and are not made authoritative again.

## Non-goals

Phase 62 does not add:

- provider/tool authority to JavaScript;
- arbitrary filesystem reads;
- browser skill activation/registry mutation;
- fake TPS/cost/progress values;
- reasoning-text exposure;
- a second settings store;
- LSP or filesystem indexing;
- Phase 63 rendering work.

## Validation target

Phase 62 must prove:

1. the local web owner receives runtime-authored resource labels;
2. `/api/session` snapshot schema carries bounded resource metadata;
3. Solid renders Context/Skills from snapshot data;
4. durable ContextBlocks augment, rather than replace, baseline Context provenance;
5. tool visibility follows canonical settings;
6. safe live Thinking follows canonical settings;
7. browser status filters to authoritative available values;
8. settings save updates presentation state;
9. prompt submission remains unchanged and does not send resource metadata back;
10. Vite production build and browser model/workspace tests remain green;
11. schema remains v32.

## Next gate

continue to explicitly authorized Phase 63.
