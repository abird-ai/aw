# Phase 28 — Workspace writer epochs and stale-writer fencing

Phase 28 adds the distributed mutable-ref authority layer promised by the Domain/AwNet design without turning SQLite into a multi-writer database.

The existing branch `head + revision` compare-and-swap remains authoritative for concurrent mutation inside one writer epoch. Phase 28 adds an orthogonal **Workspace writer epoch** so a stale AwNode cannot resume advancing canonical refs after Domain-authority failover even when its cached head/revision still looks current.

## 1. Writer model

Every Workspace has one current writer state:

```text
WorkspaceWriter {
  workspace_id,
  domain_id,
  writer_node_id,
  writer_epoch,
  authority_term,
  state,
  revision
}
```

Schema v13 bootstraps existing and newly created Workspaces to the local AwNode at `writer_epoch = 1`.

The current writer is a **mutable-ref authority**, not a statement that all Workspace data lives only on that Machine. Immutable objects/results remain separable from the canonical mutable ref.

## 2. Failover

`aw workspace writer-failover` is a Domain-authority mutation.

A failover requires:

- current Domain owner/admin authority;
- the current Phase 26/27 Domain authority write fence to be valid;
- exact `expected_epoch` equality;
- a target that is either the local AwNode or a trusted active remote Machine in the same Domain.

A successful failover:

- increments `writer_epoch` exactly once;
- records the current Domain authority term;
- advances the Workspace resource revision;
- appends an immutable `workspace_writer_events` row;
- emits `workspace.writer_failed_over` in the semantic commit log.

A stale `expected_epoch` fails closed. There is no last-write-wins fallback.

## 3. Canonical ref mutation fence

Canonical local mutations now require this store's AwNode to own the current active Workspace writer epoch.

The fence covers the authored mutation surfaces including:

- new Session creation in a Workspace;
- branch fork creation;
- checkpoint/ref advancement;
- context import/compaction that creates or advances canonical branches;
- prompt/run admission that advances the branch to a prompt node;
- final run attachment to the canonical branch.

Read-only Session/branch inspection remains available.

`ApplyMode::NextPrompt` context staging remains a non-ref staging operation and is not treated as canonical branch advancement until it is consumed by a writer-authorized prompt admission.

## 4. Writer provenance on refs

Schema v13 extends `branch_ref_events` with:

- `writer_node_id`;
- `writer_epoch`.

Every new branch-ref event obtains those values from the Session's Workspace writer state. The semantic `branch.advanced` commit event also records writer node/epoch.

This makes a mutable ref transition auditable against both:

1. branch head/revision CAS; and
2. the Workspace writer generation that was authorized to perform it.

## 5. In-flight paid work across failover

A provider/model run may already have been admitted when writer failover occurs. Phase 28 must not discard that paid work and must not silently rebase it.

At admission, aw durably pins the Workspace writer epoch into:

- `llm_operations.origin_writer_epoch`;
- `runs.produced_from_writer_epoch`.

At settlement, the result node is persisted **before** canonical attachment is attempted. Attachment requires that the current local writer epoch still equals the admitted epoch in addition to the existing branch/run/head/revision conditions.

If the writer epoch changed:

- the result remains durable;
- `runs.attach_status` becomes `conflict`;
- the stale active-run marker is cleared;
- the canonical branch head is not advanced;
- a recovery hint records `produced_from_writer_epoch` and whether the epoch was still current;
- aw does not regenerate or silently rebase the result.

This preserves the pre-existing paid-result attachment-conflict semantics while extending them across distributed writer failover.

## 6. AwNode identity bootstrap

Older/fresh stores may initially contain the legacy placeholder local node ID before the Ed25519 AwNode identity is first bound.

When `bind_local_node_identity` replaces that placeholder, Phase 28 updates **current** Workspace writer state to the cryptographic AwNode ID without incrementing the logical writer epoch. Historical bootstrap writer events remain historical evidence.

## 7. CLI

```text
aw workspace writer-show WORKSPACE
aw workspace writer-events WORKSPACE
aw workspace writer-failover WORKSPACE TARGET_NODE --expected-epoch N [--reason TEXT]
```

Normal `--domain`, `--state-dir`, and `--json` options continue to apply.

## 8. Deliberate boundary

**Phase 28 does not yet make a remote writer a complete remote Workspace replica.**

Failing a Workspace over to a remote Machine currently does two important things:

1. fences the previous/local writer immediately; and
2. records the Domain-authorized canonical writer generation and target.

But this phase does **not** copy the Workspace's immutable Session/node graph or current branch-ref state to that remote store, and it does not add typed remote writer mutation RPCs.

Therefore a remote writer designation is a real control-plane handoff/fence, but the next phase must add the minimal semantic Workspace replica/ref carriage needed for that remote node to actually serve as the canonical writer.

Do not paper over that gap with raw SQLite/WAL copying, generic row replication, last-write-wins, or a second local DB owner.

## 9. Validation

Validated in the connector environment:

- focused Phase 28 schema/source/reference checks: **44 passed**;
- `aw-store` context-flow tests: **29/29 passed**;
- `aw-store` resource tests: **18/18 passed**;
- `aw-store` store/journal tests: **28/28 passed**;
- default `aw-cli --all-targets`: **87 + 48 + 6 passed**;
- minimal, `web`, and `net` feature configurations: passed;
- actual compiled-binary q2 Workspace writer failover smoke: passed;
- full workspace `cargo check --locked --workspace --all-targets`: passed;
- full workspace `cargo test --locked --workspace --all-targets`: passed when `AW_GIT` and `AW_BASH` are explicitly bound to the trusted Nix-store executables (the connector sandbox lacks the tests' default `/usr/bin/git` and `/bin/bash`).

The store/journal regression explicitly proves that a completed result admitted under writer epoch 1 remains durable but does not attach after the Workspace passes through later writer epochs.

The complete source-bound reference profile is rerun at the final Phase 28 checkpoint.

The separate full-host clippy/Nix release gate remains environment-pending because this connector lacks `cargo-clippy` and its `/nix/store` is read-only.

## 10. Next boundary

Phase 29 should make a designated remote writer operational by adding **minimal semantic Workspace replica/handoff**:

- transfer immutable Session/context/node objects by identity/digest;
- transfer the exact Workspace/Session/branch ref snapshot needed by the new writer;
- bind imported refs to the current `writer_epoch` and Domain authority term;
- expose typed MachineLink replication/handoff capabilities rather than generic DB access;
- preserve both immutable descendants on partition/rejoin;
- reject stale epochs and never use last-write-wins;
- keep `owner.lock` per local store and leave direct multi-process SQLite for its separate stress/migration/lease phase.
