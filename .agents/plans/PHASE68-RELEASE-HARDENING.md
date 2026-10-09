# Phase 68 — Source-bound release and production hardening

**Status:** complete; implementation committed/tagged and validated
**Implementation checkpoint:** `953d6e1905d9968cce74137b4705b78d6f1aa49b` (`phase-68`)
**Version:** 0.15.0
**SQLite schema:** v33 unchanged
**Skipped phases:** Phase 64 and Phase 65 remain intentionally unused by explicit user request.
**Authority expansion:** none; this phase packages and validates existing platform feature boundaries.

## Goal

Turn the existing five-target release graph into one auditable artifact contract instead of loosely related binaries + checksum files.

Phase 68 does not claim that SHA-256 is a publisher signature. It makes build/publish/install evidence internally consistent and source-bound.

## Canonical target matrix

Exactly five release binaries remain supported:

| Asset | Rust target | Feature profile |
|---|---|---|
| `aw-linux-x86_64` | `x86_64-unknown-linux-musl` | `full` |
| `aw-linux-aarch64` | `aarch64-unknown-linux-musl` | `full` |
| `aw-windows-x86_64.exe` | `x86_64-pc-windows-gnu` | `minimal` |
| `aw-windows-aarch64.exe` | `aarch64-pc-windows-gnullvm` | `minimal` |
| `aw-macos-aarch64` | `aarch64-apple-darwin` | `full` |

No new platform is implied by Phase 68.

## Release manifest

`release-all` emits `release-manifest.json`:

```json
{
  "schema_version": 1,
  "version": "0.15.0",
  "artifacts": [
    {
      "asset": "aw-linux-x86_64",
      "target": "x86_64-unknown-linux-musl",
      "feature_profile": "full",
      "sha256": "...",
      "size_bytes": 123
    }
  ]
}
```

The artifact array has exactly the five canonical rows in stable order.

The manifest digest values must match both:
- the actual artifact bytes;
- the existing `<asset>.sha256` sidecar.

The manifest is integrity/provenance metadata only. It is not a signature, transparency-log proof or publisher identity.

## Independent release audit

the removed release_artifact_audit helper validates a materialized release directory without trusting Nix metadata strings alone.

It verifies:

1. exact manifest schema/keys and Cargo version;
2. exact five-target matrix with no duplicate asset/target;
3. exact byte size + SHA-256 for every binary;
4. strict sidecar filename/hash agreement;
5. expected binary file format/architecture from binary headers:
   - ELF x86_64/aarch64;
   - PE x86_64/ARM64;
   - Mach-O ARM64;
6. Linux release ELF binaries have no PT_INTERP segment (static release contract);
7. VERSION and PLATFORMS metadata agree;
8. the native Linux x86_64 artifact reports exactly `aw <VERSION>` when audited on a matching host;
9. installer files are present when the publisher audit requests them;
10. no unexpected release binary or checksum sidecar silently enters the set.

## Nix graph

The existing pinned Rust/Crane/Nix release graph remains the builder.

`release-all` now includes:

- five binaries;
- five SHA-256 sidecars;
- `VERSION`;
- `PLATFORMS.txt`;
- `release-manifest.json`.

The local materializer copies the exact aggregate output, adds the two installer scripts and runs the independent release audit.

## GitHub Actions

The build job:

1. checks out an exact revision with credentials disabled;
2. builds `.#release-all` once;
3. materializes the aggregate result;
4. adds installers;
5. runs `release_artifact_audit.py`;
6. uploads that exact directory.

The tag release job:

1. downloads the build artifact from the same workflow;
2. checks out the exact workflow revision for the audit code;
3. reruns the independent audit;
4. verifies `v<VERSION>` exactly matches the tag;
5. publishes the exact audited asset set including the release manifest.

All reusable Actions remain pinned to immutable commit SHAs.

## Installer contract

Installers remain deliberately dependency-light:

- HTTPS failure is fatal;
- per-asset SHA-256 sidecar is validated;
- the downloaded binary must identify itself as aw;
- explicit version requests must match the binary-reported version;
- replacement is atomic/best-effort platform-native as already implemented.

Installers do not claim that a checksum downloaded from the same release origin is a cryptographic publisher signature.

## Native optimized proof

Phase 68 attempts a real host `cargo build --release --locked -p aw-cli --bin aw --features full`.

If the link completes in this environment:

- run the optimized binary's version/help smoke;
- run representative actual-binary resource/schedule/terminal-safe smokes against that exact release binary;
- record its SHA-256 and size as local host evidence.

If the current connector cannot sustain the optimized link, report it as environment-blocked. Do not substitute a historical release binary.

## Cross-build/Nix proof

Attempt `nix build .#release-all` only if the current host has a writable usable Nix store.

If Nix is blocked by the connector/store environment:

- source/evaluation/audit architecture may still be validated;
- no five-platform build-success claim may be made;
- GitHub workflow execution is not inferred.

## Non-goals

Phase 68 does not add:

- artifact signing keys;
- Sigstore/cosign;
- transparency-log publication;
- notarization;
- Apple/Windows code signing;
- auto-update execution;
- a new installer trust root;
- new runtime authority;
- filesystem indexing/LSP;
- Phase 64/65 work.

Those would require their own explicit trust/custody design.


## Implementation evidence before tag

- Phase 68 source/release checker: **72/72**;
- synthetic exact-five-artifact audit: **4/4**;
- inherited release architecture checker: **76 checks**;
- Unix installer local release-fixture smoke: green;
- full native `aw`: **256/256**;
- minimal/no-default warning-denied native: **82/82** plus inherited library/provider tests;
- whole-workspace/all-target compile: green;
- Clippy `-D warnings`: green;
- doctests/rustdoc warnings-as-errors: green;
- static validation: **1040 files / 1009 Rust test attributes / 739 links / 0 errors / 0 warnings**.

### Environment-blocked evidence — not claimed

A fresh current-source optimized full-feature host release was attempted repeatedly. An individual optimized `aw-store` rustc invocation exceeds this connector's process lifetime, so no successful current optimized link completed. Existing/stale `target/release/aw` bytes are **not** treated as Phase-68 evidence.

Nix release evaluation was attempted and failed before derivation evaluation because this connector exposes `/nix/store` read-only (`cannot create /nix/store/tmp-*`). Therefore the five-target Nix cross-build and GitHub Actions execution remain **unexecuted/not claimed** here.

## Acceptance

1. release manifest generated by the canonical Nix aggregate;
2. independent audit validates exact matrix/hash/size/format/static/native-version contract;
3. local materializer and GitHub build/release jobs use the same audit;
4. release architecture checker covers manifest/audit wiring;
5. installer smoke remains green;
6. full native/minimal/workspace/lint/docs/reference gates remain green;
7. real optimized native release is tested if the host can link it;
8. Nix/cross evidence is claimed only if actually executed successfully;
9. schema remains v33;
10. commit/tag `phase-68`.

## Next gate

User review / explicit next task.

Do not infer Phase 69 automatically.
