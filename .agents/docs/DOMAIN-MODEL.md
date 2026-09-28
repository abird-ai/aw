# Domains, Workspaces, Sessions, Machines and Fleets

**Status:** authoritative design direction after Phase 12. Design-only unless a section explicitly names implemented Phase 12 behavior.

## 1. Vocabulary

`Realm` is retired from new product/API design. Existing persisted or historical `realm_id` fields remain compatibility vocabulary until an explicit migration.

The product vocabulary is:

- **Principal** — something that can act: a User, Agent or Service.
- **User** — a stable human principal with an opaque `user_id`; login methods are bindings, not the user identity.
- **Domain** — hierarchical ownership, administration, identity and policy namespace. A Domain may represent a person, company, team, department, customer, project authority, autonomous collective or any other governing boundary. It is broader than “organization”.
- **Group** — a set of principals used for policy. Groups do not own arbitrary resource trees and are not interchangeable with Domains.
- **Workspace** — durable body of work and policy namespace that owns Sessions and execution defaults. Every Session belongs to exactly one Workspace.
- **Session** — durable conversation/task tree. Sessions never silently migrate between Workspaces.
- **Machine** — concrete computer/AwNode capable of hosting workspace resources and execution.
- **Fleet** — named group of Machines used for placement and aggregate views.
- **FleetWorkspace** — virtual Workspace authoritative on one Workspace authority set and projected across a Fleet. It is not copied to every fleet member and is not a merged filesystem.
- **PeerWorkspace** — hosted Workspace created on a remote AwNode for sessions delegated from another Workspace, normally named `peer:<source-workspace>-<hash>`.
- **AwNet** — independently administered connectivity network of AwNodes. It is not the owner of every Domain or Workspace.

## 2. Domain hierarchy

Domains may nest:

```text
Domain: Abird
├── Domain: Engineering
│   ├── Domain: Core
│   └── Domain: Health
├── Domain: Research
└── Domain: Operations
```

Parent/child is an administrative relationship, not automatic transcript visibility. A child may explicitly inherit selected identity-provider availability, policy templates, shared provider accounts, billing defaults or fleet access. Permission inheritance is policy-driven and can be denied by the child.

A User may belong to many Domains without creating several identities:

```text
User u_7d…
├── Personal
├── Abird
├── Client-X
└── OpenSource-Y
```

OIDC identities bind to `(issuer, subject)`, not email. Unix, passkey, local login and OIDC are authentication bindings to the stable User.

## 3. Domain authority

There is no required global aw controller. Each Domain has an AuthoritySet.

A personal Domain may be authoritative on one node:

```text
Domain Personal
AuthoritySet = [pvl-a1]
quorum = 1
```

A high-availability Domain may use several authority nodes:

```text
Domain Abird
AuthoritySet = [abird-1, abird-2, abird-3]
quorum = 2
```

Domain-authority changes, memberships, policy, federation grants, identity links and credential grants require the Domain's ordered authority path. If authority quorum is unavailable, no process invents new authority. Existing short-lived capability leases may continue until expiry.

## 4. Workspace semantics

Every aw installation has a reserved HostedWorkspace named `local`. Bare `aw` conceptually means:

```text
Domain    = user's default Domain
Workspace = local
Machine   = this AwNode
Session   = new durable Session in local
```

A Workspace is not necessarily one directory. It owns Sessions, workspace policy and execution defaults. Individual Sessions capture their actual execution binding such as canonical root/worktree, machine target, tool policy and provider selection.

Tools and agents may create Workspaces for isolation without moving the parent Session. Initial isolation modes can include:

- `SessionOnly`
- `ManagedDirectory`
- `ManagedWorktree`
- later `Sandbox`

Workspace creation is explicit and policy-governed.

## 5. Workspace kinds

Keep the implementation model small:

```text
Workspace = HostedWorkspace | FleetWorkspace
```

A HostedWorkspace lives on one AwNode/Workspace authority. Its origin is metadata:

```text
BuiltinLocal | UserCreated | ToolCreated | PeerCreated
```

Therefore `local`, `scratch`, `research`, and `peer:aw-dev-k7m2` use the same HostedWorkspace semantics.

A FleetWorkspace is virtual and remains authoritative on its Workspace authority host/replica set. Fleet machines provide member execution bindings; they do not become canonical replicas merely because they execute work.

## 6. Sessions belong to Workspaces

`workspace_id` is mandatory on every new Session. A Session's immutable history does not move just because the UI changes Machine or Workspace.

To continue work elsewhere, create a new/forked Session with explicit origin/context transfer:

```text
Session S1 @ Workspace A
       │
       └── Continue elsewhere
             ↓
Session S2 @ Workspace B
origin = S1/head + authorized context manifest
```

