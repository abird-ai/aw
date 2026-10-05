# Phases 19–23 continuation status — 30 September 2026

This is the current operational handoff for the `aw` implementation after the five-phase networking/federation implementation pass and the 30 September continuation/validation work. It records what is actually committed, what is now natively validated, what remains intentionally dirty in the continuation worktree, and the exact remaining boundary for the next agent.

It does **not** replace the architectural precedence of `FULL-HANDOFF.md`, `AWNET.md`, `DOMAIN-MODEL.md`, or `CLI.md`. It is the most current implementation-status note.

## 0. Current update after native continuation validation

### Phase 35 trusted-local direct SQLite update

The live tree is schema **v20**. Phase 34's store-generation/process leases, real provider execution lease, maintenance barrier, bounded Busy/Locked semantics, two-transaction paid-result settlement and staged restore fencing remain intact. Phase 35 performed the required fresh review and found one exclusive-owner assumption before direct mode could ship: startup recovery still swept all running LLM work, started paid context operations and accepted web requests. Schema v20 now records exact process/store-generation ownership for those live state machines, so recovery is incarnation-aware.

Ordinary `SessionStore::open` remains the default owner/service mode and holds `owner.lock` exclusively. Explicit `SessionStore::open_trusted_local_direct` clients hold the same lock shared; multiple direct clients may coexist, but owner/service and direct modes may not coexist. Direct mode requires an existing exact-v20 store on a supported local Linux filesystem, never migrates/creates/restores, and is heartbeat/store-generation fenced. Every persistent ordinary store call also participates in conservative `store:ordinary-access` coordination so ordinary access and maintenance exclude one another.

Provider process death is still never interpreted as “effect stopped”: unknown/interrupted provider state and retry/no-replay policy become durable before process-level execution-lease reconciliation. Paid results still commit before branch attachment CAS. Owner exclusive-lock acquisition can immediately fence killed direct clients without waiting for their heartbeat TTL; a native kill regression proves exactly one unknown attempt and no replay.

Current native validation: Phase35 **47**, Phase34 **101**, Phase33 **119**, Phase32 **120**, Phase31 **159**, Phase30 **142**, Phase29 **75**; `aw-store` **56 + 29 + 22 + 28**; default/minimal/web/net/OIDC cuts and full-workspace all-target check/test are green; actual schema-v20 owner/backup, inline/live TUI, zero-model local-web and full-screen Git/PTY smokes are green. Phase36 adds the typed Clap/help tree without changing schema or authority; Phase37 is the queued first-run provider/model onboarding boundary.

### Phase 32 verified OIDC/OAuth update

The live tree is schema **v17**. Real OIDC authorization-code + PKCE/state/nonce/discovery/JWKS login is implemented; identity links only exact verified issuer+subject and reuses Phase-31 native auth families. OAuth connectors are user-owned and separate from inference providers; client secrets and token sets are versioned only in SecretStore. Phase32 focused checks 120/120; Phase31 159/159; Phase30 142/142; Phase29 75/75; full workspace and Phase29-32 binary smokes pass. Phase33 is explicit materialized-fact → target Session/model-context projection.

### Phase 31 native auth-session / SecretStore provider-custody update

The live tree has advanced through schema **v16**:

- native refresh sessions are random 256-bit bearers persisted only as hashes, with fixed-lifetime rotation families, whole-family reuse revocation and stable-user security-epoch fences;
- Phase-30 portable device proof can mint the same native session type, bound to the exact portable binding/source trust;
- a narrow `SecretStore` interface now owns shared/server raw provider credentials outside ordinary SQLite; the local backend is encrypted, private-file, write-once/versioned and fail-closed on missing master key;
- `provider_accounts` carry only opaque refs/version metadata plus explicit stable-user grants;
- shared credentials are pinned to provider/model/executor/endpoint/version, revalidated at dispatch and never fall back to profile/environment credentials;
- CLI adds `aw auth` and `aw provider-account`; bearer/secret material uses private files rather than normal argv;
- focused Phase31 checks **159/159**; Phase30 **142/142**; Phase29 **75/75**; `aw-store` **22 + 29 + 21 + 28**; default/minimal/web/net and actual Phase29/30/31 binary smokes pass; full-workspace all-target tests pass.

