# Invite-only team gateway — Phase 9 / source 0.10.0

**Authored application protocol, pre-1.0, not executed as Rust here.** It reuses the existing [web conversation API](WEB-PROTOCOL.md) with authenticated per-participant routing. It is not a general public multi-tenant API.

## 1. Authentication precedes controller selection

The trusted host constructs a validated roster and exactly one runtime per participant. Each runtime's Agent actor and owned persistent session must match `internal:<realm>:<id>`, with no exposed tools. An independently generated 256-bit one-use invite is hashed per participant. The invite is a bearer authentication capability, not a display username, group membership field or provider API key.

`POST /api/pair {"code":"..."}` remains the only sign-in body. No username/actor/role/admin field is accepted. The gateway checks all bounded invite hashes without trusting a browser-supplied ID, consumes the matching code atomically and returns a random cookie for that one controller. Invitations expire after five minutes and the owner admits at most10 sign-in attempts/minute globally, including successes. Bodies are capped at1024 bytes and5s; identical/reused/expired/unknown codes have no directory-discovery endpoint.

A valid cookie cannot be replaced by another participant's invitation without locking first. One browser profile therefore has one active identity at a time. A duplicate same-name cookie is rejected. HTTP aliases, alternate/multiple Origin headers, cross-site requests, non-JSON API mutations and arbitrary URI authorities are rejected by the same owner boundary. Invites/cookies never appear in JSON snapshots, URLs, provider context or SQLite.

The session cookie is the existing host-only HttpOnly SameSite=Strict cookie, absolute eight-hour lifetime. There is no password database, refresh-token rotation, independent login service, silent re-pair or OIDC in this phase. Loopback HTTP is not public TLS hosting and cannot isolate cookies from hostile same-host software. Codes must be delivered privately by the trusted operator.

## 2. Routes and public data

Public GET/HEAD routes are the static embedded assets and a generic health result. Public requests do not enumerate realm, users, paths, account names or histories. Protected requests require the exact issued cookie before the gateway selects a router. `X-Aw-Principal` and similar headers do not select an identity.

| Route | Scope |
|---|---|
| `POST /api/pair` | Unique invite selects one participant; login does not run a model. |
| `GET /api/team/me` | Current realm/id/label/principal plus `owned+explicit-shared` when `multiplayer` is compiled (`owned-only` otherwise), tools=false, surfaces=false, and the corresponding `peer_sharing` capability bit. |
| Existing session/list/tree/fork/checkpoint/usage/export routes | That authenticated participant's controller and owned histories only. |
| Existing accounts/prompt/cancel/reset/events/logout routes | That participant's account registry, exact target, receipts and stream. |
| `GET /api/workspace` | Disabled; no human view grants. |
| `POST /api/workspace/action` | Denied in this phase. |

There is no `/api/users` list, grant-sharing endpoint, arbitrary model/provider configuration, key upload, filesystem path, native socket bridge or actor impersonation parameter. Every participant has its own AppState; public assets being identical does not make private state global.

Example authenticated principal response:

```json
{"mode":"invite-only-team","realm":"engineering","participant":{"id":"alice","display_name":"Alice","principal":"internal:engineering:alice"},"session_scope":"owned+explicit-shared","tools":false,"surfaces":false,"peer_sharing":true}
```

## 3. Admission and accounting

Prompt bodies remain `{request_id, expected_revision, session_id, branch_id, prompt, account_id?, model?}`. The gateway's actor is authoritative. Unknown identity/credential/root fields are rejected; defaults must have been explicitly configured. Missing or disallowed accounts/models fail; no server/common substitution occurs.

Each participant has a separate current view and memory request controller. The shared durable `web_requests` key already includes actor and request ID; Alice and Bob using the same request ID produce independent own-scoped receipts. Retrying Alice's exact accepted request cannot execute again after her current view changes or owner restart. Different body/account/target under an existing own receipt conflicts.

Every native invocation stores the actual actor and permitted account/payer/model/custody metadata through the existing journal. A common profile has an explicit shared payer. A personal device default uses its user's identity and finite device grant. The account selector displays only choices already registered for that participant.

Same-root run hazards remain conservatively serialized by the store. A busy shared root is not permission to retry with another account, execute twice or skip a journal receipt. One participant can inspect their own idle/history view while another runs, but actual overlap is subject to existing locks.

## 4. Response and stream isolation

The gateway validates metadata before decoding protected commands, bounds concurrent admitted HTTP requests at16, and applies a30s handler-response deadline. Streaming bodies are separately bounded by the existing SSE slot/expiry rules. Timed-out commands may already have produced effects/receipts; use exact status/recovery semantics, not an unconditional retry.

Each forwarded response is rechecked for current participant authority before disclosure except the explicit logout receipt. SSE uses only that controller's broadcast and checks its authority between events. Logout closes that person's stream, not all users. A slow or revoked response is withheld where the host can still control disclosure; previously delivered data cannot be recalled.

