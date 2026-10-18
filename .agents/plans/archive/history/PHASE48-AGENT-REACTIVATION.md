# Phase 48 — safe saved-Agent reactivation and composer visibility hardening

Status: authorized 2026-10-05 by the user after Phase 47 review.

## Boundary

Phase 48 is a native usability/blocker-fix phase.

It fixes:
1. fullscreen Agent selection/resume incorrectly rejecting Sessions rooted in a different saved working/tool directory;
2. fullscreen empty-composer placeholder becoming effectively invisible on some terminal themes because muted placeholder text was additionally DIM-modified.

Phase 48 changes no provider/account/routing/team/subagent authority model and adds no SQLite migration.
SQLite remains v30.

## Saved-Agent selection semantics

An existing durable Session owns a saved SessionBinding containing working directory, tool root, system prompt, profile hint, model hint, and provider destination/auth-reference fingerprint.

Selecting an existing Agent in the native workspace must reactivate that saved Session in its own saved binding. This is distinct from asking to open or move a Session under the currently active Agent's root/binding.

### Existing relocation guard remains

check_binding(current, saved, allow_transfer) remains authoritative for explicit open/resume operations that attempt to reuse a Session under a caller-supplied current binding.
It must continue rejecting changed working directory/tool root, changed system instructions, and changed provider destination/auth binding without explicit transfer.
Phase 48 must not weaken or bypass that general protection.

### Workspace saved-session reactivation

When selecting an accessible local durable Agent:
1. activate the target Session/branch using its saved binding;
2. reject remote Machine/Fleet targets;
3. if the current Agent has unresolved reconciliation and the target is another Session, fail closed;
4. reconstruct the saved provider profile: reuse the exact current live profile when profile name and destination fingerprint match (important for safe CLI endpoint overrides), otherwise resolve the saved profile hint from configured profiles/built-ins;
5. force the saved model hint;
6. require the provider destination fingerprint to equal the saved fingerprint;
7. restore the saved system prompt;
8. recapture the saved working directory;
9. canonicalize and validate the saved tool root;
10. clear any unrelated runtime account selector rather than applying the previous Agent's optional account override to another Session;
11. rebuild the Agent/tool registry against the restored settings/context/root;
12. attach the selected Session history/journal.

No inference occurs during selection/rebuild.

### Transactional switch / rollback

Workspace saved-Agent switching is transactional from the active Host's perspective. Before rebuild, retain current settings, execution context, tool options, active Agent, current persistent Session, account selector, and retry/performance/routing transient state.
If reconstruction or Agent build fails, restore the old Host state and leave the currently active Agent usable.
A broken/missing old Session must not poison the current Agent merely because the user clicked it.

## Workspace projection

After a successful switch, the native projection refreshes active Session/title/branch, profile, model, tool root/display root, and tool mode.
The UI must not show the previous Agent's root after a successful saved-root reactivation.

## Composer placeholder visibility

The fullscreen empty Agent composer remains one top separator row plus one editable row.
Phase 48 does not change its text/layout contract.
The placeholder uses the semantic muted foreground without Modifier::DIM.
Some terminal/theme combinations render muted plus DIM so faintly that only the cursor-overlaid first character is visible, producing the observed lone A.
Placeholder visibility remains semantic-muted, not bright foreground, but is no longer double-dimmed.

## Deferred / unchanged

Phase 48 does not add Session root migration, implicit provider context transfer, automatic missing-profile repair, remote Session dispatch, persisted account-selector restoration, tool-mode persistence changes, schema migration, planner/reviewer/scheduler work, or browser redesign beyond these blocker fixes.

## Validation requirements

- ordinary open_session still rejects changing execution directories;
- open_saved_session reopens a different-root Session using its own durable binding;
- workspace Command::Resume uses saved-session reactivation rather than caller-current binding validation;
- unresolved effects block switching to another Agent;
- exact current live profile may satisfy a saved CLI-overridden profile when name/fingerprint match;
- configured/builtin fallback works only when destination fingerprint still matches;
- changed destination fingerprint fails closed;
- saved working directory/tool root are revalidated before switch;
- successful switch rebuilds Agent/tools and clears unrelated account selector;
- rebuild failure restores the prior active Host state;
- workspace snapshot refreshes root/profile/model/tool mode after switch;
- actual compiled-binary smoke creates two real SQLite Sessions in different roots, switches A to B in fullscreen, sees no old root-move denial, and successfully prompts with B's prior history;
- composer render test asserts the placeholder cell uses muted foreground and does not contain Modifier::DIM;
- Phase-47 interaction checker and workspace checker remain green;
- SQLite remains v30;
- whole-workspace/all-targets, feature cuts, doctests, rustdoc, Clippy, fullscreen/inline PTY, reference/static/Vite remain green.