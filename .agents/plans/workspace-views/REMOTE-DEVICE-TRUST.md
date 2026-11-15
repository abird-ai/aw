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

## 5. Remote Workspace projection parity — 2026-11-15

The first product-integration plumbing slice is now implemented in the current uncommitted tree after `aaab161`.

- generic `net` now has a bounded **remote Workspace wire v3** separate from legacy Machine-read v1 and protected narrow-read v2;
- it carries permission-scoped capability discovery plus the same read-only operation family used by the shipping Workspace controller: Files list/search/read and Git list/open/retained immutable Review;
- every v3 request remains inside the existing v35/device-key challenge and exact serialized-request hash, with grant reauthorization immediately before publication;
- projection errors remain typed read results rather than transport ambiguity;
- `net` does **not** depend on `aw-surfaces`;
- full/surface builds adapt an `Arc<SurfaceHost>` through `RemoteWorkspaceProjection`, so remote reads reuse the one existing bounded inventory/Review/root-identity implementation;
- remote capability projection is permission-scoped and forcibly reports `git_write=false`, `terminals=false`, empty Sites and zero terminal capacity;
- a real temporary Git fixture proves the exact retained sequence `GitList(changes) → GitOpen → Review(id)` on one projection;
- focused remote Workspace/projection tests: **5/5**;
- all AwNet tests: **34/34**, including localhost QUIC pairing;
- strict full-product binary Clippy with `-D warnings`: passed;
- `net`-only library check still passes (with the known unrelated `bind_execution_owner` warning). The pre-existing reduced-feature **binary** `subagents::Inner.state_dir` compile error is unchanged and not caused by this slice.

This does **not** yet make remote Workspace targets selectable. Route/rendezvous data remains intentionally browser-inaccessible.

## 6. Trusted host admission primitive — 2026-11-15

The current uncommitted tree now adds the next local authority boundary without exposing network or key material to the web library:

- `RemoteWorkspaceTargetAdmission` consumes a **server-issued active v35 grant**, trusted host `PunchOptions`, and private device `SigningKey`;
- admission rechecks route peer == granted target node, current local AwNode == granted requester node, current paired target peer trust, active registered remote Machine in the exact Domain, Domain policy/security epochs, stable user/security epoch, and active local device registration for the supplied key;
- revoked/expired grant, route drift, wrong device key, user epoch drift or Domain policy drift fail closed;
- the private admission object is deliberately non-serializable and its `Debug` is redacted;
- it mints a fresh opaque `rwt_...` descriptor containing only `{id,kind,label,files,git,expires_at}`;
- descriptor serialization excludes node IDs, rendezvous room/address, grant ID, Workspace ID and device public/private key;
- derived remote requests ask only for the exact Files-or-Git capability plus `IdentityAuthenticate`;
- `SessionStore::identity_device_key` is a new non-mutating owner lookup used to prove the host key is still an active device instead of silently registering/reactivating one;
- the web **library** remains independent of AwNet: it defines only a secret-free target descriptor plus a `RemoteWorkspaceTargetRouter` trait; the binary-owned registry implements that trait and captures every route/grant/key secret;
- direct descriptor HTTP regression: **1/1** proves the target descriptor is visible while representative secret/authority strings are absent;
- admission regressions: **3/3**, including post-admission local-device revocation fencing;
- all AwNet: **34/34**, including real localhost QUIC pairing and the v3 SurfaceHost Git parity tests;
- reduced `net+surfaces` library check passes with only the known pre-existing `bind_execution_owner` warning;
- strict full-product binary Clippy with `-D warnings`, formatting and diff checks pass.

## 7. Private grant loader, registry and read router — 2026-11-15

The current uncommitted tree now consumes the admission primitive in the binary owner path:

- `aw web --web-remote-target OWNER_PRIVATE_JSON` is repeatable and is accepted only by local single-user web mode with durable owner state;
- every target definition is read through platform-verified owner-private custody, is versioned, bounded to **64 KiB**, and contains only host-side network/identity configuration;
- before any network connection, startup rechecks the configured peer is currently trusted, the exact Domain is active, the peer is an active registered remote Machine in that Domain, and the supplied private key maps to an active local identity device;
- startup then runs the existing Phase-30 device-key identity-auth request with the exact Files/Git MachineLink permissions and obtains a fresh server-issued v35 grant for the exact Workspace;
- `RemoteWorkspaceTargetAdmission` performs the second local grant/route/user/Domain/device check and the result enters a binary-private `RemoteWorkspaceTargetRegistry` keyed only by its random opaque `rwt_...` ID;
- the registry revalidates local authority on **every** descriptor refresh and every action, so local device revoke or Domain/peer drift fences an already-visible target immediately;
- target capability discovery asks at most one Files and one Git request, requires both to report the same source/root when merged, and forcibly keeps `git_write=false`, `terminals=false`, empty Sites and zero terminal capacity;
- only typed `View` operations are translated to v3; non-View commands never reach AwNet;
- safe remote `Denied`, `Conflict` and `Limit` states map to equivalent Workspace errors, while transport/protocol/storage details collapse to a generic unavailable result;
- `/api/workspace/target/capabilities` accepts only `{target_id}`;
- `/api/workspace/target/action` accepts only `{target_id,command}` and `command` must be `View`; forged extra route fields are rejected by `deny_unknown_fields`;
- both endpoints run through the existing bounded surface worker pool and recheck browser authentication before and after the blocking remote request;
- CLI parse regression: **1/1**;
- private registry regressions: **2/2**;
- target admission regressions: **3/3**;
- target descriptor HTTP regression: **1/1**;
- target action HTTP regression: **1/1**;
- all AwNet: **34/34**;
- reduced `net+surfaces` library check passes with only the known pre-existing `bind_execution_owner` warning;
- strict full-product Clippy with `-D warnings`, formatting and diff checks pass.

