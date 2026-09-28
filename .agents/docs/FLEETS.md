# Machines, Fleets and Virtual Fleet Workspaces — design amendment v1

**Status:** accepted design amendment; no runtime implementation claim.
**Baseline:** Phase 12 / source 0.13.0.
**Purpose:** make local and remote machines first-class workspace execution targets, group machines into fleets, and allow virtual fleet-level workspaces stored on the main aw host while targeting all machines in a fleet.


> **Authoritative refinement:** read [Domains](DOMAIN-MODEL.md) and [AwNet](AWNET.md) together with this file. The latest model is: every Session belongs to exactly one Workspace; every AwNode has a reserved `local` HostedWorkspace; remote delegated Sessions normally live in durable `peer:<source>-<hash>` PeerWorkspaces on the remote node; a FleetWorkspace is virtual and remains authoritative on its Workspace authority set; and the machine selector is a Workspace lens (`On:`), not a command that migrates existing Sessions. `MachineLink` below should be read as the typed aw peer protocol carried over AwNet routes, not a separate product or permanent master/agent topology.

## 1. Product model

Add three durable concepts without creating a second agent/session system:

```text
Machine
  a concrete computer running `aw agent`

Fleet
  a named group of Machines

Workspace
  MachineWorkspace -> one concrete machine/root
  FleetWorkspace   -> virtual workspace definition stored on the main host,
                      targeting a fleet and resolving a member workspace on each machine
```

The main host is also a Machine. In the UI, **Local** is not a special implementation: it is the main host's local Machine target using an in-process transport. A remote machine uses the same resource contracts through an authenticated MachineLink.

A Fleet is a grouping and scheduling target, not a credential grant, user identity, filesystem merge or distributed database. A Machine can belong to multiple fleets. Fleets are not nested in the first implementation; use tags/selectors instead.

## 2. `aw agent`: the machine-side runtime

`aw agent` is the **Machine Agent**, not another LLM-agent implementation. It runs on each registered machine and establishes an outbound authenticated tunnel/link to the main aw host.

```text
Main aw host / workspace authority
  durable sessions + policy + fleet/workspace definitions
  browser/TUI/native workspace
  semantic commit log
          ^
          | authenticated multiplexed MachineLink
          | (outbound from remote host; reconnectable)
          v
Remote machine
  aw agent
    Machine identity
    local workspace registry
    Files / Git / PTY / process / logs / metrics services
    optional machine-local model executor accounts
```

The Machine Agent exposes typed capabilities. The transport must not be an unrestricted reverse TCP tunnel by default. A later explicit port/preview forwarding capability may exist, but it is separately authorized and bounded.

The link should be transport-pluggable. Same-host uses an in-process/local backend; remote deployment may use a mutually authenticated TLS/Noise/SSH-derived transport. Correctness comes from typed request IDs, durable operation receipts where effects are possible, current authorization and reconnect semantics—not from assuming a connection stayed alive.

### 2.1 Machine identity

A machine has a stable opaque `machine_id`, independent of hostname and user identity.

```text
Machine {
    machine_id,
    display_name,
    public_key/fingerprint,
    status,
    labels/tags,
    os/arch,
    capability_revision,
    security_epoch,
    created_at,
    last_seen_at
}
```

Hostname, IP and display name may change. They are metadata, not identity. Machine identity is separate from:

- authenticated human `user_id`;
- personal credential `user_device`;
- provider account;
- inference executor identity;
- sync-peer identity.

One physical computer may serve several roles, but the records are not implicitly interchangeable.

## 3. Machine workspaces

A Machine Agent advertises only explicitly registered workspaces, not its whole filesystem.

```text
MachineWorkspace {
    machine_workspace_id,
    machine_id,
    key,                 // e.g. "aw", "abird-health"
    display_name,
    canonical_root,
    project/repo identity?,
    capabilities,
    revision
}
```

The main host addresses the opaque workspace ID/key. Browser/UI payloads never provide arbitrary remote absolute paths as authority.

A machine workspace owns the remote execution scope for Files, Git, terminals, process tools, previews and other workspace resources. Model inference remains independently placed: it can be server-held, user-device-held or machine-local only when an explicit provider account/executor grant says so.

## 4. Fleets

A Fleet is a durable main-host object:

```text
Fleet {
    fleet_id,
    name,
    description?,
    membership_revision,
    default_execution_policy,
    policy_ref
}

FleetMember {
    fleet_id,
    machine_id,
    state,
    joined_at,
    labels_override?
}
```

Adding a machine to a fleet changes future target resolution. It does not silently add the machine to a run already admitted.

