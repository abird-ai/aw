# Post-Phase-58 fresh-review correctness and composability hardening — 2026-10-07

**Status:** implemented and validated; ready for user review
**Numbered phase:** none. **This is not Phase 59.**
**Source version:** 0.15.0
**SQLite schema:** v32 unchanged
**Authority:** newest narrow authority for the defects and contracts described here. The comprehensive 2026-10-07 handoff, Phase 58, and prior post-Phase-58 authorities remain authoritative elsewhere.

## Why this checkpoint exists

A clean-room review of the validated post-Phase-58 tree found several defects that the existing green checkers did not cover:

1. semantic-sync pagination could stall forever when a bounded source page contained only local-only or currently unauthorized commit-log rows;
2. Workspace still carried a second Agent composer reducer and therefore drifted from standalone prompt-history/key semantics;
3. plain Ctrl+P could be stolen globally from the Workspace Agent composer or an embedded PTY even though current UX assigns Ctrl+P/N to prompt history and gives child terminals ordinary key ownership;
4. inherited Kitty/WezTerm/Ghostty/foot environment markers could bypass tmux's immediate transport capability check for enhanced-key reporting;
5. failed asynchronous configuration writes discarded the exact failed latest snapshot instead of retaining it for retry;
6. multi-Session archive/restore could partially apply because one user action used one SQLite transaction per Session;
7. generated title normalization duplicated logic and bounded characters while durable Session labels are byte-bounded;
8. phase metadata retained a stale schema-v31 compatibility field beside the authoritative v32 field.

This hardening repairs those issues without widening provider, tool, routing, storage, synchronization, or governance authority.

## Semantic sync: source-page cursor is separate from disclosure

commit_log.seq is a physical-store source cursor. Whether a particular event may be disclosed to one caller is a separate decision.

SessionStore::commit_event_page_after now returns a typed CommitEventPage with:

- after_seq: requested source cursor;
- through_seq: last source sequence actually scanned;
- events: only events currently authorized for that actor and eligible for semantic disclosure.

through_seq advances across filtered rows. Therefore a page containing only local presentation state or inaccessible Session events can return zero disclosed events while still allowing the next request to continue after those source rows.

commit_events_after remains a compatibility convenience for callers that only want disclosed events. Cursor-based replication uses the page API.

semantic_sync_export uses the scanned source cursor, not the last disclosed event. semantic_sync_apply therefore accepts cursor-only batches and batches whose final disclosed event precedes through_seq, while still requiring monotonic non-backwards cursor movement, strict event ordering inside the scanned range, bounded event identity/payload validation, and the existing authenticated federation lease plus idempotent inbox/cursor transaction.

A filtered event never becomes authorized merely because the cursor advances over it.

## Session archive: one atomic local-view mutation

Per-user archive remains presentation state, not Session deletion and not security-audit authority.

SessionStore::set_sessions_archived is now the bounded atomic primitive:

- at most 128 Sessions;
- duplicate targets collapse deterministically;
- every target is authorized and legacy compatibility state is read before any new event is written;
- one BEGIN IMMEDIATE transaction covers the whole user action;
- either all requested view changes commit or none do;
- unchanged targets emit nothing.

set_session_archived delegates to the batch primitive.

Every new session.view.changed event explicitly carries viewer, archived, and sync_scope = local. The event remains useful as an append-only local preference journal under schema v32, but it is explicitly excluded from semantic-sync disclosure. A future cross-device user-view feature must introduce a typed synchronization contract rather than accidentally exporting local presentation state.

Workspace now calls the atomic batch API once for a multi-select archive/restore action.

## One Agent composer reducer, including Workspace

src/tui/composer.rs Interaction is now the actual shared Agent interaction reducer for standalone Scrollback, standalone Inline, standalone Fullscreen, and the embedded Workspace Agent composer.

Workspace no longer owns a second slash/history/editor key reducer.

The embedded Workspace Agent derives a bounded prompt-history projection from the latest 128 authoritative turns and feeds it to the shared reducer. Consecutive duplicate prompts collapse in that presentation projection.

The shared reducer continues to own slash selection/completion/wrap, Ctrl+P/N history and scratch restoration, multiline Up/Down, settings, ordinary submit semantics, and editor mutation. Workspace translates only the reducer's semantic actions into application effects.

## Command-palette and child-key ownership

Metadata-rich Ctrl+Shift+P and F1 remain global Workspace command-palette shortcuts.

A terminal that collapses Ctrl+Shift+P into plain Ctrl+P may use plain Ctrl+P as a fallback only in Workspace navigation mode.

Plain Ctrl+P is not intercepted when the Agent composer owns input, where it means previous prompt history, or when an embedded terminal owns input, where it is delivered to the child PTY.

This aligns implementation with the documented keyboard model instead of treating a terminal protocol ambiguity as permission to steal a meaningful focused-surface key.

## Enhanced keyboard protocol respects the immediate terminal boundary

TerminalSession now computes enhanced-key eligibility through one pure policy helper.

- Direct known-capable terminal: direct protocol may be enabled.
- Inside tmux: only tmux's positive capability result is authoritative.
- Outer terminal environment markers inherited through tmux never bypass an unsupported tmux transport.

