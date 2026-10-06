# Phase 23 — semantic cross-store sync

Phase 23 adds append-only semantic synchronization across federated stores. It does not replicate SQLite pages and does not make multiple processes writers of the same database.

## Implemented

- schema version 8 with:
  - `semantic_sync_cursors` keyed by federation + source store;
  - immutable `semantic_sync_inbox` rows keyed by source sequence and source event ID;
- typed `SemanticSyncBatch` and `SemanticSyncApply`;
- outbound export from the existing actor-filtered semantic commit log under a current `sync.read` federation lease;
- inbound import under a current `sync.write` lease;
- strict monotonic source sequences while allowing authorization-created sequence gaps;
- per-event SHA-256 binding and source-event-ID rebinding protection;
- exact retry verification and idempotent reused results;
- local-store loop prevention;
- 512-event / 256 KiB-per-event bounds and 4 MiB CLI batch-file bound;
- local inspection of imported evidence through `sync-inbox`;
- `aw domain sync-export`, `sync-import`, and `sync-inbox`.

## Canonical-state boundary

Imported events are evidence only. The import path never updates local Sessions, branch refs, nodes, execution targets, provider state, or tool state. Materializing a remote fact into local canonical state remains a separate explicitly authorized semantic operation.

This preserves one authoritative writer per local SQLite store while allowing durable cross-store knowledge/provenance flow.

## Next

The next implementation gate is native validation of Phases 19–23 on an unsandboxed Nix/Cargo host, followed by typed MachineLink carriage for federation leases and semantic sync batches, then explicit materialization policies where required.
