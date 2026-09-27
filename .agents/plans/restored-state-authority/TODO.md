# Blocked restored-State authority prerequisites

Accepted/provenance checkpoint: `9f90e3df52ef684674ee9c103f157a2f5fbc07d3`.

Design-only and blocked. No runnable activation, quarantine removal, final external-effect settlement or old Queue delivery is authorized. Keep the original Agent/TurnMachine/ToolPolicy/TurnJournal and Runtime, Actor, Message, Envelope with Next/Defer; snapshots and UI focus never mint authority.

## Completed

- [x] Original-owner cold inspection and exact-effect review REQUESTS are implemented; both filesystem and SQL quarantine remain. Copies can independently request review but neither gains execution authority.
- [x] Exact Session/branch/WriterEpoch, Actor generation, Process incarnation, source kind/ID/epoch/site/effect, payer/account/credential and immutable intent/result provenance remain distinct; uncertain commits demand inspection, not automatic retry.

## Uncommitted

- [ ] Verify every unknown provider attempt against provider-owned usage/invoice or an independently verified human accounting disposition for its original payer/account/credential. Missing evidence stays possibly charged/unresolved, never zero-cost.
- [ ] Reconcile original Tool/OS effects from exact host/Process/site/intent/result records. Failed/interrupted signals do not prove the process stopped; unverifiable effects remain unresolved/abandoned for a fresh proposal, not successfully executed/rolled back.
- [ ] Reconcile Pending/Claimed/Accepted Queue and Actor Input/Control admissions individually against original Session/branch/writer facts. No old grants/admissions automatically survive and no historical Queue item auto-dispatches.
- [ ] Establish independent live-human presence and revocation: trusted-origin, single-use short-lived challenge bound to source Store/archive/effect digest, target Host/site, restored generation, fresh scopes and coordinator term. A verified user-presence/user-verification authenticator is required; copied signatures, local identity/model text and prior review hashes are insufficient. Never copy private authenticator credentials into State, model or audit.
- [ ] Provide an independent durable, linearizable single-winner coordinator CAS for original Store/archive across clones and old owner; monotonic terms survive crashes/restart/lease expiry/replay. Partition/unreachable coordination denies activation/effects; a local lock/TTL is not a global fence.
- [ ] Only after independent proofs and fresh authorization, design crash-safe transfer: inert target plus durable coordinator pending record, fresh owner/writer/Actor/Process generations and original CAS/fsync boundaries. Every uncertain outcome remains UNKNOWN; inspect the source of truth instead of retrying the side effect.
- [ ] Prove first-open zero old provider/tool/OS effects, zero historical Queue claims/unsolicited Actor admissions and no duplicate charge at every crash/reconnect boundary; all new effects need fresh human proposals, unique intent IDs, current credentials/exact grants and the original journal handshake.
- [ ] Require real competing-host/partition/quorum/rollback/revocation and authenticator lifecycle tests, plus copies of old consent/dry-run/review receipts rejected as bearer grants. Native Linux/Mac/Windows evidence is separate from optional E6.
- [ ] Keep user-visible progression factual: Quarantined → effects need inspection → review requested → independent evidence missing → eligible for new proposal review. No Ready/Resume/Activate control before its conditions truly hold; UNKNOWN is never green.
- [ ] If the threat model requires it, separately analyze fd-bound filesystem/SQL snapshots against privileged mounts/root; current inode anchoring is not that certification.
- [ ] Keep sensitive v1 composite/SQLite Tool outputs/prompts, ciphertext and master-key custody private. Generic ToolOutput encryption/erasure/rotation, credential-neutral publication and cloud/retention-law certification remain unimplemented; do not infer them from internal Linux backup tests.
