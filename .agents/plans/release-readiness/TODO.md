# TODO — Release readiness

Status: active
Provenance: 47ee8a03abecbb3aee84a49c0a590096d76ed1a7
Accepted checkpoint: 47ee8a03abecbb3aee84a49c0a590096d76ed1a7

This plan carries unfinished validation/distribution work, not release
permission. Source 0.15.0, SQLite v33 and JSONL v1 remain unchanged.

## Committed

### `47ee8a03abecbb3aee84a49c0a590096d76ed1a7` — user-accepted historical work

- [x] Accept previously verified source/runtime/feature-cut corrections and platform-scoped native checks at this checkpoint.
- [x] Accept earlier five-target build, local assembly/Linux fixture and macOS ad-hoc signature verification within their original scope; native host execution, identity signing and publication remain separate gates.
- [x] Accept historical Rust/Nix validation and build-capacity fixes. Superseded helper counts, manifest chains and pre-rewrite commit IDs are not current runnable gates.

### `aecebbc8` — current product CI workflow

- [x] Provide a pinned Nix development-shell product gate and a separate native `.#aw` build/version job; execution status is recorded only after a current run.

### `9fbb8a3456b717c810c901460817ba1cc2317d59` — completed vault initialization lock lifetime

- [x] Explicitly release only the vault initialization lock after complete key durability work and on error/unwind, even while a duplicate open-file description survives; actual maintenance denial remains intact.
- [x] Deterministic duplicate-descriptor controls failed before the fix; six SecretStore tests and the original focused SDK test passed after it. The original `1c2ec6e` SDK failure used the same Conflict, but its exact fork timing was not captured. The cumulative `2809fa5` full gate passed.

### `2809fa58902df4dd9a23fb5a9a8fbd5e57d43189` — local Linux product gate

- [x] Align this checklist and Build status with the single product gate, without reconstructing removed phase/smoke/evidence profiles.
- [x] Remove the new Workspace-only unused-binding warning without changing control flow.
- [x] Execute the pinned full Cargo/Node gate and native Linux package/version checks on this exact commit; this does not supply hosted workflow or other-host certification.

### `3b0dbb502005570cca30e2cd52cba60c052f3587` — reviewed local product checkpoint

- [x] Pass the full pinned product gate and native Linux package/version after the Find lifecycle corrections; source version, SQLite schema and JSONL version remain unchanged.
- [x] Independently recheck the Workspace-only feature boundary and actual native terminal behavior; retain all external release gates below.

### `3851dbffd808491916e07a3642422bd0f7b465b0` — scoped Files local product checkpoint

- [x] Pass the pinned product gate and native Linux package/version after scoped Files and Windows retained-directory corrections; source version, SQLite schema and JSONL version remain unchanged.
- [x] Independently check the Workspace-only feature boundary and Windows target/test compilation. Actual Windows execution remains open and separate from compilation.
- [x] Exercise the immutable native package in real terminal sessions with two folder roots, explicit restart/reconnect, unchanged draft/caret, zero model/tool effects and restored terminal settings.

## Uncommitted

- [ ] Complete release-asset execution on remaining target hosts, locally or with hosted jobs when available; Linux emulation cannot certify missing macOS/Windows/ARM hosts.
- [ ] Review breaking CLI/version surfaces, agree any version change and obtain release authorization before distribution; 0.16.0 remains a proposal.
- [ ] Decide/configure Developer ID/Authenticode signing and notarization requirements; ad-hoc signatures and checksums do not supply those identities.
- [ ] Run live-provider/billing compatibility checks only when separately authorized and configured.
- [ ] Obtain an independent security review; product tests do not supply independent certification.

## Tests and verification

- [x] `bash scripts/bootstrap.sh` — pinned Nix shell on `3851dbf`, passed.
- [x] `cargo fmt --all -- --check` — pinned Nix shell on `3851dbf`, passed.
- [x] `cargo test --workspace --locked` — pinned Nix shell on `3851dbf`, 1,608 passed, 2 existing ignored, no failures.
- [x] `node --test tests/web_model.test.mjs tests/web_workspace.test.mjs tests/web_context_flow.test.mjs tests/web_multiplayer.test.mjs tests/web_agent_parity.test.mjs tests/web_rich_markdown.test.mjs` — pinned Nix shell on `3851dbf`, 107 passed.
- [x] `nix build 'git+file:///home/pvl/spaces/abird/src/aw?rev=3851dbffd808491916e07a3642422bd0f7b465b0#aw' --no-link --print-out-paths --no-update-lock-file` — passed; built native `bin/aw --version` exited 0 with `aw 0.15.0`.
- [ ] Release-asset native execution — exact assets and remaining hosts; unfinished external gate.
- [ ] Version/release approval — explicit owner decision; unfinished external gate.
- [ ] Identity signing/notarization — actual configured identities and execution; unfinished external gate.
- [ ] Live-provider/billing checks — separately authorized real execution; unfinished external gate.
- [ ] Independent security review — unfinished external gate.
