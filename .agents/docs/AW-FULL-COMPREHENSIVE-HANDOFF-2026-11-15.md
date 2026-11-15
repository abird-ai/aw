# AW full comprehensive handoff — 2026-11-15

Author: coordinating implementation agent
Owner: @pvl
Prepared: 2026-11-15
Canonical source host: **L5 only**
Canonical checkout: `/home/pvl/spaces/abird/src/aw`
Branch: `main`
Current pushed HEAD / `origin/main`: `62b8da6babfa729d6bbb9c21e75ac89c228b22b1`
Current source version: `0.15.0`

> This file is the self-contained current handoff for the AW runtime, shared tools,
> Workspace views, delegated browser authority, PTY custody, V1.5 UX, remote
> Workspace trust/projection/target plumbing and the exact live two-node boundary.
>
> It intentionally preserves the historical design line that led here rather
> than treating the current source tree as context-free.
>
> **Authority order:** live source/tests/executed results > current `AGENTS.md` >
> current plan queue/TODOs > current UX authority for UI/UX > current subsystem
> specs > this handoff > older dated handoffs.
>
> Source/tests/executed results always outrank prose if they disagree.

---

## 0. Immediate operating rules

These are not suggestions.

1. Use **`dotlink---l5` only** for source/implementation work.
2. Do **not** use A1.
3. Work only in `/home/pvl/spaces/abird/src/aw`.
4. Do **not** create another worktree.
5. Preserve all unrelated dirty/staged work.
6. Do not reset/stash/reconstruct someone else's work.
7. Do not commit, push, tag, release or deploy unless explicitly authorized.
8. Signed history must remain signed; never disable commit signing.
9. Keep each model/user implementation turn to roughly **30 minutes**.
10. Aim for the next coherent checkpoint, update durable docs, then pause.
11. For UI/UX, `.agents/plans/ux/**` is the user's explicit **source of truth**.
12. UX authority is **read-only**:
    - never write there;
    - never stage/reset/stash/commit it;
    - never regenerate its previews as implementation work.
13. V1.5 is the current UX target. V0–V1.4 are design lineage and should be
    consulted when inherited behavior changes.
14. View/layout selection never grants execution, Files/Git, Agent or PTY
    authority.
15. A running Agent/Session/PTY is never retargeted merely because the user
    changes what they are viewing.

### Current repository state at handoff creation

Pushed:

- `62b8da6` — `Complete remote Workspace target integration`
- `aaab161` — `Complete workspace views UX and remote trust`
- `c9aaf0e` — `Complete delegated browser authority and surface lifecycle`
- `88cd965` — `Record finalized unified runtime pins`
- `0a6c6a0` — `Complete unified Jobs and Agent runtime`

Current working tree is intentionally **uncommitted** on top of `62b8da6`.

At handoff finalization the dirty/untracked paths are:

- `.agents/docs/AW-FULL-COMPREHENSIVE-HANDOFF-2026-11-15.md`
- `.agents/docs/CURRENT_HANDOFF.md`
- `.agents/plans/workspace-views/README.md`
- `.agents/plans/workspace-views/NEXT-AGENT.md`
- `.agents/plans/workspace-views/REMOTE-DEVICE-TRUST.md`
- `.agents/plans/workspace-views/TODO.md`
- `crates/aw-surfaces/src/lib.rs`
- `src/main_full.rs`
- `src/resources_cli.rs`
- `src/web.rs`
- `src/web/agent_view_tests.rs`
- `src/web/team.rs`

Nothing is staged. There is one worktree. `.agents/plans/ux/**` has no diff.

The current uncommitted continuation contains **two accepted product changes**:

1. production remote-view **server CLI enablement**;
2. explicit browser-visible origin authority for **SSH-forwarded web access**.

Do not discard either.

---

## 1. Exact reading order for the next agent

Before editing anything:

1. `AGENTS.md`
2. `.agents/plans/README.md`
3. `.agents/plans/TODO.md`
4. **this file**
5. `.agents/docs/CURRENT_HANDOFF.md`
6. `.agents/plans/workspace-views/README.md`
7. `.agents/plans/workspace-views/TODO.md`
8. `.agents/plans/workspace-views/NEXT-AGENT.md`
9. `.agents/plans/workspace-views/REMOTE-DEVICE-TRUST.md`
10. `.agents/plans/workspace-views/DELEGATED-BROWSER-AUTHORITY.md`
11. `.agents/plans/workspace-views/NATIVE-PTY-CUSTODY.md`
12. `.agents/plans/workspace-views/ADOPTED-CHILD-LIFECYCLE.md`
13. `.agents/plans/workspace-views/LIVE-PTY-MIGRATION.md`
14. `.agents/docs/UNIFIED-JOBS-AGENT-FINAL-HANDOFF-2026-11-14.md`
15. `.agents/docs/WORKSPACE-VIEWS-UX-COMPREHENSIVE-HANDOFF-2026-11-14.md`
16. `.agents/docs/WORKSPACE.md`
17. `.agents/docs/BROWSER-WORKSPACE.md`
18. `.agents/docs/AWNET.md`
19. `.agents/docs/FLEETS.md`
20. live source/tests for the exact owner you intend to touch.

For UI/UX changes additionally read, read-only:

- `.agents/plans/ux/README.md`
- `.agents/plans/ux/V1.5-UNIFIED-FOCUS-AND-TABS.md`
- `.agents/plans/ux/NAVIGATION-AND-STATE.md`
- `.agents/plans/ux/INTERACTIONS-AND-ACCEPTANCE.md`
- `.agents/plans/ux/VISUAL-SPEC.md`
- `.agents/plans/ux/REFERENCES-AND-VISUAL-INVENTORY.md`
- `.agents/plans/ux/TERMINAL-V1.5-CELL-STUDY.md`
- relevant v1.5 reducer/prototype/tests.

For unified Jobs/runtime changes read the complete normative plan:

- `.agents/plans/ai-native-unified-jobs-agent-runtime-2026-11-11/README.md`
- `DESIGN.md`
- `TODO.md`
- `NEXT-AGENT.md`

