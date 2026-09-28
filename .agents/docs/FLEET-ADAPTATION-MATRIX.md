# Fleet / Machine design adaptation matrix

**Status:** design-only comparison against Phase 12.

| Concern | Phase 12/current direction | Fleet target design | Required implementation |
|---|---|---|---|
| Local workspace | One owner root plus worktree-scoped human views. | Local host becomes a first-class Machine target. | Introduce target-aware workspace service abstraction. |
| Remote workspace | Native attach controls owner but does not make arbitrary remote machine workspace local-like. | Remote `aw agent` exposes typed workspace services through MachineLink. | Machine identity/link/capability runtime. |
| Workspace switching | Worktree changes can change follower views; execution root stays fixed. | Machine Target picker changes Follow views and defaults together. | Add target scope to navigators/content resources. |
| Session execution binding | Session/root binding is durable and protected from silent context transfer. | Add durable Machine/Fleet execution target. | Session target schema/API. |
| Machine identity | Devices/executors exist for inference; no general workspace Machine domain. | Stable machine ID/key, distinct from user/device/sync peer. | New machine records and auth. |
| Tunnel | Personal-device link exists for inference only. | Outbound authenticated multiplexed MachineLink for workspace services. | New typed transport; no generic reverse proxy default. |
| Machine workspace | Host root/context exists. | Explicit registered workspace on each Machine. | Machine workspace registry + capability revision. |
| Fleet | Not present. | Named group of Machines, non-nested first. | Fleet/membership records + policies. |
| Fleet workspace | Not present. | Virtual main-host workspace targeting Fleet via member workspace bindings. | FleetWorkspace manifest/bindings. |
| Fleet Files | No aggregate machines. | Machine-grouped aggregate; drill down with Target picker. | Aggregate projection. |
| Fleet Git | No machine matrix. | Per-machine branch/head/dirty/worktree matrix. | Aggregate Git model. |
| Fleet terminal | PTY is host-local resource. | Terminal is always machine-scoped; broadcast is explicit separate action. | Remote PTY service + broadcast safety path. |
| Fleet sessions | Sessions can be user/autonomous but not machine-group targeted. | Machine-target and Fleet-target sessions; FleetOperation has per-machine MachineRuns. | Placement and durable per-target runs. |
| Retry | Phase 12 logical operation/attempt/recovery hint. | Reuse per machine; autonomous policy can retry machine member safely. | Bind retries to machine target and relocation policy. |
| Tunnel failure | Owner/device recovery exists in narrower forms. | Machine link loss produces offline/reconcile state, never silent reroute. | Durable connection instance + machine run recovery. |
| Provider account | Account executor/custody already separate from tool host. | Preserve separation; workspace machine selection does not change provider account. | Explicit machine-local provider metadata only where configured. |
| Fleet membership changes | N/A. | UI follows current membership; each operation pins membership snapshot. | Membership revision/snapshot. |
| Database | Schema3 has commit log, identities, leases foundations. | Add machine/fleet/workspace/target/run records and semantic events. | Future schema migration after design/compile gate. |
| Remote preview | Current website frame is not a remote preview tunnel. | Typed per-machine PreviewResource through bounded bridge. | Later machine preview capability. |
| Main-host authority | Durable history/control already owner/store centered. | FleetWorkspace/layout/policy remain main-host authoritative. | Preserve; do not make machines competing workspace stores. |
