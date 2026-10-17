# Web owner protocols

## Current local-single-user web mode

The historical paired `aw web` protocol remains relevant to paired/shared/team modes, but ordinary numeric-loopback **single-user `aw web` now has a separate `LocalWeb` authority**. It authenticates only the verified local owner path, retains exact Host/Origin/Fetch-Metadata protections, and does not use a pairing code/cookie. It is still loopback-only and must not be proxied/wildcard-bound and treated as remote multi-user authentication.

Local web can start before inference is configured. A revoked control-plane-only placeholder account lets the application expose Settings/session metadata without becoming selectable for model execution. `GET /api/settings` plus idle-gated strict Settings mutations manage secret-free provider/profile/model metadata and the shared UI preferences; provider keys remain environment-backed. Saving Settings performs validation/registration only and never dispatches inference. The first prompt after a valid default profile/model creates the real durable Session lazily.

Local UI settings include ordered status fields, inline/fullscreen preference, editor/output padding, autocomplete maximum, performance/TTFT/waterfall display, visible thinking activity and tool-call details. Hidden thinking is a presentation label only; provider-private continuation/reasoning remains private. Pairing/logout semantics for shared/paired modes are not weakened by this path.

Current embedded static assets additionally include `/settings.js` and `/workspace-sidebars.js`; the current local workspace shell is described in [BROWSER-WORKSPACE.md](BROWSER-WORKSPACE.md).

The paired route contract below retains one authenticated principal and one active Agent. Ordinary LocalWeb uses the distinct authority described above. Read-only Agent tabs project authorized history independently; they do not add execution owners or remote multi-user authentication.

**Mode distinction:** the new [team gateway](TEAM-PROTOCOL.md) authenticates a named participant before dispatching to their private instance of this API. The earlier single-owner sections below do not grant team participants Unix authority, tools or human workspace surfaces.

## Owner, pairing and data scope

`aw web` binds a numeric loopback address (default127.0.0.1:8787). Exact Host/Origin and cross-site Fetch Metadata boundaries, JSON mutations, bounded body, one-use 256-bit random pairing code, hashed code/cookie, short pairing TTL, bounded attempts, HttpOnly SameSite=Strict host cookie, private embedded no-store/CSP assets are retained. Cookie/code are memory-only and never included in the database or snapshots. Fresh owner/restart needs a new pairing code; durable conversation can be selected via `--session-id BROWSER_SESSION_ID`.

This development HTTP loopback cookie lacks Secure; browsers do not isolate cookies by port. Local malicious software/same-host services remain a trust risk. Do not proxy/wildcard-bind this owner and call it safe remote multi-user access. Authenticated actor is `browser:paired`, not the Unix launcher; CLI-owned sessions require explicit native SDK membership and are not silently exposed.

Current owner startup chooses endpoints, roots and system instructions. The browser only selects permitted opaque account/model choices and authorized saved-session controls. No key/env/endpoint/actor/payer/tool-root upload fields exist. A selected stored history must match current root/instructions/destination policy. No credentials, grants, tools or shared profiles are restored from SQLite.

## Routes

Existing routes remain: GET `/`, embedded `/app.js` `/model.js` `/style.css`, `/healthz`; POST `/api/pair`; GET `/api/accounts`, `/api/session`, `/api/events`; POST `/api/prompts`, `/api/cancel`, `/api/reset`, `/api/acknowledge`, `/api/logout`.

New authenticated routes:

| Route | Request / result |
|---|---|
| GET `/api/sessions` | Exact `store_id`, owner `source_epoch` and bounded authorized Session/branch descriptors, including before the active Agent is saved. No stored system prompt/fingerprint configuration. |
| POST `/api/agent-view` | Exact `{store_id, source_epoch, session_id, branch_id}`; authorized public transcript with decimal-string branch revision and a 15-second freshness bound. Never activates a Session. |
| GET `/api/branches` | Current session's branch ID/name/head/revision/active-run descriptors. |
| GET `/api/tree` | Current permitted immutable nodes and branches; viewing never executes tools. |
| GET `/api/usage` | Native stored invocation receipts filtered to the current actor, no fabricated money/zero for unknown usage. |
| GET `/api/export` | Bounded idle JSONL tree download, not a server-filesystem path chosen by the browser. |
| POST `/api/session/open` | `{expected_revision, session_id, branch}`; current execution/disclosure binding must match. |
| POST `/api/fork` | `{expected_revision, name, at_node?}`; root/finished/checkpoint only, then switch the active projection. |
| POST `/api/checkpoint` | `{expected_revision, summary}`; human-authored context projection, no model call. |

Persistent prompt body:

```json
{"request_id":"client-chosen-id","expected_revision":4,"session_id":"s_…","branch_id":"b_…","prompt":"Continue","account_id":"allowed-account","model":"allowed-model"}
```

Account/model are optional only together (defaultownerselection); session/branch are required exactly when persisted. Unknown fields are rejected. `--state-memory-only` retains the memory protocol without saved IDs. Accepted response: `202 {run_id, revision, state}` where state is accepted, settled or interrupted. IDs are scoped to authenticated actor and exact fingerprint. Existing receipts are looked up before active-view revision checks so a retry cannot cause a new paid call after reset/restart. Requests that omitted account/model bind the resolved defaults; changed defaults can produce an explicit fingerprint conflict rather than a new execution. They are not globally valid outside the owning store.

New prompt receipt is durable before202, then `journal.begin` links exact native run before inference. Its final native settlement updates the same receipt transactionally. Preflight-rejected receipts settle without a native call. Restart recovers unfinished receipts conservatively; no auto-resubmit. A background persistence failure blocks continued prompts and requires reload/recovery. Source-level ID/hash fields are not proof of externally verified billing.

## Snapshots and views

Existing snapshot fields remain (instance, sequence, revision, turns, pending, identity/accounts, tools, tool_host, needs_reconciliation). Added `saved` has session_id, branch_id, branch_name, head, branch_revision, title and imported; `context_checkpoint` identifies explicit shortened context. No stored credential references or provider continuation are serialized.

`sequence` and view `revision` are process/view state. `branch_revision` is persisted and checked by the journal. Restoring creates a new owner instance, while deduplication receipts survive. SSE subscribes before snapshot/high-water and on lag supplies a fresh snapshot, not a complete event-replay log. At most eight current event views remain. Current account choices and actual grants are revalidated; observation confers no execution rights.

This first owner has one active view shared by browser tabs. A tab receiving another active target while it has unsent text retains that text and disables Send until the user explicitly accepts/discards retargeting. This prevents silent accidental disclosure; it is not shared-draft multiplayer or independent tab-group cursors. Unsent editor drafts are not saved across browser close.

## Read-only Agent view authority

The owner retains one Store for saved and unsaved browsing, activation and later
Session creation. Reads use the strict exact-ID branch reader and do not open a
State directory, initialize persistence, create a SessionHandle, enter the Agent
queue or journal an effect. Memory-only owners return an empty Store catalog.
The owner lifetime is distinct from branch revision and is stable across active
Agent selection; stale lifetimes and mismatched Stores are refused.

Host/origin/transport authentication remains necessary. Every read separately
checks the trusted actor's current Store/Session/branch authority and local
execution target, including unchanged heads. The reader rejects active or
unreconciled runs; inspectable uncertain history remains visibly marked.
Native and browser projections remove invocation receipts and provider-private
context. The browser endpoint has four blocking-read slots and refuses
oversized public transcripts rather than silently truncating history.

Read-only descriptors are memory-only. Closing a tab fences delayed replies;
logout, source replacement, denied refresh and freshness expiry clear its
protected content and derived Find/selection state. Current Agent selection and
unsent drafts remain independent. See [browser views](BROWSER-WORKSPACE.md) and
the [implementation checklist](../plans/workspace-views/TODO.md).

## Management and recovery semantics

