# Navigation, identity and data projection contracts

Status: V1.4 design-only. Crosscuts V0–V1.4. No new storage schema or runtime authority is specified as implemented.

## Entity definitions (names are intentionally not interchangeable)

- **Machine**: physical/virtual execution host with authenticated connection, freshness, capability and local credential custody. May belong to several user Spaces, independently of its process ownership.
- **Space**: persistent logical collection of machine memberships, project references, local UI saved tabs/layouts and filters. Built-in Local Space per machine; named Space may contain one or many machines. Fleet is a Space cardinality property, not a distinct type or grant.
- **Home working context**: user's ~ on a single machine, visible in Local Space; not a Git repository until one is actually selected.
- **Project**: logical development/repository identity (may have multiple worktrees on separate hosts).
- **Worktree**: concrete repository checkout with branch, working directory and machine/owner site. Git file operations must pin it.
- **Session**: durable conversation and its owned history.
- **Agent Actor**: one executing actor with actual parent/child lineage, Session binding, grants and execution site. Showing a child under a machine/project never implies parent authority.
- **Terminal/PTY**: process terminal on specific machine with separate viewer and current input/resize owner.
- **Workspace tab**: saved UI layout (binary split tree + pane refs + focus, scroll positions, zoom state). Tabs do not own processes, tools or Sessions.
- **Pane**: view of a typed ref (Agent, PTY, File, Git, Diff, Activity, Machine, Space). A pane can be closed without stopping the process being viewed.
- **Automation**: approved scheduled/trigger definition with owner, budget and target Space/machine; its *runs* reference actual Agents. Definition is not an always-running Actor.
- **Recent destination**: opaque ID+kind+scope for a user-intentional visit (Agent, Workspace layout, file, project, worktree, terminal, etc.), not a task/event log.

## Primary navigation

Header is global scope:
- Space name click -> full Spaces manager.
- adjacent chevron -> list of named Space and Local Spaces.
- machine-indicators click -> full Fleet tab.
- adjacent chevron -> selected machine Local Space. This only changes view; never execution routing.

V1.4 sidebar is two independent reducers:
- Upper navigator: Projects / Recents. Project tree scoped by current Space; Recents flat visited list.
- Lower dock: Activity / Git / Files. Activity is Recents/Now/Automations; Git and Files are projections over **exact currently selected Worktree/Location**. They never create a new scanner or source of authority.
- Main tabs/panes: saved arrangements with semantic refs. A full Git/File/PTY tab can coexist with mini Git/Files in sidebar and stay focused/scroll-position stable when mini selection changes.

Global quick switch (Cmd/Ctrl-P where terminal supports it) searches opaque references across reachable Spaces/Projects/Worktrees/Sessions/Agents/PTY/layouts, with type and machine context. No search result grants remote execution rights.

## Identity and mutation rules

Visual actions:
- select Agent -> inspect/focus transcript; no new model invocation.
- select mini Git file -> open/focus exact diff; never auto stage, revert, commit.
- select mini Files entry -> inspect/open source file; never write without explicit authorized action.
- close pane -> detach/hide view, never kill process.
- machine switch -> view its Local Space, never alter current Agent tools/account/site.
- reconnect -> restore presentation only; do not auto-replay uncertain effectful work.

Material operations require separate authenticated owner controls, explicit current scope and checked writer/lease epochs. UI status is not authority. Disconnected remote machine: last known with stale label, no unsafe PTY input, disabled effectful buttons.

## Recents rules

LRU of intentional navigations only, stable while background Agents stream. Deduplicate using immutable destination ref+scope (not title). The upper Recents panel is full; lower Activity/Recents is top three of same list, not separate data. If upper is full Recents, lower Recents auto-collapses, restoring its previous preference when Projects returns. Per-client unread completion metadata is not the Agent's factual status.

## Activity and attention

