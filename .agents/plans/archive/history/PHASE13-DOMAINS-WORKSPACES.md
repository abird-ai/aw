# Phase 13 — Domain / Workspace / Machine / Fleet foundation

**Source candidate 0.14.0. Rust/Cargo/Nix compilation has not run in the authoring environment.** This phase is the first implementation slice of the consolidated AwNet/Domain/Fleet design. It makes the new durable resource vocabulary real in the existing SQLite store and canonical CLI while deliberately **not** claiming QUIC transport, remote execution, Domain federation runtime, direct multi-process SQLite, OIDC, or managed AwNet.

## 1. What becomes first-class

The durable hierarchy is now:

```text
Domain
  Workspace
    Session

Machine                 execution location
Fleet                   named Machine group
FleetWorkspace           virtual Workspace targeting a Fleet
```

Every durable Session has one `workspace_id`. Each Domain gets a reserved `local` HostedWorkspace on this AwNode; ordinary `aw` therefore continues to behave like the old local agent without requiring the user to configure a Domain or Workspace first.

The migration preserves historical principal/realm strings. Existing legacy realm membership is mapped to stable Domain records rather than rewriting transcript authorship. New APIs and UI language use **Domain**; historical serialized `realm_id` fields remain compatibility data until a later explicit object/protocol migration.

## 2. Schema 4

`004_domains_workspaces_fleets.sql` upgrades schema 3 to 4 and adds:

- `domains` and `domain_memberships`;
- a durable local `machines` / AwNode identity foundation;
- `workspaces` with Hosted and FleetVirtual kinds;
- `peer_workspace_origins` for the later stable `peer:<source>-<hash>` model;
- `fleets`, `fleet_members`, immutable fleet-membership events;
- `fleet_workspaces` and member binding records;
- `sessions.workspace_id` and `session_execution_targets`;
- secret-free `awnet_peers` and directional `domain_federations` metadata;
- Domain scope on the separate security-audit plane.

The migration backfills one Domain per legacy realm, creates the reserved `local` Workspace for each Domain, creates one stable local Machine/Node ID for this store, and binds existing Sessions to their owner's local Workspace and Machine. It leaves `owner.lock` in place and changes no network listener.

Python SQLite reference tests execute 001→002→003→004 and representative legacy backfill. This does **not** validate rusqlite/Rust integration.

## 3. Store API behavior

`aw-store` now exposes bounded resource types and operations for Domains, Workspaces, Machines, Fleets and Session execution targets. Resource lookup is authorization-filtered through current Domain membership. Ambiguous display names fail rather than selecting an arbitrary Domain/Workspace/Fleet.

Important invariants:

- `local` is reserved and cannot be archived;
- creating a Domain also creates its reserved local Workspace;
- creating a normal Workspace produces a HostedWorkspace on this AwNode;
- creating `--fleet FLEET` produces a virtual FleetWorkspace; it has no local filesystem host;
- a Session created in a FleetWorkspace captures the Fleet membership revision in its target snapshot;
- ordinary HostedWorkspace Sessions default to this local Machine;
- `/new` preserves the current Session's Workspace and execution target;
- a context-flow `new_session` destination stays in the source target Workspace instead of falling back to `local`;
- opening a non-local Machine/Fleet Session for execution currently fails explicitly because the QUIC dispatcher is not implemented yet.

That last behavior is deliberate: persisted future topology must never cause local tools to run while pretending they ran remotely.

### Phase 13 correction hardening

A post-backup correction pass tightens the same boundary without advancing to QUIC:

- `--domain` is resolved to one authorized Domain before Workspace/Fleet selection, so duplicate display names in different Domains are never selected accidentally;
- selecting a Domain without an explicit Workspace resolves that Domain's reserved `local` Workspace rather than silently falling back to another Domain;
- Machine display-name lookup fails on ambiguity and `--domain` is honored by `machine show`, `fleet show`, and `fleet add`;
- a persisted `Local` execution target must name the store's exact `local_machine_id`; callers cannot label another Machine as local;
- explicit Machine/Fleet targets are fenced to the Session Workspace's Domain; FleetWorkspace Sessions must target their bound Fleet; and impossible future Fleet membership revisions are rejected.

These are source-level corrections pending the same native Rust/rusqlite acceptance gate below.

## 4. Canonical CLI now has executable foundations

The top-level immediate-run grammar is now authored as:

```text
aw [RUN OPTIONS] [PROMPT...]
```

