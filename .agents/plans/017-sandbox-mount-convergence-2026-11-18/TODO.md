# 0.17 sandbox and mount convergence — deferred

Base: `796225e9b50e824606586859d7bcc88722b4919e`

Status: **ICEBOX / DO NOT IMPLEMENT without explicit user promotion.**

## Committed

None.

## Uncommitted

### S1 — shared mount semantic types

- [ ] Finalize `MountAccess`, `MountRequirement`, `SandboxMountSpec`.
- [ ] Define host-selected sandbox layout and reserved target/env policy.
- [ ] Preserve non-UTF-8/platform path correctness.

### S2 — layer merge and structural validation

- [ ] Target-keyed deterministic whole-record replacement.
- [ ] Required/optional source classification.
- [ ] Nested/overlapping target rejection.
- [ ] Reserved/conflicting environment validation.
- [ ] Host-integration target/environment collision checks.

### S3 — pinned capability preparation

- [ ] Pin exact source identities once.
- [ ] Carry exact authority into runtime binding/fingerprint.
- [ ] No path re-resolution after preparation.
- [ ] Required source failure vs optional source skip semantics.

### S4 — Linux Bubblewrap generic mounts

- [ ] Extend BwrapPlan with RO/RW descriptor-backed generic mounts.
- [ ] Deterministic target-parent creation.
- [ ] Preserve executable/workspace/proc/dev/tmp protections.
- [ ] Inject only validated environment.

### S5 — dotlink consumer/config v13

- [ ] Add profile `mounts`.
- [ ] Add v12 -> v13 migration.
- [ ] Add `DOTLINK_MOUNTS`.
- [ ] Add repeatable `--mount`.
- [ ] Map developer caches to optional shared mount specs.
- [ ] Preserve precedence builtin < profile < env < CLI.
- [ ] Keep NixOS use of `DOTLINK_MOUNTS`.

### S6 — release equivalence and docs

- [ ] Mirror final `ae4d623 + d855672 + 0983d39` semantics.
- [ ] Update configuration/security/Nix docs.
- [ ] Full Bubblewrap smoke and host-independent tests.

## Tests and verification

- [ ] shared mount merge/validation unit tests.
- [ ] required vs optional source tests.
- [ ] pinned capability/path-retarget tests.
- [ ] Linux Bubblewrap RO/RW/env tests.
- [ ] dotlink config migration/parser/precedence tests.
- [ ] `DOTLINK_TEST_BWRAP=1` full affected smoke.
- [ ] strict Clippy/fmt/diff and Nix checks.

No checkbox above may be worked until explicit promotion.
