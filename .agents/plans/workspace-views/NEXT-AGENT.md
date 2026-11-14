# Next agent — workspace-views delegated browser authority

Use `dotlink---l5` only.

Canonical repo:

`/home/pvl/spaces/abird/src/aw`

Do not use A1. Do not create another worktree. Never inspect or touch
`.agents/plans/ux/**`. Preserve all unrelated dirty/staged work. Do not commit,
push, tag, release or deploy unless separately authorized.

Keep this model/user turn to roughly 30 minutes. Aim for the next coherent
checkpoint, record state, then pause.

## Read first

1. `AGENTS.md`
2. `.agents/plans/README.md`
3. `.agents/plans/TODO.md`
4. `.agents/docs/CURRENT_HANDOFF.md` in full
5. `.agents/plans/workspace-views/README.md`
6. `.agents/plans/workspace-views/TODO.md`
7. `.agents/plans/workspace-views/NATIVE-PTY-CUSTODY.md`
8. `.agents/plans/workspace-views/ADOPTED-CHILD-LIFECYCLE.md`
9. `.agents/plans/workspace-views/LIVE-PTY-MIGRATION.md`
10. live source/tests for the owner you touch

Source/tests/executed results outrank prose.

## Current state

AW HEAD:

`88cd9655d402ea325873470202fd12133b1d8199`

Final unified Jobs + Agent runtime:

`0a6c6a0bea07a3886c4dc8be4f423eec3ea3c134`

Final dotlink bridge:

`c90a6603ccef0bbf626ce10d95bedf59877747b8`

The unified runtime is finalized. Do not restart D1/D2/D3, resurrect
`subagent/team/plan/review_release/job_*`, or requalify unchanged runtime
primitives by habit.

Current owned dirty workspace-plan files before this handoff refresh were:

- `.agents/plans/workspace-views/TODO.md`
- `.agents/plans/workspace-views/LIVE-PTY-MIGRATION.md`

Preserve them.

P1 release-readiness is blocked on genuine external/native-host/owner-decision
gates. Do not substitute cross-builds or local mocks for those.

## Important corrected PTY truth

Same-UID Linux live migration is complete.

`9d82f1a742e35ab2c0c36385c8ef31a03a73b056` implements the original-shell
custody transaction and shipping `terminal-promote`.

`65f04caa5305e37671b818e3e9995089b7acc1d6` proves actual different-UID FD
admission refusal.

Fresh current-HEAD validation on `88cd965` is green for:

- narrow/maximize/focus/draft preservation;
- browser docking owner tests 24/24;
- real `terminal-promote` parser/vim/less/source-crash modes.

Do not reopen historical prerequisite bullets in `LIVE-PTY-MIGRATION.md`.
Their “migration unimplemented” statements were true at those old commits only.

Still open:

- remote/distinct-Actor PTY custody;
- macOS/Windows custody;
- broker SIGKILL/reboot execution restoration;
- remote-device trust.

## Next substantive implementation boundary

Implement **distinct user/Actor delegated browser authority**, beginning with
owner-side authorization rather than UI.

Current same-owner browser pairing/docking is not enough.

Design a small exact delegated capability with:

- owner principal;
- recipient user/Actor;
- exact source/site;
- immutable resource identity;
- narrow resource kind/scope;
- grant revision/generation;
- issued/expiry;
- revocation;
- parent authority evidence where required.

Requirements:

- browser authentication alone grants nothing;
- paths, labels, tab IDs and pairing codes never become authority;
- every receiver read revalidates the current grant;
- held replies revalidate immediately before publication;
- revoke clears protected content and derived caches;
- revoke/regrant rotates generation so stale work cannot regain authority;
- reload/reconnect does not silently regrant;
- no Agent execution retargeting;
- no Files/Git/provider/tool authority unless separately explicit;
- recent ACK is not proof of perpetual receiver liveness.

Start by inspecting:

- `src/web.rs`
- `src/web/dock.rs`
- `src/web/team/`
- `src/web/team/multiplayer.rs`
- `src/actor_owner.rs`
- `src/agent_jobs.rs`
- `crates/aw-agent-core/src/tool.rs`
- `crates/aw-store/src/auth_session.rs`
- `crates/aw-store/src/authority.rs`
- current docking/team/auth tests

## Implementation order

1. Revalidate HEAD/status/staged/worktrees.
2. Write a concise design note for delegated browser read authority.
3. Define the typed owner/recipient/resource/grant-generation contract.
4. Implement owner-side grant/revoke/revalidate first.
5. Add negative/race tests:
   - wrong recipient/Actor;
   - revoke before publish;
   - regrant generation;
   - held stale response;
   - logout/security epoch;
   - receiver crash/partition;
   - unchanged resource after revoke.
6. Only then integrate with browser docking/UI.
7. Run targeted owner/store/HTTP/Node gates.
8. Do not claim live visual/browser acceptance unless that separately owned gate
   is actually executed.
9. Checkpoint and pause.

## Do not do

- Do not reimplement Linux `terminal-promote`.
- Do not make same-owner pairing equivalent to cross-user delegation.
- Do not grant server paths from browser input.
- Do not retarget saved scope/context by pathname.
- Do not invent another Runtime/Agent owner for views.
- Do not replay terminal input/shell effects after ambiguous custody state.
- Do not claim remote views before exact device/member/revision/expiry/offline
  tests exist.
- Do not touch `.agents/plans/ux/**`.