Every fleet execution pins an exact member snapshot:

```text
FleetTargetSnapshot {
    fleet_id,
    membership_revision,
    members[]: machine_id + machine_workspace_binding + capability_revision
}
```

This prevents a newly joined machine from unexpectedly receiving an in-flight command.

## 5. Virtual Fleet Workspaces

A **FleetWorkspace** is a virtual workspace stored on the **main host**. It is not copied to every machine and does not imply a merged filesystem.

```text
FleetWorkspace {
    workspace_id,
    name,
    fleet_id,
    binding_policy,
    default_target_mode,
    layout_ref,
    policy_ref,
    revision
}
```

It resolves a corresponding machine workspace for each fleet member. Paths do not need to match across machines.

Example:

```text
Fleet: gpu-lab
  pvl-a1
  pvl-x2

FleetWorkspace: aw-dev     (stored on main host)
  project key: "aw"

Bindings:
  pvl-a1 -> machine workspace "aw" -> /home/pvl/src/aw
  pvl-x2 -> machine workspace "aw" -> /srv/src/aw
```

A binding can be explicit per machine or selected through a reviewed key/label policy. A fleet member that lacks a valid binding appears as **Unbound**, never silently mapped to another path.

### 5.1 Dynamic workspace membership, pinned execution

The FleetWorkspace itself normally follows current Fleet membership. Its UI aggregate therefore updates as machines are added/removed.

But each admitted prompt, task, pipeline step or broadcast action pins the membership/workspace bindings it actually targets. A later fleet membership change applies only to later operations unless an explicit operation requests refresh/re-targeting.

## 6. Workspace target/lens — instant local-like machine switching

The workspace header gets a fast **Target** picker. It can select:

```text
Local
pvl-a1
pvl-x2
Fleet: gpu-lab
```

The selected value is the workspace **active target/lens**.

When the user selects a Machine, every unpinned workspace view that follows the active target behaves as though aw were running locally on that machine:

- Files -> that machine workspace root;
- Git -> that machine's repository/worktrees/changes;
- New terminal -> PTY on that machine;
- Logs/metrics -> that machine;
- previews/services -> that machine when capability permits;
- New session -> targets that machine by default;
- human file/diff actions -> execute against that machine binding.

The transport being remote is visible in a compact status chip, but it should not make ordinary workspace navigation feel like a separate product.

Example:

```text
[ aw-dev ▾ ] [ pvl-x2 ▾  connected · 18ms ]

Files           MAIN
src/            Agent
Cargo.toml      Terminal
...             Diff
```

Switch to `pvl-a1`, and follow-target views rebind to pvl-a1 without rebuilding the entire layout.

### 6.1 Follow vs pinned views

Each resource view has target behavior analogous to existing worktree scopes:

```text
FollowActiveTarget
Pinned(machine_id, machine_workspace_id)
FleetAggregate(fleet_workspace_id)
```

Switching the Target picker updates **followers**. Pinned tabs remain where they are and show a machine badge. This makes comparison natural:

```text
File: config.toml · pvl-a1
File: config.toml · pvl-x2
```

Moving/resizing a tab never changes its machine.

### 6.2 Sessions do not silently migrate

A running or already-executed session is pinned to its durable execution target. Switching the workspace lens does **not** silently move the session's tool root to another machine.

For a machine-pinned session, the UI offers explicit actions such as:

- **Open target machine** — switch workspace lens to the session's machine;
- **Continue on pvl-x2** — fork/new session with reviewed context transfer and pvl-x2 execution binding;
- **Retarget new task** — new logical operation, not a rewrite of old execution provenance.

This preserves the existing rule that view navigation cannot mutate a running execution binding.

## 7. Fleet aggregate workspace view

Selecting the Fleet in a FleetWorkspace changes followers into aggregate fleet projections instead of pretending all machines form one filesystem.

Recommended default behavior:

| Surface | Fleet lens |
|---|---|
| Files | Machines as top-level roots; optional common-path comparison later. |
| Git | Matrix/list of branch, HEAD, dirty state and worktrees per machine. |
| Sessions | Fleet sessions plus machine-pinned sessions filtered to fleet members. |
| Terminal | Requires selecting one machine; **Broadcast Terminal** is a separate explicit high-risk action. |
| Logs | Merged stream with mandatory machine labels/filter. |
| Metrics | Fleet overview + per-machine drilldown. |
| Preview | One selected machine or explicit tiled views; never ambiguous localhost. |
| Agent conversation | Fleet-targeted session if user starts one in fleet scope. |