Owner-private target config v1:

```json
{
  "version": 1,
  "peer_node_id": "node_...",
  "room": "<64 lowercase hex chars>",
  "rendezvous": "host-or-ip:port",
  "domain_id": "dom_...",
  "workspace_id": "ws_...",
  "device_key": "/absolute/owner-private/device-key.json",
  "permissions": ["files", "git"],
  "lease_secs": 60,
  "view_ttl_secs": 600,
  "listen": "0.0.0.0:0",
  "stun": [],
  "timeout_secs": 30
}
```

Defaults: `lease_secs=60`, `view_ttl_secs=600`, `listen=0.0.0.0:0`, empty STUN list and `timeout_secs=30`. Lease is bounded to **5..=300s** and view TTL to **5..=3600s**. `device_key` must be absolute. Neither the config body nor the device-key body is ever serialized to browser state.

## 8. ActiveTarget follower/pin browser integration — 2026-11-15

The current uncommitted tree now consumes the opaque target registry in the V1.5 browser without widening execution authority:

- the compact Workspace breadcrumb exposes **ActiveTarget = Local | opaque remote Machine** only when remote descriptors exist;
- target discovery is passive: `/api/workspace` returns descriptors, but no remote MachineLink/read is opened until explicit selection;
- remote selection calls only the opaque target capability endpoint, then follower reads use only the opaque target action endpoint;
- target-switch requests are sequence-fenced so a late remote capability response cannot overwrite a newer selection;
- Files/Git containers are `FollowActiveTarget`: their navigator presentation is snapshotted and recreated on the new exact root when that kind is granted;
- contained File/Diff previews are intentionally cleared instead of reopening the same path/inventory on another Machine;
- an explicitly broken-out File/Review becomes a per-document **PinnedResource** with the immutable current source/scope and a reader bound to the target transport at pin time;
- pinned caches survive follower source replacement and refresh only through their original transport; they cannot move into a new follower scope;
- closing a standalone pin back into a still-live contained preview explicitly unpins it;
- Review and Build PTY capability is derived from the Local owner source, not ActiveTarget, so remote selection cannot create, remove, claim, resize or retarget a PTY;
- Agent/session/draft/composer state is not part of target switching; source-level regressions assert the selector path contains no Session-open or terminal effect call;
- reconnect restores the selected opaque target only if it is still advertised; disappearance clears followers and reports the loss, never silently selecting Local;
- production Vite build and focused browser/controller tests: **85/85**;
- full-product `cargo check`, strict Clippy `-D warnings`, formatting and diff checks pass.

A fresh embedded Chrome render for this new selector was not established in this turn: the L5 synchronous build wrapper repeatedly terminated during final linking before `target/debug/aw` was replaced, with no compiler diagnostic. The previous V1.5 screenshot matrix remains valid for its previously accepted states but is **not** evidence of live remote-target switching.

## 9. Production server CLI enablement — 2026-11-15

The current uncommitted tree now exposes the already-implemented grant/projection backend through explicit production server commands:

- `identity-auth-serve` gains `--remote-view-max-ttl SECONDS` plus repeatable typed `--remote-view-grant files|git`;
- TTL and surface policy require each other, so there is no half-enabled remote-view grant mode;
- the identity-auth MachineLink always grants `identity-authenticate` and adds only the explicitly configured File/Git read capabilities;
- `machine-service-serve` gains optional `--remote-view-workspace WORKSPACE`;
- remote-view serve mode requires explicit `identity-authenticate` plus file-read and/or git-read grants;
- remote-view Machine service accepts only the exact Workspace ID, rejects a simultaneous Domain selector, and requires that Workspace to be active, hosted, and bound to this exact local Machine;
- the server then creates one retained read-only `SurfaceHost` over the host-selected `--root`, wires `SurfaceRemoteWorkspaceProjection` into Machine service v3, and remains resident across fresh MachineLink transports so retained inventory/Review IDs keep one exact source/scope lifetime;
- absent these flags, legacy identity-auth and ordinary Machine-read behavior is unchanged;
- `SurfaceHost::new` now requires an absolute shell only when `allow_terminals=true`; read-only hosts no longer need a fake shell path;
- focused CLI opt-in, exact capability, and downgrade-fence tests pass;
- full `aw-surfaces` with trusted test environment: **35/35**;
- isolated full-product `cargo check` and strict Clippy `-D warnings`, format and diff checks pass.