Do not reopen that runtime merely because this handoff mentions it; it is
finalized unless live source/tests prove a regression.

---

## 2. Permanent architecture decisions from the original AW program

These decisions predate the current Workspace work and remain structural.

### 2.1 One product/runtime owner

AW has:

- one user-facing `aw`;
- one original Agent engine;
- one TurnMachine;
- one ToolRegistry/ToolPolicy path;
- one authoritative Session/State owner;
- one TurnJournal / intent-before-effect path.

Native Workspace, browser Workspace, SDK/headless and remote views are
**projections over the same owners**, not alternate runtimes.

### 2.2 User model

Canonical user-facing resource model:

`Domain → Workspace → Session`

Machines/Fleets are execution/resource topology, not a replacement for the user
model.

### 2.3 Authority principles

Keep these distinctions explicit:

- identity vs membership;
- account vs payer;
- credential custody vs inference executor;
- Session owner vs participant;
- view authority vs execution authority;
- Process custody vs permission to send new input;
- browser authentication vs resource grant;
- path/label/tab/ticket identity vs actual resource authority.

Never infer one from another.

### 2.4 Effects and recovery

- Journal intent before model/tool effects.
- Preserve factual paid/effect results even if later attachment fails.
- Ambiguous or possibly charged effects remain unknown/inspectable.
- Never silently replay an uncertain effect.
- No silent provider/account/model/tool/network fallback.
- Exact Actor/source/site/effect/writer/grant generations are authority.
- Revocation changes future admission/disclosure; it does not rewrite factual
  historical effects.

### 2.5 Views do not retarget execution

This is foundational and now directly drives V1.5 ActiveTarget:

- switching Workspace tabs is presentation;
- maximizing a pane is presentation;
- selecting another Machine target is presentation;
- opening a File/Diff is presentation;
- none of these may retarget the current Agent Session, model, payer, tool
  executor, or PTY owner.

---

## 3. Historical lineage — September/October foundations

Older full detail remains in:

- `.agents/docs/HANDOFF.md`
- `.agents/docs/AW-COMPREHENSIVE-HANDOFF-2026-10-19.md`
- `.agents/plans/archive/history/*`

The important inherited sequence:

### 3.1 Durable runtime and Store

Historical phases established:

- durable SQLite/JSONL Session/Turn/journal ownership;
- one writer/owner model;
- domains/workspaces;
- authenticated local direct access;
- writer epochs and recovery fencing;
- device identity and portable identity continuity;
- SecretStore and provider-account authority;
- remote Machine/Fleet/Domain primitives.

### 3.2 AwNode and networking lineage

Phases 14–30 established the substrate now reused by remote Workspace views:

- QUIC pairing;
- direct reachability/rendezvous/STUN;
- route fallback;
- MachineLink signed capability lease;
- remote read services;
- remote Sessions/Fleet/federation;
- workspace replica metadata;
- authority failover/writer epochs;
- Phase-30 stable identity continuity and portable device proof.

The current remote-view work deliberately extends these primitives rather than
inventing a browser networking protocol.

### 3.3 Native Agent presentation lineage

Historical phases 51–58 and later unnumbered hardening established:

- Agent vs Workspace application separation;
- Minimal/Inline/Scrollback/Fullscreen native presentation;
- terminal lifecycle restoration;
- natural-flow composer geometry;
- shared markdown/syntax rendering;
- nonblocking UI work graph;
- bounded viewports/find/selection;
- resource provenance.

The current V1.5 browser Workspace reuses the same architectural distinction:
presentation is separate from execution.

---

## 4. Shared native tools and dotlink convergence

### 4.1 Why this matters to current work

AW and dotlink once had overlapping native filesystem/Graph/Job implementations.
The design was consolidated so remote/local execution does not diverge.

### 4.2 Canonical boundary

Current direction:

- `aw-tools` = canonical shared filesystem/Graph/finite Job semantics/runtime;
- `aw-tools-adapter` = AW-specific projection/compatibility;
- platform crates own no-follow/bounded/atomic filesystem primitives;
- AW Agent/ToolPolicy/TurnMachine remain the authority/journal owner;
- dotlink consumes the shared implementation through a thin product adapter.

### 4.3 Filesystem/Process invariants

Inherited from DL-150/shared convergence:

- compact flat filesystem tools;
- deterministic Graph `ops`;
- one JobManager;
- bounded retained output;
- exact replay/request-key semantics;
- explicit ACK where ownership release matters;
- no symlink/reparse widening;
- atomic per-file publication;
- exact Process launcher/source/sandbox identity;
- no model-visible media/sandbox claims not actually supported.

### 4.4 Important commit anchors

- `75f6f00` — shared tool runtime convergence
- `e337f0d` and following hardening commits — AI-native FS/core convergence
- dotlink final unified bridge: `c90a6603ccef0bbf626ce10d95bedf59877747b8`
- dotlink pins AW shared tools to AW `0a6c6a0`

Do not recreate a second tools core.

---

## 5. Unified Jobs + Agent runtime — finalized November 2026

### 5.1 Design authority

Normative design checkpoint:

`890e9a383fe2edc3e0bd0aa34a2b217f2724475c`

Final AW runtime:

`0a6c6a0bea07a3886c4dc8be4f423eec3ea3c134`

Final AW handoff sync:

`88cd9655d402ea325873470202fd12133b1d8199`

Final dotlink bridge:

`c90a6603ccef0bbf626ce10d95bedf59877747b8`

### 5.2 Canonical runtime model

The final model is one unified `jobs.*` surface.

Kinds include:

- Process;
- Graph;
- Agent.

Old public models such as independent `subagent/team/plan/review_release/job_*`
routes were removed/hard-cut over and must not be resurrected.

### 5.3 Identity namespace

Canonical contextual identity:

`/d:<id>/i:<id>/p:<id>/a:<id>`

with contextual aliases such as local root/current/parent where authorized.

### 5.4 Control model

Unified operations include:

- list/get/read;
- send/steer/interrupt;
- cancel;
- wake/subscribe semantics;
- graph/node execution;
- Agent child/peer coordination.

Agent self-continuation is zero-ref/current Agent, not a separate subagent
system.