No fleet view silently edits every machine just because the aggregate tree is visible.

## 8. Session and task execution targets

Extend the durable execution binding with an explicit target:

```text
ExecutionTarget =
    LocalMachine(machine_id, machine_workspace_id)
  | Machine(machine_id, machine_workspace_id)
  | Fleet(fleet_workspace_id, placement_policy, membership_snapshot)
```

A session can therefore be:

```text
Session A -> pvl-a1
Session B -> pvl-x2
Session C -> Fleet gpu-lab
```

The session history remains one immutable conversation tree. Execution placement is durable provenance and current authority, not part of the node parent graph.

### 8.1 Fleet placement policies

A Fleet target must say what "target the fleet" means. Do not infer it from a group name.

First-class modes:

```text
All             // every eligible bound member
Selector        // machines matching labels/capabilities
One             // explicit machine within fleet
AnyHealthy      // scheduler picks one eligible member
Quorum(n)       // later/advanced automation use
```

For an interactive session created while the fleet lens is active, default can be **All** only when the action is safe for fan-out and the user can see the member count. A mutating tool call still requires explicit fleet scope in its typed command.

## 9. How a fleet-targeted session executes

A Fleet session is not N copied transcripts and does not create N hidden parent branches.

The main session remains the coordinator conversation. A fleet operation expands into durable per-machine target runs:

```text
Fleet session S
  prompt P
    FleetOperation FO-17
      MachineRun pvl-a1 -> completed
      MachineRun pvl-x2 -> completed
      MachineRun pvl-a3 -> offline
    aggregate result
```

`MachineRun` records are execution children, not conversation parents. They contain target machine/workspace, request/attempt IDs, effect leases, outputs/artifacts and status.

The agent/pipeline can optionally fan out LLM work as well, but ordinary remote Files/Git/PTY/tool execution does not require a second LLM per machine. The same headless agent/runtime contracts are reused.

If a machine result deserves its own continuing conversation, **Open as machine session** explicitly creates/forks a real session targeted to that machine.

### 9.1 Fleet result semantics

Per-machine state is always visible:

```text
pvl-a1  completed
pvl-x2  completed
pvl-a3  offline / waiting
```

Fleet success policy is explicit (`all`, `best_effort`, `quorum`, etc.). Partial success is never flattened into generic success.

## 10. Tool and filesystem semantics

A Machine target turns existing tools into machine-scoped operations.

A Fleet target adds a placement dimension; it does not make existing `read`, `edit` or `bash` silently broadcast.

Examples:

```text
read(path)                       // machine session: that machine
fleet.read(machine, path)        // fleet session: one explicit member
fleet.exec(scope=all, command)   // explicit fan-out
fleet.edit(scope=selector, ...)  // explicit, version-checked fan-out
```

The exact public tool shape can remain compact, but the authority must be typed.

Mutating fleet operations:

1. pin the fleet member snapshot;
2. authorize each target machine/workspace;
3. acquire each machine's execution lease/resource fence;
4. journal the per-machine intent before effect;
5. execute independently/parallel within bounds;
6. retain every machine outcome, including unknown effects;
7. never roll back other machines by pretending the distributed operation is atomic.

No distributed filesystem transaction is promised.

## 11. Durable tunnels, reconnect and offline state

A Machine connection is ephemeral; Machine identity/workspace/session state is durable.

```text
Machine identity: durable
Fleet membership: durable
Fleet workspace manifest: durable
Session target: durable
MachineRun/operation receipts: durable
Current tunnel connection: ephemeral
```

If a tunnel drops:

- historical workspace/session state remains readable from main-host storage;
- a machine-pinned operation remains targeted to that machine;
- effectful ambiguous calls retain unknown/reconciliation state;
- no silent migration to another machine occurs;
- Fleet operations use their persisted placement/retry policy to wait, fail, or retry that member;
- autonomous retry creates a new durable attempt, following the Phase 12 retry model.

Reconnect establishes a new `connection_instance_id` under the same authenticated `machine_id` and capability revision.

## 12. Fleet workspaces and durable retry/autonomy

Fleet fan-out composes with Phase 12 durability:

```text
FleetOperation
  machine target pvl-a1
    logical operation
      attempt 1 -> unknown
      attempt 2 -> completed
  machine target pvl-x2
    logical operation
      attempt 1 -> completed
```

Interactive fleet sessions expose retry hints per machine. Autonomous sessions can persist per-machine retry policies and generation-fenced claims. A retry does not change machines unless the placement policy explicitly allows `AnyHealthy` and the operation is defined as relocatable.