Phase 32 is live OIDC authorization-code + PKCE/SSO and OAuth connector-token custody through SecretStore. Full-host clippy/Nix remains environment-pending.

### Phase 30 stable-user / cross-host authentication continuity update

The live tree has advanced through schema **v15** and the authored Phase 30 distributed-app MVP boundary:

- stable opaque `user_id` is now the cross-host Session/context authorization identity; source/local principal strings remain immutable provenance and local bindings;
- unknown `user:usr_...` principals cannot create users;
- Workspace semantic replica protocol v2 carries stable context artifact/grant/derivation user projections;
- users can explicitly register/revoke Ed25519 device public keys while private seeds stay in owned local files outside ordinary SQLite;
- OIDC linking is explicit issuer+subject metadata, never email/display-name matching, and portable OIDC data does not become a locally verified login automatically;
- target-side identity continuity grants are explicit/revocable/re-acceptable only through a fresh trust command;
- signed continuity snapshots carry only safe user metadata, public device keys/OIDC metadata and exact user/Domain security epochs; auth sessions/password/provider/OAuth/SecretStore secret data is excluded;
- separate `identity-continuity` and `identity-authenticate` MachineLink capabilities keep metadata import distinct from live authentication;
- device proof binds MachineLink lease, source/target AwNodes, target store, Domain, nonce and optional Session before producing the stable principal;
- source trust, OIDC and device revocation paths are independently validated;
- focused Phase 30 checks: **104 passed**; prior Phase 29 **72/72**; `aw-store`: **29 + 21 + 28**; default `aw-cli`: **87 + 50 + 6**; minimal/web/net configurations, full-workspace all-target tests and the compiled-binary Phase 30 two-node smoke pass.

Phase 30 reaches the core distributed-app MVP architecture finish line. Phase 31 moves to native auth sessions plus SecretStore/provider credential custody hardening. The separate full-host clippy/Nix gate remains environment-pending.

### Phase 29 semantic Workspace replica / writer-ref update

The live tree has now advanced through schema **v14** and the authored Phase 29 boundary:

- signed bounded semantic Workspace snapshots carry only safe Domain/Workspace metadata, required safe User rows, immutable Session/node/context objects with canonical digests, exact branch/ref history, writer history and `ref_writer_epoch`;
- `user_identities`, login credentials/sessions, provider secrets, execution-target rebinding, running operations and unresolved effects are excluded;
- `workspace-replica-handoff` and `workspace-writer-ref` are separate typed MachineLink capabilities;
- the remote writer can service exact branch reads, checkpoint CAS and fork under the authenticated control peer + writer epoch + authority term;
- failback now explicitly fences the old writer with the exact Domain-authorized writer event before its final snapshot is accepted;
- `ref_writer_epoch` preserves the actual authoring epoch of returned refs after the old writer has been fenced to the new current writer epoch;
- compatible previous-writer descendants fast-forward when the new epoch has not mutated; true partition divergence preserves the old descendant as a deterministic conflict branch while the newer writer stays canonical;
- focused Phase 29 checks: **72 passed**; `aw-store`: **29 + 20 + 28**; default `aw-cli`: **87 + 49 + 6**; minimal/web/net configurations, full-workspace all-target tests (with explicit trusted Git/Bash paths in this sandbox), and the compiled-binary two-node handoff/ref/fence/failback smoke pass.

Phase 29 deliberately does not infer cross-host human login identity. That explicit identity/auth continuity is Phase 30. The environment-only clippy/Nix gate remains pending.

### Phase 28 Workspace writer epoch update

The live tree has now advanced through schema **v13** and the authored Phase 28 boundary:

- every Workspace has a current writer AwNode, monotonic writer epoch, Domain authority term and immutable writer-event history;
- writer failover requires Domain owner/admin authority, the current Domain quorum fence, exact expected writer epoch and a valid local/trusted same-Domain target;
- Session creation, branch fork/checkpoint, context branch mutation and prompt admission are fenced to the local current writer;
- branch ref events/semantic branch events record writer node and epoch;
- admitted model work pins the writer epoch; settlement after failover preserves the result but refuses stale canonical attachment and emits an attachment-conflict recovery hint;
- a real two-node q2 binary smoke proves writer epoch 1→2→3, stale expected-epoch rejection and immutable writer history;
- focused Phase 28 checks: **44 passed**; `aw-store` suites: **29 + 18 + 28 passed**; default `aw-cli`: **87 + 48 + 6 passed**; minimal/web/net configurations pass.

