# AGENTS.md

<!-- BEGIN common -->
## Shared conventions

These are byte-identical across all abird repositories. Repo-specific rules
follow the markers below.

- **Source of truth.** Live source, tests and executed results outrank dated
  prose. Start at `.agents/plans/README.md` (overview + spec), then work from
  `.agents/plans/TODO.md` (the queue).
- **XDG paths.** Where the project owns user config, data, cache or state,
  resolve it under `$XDG_*_HOME/abird/<project>` — an absolute XDG base, falling
  back to `HOME` when unset. No legacy or alternate root.
- **Shared `.gitignore`.** The block at the top of `.gitignore` is identical
  across repos; repo-specific entries follow it. Keep it mechanical.
- **Preserve other work.** Never revert or overwrite changes you did not make.
  Stage explicit paths, not broad globs, and leave other agents' staged or
  uncommitted work untouched.
- **Git safety.** Commits are signed; never disable signing. Never push,
  force-push, tag, publish, deploy, or run destructive operations — history
  rewrite, `reset --hard`, branch deletion, or touching another agent's
  staged or uncommitted work — without explicit user confirmation for each
  action. A request to "commit" or "apply" is not authorization to push.
<!-- END common -->

<!-- BEGIN repo-specific -->
## Repository

Start with the plan index, then the selected plan's `TODO.md`.
The checklist rules — plan layout, the optional `README.md` details with
`Author:`/`Co-authors:`, the commit-grouped `TODO.md`, and the `## Tests and
verification` section — are in README.md.
Read the relevant live documentation under `.agents/docs/` and actual source
before changing behavior. Git, source and executed tests outrank dated prose.
There is no requirement to reconstruct or read old comprehensive handoffs.

The user accepted `47ee8a03abecbb3aee84a49c0a590096d76ed1a7` as the checkpoint
for previously verified work. That acceptance does not complete the unchecked
tests, release or external gates in current plans.

- **XDG project:** `abird/aw`.

## Plans and parallel work

Follow `.agents/plans/README.md` for the layout. The coordinator alone
reconciles the shared plan: each agent hands back its exact owned paths, result
and implementation commit; the coordinator merges by atomic replacement (temp
sibling + rename), reviews the combined change, and marks the index line done
only when nothing remains, using the expected-parent Git commit check. Agents
must not append concurrently to the shared file.

Run TODOs are ephemeral, non-authoritative scratch and are excluded from source
validation/export; their results become authoritative only after coordinator
reconciliation into the plan TODO. Plan TODOs remain validation inputs.
Completed items record the relevant test commands/results and implementation
commits (or the accepted checkpoint for older work); never create a commit
merely to insert its own hash.

Execute real tests and strict validation. Reports/logs are optional transient
diagnostics, not mandatory handoff archives. Never substitute a source check for
an unexecuted runtime test or call a partial run a full pass. Failed, blocked and
unfinished items stay unchecked.

## Ownership and Git

Another owner reserves `.agents/plans/ux/`, including its staging. Never list,
read, write, stage, reset, stash or commit that subtree or inspect its staged
entries. Inspect only owned work; preserve other agents' files/staging as opaque
data. Isolated worktrees must exclude the reserved subtree before checkout.
The coordinator owns reconciliation, commits, pushes and deletion decisions
unless explicitly delegated.

Preserve signed history and historical tags. Do not commit credentials,
runtime databases, transcripts, real grants, caches or build outputs. Review
exact owned paths before an atomic, expected-parent commit.

## Repository and history layout

- Live product source is the workspace root (`Cargo.toml`, `crates/`, `src/`,
  `examples/`, `web/`, `tests/`, `nix/`, `scripts/`).
- Live documentation is under `.agents/docs/`. The dated plan/design record is
  the rescued `.agents/plans/*.md` set; the live checklist model is
  `.agents/plans/TODO.md` plus each `<plan>/TODO.md`.
- The history was rebuilt as a progressive, buildable sequence. Superseded
  evidence dumps, archived handoff manifest trees (`docs/`, `validation/`,
  `.agents/plans/repository-docs|generated-artifacts|visuals|forward` and the
  multi-megabyte comprehensive-handoff files) and the old `phase.json` tracking
  blob are not part of this history. They are preserved in the off-repo
  pre-rewrite bundle recorded in the plan index.

- One user-facing `aw`, one original Agent/TurnMachine/ToolRegistry/ToolPolicy
  and immutable TurnJournal. Runtime primitives are Runtime, Actor, Message,
  Envelope; Hook verbs are Next and Defer. Domain → Workspace → Session remains
  the user model. Follow the live architecture,
  runtime,
  CLI and SDK for the relevant change.
- Preserve accepted history, CAS/writer epochs, exact Actor/source/site/effect
  grants and revocation. Journal intent before model/tool effects; unknown
  effects are inspected, never silently replayed, erased or called zero usage.
  Preserve paid results even if later attachment fails.
- Keep account, payer, custody, inference and execution distinct. Provider
  secrets stay with their configured executor/SecretStore. No implicit fallback,
  service/listener, model call, tool grant or browser credential disclosure.
- Owner/service holds exclusive `owner.lock`; trusted direct access is an
  explicit supported local mode. Do not delete locks, replicate raw WAL, add
  schema changes or bypass quarantine as a recovery shortcut. Restored State
  and Queue effects remain inert until their exact authority requirements pass.
- Preserve no_std/alloc and full/minimal/stateless dependency boundaries. Views
  project existing owners; changing a view must not retarget an active Agent.
  Provider-private reasoning stays private; incomplete traces fall back to the
  authoritative answer or a bounded error.
- Version is 0.15.0, SQLite schema v33 and JSONL v1 until an authorized change.
  Numbered phases stop at 68; skipped 64/65 indexing/LSP and E6 firmware remain
  parked. Do not invent Phase69 or claim complete v1.5/agentc/security parity.
  Read the affected live protocol before changing storage, retry, networking,
  device, team or disclosure behavior.

<!-- END repo-specific -->
