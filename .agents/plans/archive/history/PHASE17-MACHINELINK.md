# Phase 17 — authenticated typed MachineLink negotiation

Phase 17 adds a control-plane lease on top of the authenticated QUIC routes from Phases 15–16.

## Implemented

- a closed typed MachineCapability enum;
- signed MachineLink hello/accept transcripts bound to both stable AwNode identities;
- short-lived capability leases (5..300 seconds);
- server-side capability ceilings and exact requested/granted intersection;
- duplicate/empty/oversized capability rejection;
- generic `shell` and `execute` capability names are not part of the protocol and fail before transport;
- `aw net machine-serve` and `aw net machine-connect`;
- an actual-binary two-node capability negotiation smoke.

Pairing remains identity trust. MachineLink remains a short-lived capability lease. Neither is Domain membership.

## Capability vocabulary

Current typed names include machine metadata, Workspace discovery, read-only file/Git/session surfaces, PTY and remote-session placeholders. Merely naming a capability does not implement or grant the service; Phase 18 begins wiring the read-only service subset.

No generic command string, arbitrary reverse tunnel or ambient filesystem root exists in the MachineLink envelope.

## Next

Phase 18 serves capability-bound typed read-only Machine/Workspace/File/Git/Session operations. Effectful PTY and remote Session creation remain separately gated.