Phase 28 intentionally does **not** replicate Workspace/Session/node/ref state to a remote writer store. Remote writer designation is currently the authority fence/handoff record; Phase 29 must add minimal semantic Workspace replica/ref carriage before remote canonical writes are operational.

The environment-only clippy/Nix gate remains pending as before.

### Phase 27 authority failover / open coordination update

The current tree has advanced through schema **v12** and the authored Phase 27 boundary:

- signed minimal Domain authority-core snapshots replicate from the current live quorum leader to explicitly granted AuthoritySet standbys;
- standby import independently verifies snapshot hash, AuthoritySet hash, source AwNode signature, source membership and the accepted AuthoritySet grant;
- a source-store change is accepted only with a strictly newer authority term;
- standby promotion uses a fresh signed strict-majority election and is fenced until the replicated source leader lease expires;
- promotion moves `authority_store_id`, advances Domain policy/security epochs and materializes only safe Users, Domain memberships, AuthoritySet state and the fresh quorum certificate;
- credentials/auth sessions/identity bindings, Workspaces, Fleets, Sessions/branches/nodes and provider state are not authority-core replica payloads;
- `domain-authority-replicate` is a separate typed MachineLink capability; promotion votes reuse the Phase 26 signed vote capability;
- `aw net serve --role rendezvous` is reusable signed multi-room coordination using the same open protocol as managed/self-hosted AwNet, and it has no Domain policy authority;
- focused Phase 27 checks: **76 passed**; store resources: **17/17 passed**; default/minimal/web/net Rust configurations pass; both Phase 27 compiled-binary smokes pass.

The remaining full-host clippy/Nix gate is environment-pending as before. The next distributed consistency boundary is Workspace mutable-ref writer epochs/CAS failover, not broader authority-store row replication.


The historical Phase 19–23 chronology below is retained for audit, but the live tree has advanced through Phase 26.

Validated current boundaries:

- Phase 24 is committed at `9cddaeb` / `phase-24` and Phase 25 at `d662c0e` / `phase-25`;
- Phase 24 persistent MachineLink remote control/Fleet orchestration and Phase 25 explicit semantic materialization remain green;
- Phase 26 schema version **11** implements Domain AuthoritySets, strict-majority quorum, durable elections/votes, bounded leader leases, and explicit foreign voter grants;
- q1 Domains remain zero-coordination; q>1 Domains fail closed for authority-changing writes without a live local quorum lease;
- existing owner/admin control-plane mutations are fenced, while normal reads and already-admitted bounded work are not turned into quorum operations;
- AuthoritySet changes increment Domain policy/security epochs and revoke authority-epoch-bound federation leases;
- typed `domain-authority-vote` MachineLink voting authenticates the candidate/voter, signs the canonical vote transcript with the voter AwNode key, verifies it on receipt, and `aw-store` independently verifies the signature before durable acceptance;
- voters reject same-term divergence, stale terms and overlapping live votes for a different candidate;
- native CAP/CP validation proves an already-admitted federation lease continues after the short authority leader lease expires, while a new Domain mutation fails closed;
- focused Phase 26 reference checks: **53 passed**;
- `aw-store` resource integration tests: **16/16 passed**;
- default-feature `aw-cli --all-targets`: **87 library + 48 binary/runtime + 6 provider integration tests passed**;
- minimal, `web`-only, and `net`-only configurations pass;
- actual compiled-binary two-node Phase 26 signed-vote/quorum smoke passes.

Phase 26 is an HA **fencing/leadership** primitive, not yet complete authority-store failover. A second member does not yet hold a complete semantic replica of the minimal Domain control state needed to promote itself after the current canonical authority host disappears. That is the next implementation boundary.

The environment-only release limitation remains unchanged: `cargo clippy` is unavailable in this connector toolchain and direct Nix validation cannot write `/nix/store`. Those gates must be rerun on a capable host and are not implied by the green native/reference evidence.

## 1. Repository state verified

Current checkpoint lineage before the Phase 26 commit:

- branch: `main`;
- Phase 25 HEAD/checkpoint: `d662c0e` (`phase-25`);
- branch was 15 commits ahead of `origin/main` before the Phase 26 checkpoint;
- package version remains `0.15.0`;
- current SQLite schema version is **11**;
- `Cargo.lock` is real/resolved and now records direct `aw-store` Ed25519/base64 verification dependencies;
- Phase 14–25 native/reference validation is recorded in history/current evidence;
- Phase 26 native validation is green; only replicated authority-state promotion/managed coordination and the separate full-host clippy/Nix gate remain next.

Committed phase chain through the previous checkpoint:

| Phase | Commit | Tag | Implemented boundary |
|---|---|---|---|
| 14 native checkpoint | `af067ef` + packaging follow-up `0091d12` | `phase-14-native` | native compile/test corrections for Phase 14 |
| 15 | `5495de3` | `phase-15` | rendezvous/STUN candidate discovery + direct QUIC hole punching |
| 16 | `32029f0` | `phase-16` | direct → UDP/QUIC relay → TLS/TCP relay fallback |
| 17 | `f82b5b5` | `phase-17` | authenticated typed MachineLink capability negotiation |
| 18 | `ff279fa` | `phase-18` | capability-bound remote read services |
| 19 | `81889bb` | `phase-19` | durable PeerWorkspaces + RemoteSessionRefs |
| 20 | `ea6d723` | `phase-20` | Fleet fan-out operation snapshots/records |
| 21 | `34bd76a` | `phase-21` | reusable bounded self-hosted UDP relay service |
| 22 | `06ef0d7` | `phase-22` | Domain federation capability leases |
| 23 | `a961ef8` | `phase-23` | semantic cross-store sync inbox/cursors |
| 24 | `9cddaeb` | `phase-24` | typed federation/Fleet orchestration and persistent remote Session control/live cancel |
| 25 | `d662c0e` | `phase-25` | explicit semantic materialization policies and immutable local remote-fact projections |

Phase 26 is documented separately in `PHASE26-DOMAIN-AUTHORITY-HA.md`.

## 2. What is fully implemented through Phase 23

### Phase 15 — direct reachability

Implemented and native-validated in history:

- temporary rendezvous service;
- host/server-reflexive candidate discovery;
- STUN binding/XOR-MAPPED-ADDRESS handling;
- direct UDP hole punching;
- authenticated direct QUIC route establishment;
- peer identity remains the stable Phase 14 Ed25519 identity rather than route/address identity.

### Phase 16 — route fallbacks

Implemented and native-validated in history:

- explicit route kinds and attempt traces;
- ordered direct QUIC → UDP/QUIC relay → TLS/TCP relay selection;
- one-pair raw UDP relay carrying opaque end-to-end QUIC datagrams;
- one-pair raw TCP relay carrying opaque bytes while peers establish end-to-end TLS;
- relays remain connectivity-only and do not create trust or Workspace authority.

### Phase 17 — MachineLink

Implemented and native-validated in history:

- typed authenticated MachineLink negotiation;
- signed capability intersection;
- capability lease semantics;
- generic reverse shell/tunnel semantics remain prohibited.

### Phase 18 — remote read services

Implemented and native-validated in history:

- remote Machine info;
- Workspace discovery;
- bounded read-only file listing/read;
- read-only Git status/review-oriented access;
- Session summary/list discovery;
- capability checks are explicit and typed.

Still intentionally absent from this phase: generic remote command execution and remote PTY service. `phase18_pty_service_implemented` remains false.

### Phase 19 — PeerWorkspaces and RemoteSessionRefs

Committed in `81889bb`, schema version 5.

Implemented:

- `remote_session_refs` on the source store;
- `remote_session_origins` on the executing/remote store;
- stable PeerWorkspace naming/placement derived from source store/Domain/Workspace identity rather than connection identity;
- idempotent remote Session creation keyed by source store + logical operation ID;
- immutable request digest binding;
- source-side `pending → unknown → active` RemoteSessionRef state;
- response-loss handling that marks the source outcome unknown rather than automatically replaying;
- exact retry can resolve to the already-created canonical remote Session;
- remote Session authority/transcript remains on the remote PeerWorkspace;
- authenticated source AwNode is bound to the MachineLink peer identity;
- explicit `remote-session` capability;
- CLI:
  - `aw net remote-session-serve`
  - `aw net remote-session-create`
  - `aw net remote-sessions`;
