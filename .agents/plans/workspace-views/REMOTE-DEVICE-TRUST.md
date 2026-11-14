# Remote-device trust / remote-view backend checkpoint — 2026-11-14

**Status:** backend trust and protected AwNet Files/Git read protocol are implemented
and locally accepted in the current **uncommitted** tree at HEAD `c9aaf0e`.
Product remote Workspace integration is **not** accepted yet.

This slice deliberately does **not** enable remote Agent execution, PTY custody,
provider/tool authority, execution retargeting, fleet broadcast, browser target
switching, or an ambient remote filesystem. It establishes the reusable durable
authority/lifetime boundary that later Workspace remote views must consume.

## 1. Durable trust lease — SQLite v35

Schema v35 adds `remote_view_grants` and a narrow `aw-store::remote_view`
API.

A grant pins:

- exact Domain and hosted Workspace;
- Workspace revision and policy revision;
- target local Machine ID/Node ID, security epoch and capability revision;
- requester registered remote Machine ID/Node ID, security epoch and capability
  revision;
- paired AwNet peer ID/Node ID, security epoch and public key;
- stable user ID/principal and user security epoch;
- Domain policy revision and security epoch;
- exact portable device binding ID/device ID/public key;
- a bounded permission set: `files` and/or `git`;
- issued/expiry/revocation time and monotonic grant revision.

Issue requires the intersection of all of the following:

1. a live device-key `AuthenticatedStableUser`;
2. active stable user and exact user security epoch;
3. active Domain and exact Domain security epoch;
4. active Domain membership;
5. active imported portable device binding and active identity-continuity source
   grant;
6. currently trusted signed AwNet peer;
7. that peer explicitly registered as an active **remote Machine in the same
   Domain**;
8. active local target Machine;
9. one exact non-archived hosted Workspace on that local Machine;
10. bounded Files/Git permissions and TTL 5 seconds through 1 hour.

Pairing, a MachineLink route, a path, a Workspace label, or a grant ID alone is
never enough.

Every authorization re-reads current authority and fails closed when any pinned
fact changes. This includes user security epoch, Domain policy/security epoch,
membership, source-continuity trust, device binding, peer trust/security epoch,
requester/target Machine security or capability revision, Workspace revision or
policy revision, explicit revoke, or expiry.

Reconnect creates no authority. A fresh transport may reuse an unexpired durable
grant only while **all** pinned authority facts are still current.

Explicit persisted revoke is available to the grantee or current Domain
owner/admin. Independent upstream revocation does not need to rewrite the grant:
authorization simply stops succeeding.

## 2. Device-auth grant issuance — additive protocol

The existing Phase-30 device-key challenge remains the cryptographic admission
point. Remote-view grant issuance is optional inside that same proof.

Compatibility is additive:

- ordinary identity authentication remains **wire v2** and keeps the original
  `aw-user-device-auth-v2\0` transcript;
- remote-view grant issuance uses **wire v3** only when requested;
- absent remote-view fields are omitted from legacy v2 JSON.

The v3 device signature additionally binds:

- requested Workspace ID;
- exact sorted Files/Git permission set;
- remote-view TTL.

Server issuance is separately enabled by `remote_view_max_ttl`. Requested
Files/Git permissions must also be present in the current signed MachineLink
capability lease. If reply publication fails after issuance, the new remote-view
grant is revoked, just as a newly issued native auth-session token is revoked on
failed publication.

## 3. Protected AwNet read protocol

The Phase-18 Machine read protocol also remains backward compatible:

- ordinary Machine reads remain **wire v1**;
- only `ViewFileList`, `ViewFileRead`, and `ViewGitStatus` use **wire v2**;
- legacy v1 requests omit the new authorization field.

A remote-view read requires:

1. current MachineLink `FileRead` or `GitRead`;
2. current MachineLink `IdentityAuthenticate`;
3. the exact persisted remote-view grant;
4. the exact granted device private key;
5. a fresh server nonce and device signature for **every protected read**.

The read signature binds:

- MachineLink lease ID;
- request ID;
- grant ID;
- Domain ID;
- exact server-bound Workspace ID;
- device public key;
- target Node/store;
- requester peer Node;
- SHA-256 of the exact typed read request;
- fresh nonce.

The server authenticates the device through the existing Phase-30 portable
device verifier, re-authorizes the durable grant, performs the read, then
**re-authorizes the grant again immediately before response publication**.
A revoke/policy/epoch/device/membership change while a response is being
prepared therefore fails the final publication check.

The server mode itself is downgrade-fenced:

- a service bound to `remote_view_workspace_id` rejects every legacy v1
  Machine read;
- an ordinary Machine service rejects every protected `View*` read.

A client cannot negotiate FileRead/GitRead and then bypass the remote-view grant
by deliberately sending an older operation.

## 4. Evidence on the current uncommitted tree

Focused Store:

```text
cargo test --locked -p aw-store remote_view -- --nocapture
4 passed
```

Covers:

- reconnect under unchanged exact authority;
- explicit revoke;
- peer/requester/target/Workspace/user/Domain-policy/membership/device/source
  trust/expiry fencing;
- real v34 -> v35 migration through normal `initialize()`, preserving
  `store_id` and `PRAGMA quick_check=ok`.

Focused AwNet remote-view/wire:

```text
cargo test --locked -p aw-cli --bin aw --features net remote_view -- --nocapture
5 passed
```

Covers:

- legacy identity-auth v2 wire compatibility;
- exact MachineLink capability intersection for grant issue;
- legacy Machine-read v1 wire compatibility;
- signed read transcript binding;
- service-mode downgrade refusal.

All AwNet tests:

```text
cargo test --locked -p aw-cli --bin aw --features net 'awnet::' -- --nocapture
24 passed
```

This includes the existing real localhost QUIC pairing test.

Affected Store regression:

```text
cargo test --locked -p aw-store --lib
212 passed

cargo test --locked -p aw-store --test composite_design
7 passed
```

Strict affected-target Clippy is green for `aw-store --lib` and the `aw` net binary with `-D warnings`; formatting and `git diff --check` are green.

## 5. Still open before remote views are a product feature

This checkpoint does **not** claim completed remote views.

Still required:

- shipping Workspace target/lens integration using this authority, with pinned
  vs follow-target semantics;
- exact remote root/worktree registration/binding in the product surface;
- a production-authorized two-node device-continuity -> grant -> reconnect ->
  protected File/Git read -> revoke acceptance run;
- remote Agent/member view authority where applicable; this slice covers
  stable-user member identity plus Files/Git only;
- offline/stale presentation behavior in native/browser Workspace;
- live browser/native geometry/focus/draft/Agent-preservation acceptance;
- post-ACK arbitrary process/network partition semantics;
- remote/distinct-Actor PTY custody;
- macOS/Windows PTY custody and broker crash/reboot restoration;
- actual Windows retained-directory runtime acceptance.

Do not manufacture the two-node proof by hand-authoring an identity-continuity
snapshot that production source export would not authorize. Reuse the real
Phase-30 continuity path when the product remote-view integration supplies both
actual Machines/Domain context.

No remote view may locally fall back or retarget an existing Session/Agent.