### 5.5 Authority and replay

Important permanent decisions:

- queued Agent input is custody, not future inference authority;
- admission is revalidated before each paid Turn;
- all autonomous continuation retains a finite causal budget;
- Process stdin is current-authority gated against the original sandbox;
- replay/redelivery is checked before mutable topology/policy drift where the
  original effect is already authoritative;
- ACK/read/output ownership semantics remain exact.

### 5.6 Current status

**Complete.**

Do not reopen unless a concrete live regression forces it.

---

## 6. Workspace views — original product sequence

The Workspace-view effort began in October and accumulated in deliberate slices.

### 6.1 Native read-only Agent views

Key commit:

`1c2ec6`

Decisions:

- exact Session/branch read without minting a SessionHandle;
- no Agent/Session creation;
- exact Store/Session/branch identity carried through projection;
- independent scroll/follow/find/selection;
- bounded read state;
- revocation scrubs protected presentation;
- opening a view does not retarget current Agent.

### 6.2 Native scoped Files

Key commit:

`3851dbf`

Decisions:

- Files containers own exact opened scope/root capability;
- path/tab labels are not authority;
- reconnect uses retained physical root identity;
- per-document Find/viewport/selection;
- source replacement/revocation scrubs data;
- no implicit new Files root.

### 6.3 Native scoped Git

Key commit:

`b33a418`

Decisions:

- independent Git contexts/worktrees;
- opaque inventory IDs;
- immutable Review/Diff identity;
- moved/open Diffs remain on originating checkout;
- Git mutation admission stays serialized;
- explicit terminal launch validates exact Git context.

### 6.4 Browser Agent views

Key commits:

- `a0ffa3e`
- `68629d8`
- `94697a7`

Important behavior:

- exact read-only branch views;
- one retained Store owner;
- private drafts bound to owner/Session/branch identity;
- external retarget preserves draft/caret and blocks send until explicit consent;
- held/stale responses cannot resurrect closed/revoked DOM;
- revocation scrubs transcript/find/selection/labels.

### 6.5 Browser scoped Files/Git

Key commits:

- `57a6059`
- `0159f0b`

Behavior:

- one exact source/scope envelope;
- Files reads tied to retained physical root;
- Git inventory/open/review tied to exact repository/worktree identity;
- independent navigator state;
- stable per-document editors;
- stale/oversized/wrong-source responses refused;
- 390px merged Diff scrolling accepted;
- no execution effects.

---

## 7. Document docking and browser lifecycle

### 7.1 Same-window reparenting

`e06ffa7` established exact document identity when moving/reparenting tabs.

Presentation state remains presentation, not authority.

### 7.2 Cross-window same-owner docking

Key sequence:

- `7de8658` — exact File docking
- `00559eb` — immutable Review docking
- `41e66c4` — bounded editor presentation transfer
- `e558919` — receiver proof before release
- `0efea35` — independent browser profiles paired under same owner

Transfer lifecycle became factual:

prepare → receive/read → accept → confirm → heartbeat/release

Source closes/releases only after the receiver has proved acceptance.

### 7.3 Distinct-user delegated authority

Final commit:

`c9aaf0e0d311ba905f55875147392a071f64b037`

This is **not** same-owner pairing.

It adds exact owner→recipient delegated grants with:

- exact recipient;
- exact File or immutable Review;
- security epoch/generation;
- expiry;
- revoke/cancel;
- held-response publication revalidation;
- explicit source/receiver lifecycle.

Recipient does **not** receive:

- Files root;
- Git inventory/write;
- terminal;
- Site;
- provider/model/tool authority;
- re-delegation authority.

Ordinary Team `/api/workspace` remains disabled.

Delegated File/Review is retained independently of ordinary source replacement.
A delegated 403 scrubs only that exact grant. Browser-auth 401 scrubs protected
state globally for that browser lifetime.

---

## 8. PTY custody and original-shell migration

### 8.1 Persistent PTYs

The design moved from visual terminal panes to broker-owned retained PTYs.

Invariants:

- PTY process exists independently of a frontend pane;
- viewer vs input owner are explicit;
- input/resize ownership is leased/fenced;
- selecting or maximizing a pane does not create a PTY or take ownership;
- explicit stop is effectful.

### 8.2 Same-UID Linux original-shell migration

Key commit:

`9d82f1a`

Supporting plans:

- `NATIVE-PTY-CUSTODY.md`
- `ADOPTED-CHILD-LIFECYCLE.md`
- `LIVE-PTY-MIGRATION.md`

Implemented/accepted:

- same-UID Linux original-shell custody transfer;
- actual live PTY migration;
- source release/receiver adoption;
- parser/history continuity;
- pidfd lifecycle;
- exact release/escrow/ACK state;
- actual different-UID FD admission refusal.

Do **not** reimplement `terminal-promote`.

### 8.3 What is not claimed

Still separate/open:

- remote/distinct-Actor PTY custody;
- macOS/Windows live custody;
- broker SIGKILL/reboot restoration;
- actual Windows retained-directory runtime acceptance.

---

## 9. V1.5 UX authority and implementation

### 9.1 Authority

`.agents/plans/ux/**` is the user's explicit UI/UX authority.

Current target: **V1.5**.

Core V1.5 rules:

- exactly two presentation modes: Minimal and Workspace;
- Minimal is truly headerless;
- Agent is a Workspace tab/pane, not a third mode;
- maximize/restore is reversible presentation state;
- contained Files/Git use Preview;
- Break out creates standalone File/Diff;
- maximize affects existing pane;
- one title owner; no duplicate nested headers;
- standalone identity is canonical site/scope/resource, not filename;
- one existing command palette;
- prefixes remain `> ! @ # $ / &`;
- Ctrl+Shift+P / F1 opens palette;
- Ctrl+P stays composer/PTY-owned;
- view selection never grants authority;
- pane focus differs from PTY input/resize ownership.

### 9.2 Visual convergence work

Pushed checkpoint:

`aaab161`

The implementation was iterated against actual Chrome screenshots and the
canonical V1.5 previews.

Major corrections included:

- remove old permanent brand/New/search-heavy rail;
- compact Space/project/branch header;
- Projects upper sidebar;
- Activity/Git/Files lower dock;
- saved-layout/document tab strip;
- compact bottom status;
- true Minimal mode;
- contained Files preview;
- contained Git immutable Diff preview;
- standalone File/Diff;
- immersive maximize;
- command palette visual convergence;
- light-theme parity;
- responsive 390×844 geometry.

### 9.3 Review saved layout

A real bug was found:

- Review tab existed but `<ReviewLayout>` was never mounted.
- Review also lacked shipping grid CSS.

Fixed layout:

top:
- Agent
- immutable Diff

bottom:
- real retained PTY

Each pane supports local maximize/restore.

### 9.4 Build / terminal grid

Build is now a non-closable saved layout over existing retained PTYs.

Behavior:

- merely opening Build starts **zero** PTYs;
- `+ Terminal` / explicit Start are the only creation paths;
- 1 PTY → full pane;
- 2 → two columns;
- 3–4 → 2×2;
- narrow width → scrollable vertical stack;
- per-tile Take/Release input;
- explicit two-step Stop;
- per-tile maximize/restore.

Important custody proof:

- one claimed PTY remained sole input owner through maximize/restore;
- it remained owner after **17 seconds hidden**, beyond the 15-second lease TTL;
- all four PTY IDs remained unchanged.

### 9.5 Responsive acceptance

A real narrow CSS grid bug was fixed:

- previous main pane at 390px was only 266px due implicit tracks;
- after fix Review/Files/Build use full 390px;
- page-wide overflow = 0.

### 9.6 Browser test state

Accepted local V1.5 browser/controller matrix reached:

- **83/83** before ActiveTarget work;
- **85/85** after ActiveTarget/follower/pin regressions.

---

## 10. SSH-forwarded browser origin authority

This is part of the **current uncommitted tree** and must be preserved.

Problem:

AW may listen remotely on `127.0.0.1:8787` while the user reaches it through:

`ssh -N -L 18787:127.0.0.1:8787 host`

The backend listen address and browser-visible authority are therefore different.

### 10.1 New contract

- `--listen` controls the numeric loopback backend socket.
- optional `--browser-origin http://LOOPBACK[:PORT]` controls exact accepted
  browser Host/Origin.

Example:

`aw web --listen 127.0.0.1:8787 --browser-origin http://127.0.0.1:18787`

paired with:

`ssh -N -L 18787:127.0.0.1:8787 ...`

### 10.2 Security invariants

The override accepts only:

- HTTP;
- loopback host (`localhost`, `127/8`, `::1`);
- no credentials;
- no path;
- no query;
- no fragment.

Host/Origin remain exact.
Cross-site requests remain denied.
Cookie names derive from browser-visible authority so multiple forwarded local
ports do not collide.

Omitting `--browser-origin` preserves listener-derived behavior.

### 10.3 Evidence

Recorded current regressions:

- browser-origin parsing/authority: **2/2**;
- existing Host-rebinding/wrong-Origin-port checks remain green;
- CLI parsing green;
- full product check / strict Clippy / format / diff green.

---

## 11. Remote-device trust — schema v35

### 11.1 Why v35 exists

Pairing/MachineLink alone identifies a transport peer but is not sufficient
authority for a reconnectable remote Workspace view.

Schema v35 adds durable `remote_view_grants`.

### 11.2 Grant binds

An exact grant pins:

- Domain;
- exact hosted Workspace;
- Workspace revision and policy revision;
- target local Machine ID/node/security epoch/capability revision;
- requester remote Machine ID/node/security epoch/capability revision;
- paired AwNet peer ID/node/security epoch/public key;
- stable user ID/principal/security epoch;
- Domain policy/security epoch;
- portable device binding/device/public key;
- bounded Files/Git permission set;
- issued/expiry/revoke times;
- monotonic grant revision.

### 11.3 Issue requirements

Grant issue requires intersection of:

- active device-key authenticated stable user;
- active user/security epoch;
- active Domain/security epoch;
- active membership;
- active portable device binding;
- active continuity-source trust;
- trusted peer;
- peer explicitly registered as an active remote Machine in same Domain;
- active target local Machine;
- exact active hosted Workspace on that Machine;
- bounded permission set;
- bounded TTL.

Pairing, path, label, browser auth, MachineLink or grant ID alone are never
enough.

### 11.4 Revalidation

Every protected authorization rechecks current state.

Any of these fence future reads:

- user epoch change;
- Domain policy/security change;
- membership loss;
- continuity source revoke;
- device revoke;
- peer revoke/epoch change;
- requester Machine change;
- target Machine change;
- Workspace revision/policy change;
- grant explicit revoke;
- expiry.

Reconnect creates no authority; it only reuses the same grant if every pinned
fact remains current.

### 11.5 Explicit revoke

Grantee or current Domain owner/admin can revoke the grant.

Upstream revocation need not rewrite the grant row; authorization simply stops
succeeding.

### 11.6 Evidence

Early backend checkpoint:

- Store remote-view tests **4/4**;
- v34→v35 migration preserves Store ID and quick-check;
- strict Store/CLI checks green.

---

## 12. Protected identity-auth and Machine read protocol

### 12.1 Additive identity-auth versions

Legacy identity auth remains wire **v2**.

Remote-view grant issuance uses additive wire **v3** only when requested.

The signed v3 transcript additionally binds:

- Workspace ID;
- exact sorted Files/Git permission set;
- requested view TTL.

No remote-view request means old v2 JSON/transcript remains compatible.

### 12.2 Protected Machine reads

Legacy Machine reads remain wire **v1**.

Narrow protected `ViewFile*` / `ViewGitStatus` use wire **v2**.

Full remote Workspace projection uses wire **v3**.

### 12.3 Fresh proof per read

Protected reads require:

- MachineLink Files/Git capability;
- `IdentityAuthenticate` capability;
- exact persisted grant;
- exact device key;
- fresh server nonce/signature.

The signature binds:

- MachineLink lease;
- request ID;
- grant ID;
- Domain;
- Workspace;
- target node/store;
- requester node;
- exact serialized request digest;
- fresh nonce.

