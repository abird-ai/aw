# V0 — Original three-mode aw TUI (frozen visual baseline)

Status: historical design; preserved, not deprecated. Preview: [v0.html](v0.html).

## Thesis

One composable aw product with three progressive disclosures, not separate apps:

1. **Minimal Agent**: Pi-like native Agent conversation, unboxed/thin-accent user input, subdued green/red/neutral tool strips, no permanent sidebar and a compact footer. The transcript has stable scrollback, visible tool progress, command popup near composer, and the selected repo in footer.
2. **Agents mode**: conversation with a **left hierarchical Agent tree** reminiscent of pi-herdsman. Parent and child ownership is rendered with ASCII/Unicode connectors; Machine hints and Project groups are shown without confusing physical placement with actual parenthood. One Agent is selected and its transcript fills the main view.
3. **Full Workspace**: sidebar with Spaces, Machines, Projects, Agents, Terminals; Agent conversation plus Git/file/terminal main tabs and a contextual **right inspector** by default. Git shows actual staged/unstaged state; File view has lazy tree and line-numbered read-only preview; Terminal view aggregates sessions.

### Core image/layout elements to preserve

- Real nesting: ▾ ▸ │ ├─ └─; no artificial indentation implying authorization.
- Small indicator lights: green success/healthy; blue running; amber needs attention/dirty; red error/blocked; neutral idle/unknown.
- Thin accent on selected items; no over-boxed Agent messages; tool outputs tinted by result.
- Keyboard-first; mouse is additional, never required.
- Local/project/worktree labels never obscure which machine/branch a file or PTY belongs to.
- Preserve current Pi-like status/composer; avoid terminal-wide clears or scroll-anchor jumps.

## V0 visual sketches

    Minimal Agent
    ┌────────────────────────────────────────────┐
    │ aw / aw ◆ main                 ● local     │
    │                                            │
    │  › Review the authorization model.         │
    │  I'll inspect tool and grant paths.        │
    │  ✓ read · tool.rs                          │
    │  ✓ tests · 47 passed                      │
    │                                            │
    │  ❯ Ask anything…                           │
    └────────────────────────────────────────────┘

    Agents                        Workspace
    ┌──────────┬───────────┐      ┌─────────┬─────────┬────────┐
    │ ▾ aw     │ Agent     │      │ Spaces  │ Agent   │Details │
    │ ├ ● root │ transcript│      │ Projects│ Chat    │Git     │
    │ │ ├ ● a  │ composer  │      │ Agents  │ Git     │Agents  │
    │ │ └ ◐ b  │           │      │ Terminals│Files   │Events  │
    └──────────┴───────────┘      └─────────┴─────────┴────────┘

## State / interactions

- Mode switch never creates, destroys or migrates an Agent/Session.
- Selected Agent stays selected when changing modes; nested tree follows actual owner/parent, not machine membership.
- Side-by-side right inspector in V0 is **intentional historical design**; V1 replaces permanent inspector with optional on-demand inspector.
- Project view is a native Git worktree inspector, not a cloud or web file index.
- Full/Minimal/Stateless product cuts must retain their established rights and behavior.
- Color is redundant with shape, text and accessibility labels.

## Why V0 is kept

V0 remains the source of the three-mode mental model, Pi-like simplicity, nested Agent feedback, intuitive keyboard/text presentation, and first-class repository view. Every later version is additive, not permission to throw out these advantages.
