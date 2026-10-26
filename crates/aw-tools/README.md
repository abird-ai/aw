# aw-tools — native host tooling

Native adapters for AW's existing `aw-agent-core::Tool`. There is **one**
Agent, ToolRegistry, ToolPolicy, TurnMachine and durable intent/receipt journal;
no filesystem library may grant its own model/tool authority.

## Tool schema

| Tool | Arguments | Execution |
| --- | --- | --- |
| `ls` | `path?, pattern?, depth?, kind?, sort?, limit?, gitignore?, skip_hidden?, parallel?` | bounded glob traversal; global top-N; incomplete flag |
| `grep` | `pattern, path?, glob?, literal?, ignore_case?, output_mode?, before?, after?, numbered?, offset?, limit?, gitignore?, skip_hidden?, parallel?` | content/files/count; 32 MiB/4096 file content budget |
| `read` | `path, offset?, limit?, numbered?, parallel?` | exact UTF-8 line window; 32 MiB seek budget |
| `write` | `path, content, create_only?, expected_sha256?` | conditional atomic UTF-8 create/replace; no-op receipts |
| `edit` | `path?, edits:[{path?,old_text,new_text,replace_all?}]` | ordered exact multi-file edits |
| `patch` | `dir?, patch, best_effort?` | Codex Update/Add, preflight and staged per-file publication |
| `bread` | `path, format?, offset?, limit?, parallel?` | base64/hex byte windows; `mcp` complete-file JSON envelope |
| `bwrite` | `path, data, encoding?, create_only?, expected_sha256?` | conditional atomic binary create/replace |
| `bedit` | `path, edits:[{offset,length?,data?,encoding?}], sha256?` | original-relative bounded byte ranges |
| `bash` (Unix) / `powershell` (Windows) | `command, dir?, stdin?, timeout_secs?, max_output_bytes?, parallel?` | explicitly trusted Process |

`find` is retired only as a **model tool**. Recursive `ls` handles globbed
discovery; human Workspace Find is unaffected. Tool schema parity is checked
by tests, and the dotlink-only compatibility constraint is documented below.

## Authorization and scheduling

- `--tools-mode off` (default): no tools.
- `read-only`: `ls`, `grep`, `read`, `bread`.
- `workspace`: read-only plus `write`, `edit`, `patch`, `bwrite`, `bedit`.
- `trusted`: workspace plus the **unsandboxed** host-selected native shell
  (`--tools-bash` on Unix, `--tools-powershell` on Windows).
- `--tools-allow` intersects, never widens, those tool sets. Child agents
  retain their original narrower read/read-write-edit subsets.
- AW's TurnMachine admits concurrent calls **only** for adjacent, explicitly
  model-selected `parallel:true` reads or already-granted trusted Process
  calls. Writes remain barriers. No execution in tool metadata or discoverers.
- The host's 64 KiB argument / 128 KiB result envelopes are authoritative;
  lower per-tool budgets protect file scanning, result rendering and memory.

## Filesystem guarantees

- Descriptor-pinned, component-by-component, no-follow reads/writes; only
  singly linked regular files as content. Linux uses `openat2` with
  `RESOLVE_BENEATH|RESOLVE_NO_SYMLINKS|RESOLVE_NO_XDEV`; no fallback that
  silently drops required resolution guarantees.
- File content is validated as UTF-8 with no binary control bytes for
  `read`/`grep`/`write`/`edit`/`patch`. Arbitrary bytes use binary tools.
- Bounded heap top-K listing and sorted candidate selection. `scan_incomplete`
  and `incomplete_reason` distinguish global traversal/content budgets
  from ordinary result `truncated`. Opt-in Gitignore/hidden filtering is
  bounded by both per-file and aggregate work ceilings.
- Snapshot verification, sibling staging, per-file atomic install, no-op
  detection, optional exact SHA-256 write preconditions. Existing Unix file
  owner/mode and supported ACLs/xattrs are preserved; setuid/setgid files and
  Linux `security.capability` require separate privileged host operations.
  Linux create-only publication uses atomic no-replace rename, never a
  link-then-unlink sequence that could publish a doubly linked file.
  Windows existing-target DACL is supplied at CreateFileW time before any
  staging bytes are written, avoiding a transient permissive ACL.
- Aborted stages remove temporary files. Empty parent directory cleanup uses
  recorded file identity to avoid removing a directory already replaced by
  another actor. Non-cooperating same-user processes can still race directory
  name resolution; no filesystem capability model is claimed to sandbox them.
- Unix mutations use the same sorted ancestor-intention/nearest-parent
  exclusive advisory `flock` convention as dotlink; Windows mutations use
  the same canonical-path-derived named kernel mutex convention. Read tools
  never acquire the mutation mutex. External programs ignoring the protocol
  can still race, and a multi-file commit is **not** a transaction.
- Cancellation, tool timeout, failed staging, and uncertain Process/write
  effects remain subject to AW's durable journal and inspection/no-replay.
- Windows native Process children are started suspended, assigned to an
  invocation-owned Job Object, and only then resumed. Killing/dropping a
  Process cleans its job rather than signalling a possibly reused PID.
  AW Git/terminal launch paths are unaffected by this added managed mode.
- The Agent's shared output-budget projection and final model-visible tool
  message preserve parseable JSON, including for structured errors and
  truncated output. Human notices are appended only to plain-text results.
- `grep` preserves the location of oversized matching lines via explicit
  abbreviated-line markers, honors the requested hit limit across files
  (without an undocumented 50-hit/file cap), and distinguishes line
  abbreviation from result-page truncation. `read` reports the last returned
  line in its truncation marker.
- Discovery excludes exact AW and dotlink staging temporary filenames, not
  ordinary user files with similar names. Shell argument validators enforce
  the same limits advertised in their JSON schemas; PowerShell is registered
  under `powershell`, not `bash`.

## Deliberate compatibility boundaries

Dotlink's `bread(format=mcp)` returns true MCP typed image/audio/blob
`ContentBlock`. AW's native `ToolOutput` is presently **text-only**, so
the same `format=mcp` input returns a complete-file, MIME-labeled,
base64-encoded JSON envelope with `transport:"encoded_json"`. It does not
pretend to be a model-visible typed image. True typed output requires an
explicitly designed Agent/provider media adapter; never bypass tool grants.

Dotlink's Bubblewrap shell is a separately sandboxed *machine execution
facility*. AW retains its existing explicitly trusted, **unsandboxed**
host-selected Process boundary. Sandbox mounts, network grants, and OS
isolation must be added as a distinct host-approved execution mode, not by
silently changing the meaning of `trusted`.

Windows GNU cross-compilation of `aw-platform-win` and `aw-tools`,
including test targets, is verified. Native Windows and macOS runtime
hardening tests remain separate gates: cross-compilation cannot attest ACL,
job-object or filesystem recovery behavior. Full Windows CLI cross-compilation
requires a MinGW C compiler for the bundled SQLite dependency.

See [current tool contract](../../.agents/docs/TOOLS.md) and
[tool-port plan](../../.agents/plans/dotlink-tool-port/README.md).
