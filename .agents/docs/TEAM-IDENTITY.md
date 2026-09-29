# Named participants and private conversations — Phase 9 / source 0.10.0

**Authored Rust source, not compile-validated or security-audited.** The phase introduces a bounded invite-only internal identity directory and independent browser conversation controllers. It does not claim production multi-tenant isolation, OIDC, password login or collaborative editing. Tests in other languages do not execute this Rust implementation.

## 1. One implementation, separate controllers

```text
aw team serve — explicitly requested, numeric loopback
  shared owner-held SQLite connection and normal Agent implementation
  authenticated gateway (random invite -> opaque session cookie)
    Alice controller: internal:engineering:alice
      own current session/branch, private drafts, allowed accounts
    Bob controller:   internal:engineering:bob
      own current session/branch, private drafts, allowed accounts
  optional separately paired Alice/Bob device-inference links
```

Each controller owns an ordinary `Agent` plus its own `PersistentSession`, projection, admission lock and account registry. It reuses the existing journal/model loop, not a forked implementation. Shared HTTP providers are reused only for explicitly common server accounts; user contexts and invocation identities are still separate. All controllers share one cooperative SQLite owner process.

The core store's conservative same-tool-root hazard lock is unchanged: overlapping runs against the same configured root may fail busy/reconciliation checks instead of executing concurrently. Phase9 is independent identities and active views, not unrestricted concurrent agents or automatic queues.

Ordinary inline `aw`, `aw web`, and `aw serve`/`attach` retain their previous behavior. The default binary compiles the optional `team` feature but starts no roster, listener, watcher, participant history or extra agent unless `aw team serve` is explicitly selected. `team check` creates no store/listener and performs no inference.

## 2. Stable names are not authentication

The independent `aw-identity` crate defines `Roster`, `Participant` and validated `internal:<realm>:<id>` names. It has only serialization/error dependencies. `roster.principal("alice")` is a trusted-host lookup, NOT proof that the caller is Alice.

The gateway establishes identity through an independently generated 256-bit invitation and subsequent random session cookie. Neither a prompt body, URL, username, `X-Aw-Principal`, renderer label nor an inference tool may choose the actor. A holder of an invitation can assume that participant: the owner must deliver codes privately to their intended recipients. The trusted host/admin can see all invitations and stored history; this is not end-to-end encrypted history or security from the owner.

Realm and participant components are 1..32 lowercase ASCII letters/digits/hyphen/underscore; labels are bounded readable text. Same display name is allowed and conveys no authority. Display rename keeps identity; changing a participant ID creates another identity. **Never recycle an old ID for another person.** Removing/readding the same ID is the same account with the same owned history, not an account-reset mechanism. OIDC/email linking and account deletion/reassignment workflows are not implemented.

A private `team-realm.json` stamp binds the state directory to the selected realm. A differently stamped realm is rejected before SQLite recovery; an old unstamped CLI/web database is not automatically adopted. Missing/inconsistent stamps require explicit operator inspection. A failed first bootstrap may leave a database requiring inspection rather than silent reuse. Do not change the stamp to bypass scope checks.

## 3. Private-only history in this slice

Each participant can list, open, fork, checkpoint and export only sessions it owns. The team wrapper intentionally denies opening another owner's history even if a trusted SDK user separately created an ordinary store membership. There is no HTTP grant/share/invite-to-conversation API. The underlying SDK membership support remains for future explicit sharing.

Per-participant current branches are independent: Alice changing or resetting her view does not move Bob's view. Multiple browser tabs using Alice's same cookie still observe Alice's one current controller, with the existing draft-target-change warning. They are not independent per-tab agents or a shared composer.

All sessions created by `aw team` use a fixed chat-only instruction and explicit project/root binding. No model tools are registered, no human Files/Git/terminal surfaces are granted, and no arbitrary instruction or root fields come from the browser. Trusted native SDK code remains trusted; it can perform its own I/O and is not confined by this factory's no-tool policy.

## 4. Explicit account choices and personal-device defaults

A participant configuration contains an explicit default account/model and optional allow-lists:

| Source | Placement and accounting |
|---|---|
| `shared_profiles` | Existing named owner configuration. Server custody, explicit actor grant, payer label `team:<realm>:<profile>`. These are intentionally server-owned common accounts. |
| `device_links` | The existing Phase8 private pairing/link protocol. Personal provider configuration and secret remain on that user's device. Account owner and payer are the initiating internal principal. |

No participant gets an account merely because it exists elsewhere in the provider config. Common profile names are limited to64 bytes in team configuration so payer labels fit the core's bounded identity format. A device account cannot shadow a common profile or another participant's device account. Device link socket and pairing-file paths must be distinct; operators must also create distinct pairing secrets and must not reuse them across users.

**Device-only participants are supported.** The Agent builder now has an explicit `delegated_account` path, checked through the same account/adapter/executor validation. It does not require a dummy server-held default provider or read owner provider configuration when none is referenced. Model changes on a delegated default select only previously allowed models; they cannot expand the grant or revive a revoked lease.