Reset/new creates another saved session; previous tree, native receipts and unresolved hazards stay. In `--state-memory-only`, reset remains ephemeral but still cannot dismiss in-memory effect uncertainty. Acknowledgement requires explicit confirmation and is audit-journaled. Import review remains visible in imported provenance even after its gate is cleared.

A conversation fork does not copy/restore the workspace. Checkpoint changes only future context, while prior nodes remain exportable. Neither operation gives the caller another person's account or turns source text into a grant. Switching the current view does not alter running tools; mutations require idle owner plus expected view revision.

Browser closure disconnects the view; it does not kill server-held work. Owner stop requests cooperative cancellation; unfinished work is recovered on the next exclusive open. One physical state directory can have only one aw process owner. Stop/restart or use that owner rather than a second CLI reader. Separate stores are independent and do not coordinate effects.

## Output and validation limits

Labels use text nodes; transcripts and context cards use sanitized Markdown rendering with raw HTML disabled and inert media references. Exported content may still contain sensitive user/tool text: explicit export confirmation warns about it. No automatic browser key vault, endpoint proxy, HTML execution, user-supplied path, or current history secrecy guarantee against a compromised same-UID host is added.

Run the shared product gate in [Build status](BUILD-STATUS.md). Record actual Rust-owner HTTP, SQLite, production-bundle and real-browser results against their exact commits in the plan TODO. Pure model tests and mocked transport do not establish native-owner/browser acceptance or live provider/OAuth use. See [State](STATE.md).


## Phase 4 integration note

The optional native workspace uses the same current account/tool grants and persistent owner described above. The underlying Phase3 SQLite schema and browser account/transport behavior are unchanged. Human file views and layout changes do not enter provider context, start another loop, restore credentials from history or grant permission. Source0.5.0 adds a separate pure layout SDK and native rendering; no new remote hosting, browser key store, inference delegation, Git or embedded PTY is implied. See [WORKSPACE.md](WORKSPACE.md) and [BUILD-STATUS.md](BUILD-STATUS.md).


## Phase 5 native-service boundary

Source package0.6.0 adds native Git/PTY resources only. This browser owner exposes no new Git mutation, PTY launch/input, browser-workspace or multi-user route. Do not infer a remote shell API from a compiled Rust service crate. Its paired principal, per-prompt account selection, current credentials and durable-session authorization retain the existing behavior. A future shared browser workspace must add explicit per-resource control and authenticated transport rather than forwarding native descriptors blindly.

## Phase 6 / package 0.7.0 — optional human workspace protocol

Earlier protocol sections describe the existing conversation endpoint behavior. The numeric-loopback Host/Origin/cookie/JSON rules and one paired principal remain; this is not an exposed multi-user/remote API. Additional routes are available in the binary:

| Route | Current source contract |
|---|---|
| `GET /workspace.js`, `/workspace-model.js`, `/workspace.css` | Embedded static code, no secrets or runtime model state. Browser imports on workspace entry. |
| `GET /api/workspace` | Authenticated capabilities; `{enabled:false}` when owner did not grant surfaces. |
| `POST /api/workspace/action` | Authenticated JSON matching `aw_surfaces::Command`, tagged `op`, unknown fields rejected. Owner surface grant required. |

The trusted startup flags are --web-workspace, --web-allow-terminals and repeated --web-site LABEL=HTTPS_URL. They are not browser fields. Surface commands never accept arbitrary owner root, Git arguments, program/environment, account, credential, actor or server-fetch destination.

Action operations: `capabilities`; `files`/`file` with root-relative path; `git_list` with known context and mode; `git_open` with cached item ID; `review` with review ID; `review_feedback` with row and comment; `terminals`; `terminal_start` with request ID, known context, confirmation and dimensions; `terminal_claim`/`release`; `terminal_screen`; `terminal_input`; `terminal_resize`; `terminal_stop`; `terminal_forget` for exited resources. Inspect the actual tagged enum in [contract source](../../crates/aw-surfaces/src/contract.rs) for exact required fields.