- hidden validation-only response-drop hook exists specifically to test unknown-effect/idempotent retry semantics.

Important boundary: this creates the durable remote Session and reference. It does **not** yet turn that Session into a generic remote process/model execution channel. The remote binding is deliberately created with an unassigned inference model/profile.

### Phase 20 — Fleet fan-out durability

Committed in `ea6d723`, schema version 6.

Implemented:

- `fleet_operations`;
- `fleet_operation_members`;
- exact Fleet `membership_revision` snapshot at logical operation creation;
- deterministic active member snapshot;
- exact-operation retry returns the original member snapshot even after Fleet membership changes;
- immutable operation ID + request digest binding;
- per-member:
  - pending;
  - active;
  - completed;
  - failed;
  - unknown;
  - skipped;
- optional RemoteSessionRef binding per member;
- optional result SHA-256 binding;
- aggregate Fleet operation states:
  - pending;
  - running;
  - partial;
  - completed;
  - unknown;
  - revoked;
- CLI:
  - `aw fleet plan`
  - `aw fleet runs`
  - `aw fleet run-update`.

Important boundary: this is the durable orchestration/fan-out record layer. It does not yet automatically dispatch a remote child Session to every selected Machine. Network dispatch orchestration still needs to consume these pinned member records.

### Phase 21 — reusable self-hosted relay

Committed in `34bd76a`.

Implemented:

- reusable multi-room UDP relay service;
- `aw net serve`;
- explicit:
  - service lifetime;
  - room TTL;
  - maximum room count;
- deterministic pairing using the existing signed AwNet registrations;
- opaque end-to-end QUIC datagram forwarding;
- room expiry and route cleanup;
- bounded admission;
- service statistics:
  - rooms created;
  - pairs matched;
  - packets forwarded;
  - rooms expired.

Security boundary remains unchanged: the relay does not trust peers, terminate application QUIC/TLS, issue MachineLink capabilities, or grant Domain/Workspace authority.

Important boundary: the reusable service is UDP/QUIC. The Phase 16 TLS/TCP fallback remains the bounded one-pair fallback primitive; a reusable multi-room TCP relay service has not been added.

### Phase 22 — federation capability leases

Committed in `06ef0d7`, schema version 7.

Implemented:

- durable directional Domain federations against an already-trusted AwNet peer;
- explicit scope JSON;
- explicit permission/action list;
- child-Domain inclusion and re-delegation are false by default;
- short-lived capability leases;
- lease snapshots bind:
  - federation policy epoch;
  - Domain policy revision;
  - Domain security epoch;
  - peer security epoch;
  - scope;
  - permissions;
- validation requires:
  - matching authenticated peer Node ID;
  - active/unexpired lease;
  - current federation/Domain/peer epochs;
  - explicitly permitted action;
- lease revocation;
- federation revocation increments policy epoch and revokes active leases;
- CLI:
  - `aw domain federate`
  - `federations`
  - `lease`
  - `lease-check`
  - `revoke-lease`
  - `revoke-federation`.

Important boundary: these leases are durable authority objects and local validation APIs. They are not yet transported/negotiated over a typed MachineLink federation service.

### Phase 23 — semantic cross-store sync

Committed in `a961ef8`, schema version 8.

Implemented:

- `semantic_sync_cursors`;
- immutable `semantic_sync_inbox`;
- typed `SemanticSyncBatch`;
- typed `SemanticSyncApply`;
- actor-filtered semantic commit-log export under a current `sync.read` lease;
- import under a current `sync.write` lease;
- monotonic source sequence handling while allowing authorization-created sequence gaps;
- source event ID + SHA-256 binding;
- event-ID rebinding protection;
- exact retry/idempotent import verification;
- local-store loop prevention;
- bounds:
  - max 512 events per batch;
  - max 256 KiB per event;
  - max 4 MiB CLI batch file;
- local sync-inbox inspection;
- CLI:
  - `aw domain sync-export`
  - `sync-import`
  - `sync-inbox`.

Canonical-state boundary: imported semantic events are **evidence only**. Sync import does not mutate local Sessions, branches, nodes, execution targets, providers, or tools. Materialization remains a separate future policy/action.

## 3. Validation evidence already obtained

### Phase 14–18

