# R2a — on-demand coding search using ripgrep's Rust libraries

**Date:** 2026-10-09
**Status:** R2a implementation and focused validation complete; R2b remains independently pending.
**Program:** AGENTC-PARITY-IMPLEMENTATION-PHASES-2026-10-09.md. Not project Phase 69.
**Reference:** agentc 0.6.0 core tools ls/find/grep and tool engine; target aw 0.15.0 with previous R1 source-grant kernel.

## User decision: reuse mature Rust search, not write miniature rg/fd

- The rg and fd binaries are std/OS applications; they are not usable wholesale as no_std linked libraries.
- The **std** aw search adapter now reuses the Rust pattern/ignore stack used by their projects: regex =1.12.3, globset =0.4.18, ignore =0.4.25. This is a stable in-process backend, with no subprocess or PATH dependency and one consistent tool schema.
- **Embedded no_std** will not link aw-tools/std/filesystem. Future portable pattern evaluation can reuse regex-automata with default-features=false, alloc/syntax/meta as needed over host-provided filenames/bytes. A disposable regex-automata 0.4.18 test compiled for wasm32v1-none with no_std+alloc on the available Rust 1.95.0; it is not yet linked into aw's embedded Agent. Precompiled DFA can be considered for no-alloc Tier K, but no regex compiler or filesystem traversal is promised there.
- We do **not** implement custom regex, Gitignore or glob engines. The small aw-owned layer handles validation, budgets, descriptors/root access, cancellation, normalized result formatting and tool permission.
- The std implementation purposely does **not** spawn rg/fd as a subprocess merely because it can: subprocess search bypasses descriptor-relative root access and adds process/version/PATH coordination. R2b may add explicitly trusted optional process acceleration only after a measured need, and it must not weaken the current root boundary. This is a deliberate internal design deviation from agentc, not a claim of its engine flag parity.

## Implemented

- Model tools ls (sorted single directory), find (glob path matching), grep (regex/literal, ignore-case, file glob and context) via existing aw-tools Tool and ToolPolicy.
- ReadOnly, Workspace and Trusted modes expose the same bounded *read-only* search tools alongside their prior file tool set; Off exposes none.
- Both std/full and std/JSONL minimal CLI can optionally narrow grants with repeated/comma-separated --tool-allow NAME. The names are validated against already host-granted tool mode; a selection never grants more authority and off+selection fails before provider contact.
- The existing child-agent read-only and workspace tools remain explicitly constrained to legacy read or read/write/edit, respectively. Adding top-level search does not silently widen the previously approved delegated authority.
- Scanning uses aw-platform::scoped_fs::Root::list and Root::read exclusively. The ignore crate is used as a **matcher built from scoped read bytes**, not as an ambient-path filesystem walker.
- Parent and nested .gitignore rules are applied to the bounded scoped walk; symlink/reparse, hardlinks and unsupported entries do not contribute search content. No filesystem index/watch/LSP or executable child command.
- Conservative limits: 8000 visited entries, 64 sublevels, 512 entries per directory from scoped root, a finite search time budget (best-effort around blocking kernel calls), 50 KB output, 200 matches, 1024-byte patterns, 64 KB/1024-line .gitignore and UTF-8 text files at most 1 MiB (inherited from scoped Root).
- Invalid/out-of-scope requests are denied or reported as a tool error, not silently rerouted to a shell or external index.

## Validation scopes

- Rust search schema and real temporary root tests: grep regex/literal, ls sorting, find/glob, ancestor/root .gitignore, explicit file paths, symlink traversal rejection, hardlink/binary omission, truncation and cancellation.
- Actual minimal-binary fake OpenAI tool workflow: one full 3-call ls/find/grep loop, per-tool JSON context results, .gitignore/non-escape proof; invalid/off named selection denied before provider contact; unadvertised model tool call refused.
- Existing Phase-2 read/edit/write fake provider fixture remains tested, now correctly expecting the added read-only tool names; historical child-agent delegation tests remain unchanged.
- Native full and minimal suites, no_std R1 kernel proof, Clippy, source checks, PTY and optional reference validation remain required.
- No real model/provider, external rg/fd process acceleration, Windows cmd/PowerShell tool or production release claim is made.

## Focused verified evidence (2026-10-09)

- aw-tools unit tests **7/7** including search schema/scoped actual filesystem regressions (Root::list/read, root/nested ignore, symlink/hardlink, exact file, truncated output, cancellation).
- Full std aw CLI **258/258**, std JSONL minimal **86/86**; existing subagent read-only and workspace fixture unchanged, with trusted AW_GIT/AW_BASH paths provided for this Nix host.
- Actual minimal aw fake-provider search **4/4**: one ls/find/grep model loop and native scoped files, denied grant widening, tools-off refusal and a model proposal not in the allowlist; original Phase-2 file tool binary smoke **4/4**, earlier JSONL no-replay **6/6**.
- Actual minimal Agent PTY Inline/Scrollback/Fullscreen **3/3** and prior linked R1 wasm32v1-none grant consumer **zero imports**, with std-based R2 search remaining separate.
- Whole workspace/all-target compilation and Clippy **-D warnings** green, rustdoc **-D warnings** and doctests green. New source-only R2 contract **50/50**; R1 source contract **42/42**, handoff preservation **89/89**, static validation **1064 active files, zero errors/warnings**.
- Repository-wide rustfmt remains red in **six unchanged pre-existing files**; the R2 modified Rust sources pass format checking. No unrelated formatting normalization was performed. Full release and real Windows shell/platform testing not claimed.
- A disposable no_std+alloc regex-automata 0.4.18 project successfully compiled for wasm32v1-none on Rust 1.95.0; temporary build inputs cleaned up. This is only library feasibility, NOT a linked no_std aw search host, Linux libc-free program or pinned Rust 1.98.1 proof.

## Remaining R2b and future phases

- Optional externally executed rg/fd backend **only if** measured benchmarks justify it and a safe explicit path/tool selection can preserve rooted reads and results. A user can still invoke rg via the pre-existing explicit trusted bash tool, which is not a substitute for a no_std library.
- Windows trusted shell selection/Job Object cancellation is a separate R2b OS adapter; no new default shell permission.
- More advanced grep output contexts, binary/file-size handling and glob parity can be refined based on independently measured agentc fixtures; no false parity certification without exact tests.
- E2/E3 shared Agent extraction is still required for a full no_std embedded Agent. This R2a std tool adapter does not move OS scanning into the portable core.