The server reauthorizes the grant **again immediately before response
publication**.

### 12.4 Downgrade fence

Remote-view service mode rejects legacy ungranted reads.

Ordinary Machine service rejects protected `View*` reads.

A client cannot negotiate broad read capabilities and then bypass the grant by
sending an older operation.

---

## 13. Remote Workspace v3 projection parity

Pushed in `62b8da6`.

### 13.1 Full read-only request family

Remote Workspace wire v3 supports the same read contract used by local
Workspace:

- Files list;
- File search;
- File read;
- Git list;
- Git open;
- retained immutable Review by exact ID;
- permission-scoped capability query.

### 13.2 Do not duplicate Git semantics

The server full build adapts a retained `Arc<SurfaceHost>` through a
UI-neutral `RemoteWorkspaceProjection`.

Therefore remote views reuse:

- opaque Git inventory IDs;
- exact worktree/root identity;
- immutable Review IDs;
- bounded view envelopes;
- same source/scope checks.

Do not write a second remote Git engine.

### 13.3 Capability projection

Remote capability query is permission-scoped.

It always keeps:

- `git_write=false`;
- `terminals=false`;
- empty Sites;
- zero terminal capacity.

A Files-authorized query cannot advertise Git.
A Git-authorized query cannot advertise Files.

### 13.4 Typed read errors

Projection errors remain typed read results:

- source changed;
- scope changed;
- denied;
- invalid;
- conflict;
- limit;
- unavailable.

Do not collapse all authorization/snapshot errors into generic transport loss.

### 13.5 Evidence

- projection-focused tests: **5/5**;
- real temp Git proves:
  `GitList(changes) → GitOpen → retained Review(same ID)`;
- AwNet later reached **34/34**.

---

## 14. Trusted remote target admission

Pushed in `62b8da6`.

### 14.1 Secret-bearing admission object

`RemoteWorkspaceTargetAdmission` consumes host-only:

- active server-issued v35 grant;
- trusted `PunchOptions` route;
- private device `SigningKey`.

It is deliberately non-serializable and redacts Debug.

### 14.2 Local checks

Admission rechecks:

- route peer == granted target node;
- current local AwNode == grant requester node;
- current paired peer trust;
- active registered remote Machine in exact Domain;
- Domain policy/security;
- stable user/security;
- supplied key matches grant;
- key is an active local identity device;
- grant active/unexpired.

### 14.3 Browser-visible descriptor

Admission mints fresh opaque ID:

`rwt_<random>`

Browser-visible descriptor contains only:

- id;
- kind;
- label;
- files;
- git;
- expires_at.

Never expose:

- peer/node IDs;
- room/rendezvous;
- Workspace ID;
- grant ID;
- device key/public key;
- route config.

---

## 15. Owner-private remote target loader and registry

Pushed in `62b8da6`.

### 15.1 CLI

Repeatable:

`aw web --web-remote-target OWNER_PRIVATE_JSON`

Only supported by local single-user web mode with durable state.

### 15.2 Config custody

Each config:

- platform-verified owner-private;
- version 1;
- bounded to 64 KiB;
- absolute device-key path.

Schema:

{
  "version": 1,
  "peer_node_id": "node_...",
  "room": "<64 lowercase hex>",
  "rendezvous": "host:port",
  "domain_id": "dom_...",
  "workspace_id": "ws_...",
  "device_key": "/absolute/private/device-key.json",
  "permissions": ["files", "git"],
  "lease_secs": 60,
  "view_ttl_secs": 600,
  "listen": "0.0.0.0:0",
  "stun": [],
  "timeout_secs": 30
}

Bounds:

- lease: 5..=300 s;
- view TTL: 5..=3600 s;
- timeout: 1..=300 s.

### 15.3 Startup behavior

Before network:

- peer must still be trusted;
- exact Domain active;
- peer active registered remote Machine in Domain;
- supplied device key maps to active local identity device.

Then real Phase-30 device-key identity-auth obtains a fresh server-issued v35
grant.

The admitted target enters a binary-private registry keyed by opaque target ID.

### 15.4 Registry behavior

Per descriptor/action:

- revalidate local device/Domain/peer/Machine authority;
- derive exact Files/Git MachineLink capability + IdentityAuthenticate;
- execute v3 reader;
- capability merge requires Files/Git to report identical source/root;
- safe Denied/Conflict/Limit map to Workspace states;
- transport/protocol/storage detail collapses to generic unavailable.

### 15.5 Web boundary

The web library remains AwNet-independent.

It owns only:

- secret-free descriptor;
- `RemoteWorkspaceTargetRouter` trait.

Binary implementation captures all route/grant/key secrets.

Endpoints:

`POST /api/workspace/target/capabilities`

body:

{"target_id":"rwt_..."}

`POST /api/workspace/target/action`

body:

{"target_id":"rwt_...","command":{...typed View...}}

Only `View` commands are accepted.
Non-View → 403.
Unknown extra route fields are rejected by strict JSON decoding.

---

## 16. ActiveTarget and follower/pinned resources

Pushed in `62b8da6`.

### 16.1 ActiveTarget

Browser V1.5 supports:

- Local;
- opaque remote Machine target.

Remote descriptor discovery is passive.
No remote connection/read occurs merely because a target appears.

### 16.2 Explicit remote selection

Remote target selection:

1. calls only opaque target capability endpoint;
2. receives exact remote source/root capabilities;
3. retargets only Files/Git follower views.

It does **not** touch:

- Agent Session;
- model/provider/payer;
- unsent draft/caret;
- Review/Build PTY inventory;
- PTY input/resize owner;
- existing pinned standalone resources.

### 16.3 FollowActiveTarget containers

On target switch:

- snapshot Files/Git container presentation;
- preserve kind/query/tree-vs-list/Git mode/expansion/collapse;
- replace read source;
- recreate allowed followers on new root;
- clear contained File/Diff preview.

Never silently reopen “same path” on a different Machine.

### 16.4 PinnedResource standalone documents

Break out/direct standalone File/Review:

- captures exact current source/scope;
- installs a per-document reader bound to target transport;
- cached immutable data survives later target switches;
- explicit refresh uses original target;
- cannot move into a new follower scope.

If closed back into a still-live original contained preview, it is explicitly
unpinned.

### 16.5 Reconnect / loss behavior

Previously selected remote target may re-establish only if its opaque descriptor
is still advertised.

If it disappears:

- follower source is cleared;
- loss is shown;
- no silent Local fallback.

### 16.6 Evidence

- production Vite build green;
- focused browser/controller suite **85/85**;
- exact-source pin survival regression;
- full product check;
- strict Clippy;
- format/diff green.

A fresh embedded Chrome remote-target selector acceptance has **not** yet been
established; do not reuse older screenshots as proof.

---

## 17. Production server CLI enablement — current uncommitted continuation

This is the immediate latest implementation and is **not yet committed**.

### 17.1 Why it was needed

Client-side remote target plumbing was complete, but production server commands
still hardcoded:

- identity-auth grant issuance disabled;
- Machine-service remote Workspace projection disabled.

Therefore a real two-AwNode proof could not yet be launched through production
commands.

### 17.2 Identity-auth server policy

`aw net identity-auth-serve` now gains:

- `--remote-view-max-ttl SECONDS`
- repeatable `--remote-view-grant files|git`

TTL and surface policy require each other.

Negotiated capability lease:

- always `identity-authenticate`;
- adds `file-read` only when Files is explicitly allowed;
- adds `git-read` only when Git is explicitly allowed.

No automatic broad grant.

### 17.3 Machine service server policy

`aw net machine-service-serve` now gains:

`--remote-view-workspace WORKSPACE`

When supplied it requires explicit:

- `--grant identity-authenticate`;
- at least one of:
  - `--grant file-read`
  - `--grant git-read`

Before MachineLink:

- remote-view mode accepts only the exact opaque Workspace ID and rejects a
  simultaneous Domain selector;
- Workspace is active;
- Workspace kind = hosted;
- host_machine_id == current local Machine.

Then server creates one retained read-only `SurfaceHost` over host-selected
`--root`, installs `SurfaceRemoteWorkspaceProjection`, and remains resident
across fresh MachineLink transports so exact source/scope plus retained
inventory/Review IDs have one server-process lifetime.

Legacy mode with no flag remains unchanged.

### 17.4 Read-only SurfaceHost shell invariant

`SurfaceHost::new` now requires absolute shell only if
`allow_terminals=true`.

Remote read-only projection can therefore exist with:

- exact root;
- trusted absolute Git;
- no fake shell path;
- terminals disabled;
- Sites empty.

### 17.5 Evidence

Focused:

- typed server CLI opt-ins pass;
- exact MachineLink grant policy pass;
- remote-view service downgrade fence pass;
- read-only shell invariant pass.

Full:

- `aw-surfaces` **35/35** with trusted `AW_GIT` / `AW_BASH` test environment;
- isolated full-product `cargo check` pass;
- strict Clippy `-D warnings` pass;
- format/diff pass.

An initial unpinned `aw-surfaces` run had 8 immediate failures because the
existing suite requires `AW_GIT`. Re-run with the documented trusted environment
passed 35/35; do not misreport the unpinned run as a product regression.

---

## 18. Browser listen vs browser-visible origin — current uncommitted continuation

Also preserve this uncommitted work.

### 18.1 Problem

Backend may listen on one loopback port but browser reaches another loopback port
through SSH forwarding.

Previously authority was derived only from listener address, causing exact Host/
Origin checks to reject the legitimate forwarded browser.

### 18.2 New API

`aw web`:

- `--listen SOCKET` = backend listener;
- `--browser-origin http://LOOPBACK[:PORT]` = browser-visible exact authority.

### 18.3 Security

Browser-origin parser allows only:

- HTTP;
- loopback;
- no credentials/path/query/fragment.

The explicit browser authority replaces listener authority for Host/Origin
validation; listener authority is no longer also accepted.

Cookies are namespaced from browser-visible authority.

### 18.4 Tests

Current uncommitted regressions include:

- exact loopback origin parsing;
- forwarded port accepted;
- listener-origin refused when explicit override exists;
- pair endpoint works under forwarded origin;
- cookie name differs from backend-port default;
- Team/browser fixtures receive explicit resolved browser authority.

Do not loosen Host/Origin/Sec-Fetch checks to “fix” forwarding.

---

## 19. Validation/evidence matrix

### Unified runtime

- final AW: `0a6c6a0`
- final dotlink: `c90a660`
- cross-repo design: `890e9a`
- finalized; no open implementation TODO.

### Workspace/native/browser baseline

Historical accepted evidence includes:

- native Agent/Files/Git real PTY checks;
- exact root/worktree identity tests;
- browser Chrome acceptance;
- bounded Find/scroll/selection;
- zero unintended execution rows.

### Linux PTY migration

Recorded in current Workspace TODO:

- live migration parser/vim/less/source-crash **4 passed**;
- migration protocol **26 passed**;
- persistent PTY reconnect/competition/etc **5 passed**;
- terminal target **55 passed**.

### Delegated browser authority

Accepted at `c9aaf0e` with real two-profile browser evidence and exact
grant/lifecycle tests.

### V1.5 / Build

- focused controller/browser suite reached **83/83**;
- after ActiveTarget source regressions: **85/85**;
- live V1.5 screenshot matrix accepted for local implemented surfaces;
- Build real PTY custody proof accepted.

### Remote trust/projection

- Store grant tests **4/4**;
- projection-focused **5/5**;
- all AwNet evolved through 24/29/31 to current **34/34** before server CLI
  enablement;
- target admission **3/3**;
- private registry **2/2**;
- descriptor HTTP **1/1**;
- target action HTTP **1/1**;
- target CLI parse **1/1**.

### Current uncommitted server enablement

- focused server opt-in tests pass;
- full `aw-surfaces` **35/35** with trusted environment;
- isolated full-product check pass;
- strict Clippy pass;
- format/diff pass.

---

## 20. Commit/provenance timeline

Important lineage anchors:

- `5618a81` — planning/checklist working agreement
- `7f652a2` — browser Workspace assets
- `d4ec64e` — one aw CLI over Workspace
- `1c2ec6e` — native read-only Agent views
- `3851dbf` — native scoped Files
- `b33a418` — native scoped Git
- `a0ffa3e` — browser read-only Agent views
- `57a6059` — browser scoped Files/Git
- `e06ffa7` — exact document identity across reparent
- `7de8658` — File docking
- `00559eb` — immutable Review docking
- `41e66c4` — bounded presentation transfer
- `e558919` — receiver-proof release
- `0efea35` — same-owner multi-profile pairing
- `2bd6749` — semantic presentation/focus convergence
- `9d82f1a` — original Linux PTY custody migration
- `75f6f00` — shared tool runtime convergence
- `890e9a3` — final unified Jobs/Agent design authority
- `0a6c6a0` — complete unified Jobs/Agent runtime
- `88cd965` — finalized unified handoff pins
- `c9aaf0e` — delegated browser authority/lifecycle
- `aaab161` — V1.5 Workspace UX + remote trust
- `62b8da6` — remote Workspace target integration

Current uncommitted production server/browser-origin work is on top of
`62b8da6`.

---

## 21. Exact next phase

The real production-authorized two durable AwNode gate has now been executed
through **server-issued v35 admission**, but the first protected read still has
one transport/runtime blocker. The sequence below remains the acceptance plan;
steps 1–7 are established through production commands and step 8 starts the
currently failing boundary.

Latest executed evidence:

- two fresh file-backed durable state roots on L5;
- real direct pair/trust;
- source Domain/device setup and explicit target Machine registration;
- signed Workspace writer handoff A → B;
- target replica explicitly registers only the exact trusted recorded control
  AwNode as requester remote Machine;
- real Phase-30 continuity accept + signed sync;
- real device proof;
- target identity-auth server explicitly grants Files+Git v35 policy;
- real **server-issued v35 grant**;
- `aw web` admission creates an opaque target and browser descriptor leak
  checks passed for peer/room/Domain/Workspace/grant/device-key authority.

The first `/api/workspace/target/capabilities` call then returns factual HTTP
503 unavailable. Diagnosis-only logging, removed before this checkpoint, showed
the direct QUIC route stream reaching `open_bi/accept_bi` but closing before
the first RouteHello bytes: Quinn `ClosedStream` client-side and
`FinishedEarly(0)` server-side.

Isolation is important:

- same reusable room `net punch` succeeds twice;
- plain Machine FileRead succeeds;
- identity-auth then same-room Machine FileRead succeeds;
- two sequential same-room one-shot Machine services succeed;
- after real remote-view v3 grant issuance, clean `aw web` shutdown followed
  by same-state/same-room plain Machine FileRead succeeds.

Therefore do **not** reopen authority design or weaken v35/browser/owner fences.
Continue specifically at the in-web-owner protected-read direct-route/QUIC
stream lifecycle.

### 21.1 Required proof sequence

Use two independent durable state roots on L5.

Do not hand-author an identity continuity snapshot.

The intended production path:

1. create/bootstrap node A and node B;
2. pair/trust them using production AwNet pairing/trust flow;
3. establish shared Domain/membership/Machine registration through production
   resource commands;
4. use Phase-30 continuity export/serve/fetch/accept flow for the stable user;
5. use the real device key on requester;
6. start target `identity-auth-serve` with:
   - exact requester peer;
   - exact Domain;
   - `--remote-view-max-ttl ...`;
   - exact `--remote-view-grant files` and/or `git`;
7. requester obtains server-issued v35 grant via the same path used by
   `--web-remote-target`;
8. start target `machine-service-serve` with:
   - exact requester peer;
   - exact grants;
   - `--remote-view-workspace WORKSPACE`;
   - host-selected `--root`;
9. requester selects remote ActiveTarget;
10. prove:
    - Files list;
    - File search;
    - File read;
    - Git inventory;
    - Git open;
    - retained immutable Review ID;
11. disconnect/reconnect;
12. prove the same still-current grant revalidates and re-read succeeds;
13. revoke exact grant/device/source authority;
14. prove subsequent publication is fenced and browser source becomes
    factual unavailable/stale;
15. prove **no silent Local fallback**.

### 21.2 Preservation assertions during live gate

Record before and after:

- current Agent Session identity;
- active branch/Agent;
- unsent draft and caret;
- provider/model execution target;
- Build/Review PTY inventory;
- PTY input owner;
- PTY resize owner;
- local Git HEAD/index;
- provider/model/tool/run/effect row counts.

Target selection/reconnect/revoke must not change these except for the explicit
remote Files/Git presentation.

### 21.3 Pinned resource assertion

Break out one remote File or immutable Review.

After switching ActiveTarget:

- pin stays on original source;
- refresh still uses original target;
- pin is not moved to new target;
- exact grant revoke scrubs/refuses only that independent authority.

---

## 22. Pending work after the two-node core gate

These are separate boundaries; do not conflate them with the core remote
Files/Git gate.

### 22.1 Remote Agent/member views

Current remote Workspace scope is Files/Git/Review only.

Remote Agent/member view authority still needs its own exact design and grant
chain.

### 22.2 Offline/stale presentation

Need explicit user-facing state for:

- target offline;
- grant expired;
- target revoked;
- route unavailable;
- source/scope changed;
- reconnect in progress.

No silent fallback.

### 22.3 Arbitrary post-ACK process/network partitions

The accepted browser docking lifecycle does not prove every arbitrary partition
after ACK. Keep this separate.

### 22.4 Remote/distinct-Actor PTY custody

Not implemented by the current remote Files/Git work.

### 22.5 macOS/Windows custody + broker reboot

Still separate.

### 22.6 Windows retained-directory runtime

Compilation exists; actual Windows runtime acceptance remains open.

### 22.7 Release/readiness external gates

Release-readiness remains separate and includes external/human gates such as
version approval/signing/notarization/live provider/billing/security review.

---

## 23. Things that must not be changed accidentally