Multiple positional words are joined as the prompt, so both forms are natural:

```bash
aw "fix the failing parser"
aw fix the failing parser
aw --workspace aw-dev --profile coding --model MODEL fix the tests
```

Current canonical run modifiers include `-w/--workspace`, `--domain`, `--on`, `-p/--profile`, `-m/--model`, and `-a/--account`. The existing provider configuration still defines what a stored profile contains in this source candidate; the broader execution-profile design remains incremental.

Resource commands use noun → verb:

```bash
aw workspace list|ls
aw workspace create NAME [--domain DOMAIN] [--fleet FLEET]
aw workspace show NAME
aw workspace archive NAME
aw workspace open NAME [run options]

aw domain list|ls
aw domain create NAME [--parent DOMAIN]
aw domain show NAME

aw machine list|ls
aw machine show MACHINE

aw fleet list|ls
aw fleet create NAME
aw fleet show NAME
aw fleet add FLEET MACHINE

aw session list|ls [--workspace WORKSPACE]
aw session show SESSION
aw session tree SESSION
aw session open SESSION [run options]

aw net status
aw net peers
```

`aw workspace open NAME` opens the native Workspace presentation. `aw --workspace NAME PROMPT...` performs immediate work in that durable Workspace without implying fullscreen UI.

`aw agent` is authored as the canonical long-lived local AwNode/owner role and reuses the existing owner implementation. The historical `serve` spelling remains a compatibility path for now. If `--socket` is omitted, `aw agent` uses the private state directory's `owner.sock`.

`aw net status` reports the durable local node/store identity and explicitly reports transport as `not_enabled`. `pair`, `join`, `serve`, `relay`, remote `--on`, and QUIC are intentionally not fake-stubbed as working network operations.

## 5. Presentation and execution separation

The current Phase 13 `--on` implementation accepts only aliases for the current machine. A remote value fails with a message saying that the AwNet QUIC dispatcher is not enabled yet.

This preserves the accepted model:

```text
Workspace  = durable namespace containing Sessions
On         = execution/view location
Session    = durable task/conversation pinned to its admitted execution target
```

Future Machine/Fleet target switching will use the same fields rather than changing Session ownership or inventing another session system.

## 6. Semantic events

Domain, Workspace, Fleet and Session creation/membership mutations append bounded semantic `commit_log` events in the same transaction as their durable state. `commit_events_after` now permits either:

- Session-scoped events when the caller can currently read that Session; or
- Domain-scoped control-plane events when the caller is a current active Domain member.

Unknown event scope fails closed. This extends the Phase 12 durable event feed without turning it into a raw database/WAL subscription.

## 7. Explicitly not implemented yet

This phase does **not** claim:

- QUIC/direct peer transport;
- rendezvous, STUN or relays;
- `aw net pair`, `join`, `serve`, or `relay` runtime;
- remote Machine workspace execution;
- `peer:<source>-<hash>` automatic creation/runtime;
- live Domain federation/capability leases;
- Fleet fan-out execution or aggregate workspace UI;
- self-hosted/managed AwNet control plane;
- Domain authority quorum/writer failover;
- cross-store semantic synchronization;
- OIDC/passkey/password auth services or SecretStore backend;
- removal of `owner.lock` or direct multi-process SQLite writers.

Those build on the durable IDs/ownership/target records introduced here. Do not bypass the missing transport by interpreting remote roots locally or by restoring provider credentials from stored topology.

## 8. Required native acceptance

Before building the QUIC layer, run the real host validation and correct all Rust/rusqlite/Clap issues. At minimum verify with the compiled binary:

1. schema1/2/3 stores migrate to schema4 with foreign keys clean;
2. old Sessions land in the correct Domain/local Workspace and preserve history/retry records;
3. `aw PROMPT...` creates a durable Session in `local`;
4. `aw --workspace W PROMPT...` creates in W and refuses ambiguous names;
5. `aw workspace open W` opens the same durable Workspace without silently changing an existing Session target;
6. `/new` and context-flow new-session keep Workspace/target provenance;
7. FleetWorkspace creation is virtual and a new Session pins current Fleet membership revision;
8. attempting to execute a non-local target fails before any local model/tool effect;
9. Domain/Workspace/Fleet `commit_log` events are permission-filtered;
10. `aw agent` uses the existing durable owner lifecycle and restart/retry semantics.

Then implement AwNode QUIC identity/pairing as the next networking phase, not a generic reverse shell.