The remaining product boundary is now purely the real production-authorized two-node continuity→grant→remote target selection→Files/Git/Review→reconnect→revoke acceptance plus offline/stale presentation. No hand-authored continuity snapshot may substitute for that gate.

## 10. Still open before remote views are a product feature

This checkpoint does **not** claim completed remote views.

Still required:

- a production-authorized two-node device-continuity -> grant -> explicit
  ActiveTarget selection -> exact remote root/worktree Files/Git/Review reads ->
  reconnect/re-read -> revoke acceptance run;
- live browser/native verification that target switching preserves Agent Session,
  unsent draft/caret and Local PTY inventory/input/resize owner while follower
  Files/Git rebind and standalone pins remain on their original source;
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

## 11. Real two-AwNode production gate checkpoint — 2026-11-15

The production path has now been exercised on L5 with two fresh independent
file-backed durable state roots and public `aw` commands only. No SQLite
authority, continuity state or v35 grant was hand-authored.

### 11.1 Production authority established

The executed path reached:

1. direct QUIC pair/trust;
2. source Domain/device creation and explicit target remote-Machine
   registration;
3. signed Workspace writer handoff A → B;
4. target replica explicitly registering the exact trusted recorded control
   AwNode as requester remote Machine;
5. real Phase-30 continuity accept + signed sync;
6. requester device-key proof;
7. identity-auth server explicit Files+Git remote-view policy;
8. real **server-issued v35 grant**;
9. owner-private `aw web --web-remote-target ...` admission;
10. opaque `rwt_...` descriptor visible to browser state with peer/room/
    Domain/Workspace/grant/device-key material absent.

The replica bootstrap required two narrow production fixes, without importing
Domain-admin authority:

- `register_remote_machine` may operate on an imported replica Domain only for
  the exact trusted `workspace_replica_domains.source_node_id`; ordinary
  Domains still require owner/admin + Domain-authority write;
- v35's active-member fence on a replica-only Domain may use only the active
  portable Phase-30 binding from that exact recorded source. Local authoritative
  Domains continue to require a real active `domain_memberships` row.

Focused regression extends the existing continuity/Workspace-replica flow
through explicit Machine registration and real Files+Git grant issue/
authorization. Post-cleanup evidence: v35 **4/4**, Phase-30 → explicit replica
Machine → v35 **1/1**, writer handoff/failback **1/1**.

### 11.2 Protected service lifetime and rendezvous hardening

The live gate also exposed and fixed two independent production issues:

- a Workspace-bound remote Machine service must remain resident: one retained
  `SurfaceHost` now owns stable source/scope and retained immutable Git
  inventory/Review IDs across fresh MachineLink transports; ordinary Machine
  service remains one-shot;
- reusable rendezvous now remembers only exact recently matched
  `(node_id, nonce)` generations and ignores late retransmits from those exact
  generations. A new nonce is immediately eligible to reuse the room.

The exact-generation regression passes **1/1**. Same-room live isolation also
proves two consecutive `net punch` routes and two consecutive one-shot Machine
services succeed.

### 11.3 Current blocker: first protected read from live web owner

The gate is **not complete**. The first
`POST /api/workspace/target/capabilities` returns HTTP **503** while the same
`aw web` process owns the requester store.

Diagnosis-only logging was added temporarily and removed before this checkpoint.
It established:

- rendezvous/QUIC gets far enough for client `connection.open_bi()` and server
  `connection.accept_bi()`;
- the client send half is already closed before writing the first 4-byte
  RouteHello length: Quinn `ClosedStream`;
- the server receives a zero-byte FIN: `FinishedEarly(0)`;
- browser-facing output remains the intended bounded generic
  `remote Workspace target unavailable`.

Isolation evidence:

- plain Machine FileRead on the same two durable nodes succeeds;
- identity-auth followed by same-room plain Machine FileRead succeeds;
- two sequential same-room one-shot Machine services succeed;
- real remote-view v3 grant issuance succeeds; after cleanly stopping the
  `aw web` owner, the same durable requester state + same room performs a plain
  Machine FileRead successfully;
- the remote HTTP worker already uses `tokio::task::spawn_blocking`, so the
  defect is not simply blocking network I/O directly on an async HTTP task.

The remaining engineering target is therefore the **in-web-owner protected-read
direct-route/QUIC stream lifecycle/context**. Do not weaken:

- v35 grant/device/member/Machine fences;
- local owner lock/custody;
- opaque browser target secrecy;
- exact-source retained File/Review semantics;
- no-Local-fallback behavior.

### 11.4 Not yet accepted

Because the first protected read is blocked, do not claim:

- production Files/search/File read;
- Git inventory/open/retained immutable Review;
- reconnect/re-read of an exact retained source;
- revoke → stale/unavailable/no Local fallback after a successful protected
  read;
- live non-empty draft/caret and PTY preservation during remote selection.
