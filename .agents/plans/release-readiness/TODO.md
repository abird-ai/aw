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

### `b33a4183f6032a024d1b73409d2c9134ea0aab24` — scoped Git local product checkpoint

- [x] Pass the pinned product gate and native Linux package/version after exact Git contexts, v4 layout migration and repository/index/terminal admission corrections; product version, SQLite schema and JSONL version remain unchanged.
- [x] Check the Workspace-only feature boundary and Windows platform-test/Git-library target compilation; native Windows execution remains open.
- [x] Exercise two existing private worktrees in actual terminal sessions, including exact selected-index staging, stale refusal, restart/reconnect, unchanged Agent/Files/draft and zero model/tool effect rows.

### `e2281d642fcdc380b0a8b328c2d71d6daa132ea0` — safe release output and native package custody

- [x] Reproduce release-helper data loss in private filesystem fixtures: unrelated files and linked output were replaced; incomplete artifacts were published. Refuse unrelated/incomplete/linked destinations before building and before publication; publish a complete flat file set by Linux atomic directory exchange, preserving prior output on build/staging failure. Cleanup never recursively deletes a caller-selected directory.
- [x] Eight focused filesystem regressions pass in the pinned Nix shell, including concurrent output changes, continuous readers, quoted paths and output preservation. Add this real filesystem gate to CI; hosted execution remains unclaimed. Validate Bash syntax, pinned Nix formatting and exact helper-source inclusion.
- [x] Evaluate all 18 native/release package derivations, including five cross-target definitions, on this source. Evaluation is not compilation or target-host execution.
- [x] Build the native Linux Nix package from a scoped immutable export of this signed commit; no reserved subtree or concurrent documentation patch enters the export. `bin/aw --version` reports `aw 0.15.0`. Product Rust/Cargo/web inputs are unchanged by the release-helper fix.
- [x] Execute that exact optimized Nix binary in all 35 native scenarios: original-Workspace migration `parser`, `vim`, `less`, `source-crash` (4); adversarial migration protocol/process-loss cases (26); existing dedicated broker `reconnect`, `competition`, `workspace-exit`, `shell-exit`, `broker-loss` (5). These are local Linux synthetic fixtures, not portable musl release-asset or other-host qualification, provider/billing execution, identity signing or publication.

## Uncommitted

- [ ] Complete release-asset execution on remaining target hosts, locally or with hosted jobs when available; Linux emulation cannot certify missing macOS/Windows/ARM hosts.
- [ ] Review breaking CLI/version surfaces, agree any version change and obtain release authorization before distribution; 0.16.0 remains a proposal.
- [ ] Decide/configure Developer ID/Authenticode signing and notarization requirements; ad-hoc signatures and checksums do not supply those identities.
- [ ] Run live-provider/billing compatibility checks only when separately authorized and configured.
- [ ] Obtain an independent security review; product tests do not supply independent certification.

## Tests and verification

- [x] `e2281d6`: pinned `python3 tests/release_output.test.py` — 8 passed; `bash -n scripts/build-release-artifacts.sh`, pinned `nixfmt --check nix/source.nix`, and exact Nix helper-source inclusion passed. Pre-fix fixture control: 3 failures in the original six-case suite; post-fix eight-case suite passes.
- [x] `e2281d6`: `nix eval path:/tmp/aw-native-package-uzwr5iux/final-source#packages.x86_64-linux --apply 'p: builtins.mapAttrs (_: value: value.drvPath) p' --json --no-update-lock-file` — all 18 package derivations evaluated. `nix build path:/tmp/aw-native-package-uzwr5iux/final-source#aw --no-link --print-out-paths --no-update-lock-file` — native package built; version and the 35 actual optimized-binary native scenarios above passed with `AW_TEST_BINARY=/nix/store/yc6f12xrm9g432kzqkn87rv0jyhnkyd3-aw-0.15.0/bin/aw` in the pinned Nix shell. Scoped source export contains the signed tracked compile/build inputs, excluding `.agents` and all concurrent work. No full Rust/Node suite rerun is inferred from this focused script/package validation.

- [x] `bash scripts/bootstrap.sh` — pinned Nix shell on `b33a418`, passed.
- [x] `cargo fmt --all -- --check` — pinned Nix shell on `b33a418`, passed.
- [x] `cargo test --workspace --locked` — pinned Nix shell on `b33a418`, 1,638 passed, 2 existing ignored, no failures.
- [x] `node --test tests/web_model.test.mjs tests/web_workspace.test.mjs tests/web_context_flow.test.mjs tests/web_multiplayer.test.mjs tests/web_agent_parity.test.mjs tests/web_rich_markdown.test.mjs` — pinned Nix shell on `b33a418`, 107 passed.
- [x] `nix build 'git+file:///home/pvl/spaces/abird/src/aw?rev=b33a4183f6032a024d1b73409d2c9134ea0aab24#aw' --no-link --print-out-paths --no-update-lock-file` — passed; built native `bin/aw --version` exited 0 with `aw 0.15.0`.
- [ ] Release-asset native execution — exact assets and remaining hosts; unfinished external gate.
- [ ] Version/release approval — explicit owner decision; unfinished external gate.
- [ ] Identity signing/notarization — actual configured identities and execution; unfinished external gate.
- [ ] Live-provider/billing checks — separately authorized real execution; unfinished external gate.
- [ ] Independent security review — unfinished external gate.
