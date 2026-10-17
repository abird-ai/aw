# Git and review SDK

`aw-git` is the blocking Git library used by native human review views and the browser adapter. Native views use the existing bounded Workspace executor; explicit index actions pass through the serialized Agent owner. Use the library from a host worker, never the render/input task. It supplies no model tool, Git daemon or subprocess sandbox.

## Public model

`Repository::discover(trusted_absolute_git, directory)` resolves the checkout and common Git directory. `status()`, `worktrees()` and `branches()` use machine-readable Git output. `RepoPath` stores exact bytes separate from escaped display; no pathspec is inferred from a filename. UTF-8 failure does not merge distinct paths into replacement characters.

`Change` identifies Conflicts/Staged/Unstaged/Untracked, path, optional old path, code and submodule state. One file can occur in two groups with different diff bases. Tree and List are projections of those same records. `select_worktree` only selects a discovered existing non-bare/non-prunable checkout in the same common repository; it changes no worktree files or process-global cwd.

`review(&change)` returns a snapshot containing root, basis, path identity, escaped line projection, raw patch and content digest. `compare(&branch)` gives read-only commit comparison against a currently listed branch object. No Git branch checkout is performed by selection. Repository status and review snapshots can become stale as external programs work; they are observations, not filesystem locks.

## Example: read-only library use

```rust,no_run
use aw_git::Repository;
fn inspect() -> aw_git::Result<()> {
    let repo = Repository::discover("/absolute/trusted/git", "/project")?;
    for change in repo.status()?.changes {
        let review = repo.review(&change)?;
        println!("{} / {} / {}", review.id, review.path_label, review.basis);
    }
    Ok(())
}
```

The SDK consumer [example](../../examples/sdk-review/src/main.rs) is independent of the CLI, renderer and Agent. Paths are examples; choose the actual trusted Git executable.

## Process boundary

The library launches Git directly with argument arrays. It clears ambient environment/config overrides, declines prompts, disables pager/hooks/fsmonitor/untracked-cache/external-diff/textconv, and explicitly disables lazy object fetch. No Git command asks for credentials or performs fetch/push. Project Git configuration is still data read by the trusted Git binary; local repository operations are not safe execution of arbitrary untrusted OS programs.

Content operations reject configured clean/process filter commands and partial/promisor repositories before inspecting file content. Individual paths with filter attributes are refused, including missing/global-only filter drivers. Raw untracked staging refuses text/eol/working-tree-encoding conversion attributes and configured autocrlf to avoid silently changing conversion semantics. Use trusted external Git for these unsupported repositories/paths rather than bypassing their filters.

Global/system Git configuration is intentionally excluded, so user aliases, credential helpers and global conversions are not assumed. The service is a predictable bounded review surface, not a replacement for every Git configuration. A user-provided `AW_GIT` executable is trusted native code, not authenticated by its path string.

Each command caps captured stdout/stderr at 4 MiB and duration at 10 seconds. The child process group is stopped on bound/time failure; error bodies are withheld. Changes cap at4096 records; branches/worktrees at1024. There is no broad recursive watcher or periodic full-worktree polling. Enter/refresh and explicit mutations request fresh views.

## Exact whole-file index updates

`Repository::apply_index(review, Stage|Unstage)` requires a compatible non-conflict, non-submodule snapshot. It re-reads the status and exact raw diff, compares the digest/head, checks `git apply --cached ... --check`, then applies those same bytes to the index. It never runs `git add`/`reset`/`restore` on an arbitrary selected path, and never replaces working-file contents.

Unstage reverses the exact staged patch. Renames retain both paths; binary changes retain a binary patch. A later index race is rejected by Git's preimage check. A working-tree race after snapshot validation may leave additional unstaged edits, but cannot cause unreviewed file bytes to be substituted into the staged patch. Freshness is not an exclusive lock against external editors/shells.

Index mutations are serialized through the native owner so they do not overlap its active agent command. A human PTY/external application is not governed by that mutex and may edit concurrently. Manual Git actions are **not** model-tool calls or durable `TurnJournal` events. Timeout/I/O ambiguity is displayed as unknown; inspect/refetch the index before another action. There is no automatic retry or claim of exactly-once external effects. Git's own index lock/atomic replacement remains the final storage mechanism.

No mutate-all shortcut, per-hunk stage, checkout/commit/fetch/push, arbitrary branch/worktree creation/removal/prune or conflict resolution is shipped. Phase 42 adds only host-managed detached subagent worktree create/remove at an exact reviewed HEAD under a private aw-owned path; it is not exposed as a model Git tool. Trusted external Git remains available through an explicitly launched terminal.

## Comments and reviewed state

The native UI keeps at most eight snapshots, each with at most32 comments of8192 bytes. A comment records snapshot ID, display row, old/new line references, quote and authored text. `comment ROW TEXT` uses the row numbers printed in the Diff view. `reviewed` is keyed by the content digest, not merely by file path. A changed patch obtains a new snapshot and is not automatically reviewed or re-anchored.

