# Phase 2 source and execution evidence

**This source was not compiled.** Rust/Cargo/Nix were absent and no aw binary executed. There is no real Cargo.lock. See [build status](../docs/BUILD-STATUS.md).

| Check actually performed | Result |
|---|---|
| Python tar editor tests | 30 passed |
| JavaScript model/account/request tests | 25 passed |
| Chromium DOM with explicitly fake transport | 15 checks passed; no page errors |
| Python basic provider fixture only | 2 endpoint checks passed |
| Python tool-call fixture only | 2 endpoint checks passed |
| Static Python/TOML/JSON/shell/JS and local Markdown links | 299 links; no static errors |
| Real Rust unit/integration test attributes | 130 authored, **not executed** |
| Rustfmt/clippy/SDK compile / actual CLI and web / real tool I/O | **Not run** |
| Crane/Nix and real model interoperability | **Not run** |

Static/archive check counters describe the tested source revision, before adding this evidence documentation. The final standalone validation JSON also reports final archive integrity. They are not security, account custody, type checking or real provider-call evidence.

The Phase 1 committed archive prefix and all 51 retained design files stayed byte-identical. A direct exact patch changed one archive member, preserved earlier committed bytes, updated one build-cache file, and the next checkout updated zero. New phase source and evidence append to the existing uncompressed tar; old raw bytes/history remain.

Status JSON · Archive proof · Browser fixture checks · [Current user/device credential contract](../docs/CREDENTIAL-PLACEMENT.md).

Actual screenshots in validation/phase-2/browser/ are from the fake-transport UI test, labeled as such. They do not show a running Rust server or real model/tool execution.
