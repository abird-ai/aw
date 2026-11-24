# AW DL-200 Phase-3 — copy-ready next-agent prompt — 2026-11-24

Copy/paste the block below into the next implementation chat.

Use dotlink---l5 as the only source-authoring connector.

Continue in:

- AW: /home/pvl/spaces/abird/src/aw
- dotlink: /home/pvl/spaces/abird/src/dotlink
- read-only release: /home/pvl/spaces/abird/src/dotlink-017-mounts

Do not use A1.
Do not create another worktree.
Preserve unrelated/concurrently owned work.

Never inspect, list, stage, reset, stash, read, modify, or touch:

    AW/.agents/plans/ux/**

Sandbox physical mount/config-v13 and the release/0.17 Sandbox implementation remain frozen unless
explicitly promoted.

Do not push, tag, release, publish or deploy unless separately authorized.

Read first, in this order:

1. AW/AGENTS.md
2. AW/.agents/docs/AW-DL200-PHASE3-FULL-COMPREHENSIVE-HANDOFF-2026-11-24.md
3. AW/.agents/plans/dl200-unified-jobs-agent-events-2026-11-20/README.md
4. AW/.agents/plans/dl200-unified-jobs-agent-events-2026-11-20/SPEC.md
5. AW/.agents/plans/dl200-unified-jobs-agent-events-2026-11-20/IMPLEMENTATION.md
6. AW/.agents/plans/dl200-unified-jobs-agent-events-2026-11-20/TODO.md
7. AW/.agents/docs/CURRENT_HANDOFF.md
8. AW/.agents/docs/DL200-R1-R5-RESOLVED-DESIGN-CHECKLIST-2026-11-21.md
9. AW/.agents/docs/UNIFIED-JOBS-AGENT-FINAL-HANDOFF-2026-11-14.md
10. AW/.agents/docs/AW-DL200-R6-FULL-COMPREHENSIVE-HANDOFF-2026-11-22.md
11. AW/.agents/docs/AW-DL200-PHASE1-FULL-COMPREHENSIVE-HANDOFF-2026-11-23.md
12. live source/tests for every owner you touch.

Current signed AW checkpoint:

    184ea6b05643c49d34a7f3bfdbfed86f26753162
    Establish SourceDriver recovery contracts

Parent:

    e0c6bb3184bf87fc1bb2186ddc7f5ffb97a39c96
    Cut over Event delivery ownership

Current Store schema: v54.

Phase status:

- Phase 0 / 0B: complete/published.
- Phase 1: complete locally.
- Phase 2: complete locally through v54.
- Phase 3: underway.
- Phases 4-7: pending.

Verified publication baseline carried by the handoff:

- AW verified remote main:
  78339410710472ec51d5ff46a5c9e586b67ff30d — Own logical Sandboxes
- AW shared Process/R6 publication:
  20a63a4d4547cc20b2a154787823125b82f5e385
- dotlink local/published-main baseline:
  85ceccc1088e201c98c994a46a68834440eeab32
- dotlink AW pin:
  20a63a4d4547cc20b2a154787823125b82f5e385
- release/0.17:
  576d0a1a3fe0a5522a02d51f1d0598c3ddc160c2 = v0.17.9

Do not treat a local origin/main tracking ref as publication truth without separate verification.

Phase-3 prerequisite already landed:

- final add-only SourceDriverRegistry leaf reusing the private staged-registry core;
- stable durable DriverKey = built-in|Extension namespace + local ID;
- independent binding/anchor/recovery artifact schema pins;
- typed SourceDriverHandle<T> with runtime registration revision/TypeId;
- durable resolution by DriverKey + artifact schema, not registry generation;
- validated SourceName [a-z][a-z0-9_-]{0,63};
- SourceAccess = ReadOnly | ReadWrite;
- CheckpointPolicy = Required | Ephemeral;
- effective SourceSet default max 128; host may only lower;
- absolute defensive max 1024;
- symmetric PreparedSourceOperation for Capture and Restore;
- non-secret RecoveryDescriptor;
- RecoveryInspection = NotStarted | Completed { result, evidence } | Unknown;
- ExternalEvidence = Immutable | HeldFence;
- opaque held-fence handle is identity only, never publication permission; live provider fence must be
  held valid through the entire final AW commit.

The next coherent Phase-3 slice is:

1. persist named SourceBindings under the existing logical Sandbox owner;
2. persist stable DriverKey + binding_schema adjacent to each binding;
3. persist/validate SourceName, SourceAccess and CheckpointPolicy;
4. enforce the effective SourceSet limit before any external preparation/effect;
5. on Store open/startup, resolve every live SourceBinding through the installed SourceDriver registry;
6. if driver/schema is missing or incompatible, fail closed into the smallest durable/readable
   degraded/recovery-required state;
7. do not reinterpret, delete, retry or bind the source to another driver;
8. keep Agent-to-Sandbox immutable;
9. keep the SourceDriver leaf add-only until Store can enumerate all live binding/anchor/unresolved
   recovery dependencies.

Do not jump straight to Git checkpoint capture/anchors unless the durable SourceBinding + restart
compatibility contract is already coherent and green.

After SourceBinding/restart ownership is durable, the intended Phase-3 order is:

- Git SourceDriver clean/dirty capture + private retention refs;
- SourceAnchor / Checkpoint / SourceSet storage;
- multi-source capture barrier/revalidate/recovery;
- one shared replay-safe Prepared constructor used by checkpoint restore and later Agent construction;
- checkpoint list/get/create/restore/delete;
- VaultProvider/VaultGrant;
- grouped Phase-3 qualification.

Important sealed semantics:

- checkpoint restore always creates a new Agent + new Sandbox;
- never restore/inherit Vault secrets, credentials, live leases, device/network authority, active Processes,
  staged inputs or mutable security state;
- dirty Git capture uses a private-index synthetic commit but restore must not expose it as visible HEAD;
- Capture and Restore both persist a non-secret Prepared recovery descriptor before external effect;
- crash in Prepared means inspect/recover, never blind retry;
- Unknown stays Unknown;
- external immutable evidence may publish directly;
- provider-held revocable evidence requires a live fence/lease held through the entire AW commit;
- final AW transaction is the only public visibility/linearization point;
- post-commit wakes/cache refreshes are reconstructible accelerators only.

Do not create:

- TransactionManager / OperationManager
- global replay ledger
- universal revision clock
- generic Jobs+Events traversal engine
- distributed Event router
- public spawn-pool framework
- second Process/Event/Project/Session/Source/Agent-ACL owner
- unused registry implementations
- Agent-to-Sandbox rebind API
- cross-store Event-to-Agent delivery in v1
- cross-store ordered recursive Jobs traversal in v1
- restore-specific Agent/Sandbox constructor
- arbitrary Vault env/path/mount target language
- runtime registry generation as durable driver identity
- persisted fence handle treated as authority
- another SourceDriver replacement/removal path before Store can prove all durable dependencies.

Implementation style:

- elegant, simple, clear and intuitive;
- reusable/composable Rust primitives;
- zero-cost/static abstraction where practical;
- take easy performance wins when clean;
- otherwise clean/simple first;
- do not over-engineer test/evidence granularity;
- one grouped real owner regression is preferred over fixture proliferation;
- keep each implementation turn around 30 minutes and stop at the next coherent green checkpoint.

At each checkpoint:

1. run focused real regression(s);
2. run strict relevant check/Clippy/fmt/diff;
3. update CURRENT_HANDOFF + plan TODO factually when the phase advances;
4. commit exact owned paths only using private GIT_INDEX_FILE;
5. expected-parent CAS update of main;
6. one signed terse checkpoint commit;
7. verify signature;
8. pause;
9. push only when separately requested.

Expected signing identity:

    Good "git" signature for pvl@prasannavl.com with ED25519 key
    SHA256:4aB+NvRb1shCx4xjtZDr2Y7FzfHGph80eFtpF+pHuCI

Before changing code, revalidate HEAD/main and inspect only the exact owner paths you plan to touch.
Preserve all unrelated dirty work.