A machine-pinned effect is **not relocatable** merely because another machine is online.

## 13. Main-host database additions

Fleet/machine state belongs in the main durable control plane and semantic `commit_log`.

Target logical records:

```text
machines
machine_identities / keys
machine_connections       // current projection + connection instance
machine_capabilities
machine_workspaces

fleets
fleet_members
fleet_membership_events

workspaces                // main-host logical workspace definitions
fleet_workspaces
fleet_workspace_bindings

session_execution_targets
fleet_operations
machine_runs

machine_policy_events
```

Important semantic events include:

```text
MachineRegistered
MachineConnected
MachineDisconnected
MachineCapabilityChanged
MachineWorkspaceChanged
FleetCreated
FleetMembershipChanged
FleetWorkspaceChanged
SessionTargetBound
FleetOperationAdmitted
MachineRunStarted
MachineRunSettled
MachineRunNeedsAttention
```

Presence/latency/heartbeat detail is short-lived current state; security-sensitive registration/trust changes also emit separate `audit_events`.

## 14. Authorization and credential boundaries

A Fleet is never an authority union.

For each machine operation the owner must validate:

```text
human/service principal
  + workspace permission
  + machine/fleet permission
  + resource/tool permission
  + current policy epoch
  + effect lease
```

Fleet membership does not grant:

- access to a machine;
- access to all of its workspaces;
- shell authority;
- provider credentials;
- another user's personal inference executor.

### 14.1 Provider accounts

Provider placement remains independent from workspace placement.

Possible valid combinations:

```text
UI on laptop -> workspace pvl-x2 -> model via user's personal laptop account
UI on laptop -> workspace pvl-x2 -> model via pvl-x2 local Ollama account
UI on laptop -> workspace pvl-x2 -> model via team server account
```

Each combination requires an explicit permitted account/executor. Switching the workspace Machine dropdown never changes the model account automatically.

## 15. Fleet-level virtual workspace storage and sync

The FleetWorkspace manifest is authoritative on the main host. Machines receive only the bounded configuration/request information needed to serve their registered MachineWorkspace.

Do not replicate the entire FleetWorkspace layout/config to every machine as another source of truth.

A future main-host replica may synchronize the virtual workspace through the existing semantic sync architecture, but fleet members are execution endpoints, not automatically full control-plane replicas.

## 16. Preview and localhost semantics

A remote Machine workspace needs explicit preview routing because `localhost` in the user's browser means the browser device.

A machine preview resource therefore has identity:

```text
PreviewResource {
    machine_id,
    workspace_id,
    service_id,
    local_port/path,
    expiry,
    owner/session
}
```

The MachineLink may expose a bounded authenticated preview bridge. The browser receives an aw-controlled URL/resource, never a generic unrestricted host:port tunnel. Moving a preview tab does not restart its service.

## 17. UI model

The normal header should make target switching extremely fast:

```text
[ Workspace: aw-dev ▾ ] [ Target: pvl-x2 ▾ ] [ connected · 18ms ]
```

Target picker groups:

```text
This host
  Local

Machines
  pvl-a1        connected
  pvl-x2        connected
  build-3       offline

Fleets
  gpu-lab       2/2 online
  builders      5/7 online
```

Keyboard action: `Switch target…` opens the same fuzzy picker. Recent targets appear first.

When a FleetWorkspace is selected, choosing one member is a **drill-down lens**; choosing the Fleet returns to aggregate mode. The workspace/layout itself stays the same main-host virtual workspace.

## 18. Interaction examples

### 18.1 Remote machine feels local

```text
Target: Local
  Files/terminal/git are local

switch -> pvl-x2

Target: pvl-x2
  same pane layout
  Files now pvl-x2:/srv/src/aw
  Terminal starts on pvl-x2
  Git shows pvl-x2 state
  new session defaults to pvl-x2
```

No SSH terminal/window context switch is exposed as a separate product workflow.

### 18.2 Fleet workspace

```text
Workspace: inference-lab
Target: Fleet gpu-lab

Git matrix
  pvl-a1   main @ abc123 clean
  pvl-x2   main @ abc123 dirty +2

Sessions
  fleet benchmark #42
  pvl-a1 debugging
  pvl-x2 model install
```

Switch Target -> pvl-x2 and the same workspace becomes a machine-local lens into pvl-x2.

## 19. Safety rules and non-goals

Do not:

1. make a Fleet appear to be one merged writable filesystem;
2. silently retarget an existing/running session when the UI target changes;
3. use Fleet membership as a permission grant;
4. silently broadcast a terminal command or write to every machine;
5. move a machine-pinned ambiguous effect to another machine on retry;
6. send provider keys through the MachineLink merely because a workspace is remote;
7. make every Machine Agent a full database replica;
8. treat tunnel connectivity as durable execution state;
9. assume two machines have the same path/repository just because workspace keys match;
10. add a newly joined fleet member to an already admitted run;
11. hide partial fleet failure behind one green result;
12. turn a generic reverse port tunnel into the default machine protocol.

## 20. Implementation sequence

This is a design amendment. Recommended implementation order after the current Phase 12 compiler/runtime gate:

1. **Machine identity + local abstraction** — model the main host as a Machine, introduce target-aware workspace service interfaces without remote transport.
2. **Machine Agent + secure link** — outbound authenticated `aw agent`, registration, capabilities, machine workspaces, reconnect.
3. **UI target lens** — fast Local/Machine picker; Follow/Pinned resource semantics; Files/Git/PTY over the same service interfaces.
4. **Durable machine targets** — persist session execution target and machine-run provenance; integrate recovery hints/commit log.
5. **Fleet registry** — fleets, memberships, epochs, fleet picker and aggregate machine/session views.
6. **Virtual FleetWorkspace** — main-host manifest + per-machine workspace binding; aggregate Files/Git/logs/metrics views.
7. **Fleet execution** — durable FleetOperation + MachineRun fan-out, explicit placement/success policy, retries and partial outcomes.
8. **Preview bridge and richer remote surfaces** — typed bounded preview/services routing.
9. **Autonomous fleet pipelines/scheduling** — after the general pipeline runtime exists and machine/fleet durability is proven.

Do not build fleet-wide writes before machine-targeted execution, leases, retry lineage and partial-result UI are correct.

## 21. Acceptance requirements

Before calling remote-machine/fleet support complete:

- Local target and remote target use the same workspace reducers/renderers.
- Switching Machine target updates all Follow views without moving pinned tabs.
- Existing running session does not silently retarget.
- A new session inherits the currently selected target.
- Remote Files/Git/terminal behavior uses target machine authority and no local-path confusion.
- Tunnel loss preserves durable session/run identity and does not replay uncertain effects.
- Reconnect to same machine ID resumes observation without duplicating work.
- Fleet operation pins membership revision and does not absorb new members mid-run.
- Per-machine result/unknown/retry state is durable and visible.
- Partial fleet completion stays partial.
- Fleet retry never silently relocates machine-pinned effects.
- Personal provider key remains on its configured personal executor.
- Machine-local provider account is used only when explicitly selected/granted.
- Fleet membership adds no authority by itself.
- Fleet virtual workspace remains main-host authoritative and member path differences are explicit.
- Aggregate Fleet Files view never masquerades as a transactional merged filesystem.
- Broadcast terminal/write requires an explicit high-risk action and target snapshot.


## 18. Workspace-first and PeerWorkspace refinement

The durable hierarchy is **Domain → Workspace → Session**. `local` is the universal default HostedWorkspace. Machines are execution locations, not Session containers. A user/tool may create additional HostedWorkspaces for isolation.

When a source Workspace delegates an agent/subagent to a remote AwNode, the remote node normally resolves or creates a durable PeerWorkspace named `peer:<source-workspace>-<short-stable-hash>`, where the hash derives from stable source store/Domain/Workspace identity rather than the connection ID. Remote child Sessions are canonical in that PeerWorkspace. The source stores a RemoteSessionRef and may subscribe/summarize/import permitted child results; it does not maintain a competing transcript copy.

The remote node's local user can switch into the PeerWorkspace and supervise its remote agents according to policy. Execution supervision, transcript read and Session control should remain separable capabilities. Tunnel loss never deletes the PeerWorkspace or its Sessions.

## 19. FleetWorkspace remains virtual

A FleetWorkspace's Sessions remain in that virtual Workspace on its authority host/replica set. Choosing `On: pvl-x2` means “project this FleetWorkspace onto pvl-x2's binding” rather than “move into pvl-x2's local Workspace”. A fleet-target parent Session remains one conversation; remote subagents may be real child Sessions in member PeerWorkspaces. Fleet membership and bindings are pinned per operation.

## 20. AwNet transport refinement

Remote machine/workspace connectivity uses the QUIC-native AwNet peer protocol described in [AWNET](AWNET.md). Direct QUIC is preferred; relays and TLS/TCP fallback are route choices, not authorization authority. WireGuard is not the core transport. One aw installation may simultaneously control many peers and serve many other peers; there is no permanent global master. Domain federation and Workspace policy remain authoritative above the route.
