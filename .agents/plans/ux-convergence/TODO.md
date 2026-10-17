# Remaining native and browser UX convergence

Accepted/provenance checkpoint: `47ee8a03abecbb3aee84a49c0a590096d76ed1a7`.

Continue only authorized bounded slices after current validation. This is additive visual/interaction work, not permission to remove capabilities, implement another Agent or touch the separately owned UX subtree.

## Completed

- [x] Shipping browser/native shell and palette/focus slices use existing state/execution owners; strict browser CSP and accessible file/editor semantics remain.
- [x] Actual palette grammar is `>` Actions, `!` Windows, `@` Files, `#` Sessions, `$` Terminals, `/` Commands and `&` Agents. Empty-query Enter is unarmed; selected identity survives async insertion and invalidation stays inert.
- [x] F1/Ctrl+Shift+P, existing browser Ctrl/Cmd+K, exact draft/caret cancel, busy admission and original child-PTY/history key ownership are preserved.
- [x] Source-backed shipping native/browser UX **inventory** recorded in [README.md](README.md): 90 real Ratatui TestBackend frames (Dark/Light/System), isolated native child-shell/resize/maximize restoration checks, and four actual Chrome 155 initial-Agent screenshots (1440×900 and 390×844, Light/Dark) with no document overflow, missing composer or uncaught runtime exceptions. Exact current browser Files/Git/modal and contrast/keyboard parity remain **separate open checks**, not inferred from initial-shell screenshots.

## Uncommitted
- [ ] Apply shared semantic roles across native/browser with System/Light/Dark and 16/256/truecolor fallback: readable primary/secondary text, independent focus/selection and factual status words/icons, not color alone.
- [ ] Complete restrained chrome where gaps remain: borderless primary surfaces, one sidebar boundary, necessary split separators, single-row compact filled active tabs, quiet inactive tabs/overflow and one context-bearing title.
- [ ] Preserve user prompt emphasis, unboxed assistant prose and one compact tool block: success green, failure/denial/interruption red, running/UNKNOWN neutral or attention; unsettled outcomes must never appear successful/free.
- [ ] Preserve composer/menu/multiline/cancellation behavior, compact factual status rows and all security/source/process metadata; no extra decorative toolbar or nested box to fill space.
- [ ] Converge remaining Explorer/Git/Diff/terminals/Agent hierarchy/Sessions/Queue/activity/Settings/modal presentations without removing commands or weakening destructive/permission confirmations.
- [ ] Validate keyboard/mouse geometry and focus across narrow 80×24, Unicode/graphemes, overflow, split/maximize, expand/collapse, modal cancellation and repeated resize; inspect real native frames/PTY restoration and shipping browser light/dark screenshots.
- [ ] Implement viewers/containers/docks only through [workspace views](../workspace-views/TODO.md), preserving resource identity, authority, pending work and exact focus ownership.
