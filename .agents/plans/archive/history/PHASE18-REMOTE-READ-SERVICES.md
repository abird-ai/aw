# Phase 18 — capability-bound typed remote read services

Phase 18 places real read-only operations behind the signed MachineLink lease from Phase 17.

## Implemented services

- Machine metadata summary;
- accessible Workspace summary listing;
- bounded descriptor-relative file listing and UTF-8 file read;
- read-only Git status through the existing hardened aw-git service;
- redacted Session summary listing.

Each operation maps to one exact MachineCapability. The client cannot select the server root or Git executable. The serving process binds those locally before the route is opened.

## Redaction and filesystem boundary

Remote Session summaries intentionally omit SessionBinding fields including working directory, tool root, system prompt, profile hint and model hint.

File operations reuse WorkspaceFiles/scoped-root semantics: no symlink escape, traversal, hardlink ambiguity or files above the existing size bound. Git operations reuse aw-git and accept no caller-provided command or pathspec.

## Not implemented here

PTY and RemoteSession capabilities remain named but inactive. There is still no generic command RPC, reverse shell, arbitrary port forward or remote process launcher.

## Next

Phase 19 adds durable PeerWorkspace and RemoteSessionRef state plus an explicitly granted typed remote-session operation. Remote work remains authoritative on the executing AwNode; the source stores a reference rather than a second canonical transcript.