```json
{"op":"git_list","context":"root","mode":"changes"}
```

```json
{"op":"terminal_start","request_id":"example-once","context":"root","confirmed":true,"rows":24,"cols":80}
```

```json
{"op":"terminal_input","terminal_id":"returned-id","client_id":"this-tab-id","lease":"returned-lease","sequence":1,"input":{"kind":"key","key":"Enter"}}
```

Unknown command/extra fields are 400; missing grant 403; stale handles/lease/sequence 409; bounded-capacity overflow 413; busy worker pool 429; unavailable service 503. Authentication failure remains 401. A response/queued-input receipt is not evidence of completed OS side effects. Model prompt deduplication remains the existing durable store protocol; surface launch/input receipts last only while this owner lives.

Four admitted blocking workers share one resource operation queue. Authority is rechecked after queue acquisition and data rechecked before disclosure. Logout/expiry shuts the surface host and requests terminal termination; a previously admitted operation can still have completed effects. No raw PTY WebSocket or viewer-supplied filesystem service is added.

When enabled, CSP adds exact configured `frame-src` origins and permits dynamically computed style attributes; `script-src` and `connect-src` stay same-origin, and top-level framing stays denied. Embedded page sandbox/Permissions Policy/no-referrer rules are in [BROWSER-WORKSPACE](BROWSER-WORKSPACE.md). No arbitrary CORS override, query-token URL, forwarded provider key or browser localStorage credential store is introduced.

Current browser terminal claims are cooperative per-client control within one authenticated principal, not identity isolation against hostile same-origin code. Cross-principal terminal ownership and end-to-end personal-device inference require later separate contracts.


## Phase 7 shared-owner integration

The new `aw agent --server-socket PATH` explicitly starts a Linux same-UID native owner. Add `--web-serve` to attach the existing numeric-loopback HTTP API to THAT same Agent/store. In this opt-in mode the configured actor is the verified Unix user, and the printed pairing warning states that a paired browser receives that owner role. The historical standalone paired adapter uses `browser:paired`; ordinary `aw web` instead uses the distinct LocalWeb authority described above, with no native socket or implicit remote sharing.

The existing routes and browser source are preserved. Session open/fork/new/checkpoint still change one common active projection; other views rely on revision/saved-target checks and draft retarget guards. Native `Inspect` is read-only and does not change the browser selection. Native commands do not bypass browser cookies: they enter through a separately peer-validated Unix transport and internal authority context. No HTTP header can select that native context.

Browser logout/expiry still revoke HTTP observation and clean up browser terminal surfaces. Only in shared-owner mode they do not cancel the common agent or stop native peers. The owner itself controls lifetime. This refinement is explicit; the prior standalone web cancellation behavior remains. Pairing remains one use and re-pairing requires owner restart.

Authentication is now rechecked after waiting for the mutation/admission locks, including view changes and inspection acknowledgement. This closes a stale-browser-permission window; it is not evidence of a security audit. Request receipts/result metadata are shared with the native control path under the same principal, not a second usage or history ledger.

[Local protocol](LOCAL-PROTOCOL.md) · [Owner lifecycle](LOCAL-OWNER.md).

## Phase 9 / 0.10.0 — explicit named-participant gateway

`aw team serve` is a new, separately selected owner mode. Prior `aw web` and Unix `aw agent --web-serve` roles are unchanged. Team cookies independently select `internal:<realm>:<id>` controllers with owned-only histories and explicit account/model grants. Each participant has a different active session/branch; same-user browser tabs still share that user's controller. Tools and human workspace surfaces are not granted.

The existing JSON routes are reused behind the authenticated gateway; `GET /api/team/me` adds current identity metadata only. No actor/user selector, provider key, account administration, public directory or conversation-sharing API exists. Logout affects that participant; semantic host-config changes stop the whole team owner. The same-origin numeric-loopback, cookie limitations, snapshot/deduplication and immutable journal rules remain.

