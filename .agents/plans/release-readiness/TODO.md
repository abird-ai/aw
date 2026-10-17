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

## Uncommitted

- [~] Reconcile this checklist and Build status with the current single product gate — coordinator.
- [ ] Run the current product gate on a committed candidate in the pinned development shell; do not reconstruct removed Full/phase/smoke profiles.
- [ ] Build the exact candidate's native package and confirm its version; cross-built assets require separate native execution.
- [ ] Complete release-asset execution on remaining target hosts, locally or with hosted jobs when available; Linux emulation cannot certify missing macOS/Windows/ARM hosts.
- [ ] Review breaking CLI/version surfaces, agree any version change and obtain release authorization before distribution; 0.16.0 remains a proposal.
- [ ] Decide/configure Developer ID/Authenticode signing and notarization requirements; ad-hoc signatures and checksums do not supply those identities.
- [ ] Run live-provider/billing compatibility checks only when separately authorized and configured.
- [ ] Obtain an independent security review; product tests do not supply independent certification.

## Tests and verification

- [ ] `bash scripts/bootstrap.sh` — once per fresh checkout, pinned Nix development shell.
- [ ] `cargo fmt --all -- --check` — candidate commit.
- [ ] `cargo test --workspace --locked` — candidate commit, with shell/repository test tools supplied by the development shell.
- [ ] `node --test tests/web_model.test.mjs tests/web_workspace.test.mjs tests/web_context_flow.test.mjs tests/web_multiplayer.test.mjs tests/web_agent_parity.test.mjs tests/web_rich_markdown.test.mjs` — candidate commit.
- [ ] `nix build .#aw --no-link --print-out-paths --no-update-lock-file` and built `aw --version` — exact candidate.
- [ ] Release-asset native execution — exact assets and remaining hosts; unfinished external gate.
- [ ] Version/release approval — explicit owner decision; unfinished external gate.
- [ ] Identity signing/notarization — actual configured identities and execution; unfinished external gate.
- [ ] Live-provider/billing checks — separately authorized real execution; unfinished external gate.
- [ ] Independent security review — unfinished external gate.