This preserves provenance, execution bindings, permission lineage and provider/tool authority.

## 7. PeerWorkspaces

When Workspace `aw-dev` delegates a Session/subagent to another AwNode, the remote node normally resolves or creates:

```text
peer:aw-dev-k7m2
```

The suffix derives from stable source identity, for example:

```text
source_store_id + source_domain_id + source_workspace_id
```

not the temporary connection ID. Reconnect therefore reuses the same PeerWorkspace.

A PeerWorkspace is durable. Remote Sessions created there are authoritative on that remote Workspace. The source stores a `RemoteSessionRef` rather than a competing canonical transcript copy.

The remote machine user can switch locally to that PeerWorkspace and inspect/control the remote agents according to local policy. Execution supervision, transcript read, and Session control should remain separable capabilities.

## 8. Machines and Fleets

A Machine is a concrete AwNode with a stable cryptographic `machine_id` independent of hostname/IP/user identity. A Machine may belong to several Domains/Fleets through explicit policy.

A Fleet is a grouping/placement object, not a credential grant or distributed filesystem.

Every fleet operation pins the exact membership/binding revision it uses. Membership changes affect future operations, not already-admitted work.

## 9. FleetWorkspace

A FleetWorkspace lives on its authority host/replica set:

```text
Domain: Abird
Workspace: gpu-dev            (virtual)
Fleet: gpu-lab
├── pvl-a1 -> /home/pvl/src/aw
├── pvl-x2 -> /srv/src/aw
└── build-3 -> /work/aw
```

Member paths may differ. A missing mapping is `Unbound`; never guess another path.

Sessions created in `gpu-dev` belong to `gpu-dev`, even if a particular Session targets only `pvl-x2`.

## 10. Workspace lens

The UI mental model is:

```text
Domain / Workspace          On: Machine-or-Fleet
```

Examples:

```text
Personal / local            On: Local
Abird / gpu-dev             On: Fleet gpu-lab
Abird / gpu-dev             On: pvl-x2
Research@peer / experiment  On: gpu-server
```

`On` is a view/execution lens. Changing it updates Follow views and defaults; it does not silently retarget an admitted Session.

Resource views use:

```text
FollowWorkspaceLens
PinnedMachine(machine_id, workspace_binding)
FleetAggregate(fleet_workspace_id)
```

Pinned tabs stay pinned while the lens changes.

## 11. Domain sharing and federation

A peer connection may expose selected Workspaces, Machines, Fleets, or a whole Domain, but the Domain remains the policy authority.

Logical federation is durable and independent of one physical tunnel:

```text
DomainFederation {
  source_domain
  destination_domain
  direction
  maximum_resource_scope
  permitted_actions
  delegation_policy
  policy_epoch
}
```

Scopes may be dynamic (`AllDomain`, `AllWorkspaces`, `AllMachines`, `AllFleets`) or explicit resource IDs. Child Domains are not silently included; including descendants is explicit and every child still enforces its own policy.

Federation is directional. A↦B and B↦A may have different privileges.

No transitive re-sharing is implied. Re-delegation requires an explicit capability and provenance.

## 12. Effective authority

An operation is permitted only by the intersection of all relevant ceilings:

```text
peer/federation capability ceiling
∩ Domain policy
∩ Workspace/resource policy
∩ authenticated Principal authority
∩ current delegation/capability lease
∩ tool/provider specific policy
```

A downstream layer cannot widen an upstream grant. Provider credential authority remains separate from transcript/workspace access.

## 13. Provider credentials

Workspace location never selects provider credentials implicitly. All are valid:

```text
Workspace on pvl-x2 -> model on user's laptop
Workspace on pvl-x2 -> Ollama on pvl-x2
Workspace on pvl-x2 -> shared server account
```

The user/account/payer/custody/executor/model selection remains explicit. Personal credentials stay on user-controlled executors; shared credentials use SecretStore references. Offline personal credentials never fall back silently to a shared account.

## 14. Non-negotiable invariants

1. Every Session belongs to exactly one Workspace.
2. `local` is the reserved default HostedWorkspace.
3. Workspace is a durable namespace/policy boundary, not necessarily a directory.
4. FleetWorkspace is virtual and remains authoritative on its Workspace authority set.
5. Machine/Fleet selection never silently changes Session history or provider credentials.
6. Remote delegated Sessions are first-class durable Sessions in PeerWorkspaces.
7. Tunnel/route loss never deletes Workspaces/Sessions or proves an effect did not occur.
8. Domain is the ownership/policy boundary; Groups only group Principals.
9. Parent Domains do not automatically read child content.
10. Domain federation survives individual transport connections and is non-transitive by default.