Native validation is recorded as complete in repository history/phase metadata, including actual binary/network smokes for the corresponding networking slices.

### Phase 19–23 reference/static checks

All current Phase 19–23 executable reference checks pass on the current working tree:

- Phase 19: 20 checks;
- Phase 20: 16 checks;
- Phase 21: 14 checks;
- Phase 22: 23 checks;
- Phase 23: 25 checks;
- total: **98 checks**.

Current source/static validator result:

- active files: 881;
- workspace manifests: 24;
- Python sources parsed: 53;
- TOML files parsed: 28;
- JSON files parsed: 103;
- shell scripts syntax checked: 4;
- JavaScript sources syntax checked: 8;
- Rust test attributes authored/not-run: 586;
- local links checked: 716;
- errors: 0;
- warnings: 0;
- `git diff --check`: clean.

The reviewed validation gate already includes the five Phase 19–23 **reference** steps.

## 4. Native validation status — do not overclaim

`phase.json` correctly says:

- Phase 19 native execution validated: false;
- Phase 20 native execution validated: false;
- Phase 21 native execution validated: false;
- Phase 22 native execution validated: false;
- Phase 23 native execution validated: false.

The later validation session was blocked by the connector sandbox, not by a known Rust failure:

- online Cargo attempted to refresh crates.io but the sandboxed CA/TLS path failed;
- `cargo --offline` could not see the host Cargo registry/index;
- `nix develop` needed explicit `nix-command flakes`, then failed because `/nix/store` was mounted read-only;
- therefore no complete native Cargo/Nix receipt exists for the exact Phase 19–23 tree.

Do not flip the Phase 19–23 native-validation booleans until the exact current worktree is compiled/tested on an unsandboxed host.

## 5. Current uncommitted validation worktree — DO NOT RESET

At the time of this note, HEAD is `phase-23`, but there are intentional post-Phase-23 validation fixes in the working tree.

Modified tracked files:

1. `crates/aw-git/src/process.rs`
   - behavior-preserving Clippy simplification of the accepted Git exit-code condition.

2. `crates/aw-store/src/lib.rs`
   - behavior-preserving fork predicate simplification.

3. `crates/aw-store/src/resources.rs`
   - replaces a large RemoteSession origin tuple with a named internal struct;
   - fixes query/iterator borrow lifetime shape;
   - drops the connection guard before recursively reacquiring it through `fleet_operation`, preventing a potential non-reentrant store mutex deadlock in Fleet member state updates.

4. `crates/aw-store/tests/resources.rs`
   - adds an assertion that a completed Fleet member settles the aggregate Fleet operation to `completed`.

5. `src/awnet/reachability.rs`
   - bounded wire enum receives an explicit `clippy::large_enum_variant` allowance to avoid adding an allocation solely for a bounded protocol packet.

Untracked but intentional native smoke scripts:

- the removed phase19_remote_session_smoke helper
  - pairs two nodes;
  - injects response loss after durable remote create;
  - verifies source state becomes unknown;
  - retries the exact operation;
  - verifies remote side reuses the original Session;
  - verifies exactly one canonical remote Session exists.

- the removed phase20_fleet_smoke helper
  - creates a Fleet;
  - snapshots an operation;
  - advances Fleet membership revision;
  - retries the exact operation;
  - verifies membership revision/member snapshot stays pinned;
  - rejects changed immutable intent;
  - verifies aggregate completion.

These changes should be compiled/tested and then committed as the native-validation correction checkpoint. Do not discard them just because they are not part of tag `phase-23`.

## 6. What still needs to be done immediately

### A. Native-compile/correct exact current tree

On an unsandboxed Nix/Cargo host, run at minimum:

```sh
cargo fmt --all -- --check
cargo metadata --locked --format-version 1
cargo check --locked --workspace --all-targets
cargo test --locked --workspace --all-targets
cargo test --locked --workspace --doc
cargo clippy --locked --workspace --all-targets -- -D warnings
RUSTDOCFLAGS='-D warnings' cargo doc --locked --workspace --no-deps
```

Prefer running these inside the repository's real Nix dev shell and then run the complete reviewed gate:

```sh
python3 the removed validation_gate helper run --profile full
```

### B. Execute/wire native binary smokes

Already authored but not yet executed/wired:

- Phase 19 actual-binary remote Session unknown/retry smoke;
- Phase 20 actual-binary Fleet snapshot/completion smoke.

Still to author and/or wire:

- Phase 21 reusable multi-room relay native smoke;
- Phase 22 federation lease native/CLI smoke;
- Phase 23 semantic sync native/CLI smoke.

The validation gate currently includes Phase 19–23 reference checks, not those native smoke scripts.

After the smokes are green, wire them into the Rust/full validation profile so future validation cannot regress to source-only evidence.

### C. Commit native-validation corrections

After native validation:

- commit the five tracked validation fixes;
- add/commit the Phase 19/20 smoke scripts;
- add any Phase 21–23 native smoke scripts;
- update `phase.json` native-validation booleans/status;
- create a native-validation tag/checkpoint after Phase 23;
- refresh this planning mirror.

## 7. Product work after native validation

The next product slices are **not** Phases 19–23 reimplementation. Those source phases already exist.

Next implementation work:

1. **Carry federation leases over MachineLink**
   - typed request/response service;
   - authenticated peer identity must match the lease;
   - remote side validates current epochs and action;
   - no bearer-style generic remote authority.

2. **Carry SemanticSyncBatch over MachineLink**
   - typed bounded batch stream;
   - `sync.read` / `sync.write` lease intersection;
   - preserve source sequence/event hashes;
   - reconnect must resume by durable cursor, never replay unknown canonical effects.

3. **Explicit semantic materialization policy**
   - selected remote facts may be converted into local canonical state only by a separate authorized operation;
   - inbox evidence never auto-mutates Session/branch/tool/provider state.

4. **Fleet dispatch orchestration**
   - consume Phase 20 pinned member records;
   - create/bind per-member RemoteSessionRefs;
   - preserve per-member unknown-effect state;
   - never re-evaluate Fleet membership on retry.

5. **Remote Session execution/supervision beyond creation**
   - Phase 19 creates durable remote Sessions, but the inference binding is deliberately unassigned;
   - add separately authorized typed prompt/run/supervision operations;
   - provider credentials remain with the executor;
   - no generic remote shell authority.

6. **Remote PTY capability**
   - still intentionally absent;
   - if implemented, it must be a separate explicit MachineLink capability from Session transcript read/control and model execution.

7. **Managed AwNet / coordination**
   - build on the exact same peer identity, relay, MachineLink, federation, and sync protocols;
   - do not introduce a privileged central trust plane.

8. **Domain AuthoritySet / quorum / failover**
   - mature federation/domain availability semantics without making every SQLite database multi-writer.

9. **Database ownership evolution only after dedicated gates**
   - `owner.lock` remains authoritative now;
   - direct multi-process access requires separate lease/WAL/migration/stress validation;
   - do not infer that AwNet/federation work authorizes multiple SQLite writers.

## 8. Invariants that must remain true

- one `aw` Rust binary;
- Domain → Workspace → Session durable ownership;
- every node has reserved `local` HostedWorkspace;
- remote delegated Sessions live in durable PeerWorkspaces;
- remote Session transcript authority lives where that Session lives;
- source stores a RemoteSessionRef, not a competing transcript;
- FleetWorkspace is virtual; Machine selection is a lens, not silent Session migration;
- retries use immutable logical operation identity;
- unknown effects are never automatically replayed;
- pairing creates peer identity trust only;
- relays provide connectivity only;
- MachineLink capabilities are explicit intersections;
- federation is directional and lease/epoch bounded;
- semantic sync imports evidence, not replicated canonical SQLite state;
- provider credentials stay with the executor/payer;
- no generic reverse shell/tunnel as the Workspace API;
- no global SQLite multi-writer assumption.

## 9. Recommended continuation order

A receiving agent should:

1. start at `.agents/plans/MASTER-HANDOFF.md` for the complete project/design chronology and document map;
2. use this file for the exact current Phase 19–23 implementation/native-validation boundary;
3. read `FULL-HANDOFF.md` plus `PHASE19-PEER-REMOTE-SESSIONS.md` through `PHASE23-SEMANTIC-SYNC.md`;
4. inspect the uncommitted validation worktree before editing anything;
5. native-validate and commit that worktree;
6. add/wire the missing native smokes;
7. only then begin MachineLink carriage for federation leases + semantic sync and Fleet dispatch orchestration.