All responses, including denials, retain no-store, nosniff, no-referrer, deny-framing and restrictive script/connect CSP. Data is rendered as text by the browser. The new principal chip is display-only; the JS never copies its actor or payer into a prompt body. Lock clears identity/account/transcript state, and stale in-flight snapshots cannot restore it.

## 5. Host changes and persistence

A semantic change or unreadable trusted team/provider/link configuration triggers owner-wide stop, session invalidation and cooperative cancellation. This is a stop/restart policy, not a live permissions editor. Root/provider/link paths and actual key values stay outside the web protocol. Secret values changed behind the same already-loaded reference require explicit restart/rotation; the config fingerprint is not attestation.

Participant names live in the host JSON. A private state realm stamp prevents a different realm or an unstamped CLI/web database being silently adopted. Each participant normally starts a new saved conversation; `resume` is a trusted, explicit owned-session/branch selection. Cookies/invites are never persisted. Restart prints fresh invites and restores only explicitly selected histories using current account bindings.

## 6. Testing and remaining work

`src/web/team/tests.rs` contains real Rust router/Agent/store test source. the removed phase9_smoke helper requires an actual compiled binary and checks two cookie jars, distinct histories/accounts, same-ID attribution, logout, owner config revocation and restart no-replay. Neither was executed here.

the removed team_boundary_tests helper uses independent identity/invite reference logic and the shipped DDL; it is not server authentication execution. the removed browser_team_offline_smoke helper uses the actual browser assets and separate fake transport contexts; it tests UI state only, not cookies/HTTP/SQLite/provider isolation. Consult [build status](BUILD-STATUS.md) for observed results.

Future shared-conversation membership must add explicit disclosure, append and revoke semantics before widening `owned-only`. Future tools require per-user runner/credential isolation. Future remote login needs transport and account-lifecycle validation; do not turn this loopback gateway into one by changing a listen address.

The actual-binary Phase9 harness also includes two personal-only participants with a deliberately missing owner provider config. Separate local workers use different disposable fixture credentials and scoped grants; offline/peer-account selections cannot use another account. This is authored host-acceptance source, not an executed local-credential or SSH test.

## Phase 10 context-source protocol

The authenticated gateway now routes `/api/context/action`, `/api/context/cancel`, `/api/context/recipients` and `/api/context/recovery` to the already authenticated participant controller. Bodies cannot choose actor, payer, endpoint or credential. `grant_read` is source-owner-authorized and restricted to known configured realm principals; the account release list is explicit. Source read and destination append/disclosure are separate checks. Active destination sessions remain owned/compatible.

Generation is per-controller serialized, returns an accepted request ID, and is observed through exact operation lookup. Revocation closes protected event views with `context_revoked`; the browser discards cached derived text instead of merely hiding its source link. Recovery returns only current view revision/busy metadata, allowing an explicit reset to a new private session without reopening a denied transcript. [Current web contracts](WEB-PROTOCOL.md) and [context guide](CONTEXT-FLOW-IMPLEMENTATION.md) provide payload examples and limitations.

## Phase 38 — explicit shared prompt collaboration

Phase 38 intentionally supersedes only the old blanket `owned-only` browser wrapper for Sessions that an owner explicitly shares. Private team Sessions remain private by default. `GET /api/team/me` reports `owned+explicit-shared` only when the multiplayer feature is present; tools and workspace surfaces remain false.

An owner may explicitly share the current Session with one named authenticated participant. This creates ordinary stable-user Session membership needed for read/append admission plus a separate shared-draft grant. The draft grant controls read/edit/submit/manage/presence permissions and never copies provider credentials, payer identity, model defaults, tool grants or browser cookies. Revocation removes future discovery/draft access and restores any separate context-source membership to read-only rather than leaving append authority behind.

Shared Sessions are discovered only through `/api/team/shared-sessions` and opened only through `/api/team/session/open-shared`. The dedicated opener is an explicit context-transfer decision: root/tool-root and system instructions must still match, while a different participant destination/account fingerprint is permitted only through this reviewed action. Ordinary `/api/session/open` keeps the historical owned-only fence.

For a peer-owned current Session, ordinary prompt/fork/checkpoint/reset/context mutations are denied. Shared prompt execution must go through the exact draft path. Draft text is mutable pre-submission data; exact Send seals text, metadata revision and branch head/revision, binds one submission/request identity, then enters the existing durable prompt admission path with the submitter's own selected account/model. Duplicate Send is lookup/deduplication, not another inference call. Late edits after seal are rejected/recoverable rather than silently executed.

Presence is in-memory, permission-filtered and expiry-based. It is not prompt content, durable activity history or proof of availability. Joining/cursor movement/typing causes no model call.
