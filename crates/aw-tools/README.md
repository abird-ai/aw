# aw-tools — native host tooling

Native implementations of AW's **existing** `aw-agent-core::Tool` contract.
The tool registry, exact actor/source/site/effect grants, immutable
`TurnMachine`, journal-first dispatch, cancellation and model-directed
parallelism remain in `aw-agent-core` / `aw-agent-kernel`.

## Surface

| Mode | Built-in tools | Authority |
| --- | --- | --- |
| `off` | none | default |
| `read-only` | `ls`, `grep`, `read`, `bread` | scoped reads |
| `workspace` | read-only + `write`, `edit`, `patch`, `bwrite`, `bedit` | scoped writes |
| `trusted` | workspace + optional `bash` (Unix) | unsandboxed OS-user process |

`--tools-allow` intersects the mode; it never adds authorization.
Read tools accept `parallel: true` as the model's *explicit* scheduling
choice. AW's existing host ceiling and journal enforce ordering. No implicit
parallelism, no parallel writes, no source-grant changes, and no untrusted MCP
read-only hints. Child agents retain their **existing**, narrower selection.

`find` has been retired as a model-facing tool. Recursive `ls` with
`pattern` and `depth` subsumes globbed path discovery. The human-facing
find/search UI is unchanged.

- `ls`: recursive glob, `depth`, `kind`, `sort`, `limit`.
- `grep`: regex/literal, glob, `content|files|count`, numbered context,
  offset/limit and opt-in Gitignore/hidden filtering.
- `read`: streamed 1-based `offset` / `limit`, optional line numbers,
  exact CRLF/text window with `numbered=false`.
- `write`: atomic UTF-8 create/replace, missing parent directories permitted.
- `edit`: ordered exact multi-file `edits[]`, unique matches or `replace_all`.
- `patch`: bounded Codex-style update/add grammar, strict or `best_effort`.
- `bread`: bounded base64/hex byte windows; `bwrite`: atomic binary replace;
  `bedit`: SHA-256-conditional, multi-range binary edits.

File safety belongs to `aw-platform-{unix,win}::scoped_fs`, not the agent:
pinned/no-follow paths, single-link regular files, bounded IO, snapshot
verification, sibling staging, atomic per-file publication, cancellation and
cooperating mutation locks. Reads are independently concurrent; writes on a
shared Unix workspace coordinate via advisory directory `flock`. Filesystem
locks are not a sandbox against an unrelated shell, external editor or malicious
same-UID process. Multi-file publication is **not a transaction**; a late
commit failure may leave earlier files updated and must be inspected.

AW has a text-only `ToolOutput` today. `bread` returns base64/hex JSON, not
dotlink's typed MCP image/audio/blob result. Typed media needs a separately
reviewed adapter; no raw media is implicitly placed into model context.

Windows uses the existing conservative per-root mutation mutex; the new
staging path copies the target DACL to the empty temporary and uses
`ReplaceFileW` with backup recovery. Native Windows compile/runtime/ACL
verification remains outstanding and is not implied by Linux tests.

See [live tool documentation](../../.agents/docs/TOOLS.md) and
[port plan](../../.agents/plans/dotlink-tool-port/README.md).
