# Phase 38 — shared prompt drafts and presence

**Status:** implementation complete and locally validated on 2026-10-03; ready for Phase-38 checkpoint commit/tag and user review.
**Base:** `83f7ec7` on `main`; historical `phase-37` remains fixed at `9941725`.
**Schema target:** SQLite v21.
**Product boundary:** optional pre-submission shared prompt collaboration; no pipeline/autonomy work.

## Goal

Add the first intentionally deferred v0.7 multiplayer product layer without weakening any existing execution, credential, storage, or context boundary.

Several authenticated human participants may collaborate on **one unsent prompt** for an explicitly shared Session/branch. Collaboration is mutable pre-submission state. Submission freezes one exact candidate and then enters the ordinary prompt admission path owned by the submitter. Typing, cursor motion, joining, or sharing causes **no extra model call** and never starts inference.

## Phase 38 scope

1. A transport-neutral `aw-collab` crate owns bounded Yjs-compatible plain-text update handling and typed draft protocol data. It has no provider/tool/store/network authority.
2. The `multiplayer` feature remains optional and is the only feature that activates the collaboration engine. The ordinary solo composer does not allocate a CRDT document or presence timer.
3. SQLite schema v21 adds durable shared-draft policy, grants, active generations, accepted updates, exact sealed candidates and submission receipts. Presence remains in memory only.
4. Session owners explicitly share a current Session with named team participants. Session membership and draft permissions are distinct; neither copies provider credentials, tool grants, billing authority, or model defaults.
5. Shared draft permissions are explicit: read, edit, submit, manage and presence-read. Audience changes invalidate the current document epoch rather than exposing prior CRDT history to a newly different audience.
6. Draft modes are per Session: `Off | Auto | On`. `Off` keeps the private path. `On` permits explicit shared-room creation. `Auto` permits creation only through an explicit collaborative join in this phase; it does not start rooms for historical chats.
7. Private nonempty composer text is not published by joining. The UI must require an explicit **Share this draft** action; keeping it private remains available.
8. Text synchronization uses Yjs-compatible V1 binary updates. Rust uses Yrs with pre-Yjs-v14 client-ID compatibility; the browser dynamically loads Yjs only when multiplayer activates.
9. Every accepted text update receives a durable server revision and authenticated stable-user contributor attribution. Client IDs are synchronization IDs only.
10. Presence/cursor state is authenticated, bounded, latest-wins, expiry-based and non-durable. It is never prompt content or activity metrics.
11. Send flushes the submitter's pending update, then seals exactly the acknowledged draft revision, materialized UTF-8 text, metadata revision and Session branch head/revision. A racing edit produces `DraftChanged`; a late edit after seal produces `GenerationSealed` and remains recoverable locally.
12. Exactly one durable submission identity exists per generation. Duplicate Send resolves to that submission; it cannot create a second model run. The submitter's normal account/model selection remains authoritative.
13. Phase 38 team-web integration may open an explicitly shared Session only through the dedicated shared opener. Root/tool-directory and system-instruction binding must still match. If the participant's destination/account fingerprint differs, that dedicated action is the explicit context-transfer approval; ordinary Session open remains denied and no provider receives history until the participant explicitly submits.
14. Shared Session/history discovery remains explicit; hidden Sessions do not become enumerable merely because multiplayer exists.

## Non-goals

- collaborative source-file editing;
- shared PTY input, desktop control, scrolling or workspace geometry;
- per-character secrecy inside one CRDT document;
- peer-to-peer/WebRTC collaboration;
- background rooms for every shared Session;
- new provider/account fallback;
- sharing personal credentials or tool grants;
- semantic moderation/classifier/model calls per keystroke;
- pipeline/routing/autonomy work;
- changing Domain -> Workspace -> Session ownership;
- changing writer epochs, no-replay, paid-result durability, semantic sync, materialization/context projection or trusted-direct locking.

## Authority rules

- Session owner remains the only actor that manages Session membership/draft grants in this phase.
- Read/edit/submit/manage/presence permissions are server-validated against stable user identity on every operation.
- `draft.submit` does not itself grant generic Session append, fork, checkpoint, branch, model, account, tool or budget authority. The draft ACL is separate from `members.can_append`. A sealed submission may cross ordinary prompt admission only through its unique durable request/receipt/run binding; branch CAS and all provider/account checks are still revalidated there.
- A shared editor cannot select or mutate another participant's account, model, payer, credential custody, tool grants or provider continuation.
- CRDT bytes are data, never ACL/policy/account/routing authority.
- Provider-private reasoning remains private and is unrelated to draft collaboration.

## Durability rules