`send-review` copies existing comments to the unsent conversation composer only if the repository root exactly matches the current agent tool root. It does not auto-submit, elevate the comment to system instructions, or switch agent credentials. Comments and markers are memory-only and can expire with the bounded review cache; they are not yet part of durable session review storage.

The raw patch is capped at2 MiB; display rows cap at4096. Escaped/truncated display never changes bytes used for index application. Binary/combined conflict data has an explicit read-only or bounded presentation, not false source-line annotations.

## Scoped native Git containers

Choose **Open Git repository…** in the existing Workspace palette, or use
`git-open PATH`, to open a Git navigator and document group. The same relative
path in two checkouts has distinct exact review identity. Preview stays with
the invoking container's remembered document group and preserves Explorer
focus; changing focus while Git works cannot redirect its eventual preview.

Containers own placement. Their Files scope and each Git navigator's exact
checkout context are separate bindings. Existing navigators can share a
container/group without coupling their resource roots. Selecting an existing
verified worktree changes only that Git navigator's context; branch Enter is
read-only comparison. Files and the current Agent retain their bindings.

An explicit terminal launch uses the focused Git navigator or exact Diff
repository, otherwise the original human launch context. An unavailable focused
Git/Diff context refuses launch; it never borrows another container's root.
The background launch job revalidates the captured Git binding before starting
the shell. Existing terminals retain their own bindings.

Opaque context IDs and bounded saved labels are requests, never root grants.
Host retains opened contexts across Agent/Workspace presentation switches;
launch discovery stays lazy and uses the original human launch directory.
After process restart an external context is unavailable until the user focuses
its Git navigator and chooses **Reconnect Git repository…**, or
`git-reconnect PATH`. A saved Diff ID never reconstructs a patch or index action.

A moved or still-open Diff retains its original Repository/Review/context even
if its navigator selects another worktree or closes. Explicit stage/unstage
still targets that exact visible snapshot's index and requires fresh review and
HEAD validation. Reconnect, final-reference closure and layout restore invalidate
queued index admission. The owner checks that admission immediately before the
existing index operation; a mutation already admitted into Git is not silently
cancelled or replayed. External commands can race, so uncertain results still
require inspection and an explicit new action.

Retained physical directory identities and fresh Git mapping checks detect
replacement of a context's root, worktree Git directory or common directory.
These checks do not exclude concurrent external filesystem/Git writers. Human
Git reads and cached comments do not become provider context automatically;
`send-review` still requires the canonical current Agent root.

Native saved layout v4 migrates consistent v1/v2/v3 inputs while preserving
geometry, pane/tab identities and Files bindings. Legacy Git navigators join
existing group ownership where possible. If a genuinely new container ID is
required and its allocator is exhausted, migration fails atomically and leaves
the current layout and saved file intact. Old readers reject v4; SQLite v33 and
JSONL v1 are unchanged. Browser containers and docking remain separate work.

## Tests and primary references

The [actual Rust repository tests](../../crates/aw-git/tests/repository.rs) run
real Git subprocesses against private fixtures. The shared product gate is the
locked workspace Cargo suite and six web suites in the [checklist
specification](../plans/TODO-SPEC.md). The [workspace plan
TODO](../plans/workspace-views/TODO.md) records exact tested commits, native
package/terminal checks and any open platform gates. A source or cross-target
compile check does not supply an unexecuted runtime result.

Official Git status/worktree documentation and upstream API references are
recorded in [SOURCES](SOURCES.md); protocol references do not certify execution
or audit the adapter.

## Phase 6 web integration

The browser adapter exposes the bounded read-only portions of this service through registered context and inventory handles. Worktrees/Branches/Changes remain exclusive views; List/Tree is a browser projection of the same grouped rows. Web open/review/feedback does not checkout, mutate the index or initiate network Git operations. Native index commands remain available only in their original explicitly serialized path. See [browser workspace](BROWSER-WORKSPACE.md) and [surface SDK](SURFACES.md).


## Phase 42 managed subagent worktrees

`Repository::clean_head_for_managed_worktree`, `create_managed_worktree`, and `remove_managed_worktree` form a narrow host-only lifecycle. Creation requires a clean parent, concrete exact object ID, host-generated absolute nonexistent path, and rejection of executable clean/smudge/process filters or promisor/lazy-fetch repositories. Git runs `worktree add --detach` only after the store records external-effect intent; post-command aw re-discovers the checkout in the same common repository, exact HEAD and clean state. Any ambiguous post-entry failure is `MutationUnknown`.

Removal targets only a discovered same-repository worktree. Normal removal refuses dirty state; explicit `--discard` maps to the reviewed force-removal path. Inventory/path absence is verified before the durable binding becomes removed. No automatic prune/merge/commit is performed.
