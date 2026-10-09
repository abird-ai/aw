# Phase 66 — Portable instruction-skill bundles

**Status:** complete; implementation committed/tagged and validated
**Implementation checkpoint:** `38b9f8b55cc3baf7a07e4ceec5602873b4a68237` (`phase-66`)
**Version:** 0.15.0
**SQLite schema:** v32 unchanged
**Skipped phases:** Phase 64 and Phase 65 are intentionally unused by explicit user request.
**Authority expansion:** none

## Goal

Build a portable skill ecosystem on top of the explicit Phase-60 loader and Phase-61 registry without turning skills into executable plugins or automatic capabilities.

## Bundle format

A Phase-66 skill bundle is a bounded JSON document:

```json
{
  "format_version": 1,
  "name": "review",
  "description": "Review invariants",
  "content": "...UTF-8 instructions...",
  "content_sha256": "<64 lowercase hex>"
}
```

The digest proves content integrity only. It is **not** a publisher signature, identity claim, trust grant, or authorization proof.

Unknown fields, unsupported versions, invalid names, oversized content, invalid descriptions, malformed digests, and digest mismatches fail closed.

## CLI

Extend the existing typed skill resource:

```text
aw skill export NAME OUTPUT [--replace]
aw skill verify BUNDLE
aw skill install BUNDLE [--replace]
aw skill import BUNDLE [--replace]   # alias of install
aw skill update BUNDLE
```

Existing Phase-61 commands remain:

```text
aw skill list
aw skill show NAME
aw skill add NAME PATH
aw skill remove NAME
```

Registry presence still never activates a skill. Runtime activation remains explicit `--skill NAME` or one-run `--skill NAME=PATH`.

## Managed installation

Installing a valid bundle:

1. validates the complete bundle before mutation;
2. writes the instruction content through the platform atomic metadata writer;
3. stores it at a content-addressed path under the selected config directory:
   `skills/<content_sha256>.md`;
4. updates the existing Phase-61 registry only after the managed content is durably published;
5. requires `--replace` when the name already exists.

`update` is stricter: the named skill must already exist.

Old content-addressed files may remain unreferenced after an update. Phase 66 does not delete unknown/unreferenced files automatically.

## Export

Export resolves the current registered path, loads it through the same Phase-60 UTF-8/64-KiB admission rules, computes the digest, and writes a deterministic version-1 bundle.

Export refuses to overwrite an existing output unless explicitly requested.

## Safety and authority

Bundles contain instruction text only.

Phase 66 does not add:

- scripts, binaries, WASM, shell commands, or executable hooks;
- tool definitions or tool grants;
- provider/account/routing changes;
- automatic activation on install;
- project/config-directory discovery;
- network package fetching;
- package indexes/marketplaces;
- signatures or publisher trust;
- dependency execution;
- remote skill synchronization;
- schema v33.

Instruction text remains untrusted user-selected system context. Installing a bundle is a local catalog mutation, not model authority.

## Validation target

1. deterministic bundle digest and round trip;
2. unknown fields/version/digest mismatch rejected;
3. content still bounded by Phase-60 64-KiB rule;
4. install writes content-addressed managed source and registers but does not activate;
5. install collision requires explicit replacement;
6. update requires existing registered name;
7. export refuses overwrite without explicit replacement;
8. runtime `--skill NAME` loads an installed bundle through the existing registry/loader path;
9. no executable extension/process/tool authority appears;
10. full/minimal inherited tests remain green;
11. schema remains v32.

## Implementation evidence before tag

- Phase 66 checker: **49/49**;
- full native `aw`: **256/256**;
- minimal/no-default warning-denied native: **82/82** plus inherited library/provider tests;
- actual-binary bundle round trip: **4/4**;
- whole-workspace/all-target compile: green;
- Clippy `-D warnings`: green;
- doctests/rustdoc warnings-as-errors: green;
- static validation: **1030 files / 1007 Rust test attributes / 739 links / 0 errors / 0 warnings**.

## Next gate

continue to explicitly authorized Phase 67.

Do not infer Phase 69 or any skipped Phase 64/65 implementation.