- Do not write `.agents/plans/ux/**`.
- Do not reimplement `terminal-promote`.
- Do not resurrect old subagent/team/job model routes.
- Do not create another Agent runtime for Workspace/browser.
- Do not treat a view as authority.
- Do not treat Team membership/browser auth/path/label/tab/ticket as resource
  authority.
- Do not grant Files root from one File.
- Do not grant Git inventory/write from one Review.
- Do not make same-owner browser pairing equivalent to delegated distinct-user
  authority.
- Do not let a delegated or pinned resource move into an unrelated ordinary
  scope.
- Do not auto re-delegate.
- Do not retarget Agent/model/provider/PTY because ActiveTarget changes.
- Do not silently fall back Local if remote target disappears.
- Do not expose remote route/grant/key material to browser state.
- Do not weaken exact Host/Origin/Sec-Fetch checks for SSH forwarding.
- Do not auto-start a PTY from Build selection/maximize.
- Do not make pane focus imply PTY input/resize ownership.
- Do not claim Windows/macOS/remote PTY custody without those runtime gates.
- Do not claim the real two-node remote-view gate until it actually runs.

---

## 24. Current uncommitted change inventory

At this checkpoint, preserve the complete current dirty set exactly. In
addition to the original production-server/browser-origin continuation it now
includes the production-gate fixes below:

1. `.agents/docs/AW-FULL-COMPREHENSIVE-HANDOFF-2026-11-15.md`
2. `.agents/docs/CURRENT_HANDOFF.md`
3. `.agents/plans/workspace-views/README.md`
4. `.agents/plans/workspace-views/NEXT-AGENT.md`
5. `.agents/plans/workspace-views/REMOTE-DEVICE-TRUST.md`
6. `.agents/plans/workspace-views/TODO.md`
7. `crates/aw-store/src/remote_view.rs`
8. `crates/aw-store/src/resources.rs`
9. `crates/aw-store/tests/resources.rs`
10. `crates/aw-surfaces/src/lib.rs`
11. `src/awnet/reachability.rs`
12. `src/main_full.rs`
13. `src/resources_cli.rs`
14. `src/web.rs`
15. `src/web/agent_view_tests.rs`
16. `src/web/team.rs`

They contain:

- production remote-view server CLI opt-ins;
- read-only SurfaceHost shell invariant;
- explicit replica-control remote-Machine registration without importing
  owner/admin authority;
- replica-only v35 membership evidence tied to the exact accepted Phase-30
  source;
- exact local-Machine hosted-Workspace lookup for remote service;
- resident remote-view Machine service retaining one `SurfaceHost`;
- exact-generation reusable-rendezvous retransmit hardening;
- browser-origin/SSH-forward authority support;
- updated tests/fixtures;
- durable documentation.

Do not reset or reconstruct them.

---

## 25. Verification commands appropriate to the current boundary

Before live two-node work:

- `cargo fmt --all -- --check`
- `git diff --check`
- strict full-product Clippy if source changed;
- use isolated `CARGO_TARGET_DIR` if another host build owns Cargo's normal target
  lock.

For aw-surfaces tests use the trusted environment already required by the suite:

- pin `AW_GIT` to the trusted absolute Git path;
- pin `AW_BASH` when terminal-enabled tests require it.

Do not report the known unpinned AW_GIT fixture failures as product failures.

For live work:

- use real production commands;
- use private temporary state roots;
- clean only fixtures created by the acceptance run;
- preserve the canonical source tree.

---

## 26. Quick subsystem source map

### Unified jobs/runtime

- `crates/aw-tools*`
- Agent/Jobs runtime owners referenced by the unified plan
- `src/agent_jobs.rs` and related adapters

### Store / authority

- `crates/aw-store/src/resources.rs`
- `crates/aw-store/src/identity_continuity.rs`
- `crates/aw-store/src/remote_view.rs`
- schema migration 35

### AwNet

- `src/awnet/machinelink.rs`
- `src/awnet/reachability.rs`
- `src/awnet/identity_continuity.rs`
- `src/awnet/services.rs`
- `src/awnet/remote_workspace_surface.rs`
- `src/awnet/remote_workspace_target.rs`
- `src/awnet/remote_workspace_registry.rs`

### Workspace/browser

- `src/web.rs`
- `src/web/workspace.rs`
- `web/src/main.jsx`
- `web/src/workspace-views.js`
- `web/src/terminal-layout.jsx`
- `web/src/document-docking.js`
- `web/src/styles.css`

### Surfaces

- `crates/aw-surfaces/src/lib.rs`
- `crates/aw-surfaces/src/contract.rs`
- `crates/aw-surfaces/src/views.rs`

### CLI

- `src/main_full.rs`
- `src/resources_cli.rs`

### Main browser tests

- `tests/web_workspace.test.mjs`
- `tests/web_ux_convergence.test.mjs`
- `tests/web_docking_handoff.test.mjs`

---

## 27. Final state summary

### Fully accepted / do not reopen casually

- shared tools convergence;
- unified Jobs/Agent runtime;
- native/browser Agent views;
- scoped Files/Git;
- same-window/cross-window docking;
- same-owner pairing;
- distinct-user delegated File/Review authority;
- persistent PTY ownership;
- same-UID Linux original-shell migration;
- V1.5 local Workspace/Minimal shell;
- Review 2-over-1 layout;
- Build terminal grid;
- local browser V1.5 visual matrix;
- v35 remote-view trust;
- protected read protocol;
- remote Workspace v3 projection;
- trusted target admission;
- owner-private loader/private registry/action router;
- ActiveTarget follower/pin semantics.

### Implemented but currently uncommitted

- production remote-view server CLI opt-ins;
- read-only SurfaceHost shell requirement cleanup;
- explicit replica-control Machine registration and replica-only v35 membership
  evidence;
- exact local-hosted Workspace remote service + resident retained SurfaceHost;
- exact-generation reusable-rendezvous retransmit hardening;
- explicit browser-visible origin for SSH-forwarded web access;
- associated docs/tests.

### Primary next gate

**Fix the in-web-owner protected-read direct-route zero-byte stream close, then
resume the already-established production two-AwNode gate at remote capability
read → Files/search/Git/Review → reconnect/re-read → revoke.**

That is the next agent's starting point.
