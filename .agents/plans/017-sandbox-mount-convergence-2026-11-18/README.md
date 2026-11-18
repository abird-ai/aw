# 0.17 sandbox and mount convergence — deferred

Author: Prasanna Loganathar
Co-authors: ChatGPT GPT-5.6 Sol
Owner: @pvl

**Status: designed and frozen. Do not implement until the user explicitly
promotes/pulls this plan.**

This plan captures the shared-first design for dotlink v0.17.2/v0.17.3 generic
shell mounts. It is deliberately excluded from the active 0.17 shared-runtime
implementation.

Reference release implementation:

- mount core through `ae4d623`;
- platform-independent follow-ups `d855672`, `0983d39`;
- no mount-core drift from v0.17.3 through v0.17.4 `fd1cdb6`.

## Why AW should own the mount semantics

AW already owns:

- `aw-tools::sandbox::SandboxPolicy`;
- pinned sandbox directories;
- `aw-tools::sandbox_linux::BwrapPlan`;
- descriptor-backed Bubblewrap bind planning;
- sandbox network and writable-root policy.

A permanent dotlink-only `src/mounts.rs` would create a second sandbox policy
engine for target validation, nesting, environment conflicts, pinning and bind
planning.

The shared design therefore extends AW's existing sandbox abstraction, while
dotlink keeps configuration/discovery syntax.

## Shared mount model

Candidate semantic values:

```rust
pub enum MountAccess {
    ReadOnly,
    ReadWrite,
}

pub enum MountRequirement {
    Required,
    Optional,
}

pub struct SandboxMountSpec {
    pub source: PathBuf,
    pub target: PathBuf,
    pub access: MountAccess,
    pub requirement: MountRequirement,
    pub env: ...,
}
```

Do not freeze exact field/container types until no_std/std and non-UTF-8 path
requirements are reviewed.

### Required vs optional

Map dotlink semantics:

- profile/env/CLI explicit mount -> Required;
- discovered developer cache -> Optional.

Required invalid/unusable mount fails closed or yields the host's retryable
runtime failure. Optional discovered mount may be skipped with bounded
diagnostic evidence.

## Sandbox layout

Do **not** hardcode dotlink's release `/tmp/home` into AW.

AW sandbox validation accepts a host-selected layout/policy including:

- permitted mount target root(s);
- reserved targets;
- reserved environment names;
- host integration targets/environment;
- workspace/root semantics.

Dotlink may continue to select its product layout. AW owns validation of the
selected layout.

## Shared validation

AW should own:

- absolute source/target validation;
- NUL/control-invalid path rejection as platform appropriate;
- target must remain inside configured mount root;
- target-keyed whole-record layer replacement;
- deterministic layer order;
- nested/overlapping target rejection where bind semantics would be ambiguous;
- reserved environment rejection;
- conflicting environment-value rejection;
- host integration target/env collision rejection;
- source capability pinning;
- final validation at bind preparation, not only config parse;
- runtime-binding/fingerprint contribution from exact pinned mount authority.

## Linux Bubblewrap binding

Extend `sandbox_linux::BwrapPlan` to consume validated pinned generic mounts:

- RO -> `--ro-bind-fd`;
- RW -> `--bind-fd`;
- create target parent directories deterministically;
- bind exactly the pinned source descriptor;
- keep descriptors alive through spawn;
- preserve existing executable pinning;
- preserve workspace/proc/dev/tmp/reserved-target protections;
- environment injection comes from validated mount facts.

No path re-resolution after authority preparation.

## Dotlink consumer responsibilities

Dotlink keeps:

- config schema v13 migration;
- profile `mounts`;
- `DOTLINK_MOUNTS`;
- repeatable `--mount`;
- one-line `SOURCE:TARGET[:ro|rw]` parsing;
- profile-only richer environment declarations;
- developer cache discovery;
- precedence:
  builtin/discovered < profile < environment < CLI;
- NixOS choice to use `DOTLINK_MOUNTS`;
- product diagnostics naming `mounts[i]` / cache families.

Dotlink translates these into shared AW mount specs and supplies its layout.

## Release semantics to preserve

- merge by sandbox target, last layer wins;
- replacement replaces the whole record;
- nested targets fail closed;
- explicit invalid declaration fails closed;
- explicit temporarily unusable source is not silently dropped;
- discovered cache can be skipped with one warning;
- environment cannot repoint sandbox-owned variables;
- shared environment key is allowed only when values agree;
- fixed host integrations cannot be shadowed/nested;
- final binder revalidates the post-skip assembled set;
- tests remain host/platform independent where possible.

## Promotion rule

No source, config schema, CLI, Nix or test changes from this plan may begin
until the user explicitly asks to promote/pull the sandbox plan. Merely working
the active 0.17 shared-runtime plan is not authorization.
