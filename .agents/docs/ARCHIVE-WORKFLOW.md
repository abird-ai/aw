# Direct tar editing and phase iteration — Phase 3 revision workflow

## Superseded by Git from Phase 8

This file records the Phase0–7 archive mechanism only. The 52 completed snapshots have been imported into Git and verified, with original phase tags. Work directly in the Git checkout and deliver full-history bundles via [GIT-WORKFLOW.md](GIT-WORKFLOW.md). Do not apply current changes to the old tar or extract/retar for builds. The legacy helper remains only for historical import/reference tests.


## Canonical file, incremental build cache

`aw-phase-3.tar` is an **uncompressed USTAR** archive with one top-level `aw/` directory. It is also a lightweight append-updated source workspace. `the removed `awtar` helper` is a standalone Python 3.11+ standard-library utility for Unix hosts. It is development tooling, not part of the Rust runtime and not an LLM-accessible agent tool.

Normal list/read/put/patch/delete operations work against archive members directly. They never unpack the project. On commit, the helper overwrites the zero footer, appends only changed file records and a complete logical snapshot manifest, restores the footer, flushes, and fsyncs. Existing committed file bodies remain in place. The source tar is not rebuilt on each change.

Cargo/Nix compilers still require directory inputs. `checkout` materializes a build cache once and then changes only differing managed files; it preserves untracked `target/` output and refuses to overwrite unsynced local edits. This is not a promise of compiling directly from a tar byte stream or never extracting any file once.

## Commands

```sh
python3 awtar.py aw-phase-3.tar list
python3 awtar.py aw-phase-3.tar read Cargo.toml
python3 awtar.py aw-phase-3.tar history
python3 awtar.py aw-phase-3.tar verify

# Replace/add one text or binary file:
python3 awtar.py aw-phase-3.tar put src/main.rs --file main.rs
python3 awtar.py aw-phase-3.tar put scripts/new-check.sh --file new-check.sh --executable

# Apply a standard exact unified patch in one commit:
python3 awtar.py aw-phase-3.tar patch --file change.patch --note 'Fix provider parsing'

# Optional compare-and-swap protection for an agent holding an old snapshot:
python3 awtar.py aw-phase-3.tar patch --file change.patch --expect FULL_COMMIT_ID

# Remove from the logical source tree, without rewriting old tar entries:
python3 awtar.py aw-phase-3.tar delete .agents/docs/obsolete.md
```

Patches accept multiple files/hunks, additions/deletions using `/dev/null`, UTF-8 text and no-final-newline markers. No fuzzy hunk offsets are guessed. Invalid/overlapping/stale patches reject before modifying the committed archive. Binary files use `put`. Renames use explicit put/delete; their combined form is available via the Python API's single `commit` mapping if atomicity is required.

## Build and persist generated locks

```sh
python3 awtar.py aw-phase-3.tar checkout ./aw-build
cd aw-build
nix develop -c bash cargo test --workspace --locked

# Preserve fixes and official rustfmt results to tracked sources.
python3 the removed `awtar` helper ../aw-phase-3.tar sync . --note 'Host validation corrections'
# Generated untracked lock must be explicitly admitted into the archive.
python3 the removed `awtar` helper ../aw-phase-3.tar sync . Cargo.lock flake.lock --note 'Dependency locks'
```

Default sync considers previously tracked names only. A new test/source/generated lock must be named or put explicitly. Logs under validation/host can be added after reviewing them for private endpoint/model/path information. Never sync `.env`, credentials, target output or an entire home directory. Both checkout and sync use the recorded common commit and refuse conflicting local changes/stale revisions.

When another agent edits the tar, run `checkout` again against the same build directory; unchanged files retain their content/mtime and build products remain. If you independently modified the cache first, sync or reconcile it explicitly instead of blindly overwriting.

## Snapshot format

Each logical revision has a content-hashed JSON manifest at `aw/.awpack/commits/<sha256>.json`:

```text
schema, parent, revision, timestamp, note
files[path] = {data offset, size, mode, sha256}
```

File paths are restricted relative POSIX names; archive records are regular files only. No symlinks, device nodes, extraction traversal or executable metadata hooks are accepted. Scripts preserve only ordinary 0644/0755 modes. The current snapshot is the latest valid complete commit chain, not simply the last matching header seen by a generic tar reader.

All operations acquire an advisory `flock` on the same archive descriptor. Independent programs ignoring that lock are not coordinated. This is a single-file local development store, not a multi-machine distributed filesystem, authorization system or safe place to run untrusted code.

## Crash and integrity behavior

A commit record follows its member data. An interrupted append can leave a dirty tail; the last fully valid prior commit remains readable. `verify` checks current file hashes and reports the dirty tail. Mutating commands refuse to append over it until you explicitly inspect and choose:

```sh
python3 awtar.py aw-phase-3.tar repair --discard-uncommitted
```

This discards only uncommitted tail bytes. It is not recovery for tampered/lost bytes belonging to an earlier acknowledged snapshot. Keep backups or phase snapshots on reliable storage. A successful fsync is the local commit boundary; the utility does not claim resilience beyond the underlying filesystem/storage guarantees.

## Conventional tar compatibility, compaction and secrecy

The file remains a parseable tar archive. However, repeated member names and manifest-based deletions mean generic `tar -xf` does **not** necessarily represent the latest logical snapshot. Use `checkout`, or create a clean single-snapshot export only when required for another tool:

```sh
python3 awtar.py aw-phase-3.tar export aw-clean-export.tar
```

Export copies active files into a new archive; it is an explicit occasional normalization, not the edit loop. Gzip/zstd compression prevents efficient in-place append semantics here; keep the editable master uncompressed. Copying the master to a new phase filename preserves its history without untarring it.

Deleting a secret does not remove its historical bytes. Do not put secrets in the archive. If one was included, revoke it and use an explicitly sanitized clean export and verified deletion of older distributions; a logical tombstone is not secure erasure. Commit notes, filenames and retained docs can also be sensitive.

The outer `.sha256` is a distribution-snapshot checksum. It changes after intentional edits. The internal manifests retain per-revision/content integrity; neither signature authenticates an unknown archive author's code.