This keeps modifier disambiguation opportunistic rather than breaking portable fallback behavior.

## Configuration persistence retains failed latest state

The one-token coalescing persistence service remains the only application config writer.

If config::save fails:

- the newest pending complete FileConfig is retained;
- a newer concurrently queued snapshot wins rather than being overwritten by the failed older snapshot;
- the failure remains observable through last_error;
- flush retries the retained snapshot and remains the explicit durability barrier;
- a successful retry clears the remembered error.

queue means the latest in-memory snapshot was accepted for persistence. A previous asynchronous disk failure does not falsely reject an already-applied UI mutation.

No synchronous UI-thread saves or ad-hoc persistence threads were added.

## Generated Session title normalization

Standalone Agent and Workspace now share normalize_generated_title.

It sanitizes display controls through the existing presentation sanitizer, takes the first non-empty returned line, removes simple quote/backtick wrappers, limits to at most 100 Unicode graphemes for the UX contract, simultaneously limits to 160 UTF-8 bytes for the durable Session-label contract, never splits a grapheme, and fails if no usable title remains.

The durable store remains the final label validator.

## Metadata consistency

phase.json continues to report Phase 58 and schema v32. The stale sqlite_schema_version value is corrected from 31 to 32 so machine-readable current-schema fields no longer disagree.

No Phase 59 tag or schema v33 exists.

## Validation additions

New focused regressions cover:

- atomic archive rollback on one invalid target;
- local-only archive events advancing source cursor without semantic disclosure;
- later semantic facts remaining reachable after filtered rows;
- cursor-only semantic-sync application and trailing filtered source sequences;
- Workspace Ctrl+P/N using the shared prompt-history reducer;
- navigation-only collapsed Ctrl+P command-palette fallback;
- tmux blocking inherited outer-terminal capability markers unless tmux reports support;
- failed config persistence retaining and retrying the newest snapshot;
- generated multilingual titles respecting both grapheme and durable byte bounds.

the removed test_post_phase58_fresh_review_hardening helper is the dedicated static/reference checker and is part of the reference validation matrix.

### Validation evidence on the implementation candidate

Green on the exact candidate before the implementation checkpoint commit:

- dedicated fresh-review checker: **42/42**;
- inherited post-Phase-58 review: **49/49**;
- inherited post-Phase-58 UX hardening: **163/163**;
- Workspace/interaction checker: **264/264**;
- Phase 58: **38/38**; Phase 57: **54/54**; Phase 56: **99/99**; Phase 55: **117/117**; Phase 54: **136/136**;
- full native `aw` binary suite with `full`: **240/240**;
- `aw-store --lib`: **75/75**;
- store integration: **39/39**;
- resources/distributed integration: **23/23**;
- whole-workspace/all-targets compilation (`--no-run`): green;
- whole-workspace/all-targets Clippy with `-D warnings`: green;
- doctests and rustdoc warnings-as-errors: green;
- SDK dependency-boundary checks: green;
- warning-denied minimal/no-default, Workspace, routing and pipelines runtime cuts: green;
- warning-denied `team,multiplayer` and `web,oidc` combined feature closures compile green; the `team,multiplayer` full runtime wrapper exceeds this connector's 30-second call ceiling and is not falsely claimed as rerun here;
- Phase-23 semantic-sync actual-binary smoke: green;
- Phase-55 terminal lifecycle PTY: **8/8**;
- Phase-57 natural-flow tmux: **6/6**;
- Phase-58 Scrollback saturation tmux: **4/4**;
- post-Phase-58 real modifier/resize/exit terminal smoke: **4/4**;
- three-mode Agent PTY: **3/3**;
- live F6/F7 PTY: green;
- Workspace actual-binary PTY: **11/11**, using F1 as the reliable global palette path so focused Ctrl+P remains owned by the Agent composer/child PTY;
- static source validation: **1001 active files / 988 authored Rust test attributes / 732 local links / 0 errors / 0 warnings**;
- reference validation plan now contains **66** steps.

The monolithic whole-workspace runtime wrapper still exceeds the dotlink per-call ceiling and is not separately claimed; its relevant changed-path runtime suites and warning-denied compile/lint/doc constituents above were executed individually. No optimized-release, live-provider, billing, Nix-release or security-audit claim is added.

## Unchanged boundaries

- schema remains v32;
- immutable phase-58 tag remains untouched;
- no Phase 59;
- no provider/tool/routing/account authority expansion;
- no change to effect intent-before-dispatch or no-replay semantics;
- no raw SQLite/WAL synchronization;
- no destructive Session purge;
- no model-selected authority;
- no push.

## Remaining decomposition direction

The fresh review does not justify a broad rewrite of src/workspace/ui.rs.

Continue only through behavior-preserving typed seams. The highest-value remaining boundaries are still command execution, Files/Git async coordination and cache ownership, remaining application interaction reduction, and renderer/hit-map façade.

The key rule is now stronger because of this repair: shared Agent behavior must be consumed through the shared reducer/service rather than reimplemented inside Workspace.