A paired browser sees its own permitted opaque account/model IDs. It supplies no key, endpoint, auth-reference name, actor or payer. Each prompt resolves that participant's current lease and stores actual invocation provenance. Selecting another person's account or an offline device fails without common-account fallback. Usage remains provider-reported, not verified billing or a guaranteed currency budget.

## 5. Trusted configuration and startup

The JSON file is an owned non-symlink mode0600 regular file in a private owned directory; use the existing private-file validator. It is a local owner configuration, not browser-editable account administration.

```json
{
  "version": 1,
  "realm": "engineering",
  "directory": "/ABSOLUTE/checkout",
  "state_dir": "/ABSOLUTE/PRIVATE/team-state",
  "timeout_seconds": 120,
  "participants": [
    {"id":"alice","display_name":"Alice","shared_profiles":["team-local"],"device_links":[],"default_account":"team-local","default_model":"YOUR_MODEL"},
    {"id":"bob","display_name":"Bob","shared_profiles":["team-local"],"device_links":[],"default_account":"team-local","default_model":"YOUR_MODEL"}
  ]
}
```

Replace placeholders in [the common-account example](../../examples/team.example.json) and [profiles](../../examples/team-profiles.toml). For personal accounts use [the personal example](../../examples/team-personal.example.json) and the Phase8 [link](../../examples/personal-inference-link.example.json). Real configuration and secrets belong outside the repository.

```sh
aw team check --team-file /PRIVATE/team.json --config /PRIVATE/profiles.toml
aw team serve --team-file /PRIVATE/team.json --config /PRIVATE/profiles.toml
```

`check` reads configuration references but not model credential values, and outputs only the configured principal/default/allowed account metadata. It is not proof that models are installed or accounts can authenticate. `serve` creates or explicitly resumes one owned session per participant, then prints the exact local URL and each one-use invitation to the launching terminal. Server account HTTP clients and device inference remain lazy; no model runs just from creating a session.

Caps: 1..8 participants, at most8 account choices/person, 16 distinct common profiles, 4 total personal links, and timeout1..600s. Referenced profiles need explicit models. Unknown JSON fields and ungranted defaults fail instead of being ignored. Bounds limit resource growth; they do not establish fairness or resistance to hostile users/same-UID code.

## 6. Resume and lifecycle

The default startup creates new private sessions, retaining prior history. To resume explicitly, add this to that participant's config:

```json
{"resume":{"session_id":"ACTUAL_OWNED_SESSION_ID","branch":"ACTUAL_BRANCH_ID_OR_NAME","allow_context_transfer":false}}
```

Saved directory/instructions/account-set fingerprint must match current configuration unless the owner explicitly authorizes a context transfer. A fingerprint change includes the configured common profiles and personal link account/model/grant metadata; it is not executor attestation. A new device grant may need an explicit reviewed context-transfer decision. The server does not know the device's endpoint or secret.

A config watcher exists only in team serve. It checks roughly every second; a semantic change or read failure in the selected roster/profile/link configuration stops the whole team owner, invalidates sessions/accounts and cooperatively cancels active work. No permissive live reload, automatic new invite, or hot reassignment is performed. Already-dispatched model work may have incurred usage. Native blocking code or bounded shutdown delays can delay cancellation.

Changing secret values behind the same environment reference, CA bytes behind the same path, or pairing-key bytes already loaded into memory is not automatically detected by this semantic fingerprint. Stop/restart explicitly when rotating those secrets. Personal workers retain their independent local grant-expiry/revocation checks.

Logout/absolute expiry cancels only that participant's active work and closes its authorized event streams. It does not terminate Bob because Alice locks her browser. The consumed code cannot be used again; a fresh invitation requires an owner restart in this initial implementation. Restart invalidates all cookies and prints fresh codes. Browser close detaches, rather than revoking or cancelling already accepted work. Team owner shutdown closes all its controllers and personal link listeners.

## 7. Personal-device setup

Create a distinct owner link for Alice with `account_id: alice-laptop`, its own socket and pairing capability, fixed model, executor, and grant ID. Reference that link in Alice's `device_links`; set it as her default or choose it explicitly. Bob's link/default is configured separately. Do not copy personal model API keys to the owner.

After Alice pairs, her authorized `GET /api/session` snapshot supplies `saved.session_id` and `saved.branch_id`. Grant on Alice's device using **`internal:engineering:alice`**, not the Unix owner or display name, and those exact durable IDs:

```sh
aw device grant --out /LOCAL/PRIVATE/grant.json \
  --actor internal:engineering:alice --account alice-laptop \
  --executor alice-device --model YOUR_MODEL --grant-id YOUR_LINK_GRANT_ID \
  --session ACTUAL_SESSION_ID --branch ACTUAL_BRANCH_ID --ttl 1800 --max-calls 8
aw device run --socket /LOCAL/PRIVATE/device-forward.sock \
  --key-file /LOCAL/PRIVATE/pair.key --grant /LOCAL/PRIVATE/grant.json \
  --receipts /LOCAL/PRIVATE/receipts.json --profile PERSONAL_PROFILE
```