- Now: active/important Agent instances with exact owner/subagent tree and read-only hints for background/unattached. Stale/disconnected statuses have age/timestamp.
- Automations: definitions (enabled, paused, next trigger), their current and last runs as references, not duplicated Agent objects.
- Recents: visited destinations, not tasks.
- Attention: number of current actionable approvals/conflicts/error acknowledgements. Clicking count filters/jumps to true owning Agent/item; successful background completion is quiet and may have separate unread marker. Attention rollups do not multiply per ancestor.
- Detailed Activity is available as full pane/searchable listing if dozens of live Agents exist, while sidebar keeps a handful of root groups.

## State/restoration keys

User/client-visible layout choices: selected Space, upper navigator, dock type, dock height/collapse, Activity expanded group, main tab/layout split ratios, focus, selected scope, per-pane scroll, active machine view and unread markers. Stored presentation state may be per Space, user and client according to privacy; runtime Session/Actor/journal/process sources remain independent.

UI restores a layout after restart even if its original OS process is absent; in that case pane becomes stale/disconnected or requires explicit reattachment. A remembered terminal pane MUST NOT imply that previous process still runs.

## Security/maintenance

No new indexing/LSP engine, database migration or second Agent runner is justified by this UX. Use existing FS/Git/PTY services when implementation is approved. Separately enforce exact grant source/epoch/effect and machine location; a displayed parent Agent does not gain privileges to child or reverse merely by tree nesting.

## V1.5 current state override — two modes and typed tab identity

Presentation enum becomes Minimal / Workspace only. Earlier descriptions of an Agents mode remain historical V0–V1.4. A native Agent pane maximized inside Workspace reproduces the Agent coordination presentation. Minimal removes persistent top chrome; the footer carries location/model/context/attention as one status line.

Every Workspace tab references either a saved binary pane-layout tree, an exact FileRef (including site and worktree), or a scoped DiffRef (including staged/unstaged/commit selection). Contained Files/Git viewers keep independent selection and scroll; opening standalone tabs does not mutate the contained tree. A zoom is a tab-local optional PaneId, not another UI mode or separate tab. Standalone File/Diff immersive focus may hide navigation chrome, while restore recovers exact previous state.

Opening a tab from sidebar/Quick Switch/Recents deduplicates by canonical target ref, not basename. A standalone tab remembers its originating tab for predictable close. Closing a tab detaches only its visual reference and never terminates a process, destroys an Agent Session, or writes to a file. Pending Agent models, tool grants and runtime journal ownership remain unchanged by any tab focus/zoom.

All V1.4 Space/Local Home/fleet semantics, upper Projects/Recents icon navigator and lower Activity/Git/Files dock continue unchanged. See [V1.5-UNIFIED-FOCUS-AND-TABS.md](V1.5-UNIFIED-FOCUS-AND-TABS.md).

## V1.5 polished UI reducer / palette integration

Current V1.5 has one modal command-palette state (open, raw prefixed query, selected result). This is a **view of the existing aw command palette**, not a new semantic operation or an independent global quick-switch service. Category parse/rewriter obeys the real aw prefixes > ! @ # $ /, extended with one new & Agents typed category. Space/Fleet management stays in Actions/Windows and the global header. Raw Ctrl+P is not hijacked by a second palette while the native Agent editor or PTY owns keyboard input.

No generic second global context/title line is part of the V1.5 Workspace tree. For a standalone File/Diff tab, title belongs to the selected tab; its only document overlay action is a reachable maximize/restore and optionally subtle new worktree/site/scope. When the tab bar hides in immersive zoom, small floating filename + Restore temporarily own the document label. For contained Files/Git view, filename belongs to the selected leaf pane header; the body does not restate it.

Unifying palette search cannot fuse Sessions and Agents or grant extra tool access. Clicking an Agent result selects its actual AgentRef/Session owner and execution site; File/Diff results open the same typed file/diff references already used by mini docks and full views. Presentation-only actions never enter a model/tool/PTY execution path.
