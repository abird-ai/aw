# Git is authoritative from Phase 8

The working source is now a normal Git repository, not an append-edited tar.
Every complete committed snapshot in the Phase 0–7 archive was imported, in order,
as a Git commit. Original file bytes, executable bits, deletions, timestamps and
notes were preserved. The archive did not record human authors: imported commits
use an explicitly synthetic `aw archive import` identity and identify their
original revision/hash in the commit message. No prior Git authorship is implied.

The import has 52 commits, 52 verified snapshot trees and tags `phase-0` through
`phase-7` at the exact delivered archive heads. See [accepted history](../plans/accepted-history/TODO.md); Git retains the original mapping.
Uncommitted historical experiments cannot be recovered; no history is invented.
All subsequent work is ordinary commits on `main`. `phase-8` labels the source
candidate, not a tested release. Earlier build limitations remain honest history.

## Clone this phase

```sh
git clone aw-phase-8.bundle aw
cd aw
git fsck --full
git log --oneline --decorate --all
nix develop -c bash cargo test --workspace --locked
```

The bundle is self-contained; no preceding tar/bundle, network remote, Git LFS,
submodule, compiler cache or special archive reader is needed to clone its source.
Cargo/Nix dependencies still need resolution/fetching; a Git bundle is not a
prebuilt binary or vendored dependency cache.

## Iterate and deliver a later phase

Edit the checkout, run the applicable tests, and commit changes normally. Generate
and retain the real Cargo.lock on a capable host. Keep secrets, grant/key files,
actual session databases, private transcripts and build artifacts out of commits.

```sh
git add path/to/reviewed/source Cargo.lock
git commit -m 'Describe the verified change'
git tag phase-9                   # only when that phase is intentionally prepared
bash scripts/bundle.sh ../aw-phase-9.bundle
```

`bundle.sh` refuses a dirty working tree and produces a **full-history** bundle.
It does not mutate/append an existing bundle or rewrite commits. To receive a
newer phase in an existing checkout, fetch its `main` into a remote-tracking ref,
inspect it, and fast-forward or merge deliberately; do not discard local commits.

```sh
git fetch ../aw-phase-9.bundle refs/heads/main:refs/remotes/phase-drop/main
git log --oneline HEAD..refs/remotes/phase-drop/main
git merge --ff-only refs/remotes/phase-drop/main
# Fetch wanted phase tags separately after review; no forced tag replacement.
```

The legacy `the removed `awtar` helper`, archive tests and prior archive instructions stay in
history/the source for provenance only. They are not the active edit/build or
phase-delivery workflow. Keep the Git repo between phases; do not re-import the
old archives. See `git bundle` official documentation in [SOURCES](SOURCES.md).

## Phase 9 continuation

Phase9 is ordinary Git development on top of the verified Phase8 history. Tag `phase-9` marks source0.10.0, not a compiled release. Clone the new full bundle directly, or fetch its main ref into a review branch before deliberately fast-forwarding/merging your local work. Prior phase tags and archive import mappings stay unchanged. No tar re-import or history rewrite is used.

```sh
git clone aw-phase-9.bundle aw
# Or from an existing reviewed checkout:
git fetch ../aw-phase-9.bundle refs/heads/main:refs/remotes/phase-drop/main
git log --oneline HEAD..refs/remotes/phase-drop/main
git merge --ff-only refs/remotes/phase-drop/main
```


## Phase 10 source delivery

Phase 10 continues the existing 60-commit baseline (including the separate context-flow design amendment) with ordinary commits. All previous phase and design tags are retained. `phase-10` labels a source candidate, not compiler, model-quality or security validation.

```sh
git clone ./aw-phase-10.bundle aw
cd aw
nix develop -c bash cargo test --workspace --locked
```

Commit actual resolver output, formatter/compiler corrections and real host evidence after review. Do not rewrite the earlier source/test history. The bundle is complete and requires no previous archive. The current source includes the SQLite v2 migration and device wire v2; read [context-flow implementation](CONTEXT-FLOW-IMPLEMENTATION.md) and [device protocol](DEVICE-PROTOCOL.md) before upgrading existing state or a device pair.


## Phase11 validation receipts

[The new gate](../plans/TODO-SPEC.md) hashes working-source bytes/executable bits and the actual lock, excluding generated validation evidence. Commit the exact tested source after corrections; an unchanged content digest can remain valid across that commit. New edits or missing test logs invalidate it. Full-history bundle creation remains independent of execution evidence: a verified clone/bundle is not a native build result. Run `git archive` to export only reviewed report/log/lock/patch evidence; do not commit a host scratch directory or actual runtime databases.