Use an explicitly configured authenticated SSH StreamLocal forward when on different machines, as described in [device setup](DEVICE-INFERENCE.md). No new public transport, CORS widening, browser key vault or automatic localhost proxy is introduced. A new session/fork needs a newly permitted device scope. Local device controls remain the authority for personal model use; participant login is not an unlimited spending grant.

## 8. Security and delivery boundary

This owner intentionally binds only numeric loopback and validates exact Host/Origin. Cookie protection is the existing HttpOnly/SameSite development boundary; the HTTP loopback cookie lacks Secure and is not port-isolated. This is **not** a reverse-proxy-ready TLS/public service, a defense against root/same-UID compromise, or a guarantee against malicious code served by the trusted owner. Use separate browser profiles/contexts to represent different people; normal tabs in one profile share cookies.

Cross-user sharing, group policy administration, passwords/recovery, OIDC, isolated tool runners, native team attach, remote fullscreen views, simultaneous collaborative typing and fine-grained per-node policy are future phases. Do not enable shell/workspace capabilities by forwarding another mode's grants into this one.

Actual host tests must run the removed phase9_smoke helper and the Rust router/SDK tests, including two distinct browser cookies and actual personal-device actors. Independent Python fixtures and fake-transport screenshots are narrower evidence. [Protocol](TEAM-PROTOCOL.md) · [SDK](SDK.md) · [Build status](BUILD-STATUS.md) · [current plans](../plans/TODO.md).

## Planned accessible-branch summaries do not widen today's access

Phase 9 still admits owned-private histories only. The [context-flow proposal](CONTEXT-FLOW.md) starts with explicitly selected same-principal sources and later permits cross-user sources only after read/release/derived-data revocation controls and explicit session sharing are implemented. A participant who can read source A may still be forbidden to publish its summary into target B or send it to B's provider. Do not remove the owned-only wrapper merely to make the future source picker show more rows.

## Phase 10 source-sharing amendment

The earlier private-only source inventory is narrowed as follows: a session owner may grant a **known enabled same-realm participant** read-only source access and explicitly list that recipient's account IDs permitted for summarization/derived provider context. This is not source append, a shared key, tool authority, public sharing or a grant to use any model endpoint. An empty list means no provider disclosure. Destinations in team mode remain owned and context-compatible; ordinary open/fork/controller selection does not accept another owner's writable history.

Each release has an epoch. Revoking or renewing it invalidates prior derived artifact/history access; subsequent answers, manual checkpoints, compactions and nested imports retain dependencies. UI snapshot/tree/usage/export checks protect derived output as well as source nodes. Changed access can deny an entire dependent session until a clean private continuation is chosen. Previously delivered text is not recalled, and account IDs remain trusted host labels rather than verified physical provider identities.

No password/OIDC/public hosting, team tool runners, shared composer, cross-realm permission merge or declassification policy engine is introduced. Numeric-loopback, private invite cookies and trusted operator configuration remain the deployment boundary. [Current implementation](CONTEXT-FLOW-IMPLEMENTATION.md) owns this intentional exception to the Phase9 no-source-sharing limitation.

## Database architecture v2 — stable identity refinement

`internal:<realm>:<id>` remains a useful qualified participant/principal label in the current source, but the adopted durable model uses a stable opaque aw `user_id` as the canonical database identity. Internal usernames, Unix-account bindings and OIDC identities become authenticated bindings to that user. OIDC links are keyed by `(issuer, subject)` and never by email alone. Historical authored content retains the stable user/principal even if login identities are renamed, unlinked or rotated.

General hosted/team auth adds durable login sessions, revocation/security epochs, organization/workspace membership and roles without granting provider credentials or private branch access implicitly. This is design direction; current Phase 9/11 invitation/cookie behavior remains the implemented-source boundary until a later auth phase. See [DATABASE-ARCHITECTURE](DATABASE-ARCHITECTURE.md#6-stable-user-identity-and-authentication).

## Phase 38 shared-draft identity amendment

The old Phase-9 owned-only wrapper remains the default, but Phase 38 now has one explicit cross-user exception: a Session owner can share a Session with a named authenticated participant for prompt collaboration. The store projects both participants to stable opaque aw user IDs before draft authority is granted. Display name, `internal:<realm>:<id>` label, browser tab, Yjs client ID and cursor metadata are never authorization identity.

Contributor provenance is derived from authenticated accepted draft updates. It means “this stable user contributed accepted edits,” not surviving-character authorship, endorsement, billing approval or tool approval. The authenticated submitter remains the normal author/actor for provider admission and resolves only their permitted account/model/payer/custody.

Revoking a draft grant stops future discovery/disclosure/update/submit under that grant and rotates/abandons the active draft generation. Already delivered text cannot be recalled. Presence is intentionally non-durable and must not be repurposed into employee-activity metrics.
