# Plan and run checklist specification

This repository tracks work with hand-edited markdown checklists. There is no
generated report, tracking JSON or evidence tree: the plan `TODO.md` **is** the
source of truth, the commit is the evidence, and a user-accepted checkpoint is
the proof of record.

Read this before opening a plan.

## Plan layout

A plan is a directory under `.agents/plans/<plan>/`. `TODO.md` is required; the
optional `README.md` is the single home for everything else:

- `README.md` — the plan details: what it is and why it exists, plus goals,
  scope, and links, and sections as needed:
  - **Design** — interfaces, invariants, alternatives considered, migration and
    rollback.
  - **Decisions** — user decisions that bind the plan, with their rationale, so
    agents follow them instead of re-litigating settled questions.
  - **References** — research and source references: papers, specs, prior art,
    source files, benchmarks, with enough provenance to reproduce the reasoning.

  Open it with the plan's authorship: when human-led, `Author: <user>` plus
  `Co-authors: <comma-separated AI models used>`; when AI-led,
  `Author: <the AI models only>`. Keep it one file while it stays readable. When
  a section grows large, split it into `DESIGN.md`, `DECISIONS.md`, or
  `REFERENCES.md` beside `README.md` and link back from `README.md`.
- `TODO.md` — the detailed todo: implementation, tests, verification, gates, and
  evidence. Required, and it needs to be extensive — every step, test, and gate
  gets its own item. Completed work is grouped under the commit that landed it;
  work without a commit yet goes under `## Uncommitted`.

```text
.agents/plans/TODO.md                      index: one checkbox and link per plan
.agents/plans/<plan>/README.md             plan details (optional)
.agents/plans/<plan>/DESIGN.md             detailed architecture (optional)
.agents/plans/<plan>/DECISIONS.md          user decisions (optional)
.agents/plans/<plan>/REFERENCES.md         research and source references (optional)
.agents/plans/<plan>/TODO.md               the plan's implementation checklist
.agents/runs/<plan>-<agent-id>/TODO.md     one agent's working checklist (in flight)
```

Keep plans flat: a plan is a top-level directory under `.agents/plans/`, not
nested inside another, and one flat plan per effort is preferred while its
`TODO.md` stays tractable. Only an extremely complex or large design splits into
smaller plans that stand on their own — added as sibling plan directories, with
the larger plan's `TODO.md` referring to them by link instead of carrying every
item.

`.agents/runs/` is ephemeral, non-authoritative scratch; its results become
authoritative only after coordinator reconciliation into the plan TODO.

## TODO shape

```markdown
# TODO — <plan>

Status: <active | done | blocked>
Provenance: <base-sha>            # commit the plan started from
Accepted checkpoint: <sha>        # user-accepted snapshot, when closed

## Committed
<!-- grouped by the commit that landed each set; newest last -->
### `<sha>` — <what this set landed>
- [x] <item>
- [x] <item>

## Uncommitted
<!-- the working list; the only place new work appears -->
- [ ] <item> — <owner / reason>
- [~] <item> — in progress
- [!] <item> — blocked outside this repository

## Tests and verification
<!-- every test and gate the plan must pass; all [x] before Status: DONE -->
- [ ] <test or gate> — <command / evidence>

## Deferred
<!-- optional; accepted, fail-closed, with the reason -->
- [ ] <item> — <reason>
```

## The rule

Tick an item only after running its named command in the same session, on the
exact commit you are marking. Record that fact on the line, for example:

```text
- [x] `cargo test --workspace --locked` — passed on `285abc0`
- [x] `cargo fmt --all -- --check` — passed on `285abc0`
- [x] `node --test tests/web_*.test.mjs` — passed on `285abc0`
```

The line *is* the proof: it names the command that was run and the commit it ran
on. Completed work is grouped under a `### <sha>` heading; the heading is the
provenance for every item beneath it. An item moves from `## Uncommitted` into a
`## Committed` group only after the commit exists and its gates pass; never
reorder or rewrite a committed group.

`## Tests and verification` lists every test and gate the plan must pass, each
with the command or evidence that proves it. A plan is not done while any of
these is open; all must be `[x]` on the closing commit. Do not store a separate
artifact, and never write that a command passed unless it was actually run.
Never mark an external gate done from authority alone; leave device, provider,
signing and release gates open until executed.

## Glyphs

`[x]` done · `[~]` in progress · `[ ]` not started · `[!]` blocked outside this
repository.

## The single gate

The only shared, runnable gate is the product build and test:

```sh
bash scripts/bootstrap.sh          # once per fresh checkout
cargo fmt --all -- --check
cargo test --workspace --locked
node --test tests/web_model.test.mjs tests/web_workspace.test.mjs \
  tests/web_context_flow.test.mjs tests/web_multiplayer.test.mjs \
  tests/web_agent_parity.test.mjs tests/web_rich_markdown.test.mjs
```

There is no phase/smoke/validation script beyond these commands. The Nix dev
shell exports `AW_BASH` and `AW_GIT`; the `aw-tools` process/shell tests and the
`aw-git` repository tests require them.

## Index line

```text
- [~] [Remaining native/browser UX convergence](ux-convergence/TODO.md)
```

The plan ID is a safe, lowercase, hyphenated directory name. The reserved `ux`
ID is owned elsewhere and must not appear in the index.

## Flow

1. **Open** — create `.agents/plans/<plan>/TODO.md` with a `## Tests and
   verification` list; add its line to `.agents/plans/TODO.md`.
2. **Work** — the agent works in `.agents/runs/<plan>-<agent>/TODO.md`.
3. **Commit** — add a `### <sha>` group under `## Committed` and move the items
   it landed into it; leave the rest in `## Uncommitted`.
4. **Integrate** — the agent appends its finished lines to the plan TODO in one
   atomic replacement and deletes its run directory.
5. **Close** — when `## Uncommitted` is empty and every `## Tests and
   verification` item is `[x]`, mark the index line `[x]` and record the accepted
   checkpoint.
