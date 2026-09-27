# Plan and run checklist specification

This repository tracks work with hand-edited markdown checklists. There is no
validation harness, generated report, tracking JSON or gate script. The plan
TODO **is** the source of truth, and the proof of a runnable item is the TODO
line that records the command and its result.

## Files

```
.agents/plans/TODO.md               one line per plan (the index)
.agents/plans/TODO-SPEC.md          this specification
.agents/plans/<plan>/TODO.md        the plan's checklist
.agents/runs/<plan>-<agent>/TODO.md one working agent's scratch checklist
```

`.agents/runs/` is ephemeral, non-authoritative scratch; its results become
authoritative only after coordinator reconciliation into the plan TODO.

## The rule

Tick an item only after running its named command in the same session, on the
exact commit you are marking. Record that fact on the line, for example:

```
- [x] `cargo test --workspace --locked` — passed on `285abc0`
- [x] `cargo fmt --all -- --check` and `node --test tests/web_*.test.mjs` — passed on `285abc0`
```

The line *is* the proof: it names the command that was run and the commit it ran
on. Do not store a separate artifact, and never write that a command passed
unless it was actually run. Never mark an external gate done from authority
alone; leave device, provider, signing and release gates open until executed.

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

```
- [~] [Remaining native/browser UX convergence](ux-convergence/TODO.md)
```

The plan ID is a safe, lowercase, hyphenated directory name. The reserved `ux`
ID is owned elsewhere and must not appear in the index.

## Plan checklist

```markdown
# <Plan title>

Accepted/provenance checkpoint: `<40-hex commit>`.

## Completed

- [x] <finished item; runnable items record the command and the commit it passed on>

## Uncommitted

- [ ] <unfinished item>
- [~] <item under active work>
- [!] <item blocked outside this repository>
```

## Flow

1. An agent works in `.agents/runs/<plan>-<agent>/TODO.md`.
2. When work completes, the agent appends its finished lines to the plan TODO in
   one atomic replacement and deletes its run directory.
3. The coordinator reconciles the plan into `.agents/plans/TODO.md` and marks the
   index line `[x]` only when nothing remains.