- accepted text update -> durable commit -> acknowledgement;
- sealed candidate bytes/digest/target -> durable commit before submission is reported accepted;
- duplicate submission lookup precedes any model dispatch;
- unknown provider/tool effects retain existing no-replay semantics;
- draft recovery replays accepted text state only, never model/tool effects;
- presence is explicitly ephemeral and is never reconstructed as proof of availability or activity.

## Validation gates

Minimum Phase 38 completion evidence:

- `aw-collab` Rust convergence tests, including concurrent inserts/deletes and Unicode;
- Yrs/Yjs cross-language V1 fixture convergence;
- schema20 -> schema21 migration regression;
- store tests for Off/Auto/On, explicit grants, revocation/epoch rotation, accepted revisions, duplicate update safety, exact sealing, DraftChanged, GenerationSealed, duplicate Send identity and recovery;
- team router tests for explicit Session sharing, shared-session open, unauthorized non-discovery, draft update isolation, submitter-owned account selection and duplicate Send no-double-run;
- browser model tests for private-buffer publication choice and lazy collaboration loading;
- actual two-browser collaboration smoke if the connected environment can execute it;
- solo feature-off regression: no collaboration route/timer/document allocation in the explicit no-multiplayer feature cut;
- warning-clean minimal/default/full affected feature cuts;
- whole-workspace tests and source/reference validation before completion.

A screenshot is not evidence of convergence or exact-send correctness.

## Executed closeout evidence — 2026-10-03

The implemented Phase-38 bytes were validated locally with the following evidence:

- `python3 the removed test_phase38_multiplayer helper: **109 / 109** source/authority/schema checks.
- Browser dependency/install/build:
  - fresh `npm ci`;
  - `npm audit --audit-level=high`: **0 vulnerabilities**;
  - Vite production build green;
  - multiplayer remains a lazy `chunk-multiplayer.js` rather than an eager solo-path import.
- Browser model suite: **102 / 102** tests green, including frozen Yjs/Yrs V1 cross-language fixtures and private-draft publication consent.
- `aw-collab`: convergence, Unicode and frozen Yjs/Yrs V1 materialization tests green.
- `aw-store --features multiplayer`: focused shared-draft regressions green; inherited context/resource/store suites remain green.
- Authenticated team router suite: **35 / 35** tests green, including non-discovery before share, dedicated shared open, ordinary-prompt bypass rejection, exact sealing, duplicate Send, presence and revocation.
- Actual compiled `aw team serve` smoke with two independent authenticated HTTP cookie jars and a fake provider: **8 / 8** checks green. It proves one explicit shared Session, one Yjs draft, presence without inference, submitter-owned `bob-only` account execution, duplicate Send -> one provider request/run, late-edit rejection, and revocation.
- Warning-denied Cargo cuts are green for:
  - `aw-collab`;
  - explicit `--no-default-features` tiny agent;
  - `--no-default-features --features team,multiplayer`;
  - normal `--features full`.
- Entire Cargo workspace/all-targets suite is green with `RUSTFLAGS=-D warnings`. On this NixOS host the pre-existing external-tool fixtures require the documented overrides `AW_GIT=/run/current-system/sw/bin/git` and `AW_BASH=/run/current-system/sw/bin/bash` because `/usr/bin/git` and `/bin/bash` are absent.
- Entire workspace doctest suite is green with warnings denied.
- Pinned immutable-store Rust/Clippy evidence:
  - `rustc 1.98.1`;
  - `clippy 0.1.98`;
  - full `cargo clippy --locked --workspace --all-targets -- -D warnings` green.
- `RUSTDOCFLAGS=-D warnings cargo doc --locked --workspace --no-deps`: green.
- Full source-bound `reference` validation profile: green, including all inherited Phase 14–37 reference checks plus Phase 38.
- Final static source validation: **876 active files**, **33 workspace manifests**, **94 Python sources**, **38 TOML files**, **104 JSON files**, **18 JavaScript sources**, **739 authored Rust test attributes**, **704 local links**, **0 errors**, **0 warnings**.
- `git diff --check`: green.

Evidence boundaries remain explicit:

- this connector has a read-only `/nix/store`; `nix build .#release-all` and the five-target release graph were **not** executed here and are not claimed;
- the actual collaboration smoke used two authenticated HTTP clients, not a rendered two-browser visual test;
- no real-human ChatGPT OAuth authorization round trip was exercised;
- no GitHub Release workflow was executed;
- no push was performed.

## Review boundary

Phase 38 ends at the narrow collaborative composer. Do not begin pipeline/routing/autonomy work as part of this phase. Do not move `phase-37`, and do not push unless explicitly requested.