See the exact [team protocol](TEAM-PROTOCOL.md), [identity/lifecycle contract](TEAM-IDENTITY.md) and [build boundary](BUILD-STATUS.md). Gateway authority tests run in the product gate; executed commits and remaining runtime gates belong in the plan TODO.

## Planned context-flow UI/API

No summary-generation or context-import route is added by this design amendment. Future prepare/summarize/preview/apply requests use the authenticated participant's source/target permissions and current account. Independent source snapshot and target revision checks prevent navigation races from changing the input or destination. See [CONTEXT-FLOW](CONTEXT-FLOW.md); existing `/api/checkpoint` remains a human-authored context cut.

## Phase 10 / 0.11.0 — explicit summaries and read-only sources

Retain Host/Origin/cookie/body and participant/controller checks. The lazy module GET `/context-flow.js` is static code, not private data. Authenticated APIs:

| Route | Source behavior |
|---|---|
| POST `/api/context/action` | `{expected_revision, command}` where command is `aw::context_flow::Command`; unknown fields rejected. Reads do not dispatch a provider. Mutations compare idle view revision; destination head checks happen in the store. |
| POST `/api/context/cancel` | `{request_id}`; cancels only the matching summary operation. |
| GET `/api/context/recipients` | Current configured principal/label choices for a source-owner grant, not grants themselves. |
| GET `/api/context/recovery` | Current revision/busy status only, no denied source/body; permits an explicit fresh private conversation through normal reset. |

Generate is an explicit confirmed account/model action that starts a tool-free SDK operation and returns202. The same accepted ID/body is lookup-only, not a second model call. Poll with `operation`; apply has a separate request ID and immutable artifact. `application` recovers a previously committed result without moving views or reexecuting. No account or output fallback exists.

Snapshots add `context_blocks`, in projection order. Source-derived output stays buffered until final authorization/journal checks. Before snapshots/events/candidate retrieval, current dependencies are rechecked. A denied session yields403 and SSE `context_revoked`, not an old cached summary. Code may clear the browser view, but cannot erase content a client/vendor already received. A user may explicitly create a clean private session; that does not restore revoked context.

No file/model tool is added by opening the picker. The browser maintains unsaved candidate changes locally and refuses to apply them until a new manual artifact is saved. Previews render inert text; no raw HTML/Markdown execution. See [exact command shapes](CONTEXT-FLOW-IMPLEMENTATION.md) and [the packaged UI](../../web/context-flow.js). Real HTTP/auth/source checks belong in the product gate and must be recorded against their executed commit; mocked browser transport does not establish those results.

## Phase 38 — team shared-draft browser protocol

When compiled with `multiplayer`, team mode exposes explicit collaboration routes in addition to each participant's private controller:

- `GET /api/team/collaborators`
- `GET /api/team/shared-sessions`
- `GET /api/team/session/collaboration`
- `POST /api/team/session/mode`
- `POST /api/team/session/share`
- `POST /api/team/session/open-shared`
- `POST /api/team/draft/open|status|update|submit|submission|recover`
- `POST /api/team/draft/presence|presence/list`

These routes never accept an actor/payer/credential authority supplied by the browser. Session sharing is owner-explicit, draft updates are bounded Yjs-compatible V1 bytes, and server-assigned accepted revisions plus stable-user contributor identity are authoritative. Presence is ephemeral and separately bounded.

The Solid client lazy-loads the pinned Yjs collaboration chunk only after collaboration is activated. A nonempty private composer presents an explicit **Share this draft** / **Keep private** choice. Local editing paints immediately; Send first flushes pending updates, refreshes the accepted snapshot, and submits its exact digest/revision/target. SSE carries authorized `draft_changed`, `draft_presence_changed`, `draft_scope_changed`, `draft_submitted` and `draft_settled` notifications.

A peer-owned shared Session cannot use ordinary `/api/prompts`, fork, checkpoint, reset or context mutation. Only exact `draft.submit` enters the existing durable prompt receipt/journal path, preserving no-replay and duplicate-request semantics.
