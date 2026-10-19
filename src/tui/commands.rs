#[cfg(any(feature = "durable", test))]
use unicode_segmentation::UnicodeSegmentation;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommandSurface {
    Agent,
    Workspace,
    Both,
}

impl CommandSurface {
    pub fn available(self, workspace: bool) -> bool {
        match self {
            Self::Agent => !workspace,
            Self::Workspace => workspace,
            Self::Both => true,
        }
    }
}

impl SlashCommand {
    pub fn available(&self, workspace: bool) -> bool {
        self.surface.available(workspace)
            && (self.name != "workspace" || cfg!(feature = "workspace"))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SlashCommand {
    pub name: &'static str,
    pub usage: &'static str,
    pub description: &'static str,
    pub surface: CommandSurface,
    pub supports_inline_args: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TuiTarget { Scrollback, Inline, Fullscreen, Workspace }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TuiRequest { Show, Switch(TuiTarget) }
impl TuiTarget {
    pub fn parse(value:&str)->Option<Self>{match value.trim().to_ascii_lowercase().as_str(){
        "scrollback"=>Some(Self::Scrollback),"inline"=>Some(Self::Inline),"fullscreen"=>Some(Self::Fullscreen),"workspace"=>Some(Self::Workspace),_=>None
    }}
    pub fn agent_mode(self)->Option<aw::config::TuiMode>{match self{
        Self::Scrollback=>Some(aw::config::TuiMode::Scrollback),Self::Inline=>Some(aw::config::TuiMode::Inline),Self::Fullscreen=>Some(aw::config::TuiMode::Fullscreen),Self::Workspace=>None
    }}
}
pub const TUI_USAGE:&str="/tui scrollback|inline|fullscreen|workspace";
pub fn tui_request(line:&str)->Option<Result<TuiRequest,&'static str>>{
    let trimmed=line.trim();
    if trimmed=="/workspace" { return Some(Ok(TuiRequest::Switch(TuiTarget::Workspace))); }
    let mut parts=trimmed.split_whitespace();
    if parts.next()!=Some("/tui") { return None; }
    match (parts.next(),parts.next()) {
        (None,None)=>Some(Ok(TuiRequest::Show)),
        (Some(value),None)=>Some(TuiTarget::parse(value).map(TuiRequest::Switch).ok_or(TUI_USAGE)),
        _=>Some(Err(TUI_USAGE)),
    }
}

/// Presentation order is intentional: frequent commands first, like Codex.
/// Keep parser behavior and popup discoverability sourced from this registry.
pub const SLASH_COMMANDS: &[SlashCommand] = &[
    SlashCommand {
        name: "prompt",
        usage: "/prompt or /prompt:NAME [INPUT]",
        description: "list approved named project prompt templates or run one with explicit input",
        surface: CommandSurface::Agent,
        supports_inline_args: true,
    },
    SlashCommand {
        name: "skill",
        usage: "/skill or /skill:NAME [TASK]",
        description: "list approved project skills or admit one into the next user request",
        surface: CommandSurface::Agent,
        supports_inline_args: true,
    },
    SlashCommand {
        name: "model",
        usage: "/model [MODEL]",
        description: "show or choose the model for the next turn",
        surface: CommandSurface::Both,
        supports_inline_args: true,
    },
    SlashCommand {
        name: "think",
        usage: "/think instant|low|med|high|xhigh|max",
        description: "set reasoning effort for subsequent model calls",
        surface: CommandSurface::Both,
        supports_inline_args: true,
    },
    SlashCommand {
        name: "theme",
        usage: "/theme auto|dark|light",
        description: "set the current UI theme",
        surface: CommandSurface::Both,
        supports_inline_args: true,
    },
    SlashCommand {
        name: "new",
        usage: "/new",
        description: "start a new agent session",
        surface: CommandSurface::Both,
        supports_inline_args: false,
    },
    SlashCommand {
        name: "title",
        usage: "/title TITLE",
        description: "set the current durable agent session title",
        surface: CommandSurface::Both,
        supports_inline_args: true,
    },
    SlashCommand {
        name: "title-generate",
        usage: "/title-generate",
        description: "generate a concise title with a tool-free model call",
        surface: CommandSurface::Both,
        supports_inline_args: false,
    },
    SlashCommand {
        name: "btw",
        usage: "/btw QUESTION",
        description: "ask a tool-free side question without changing Agent history",
        surface: CommandSurface::Both,
        supports_inline_args: true,
    },
    SlashCommand {
        name: "resume",
        usage: "/resume SESSION [BRANCH]",
        description: "resume an existing durable session",
        surface: CommandSurface::Both,
        supports_inline_args: true,
    },
    SlashCommand {
        name: "tui",
        usage: TUI_USAGE,
        description: "switch Agent presentation or open the fullscreen Workspace",
        surface: CommandSurface::Agent,
        supports_inline_args: true,
    },
    SlashCommand {
        name: "workspace",
        usage: "/workspace",
        description: "open the fullscreen Workspace application",
        surface: CommandSurface::Agent,
        supports_inline_args: false,
    },
    SlashCommand {
        name: "minimal",
        usage: "/minimal",
        description: "return from Workspace to Agent using its configured TUI mode",
        surface: CommandSurface::Workspace,
        supports_inline_args: false,
    },
    SlashCommand {
        name: "settings",
        usage: "/settings",
        description: "open interactive UI settings",
        surface: CommandSurface::Both,
        supports_inline_args: false,
    },
    SlashCommand {
        name: "status",
        usage: "/status",
        description: "show the current session/profile/model binding",
        surface: CommandSurface::Both,
        supports_inline_args: false,
    },
    SlashCommand {
        name: "history",
        usage: "/history",
        description: "show the current conversation history",
        surface: CommandSurface::Agent,
        supports_inline_args: false,
    },
    SlashCommand {
        name: "sessions",
        usage: "/sessions",
        description: "list durable sessions",
        surface: CommandSurface::Both,
        supports_inline_args: false,
    },
    SlashCommand {
        name: "session",
        usage: "/session",
        description: "show the current durable session",
        surface: CommandSurface::Agent,
        supports_inline_args: false,
    },
    SlashCommand {
        name: "context",
        usage: "/context",
        description: "inspect current context/provenance",
        surface: CommandSurface::Both,
        supports_inline_args: false,
    },
    SlashCommand {
        name: "retry",
        usage: "/retry",
        description: "explicitly retry a retryable attempt",
        surface: CommandSurface::Agent,
        supports_inline_args: false,
    },
    SlashCommand {
        name: "profile",
        usage: "/profile [NAME]",
        description: "show or choose a configured profile",
        surface: CommandSurface::Agent,
        supports_inline_args: true,
    },
    SlashCommand {
        name: "tree",
        usage: "/tree",
        description: "open/show the durable session tree",
        surface: CommandSurface::Both,
        supports_inline_args: false,
    },
    SlashCommand {
        name: "branches",
        usage: "/branches",
        description: "list branches in the current session",
        surface: CommandSurface::Agent,
        supports_inline_args: false,
    },
    SlashCommand {
        name: "switch",
        usage: "/switch BRANCH",
        description: "switch the current session branch",
        surface: CommandSurface::Both,
        supports_inline_args: true,
    },
    SlashCommand {
        name: "fork",
        usage: "/fork NAME [NODE]",
        description: "fork from a safe durable boundary",
        surface: CommandSurface::Both,
        supports_inline_args: true,
    },
    SlashCommand {
        name: "checkpoint",
        usage: "/checkpoint SUMMARY",
        description: "write an explicit user-authored context checkpoint",
        surface: CommandSurface::Both,
        supports_inline_args: true,
    },
    SlashCommand {
        name: "usage",
        usage: "/usage",
        description: "show provider-reported usage for this session",
        surface: CommandSurface::Agent,
        supports_inline_args: false,
    },
    SlashCommand {
        name: "export",
        usage: "/export PATH",
        description: "export the current session to a new private file",
        surface: CommandSurface::Agent,
        supports_inline_args: true,
    },
    SlashCommand {
        name: "backup",
        usage: "/backup PATH",
        description: "create a private SQLite snapshot",
        surface: CommandSurface::Agent,
        supports_inline_args: true,
    },
    SlashCommand {
        name: "acknowledge",
        usage: "/acknowledge [NOTE]",
        description: "record explicit inspection of uncertain effects/context",
        surface: CommandSurface::Both,
        supports_inline_args: true,
    },
    SlashCommand {
        name: "context-flow",
        usage: "/context-flow JSON",
        description: "run an explicit typed context-flow operation",
        surface: CommandSurface::Agent,
        supports_inline_args: true,
    },
    SlashCommand {
        name: "keymap",
        usage: "/keymap",
        description: "show active keyboard shortcuts",
        surface: CommandSurface::Both,
        supports_inline_args: false,
    },
    SlashCommand {
        name: "help",
        usage: "/help",
        description: "show commands and interaction help",
        surface: CommandSurface::Both,
        supports_inline_args: false,
    },
    SlashCommand {
        name: "quit",
        usage: "/quit",
        description: "exit aw",
        surface: CommandSurface::Both,
        supports_inline_args: false,
    },
];

pub fn slash_aliases(name: &str) -> &'static [&'static str] {
    match name {
        "fork" => &["clone"],
        "resume" => &["open"],
        "btw" => &["side"],
        _ => &[],
    }
}

pub fn canonical_slash_name(name: &str) -> Option<&'static str> {
    SLASH_COMMANDS
        .iter()
        .find(|spec| {
            spec.name == name || slash_aliases(spec.name).contains(&name)
        })
        .map(|spec| spec.name)
}

/// Normalize tool-free model output into one durable Session display title.
///
/// The UX asks for at most 100 characters while durable Session labels are
/// bounded to 160 UTF-8 bytes. Enforce both without splitting a grapheme so
/// standalone Agent and Workspace cannot drift.
#[cfg(any(feature = "durable", test))]
pub fn normalize_generated_title(value: &str) -> aw::Result<String> {
    const MAX_GRAPHEMES: usize = 100;
    const MAX_BYTES: usize = 160;

    let safe = aw::presentation::safe_text(value);
    let raw = safe
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("")
        .trim_matches(|c| matches!(c, '"' | '\'' | '`'))
        .trim();

    let mut title = String::new();
    for grapheme in raw.graphemes(true).take(MAX_GRAPHEMES) {
        if title.len().saturating_add(grapheme.len()) > MAX_BYTES {
            break;
        }
        title.push_str(grapheme);
    }
    let title = title.trim().to_owned();
    if title.is_empty() {
        return Err(aw::Error::Protocol(
            "title generator returned no usable title",
        ));
    }
    Ok(title)
}

#[cfg(feature = "workspace")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WorkspaceCommand {
    pub command: &'static str,
    pub label: &'static str,
    pub description: &'static str,
    pub shortcut: &'static str,
}

#[cfg(feature = "workspace")]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WorkspaceCommandContext {
    pub terminal_focused: bool,
    pub diff_focused: bool,
    pub navigator_focused: bool,
    pub has_recovery: bool,
    pub has_unsent_text: bool,
    pub has_live_terminals: bool,
    pub has_layout_file: bool,
    pub agent_busy: bool,
    pub needs_inspection: bool,
}

#[cfg(feature = "workspace")]
impl WorkspaceCommand {
    fn available(self, context: WorkspaceCommandContext) -> bool {
        match self.command.trim_end() {
            "terminal-stop"
            | "terminal-live"
            | "terminal-scroll 20"
            | "terminal-scroll -20"
            | "terminal-forget" | "terminal-promote" | "terminal-dock" | "terminal-dock-cancel" => context.terminal_focused,
            "stage" | "unstage" | "hunk-next" | "hunk-prev" | "reviewed" | "comment"
            | "send-review" => context.diff_focused,
            "filter" => context.navigator_focused,
            "recover-draft" | "discard-recovery" => context.has_recovery,
            "layout-save" | "layout-load" => context.has_layout_file,
            "quit" => !context.has_unsent_text && !context.has_live_terminals,
            "quit-discard" => context.has_unsent_text && !context.has_live_terminals,
            "quit-terminals" => context.has_live_terminals && !context.has_unsent_text,
            "quit-all" => context.has_unsent_text && context.has_live_terminals,
            _ => true,
        }
    }
}

/// Searchable workspace actions. Order is presentation order, not alphabetical.
#[cfg(feature = "workspace")]
pub const WORKSPACE_COMMANDS: &[WorkspaceCommand] = &[
    WorkspaceCommand {
        command: "open-agent-view",
        label: "Open read-only Agent view",
        description: "open the selected saved branch without changing the current Agent or draft",
        shortcut: "o in Sessions",
    },
    WorkspaceCommand {
        command: "conversation",
        label: "Focus Agent",
        description: "open/focus the current agent composer",
        shortcut: "Esc then i",
    },
    WorkspaceCommand {
        command: "settings",
        label: "Settings",
        description: "configure TUI mode, padding, autocomplete, performance and status line",
        shortcut: "",
    },
    WorkspaceCommand {
        command: "new",
        label: "New Agent",
        description: "start a new agent session without changing provider/tool grants",
        shortcut: "",
    },
    WorkspaceCommand {
        command: "status",
        label: "Session Status",
        description: "show current session, branch, user, profile, model and tool mode",
        shortcut: "",
    },
    WorkspaceCommand {
        command: "model ",
        label: "Set Model…",
        description: "set the model for the next turn",
        shortcut: "",
    },
    WorkspaceCommand {
        command: "switch ",
        label: "Switch Branch…",
        description: "switch the current durable session branch",
        shortcut: "",
    },
    WorkspaceCommand {
        command: "fork ",
        label: "Fork Conversation…",
        description: "fork at the current safe boundary or an exact node",
        shortcut: "",
    },
    WorkspaceCommand {
        command: "checkpoint ",
        label: "Create Context Checkpoint…",
        description: "write an explicit user-authored context checkpoint",
        shortcut: "",
    },
    WorkspaceCommand {
        command: "acknowledge",
        label: "Acknowledge Inspection",
        description: "record explicit inspection of uncertain effects/context",
        shortcut: "",
    },
    WorkspaceCommand {
        command: "files",
        label: "Show Files",
        description: "focus the Files navigator",
        shortcut: "Ctrl+Shift+E / Alt+E → Alt+2",
    },
    WorkspaceCommand {
        command: "files-open ",
        label: "Open Files folder…",
        description: "browse another local folder read only without changing the Agent",
        shortcut: "",
    },
    WorkspaceCommand {
        command: "files-reconnect ",
        label: "Reconnect Files folder…",
        description: "reconnect the selected Files folder without changing the Agent",
        shortcut: "",
    },
    WorkspaceCommand {
        command: "git-open ",
        label: "Open Git repository…",
        description: "browse another repository without changing the Agent or Files folder",
        shortcut: "",
    },
    WorkspaceCommand {
        command: "git-reconnect ",
        label: "Reconnect Git repository…",
        description: "reconnect the selected Git repository without changing the Agent",
        shortcut: "",
    },
    WorkspaceCommand {
        command: "git changes",
        label: "Show Git Changes",
        description: "focus Git → Changes",
        shortcut: "Ctrl+Shift+G / Alt+E → Alt+3",
    },
    WorkspaceCommand {
        command: "sessions",
        label: "Show Sessions",
        description: "focus the Sessions navigator",
        shortcut: "Alt+E → Alt+1",
    },
    WorkspaceCommand {
        command: "terminal",
        label: "Focus Terminal",
        description: "focus an existing terminal or show how to start one",
        shortcut: "Ctrl+`",
    },
    WorkspaceCommand {
        command: "terminal-start",
        label: "New Terminal",
        description: "explicitly launch a trusted local shell",
        shortcut: "",
    },
    WorkspaceCommand {
        command: "terminals",
        label: "List Terminals",
        description: "show retained local terminal resources and status",
        shortcut: "",
    },
    #[cfg(target_os = "linux")]
    WorkspaceCommand {
        command: "terminal-promote",
        label: "Keep original PTY in an independent owner…",
        description: "move this running shell to a local broker; attach from a separate terminal",
        shortcut: "",
    },
    #[cfg(unix)]
    WorkspaceCommand {
        command: "terminal-dock",
        label: "Move PTY control to another terminal…",
        description: "one-use 90s private invitation; original owner stays running; no new shell",
        shortcut: "",
    },
    #[cfg(unix)]
    WorkspaceCommand {
        command: "terminal-dock-cancel",
        label: "Cancel pending PTY invitation",
        description: "revoke an unused native terminal invitation without stopping its shell",
        shortcut: "",
    },
    WorkspaceCommand {
        command: "terminal-forget",
        label: "Forget Focused Exited Terminal",
        description: "remove the focused exited terminal resource; never terminates a live process",
        shortcut: "",
    },
    WorkspaceCommand {
        command: "cancel-agent",
        label: "Cancel Agent",
        description: "request cancellation of current agent work; effects may remain",
        shortcut: "",
    },
    WorkspaceCommand {
        command: "tree",
        label: "Session Tree",
        description: "open the durable conversation tree",
        shortcut: "",
    },
    WorkspaceCommand {
        command: "activity",
        label: "Activity",
        description: "open current agent activity",
        shortcut: "",
    },
    WorkspaceCommand {
        command: "context",
        label: "Context",
        description: "open the explicit context/provenance view",
        shortcut: "",
    },
    WorkspaceCommand {
        command: "split right",
        label: "Split Editor Right",
        description: "split the active content group to the right",
        shortcut: "Ctrl+\\",
    },
    WorkspaceCommand {
        command: "split below",
        label: "Split Editor Down",
        description: "split the active content group below",
        shortcut: "",
    },
    WorkspaceCommand {
        command: "close-tab",
        label: "Close View",
        description: "close the current non-terminal view",
        shortcut: "Ctrl+W",
    },
    WorkspaceCommand {
        command: "maximize",
        label: "Toggle Maximize",
        description: "maximize/restore the focused pane",
        shortcut: "Ctrl+K Z",
    },
    WorkspaceCommand {
        command: "explorer-left",
        label: "Move Explorer Left",
        description: "place the Explorer column on the left",
        shortcut: "",
    },
    WorkspaceCommand {
        command: "explorer-right",
        label: "Move Explorer Right",
        description: "place the Explorer column on the right",
        shortcut: "",
    },
    WorkspaceCommand {
        command: "explorer-hide",
        label: "Hide Explorer",
        description: "show only the main content area while preserving Explorer state",
        shortcut: "",
    },
    WorkspaceCommand {
        command: "explorer-show",
        label: "Show Explorer",
        description: "restore the Explorer column",
        shortcut: "Alt+E",
    },
    WorkspaceCommand {
        command: "find",
        label: "Find in Focused View",
        description: "incrementally find text and jump among matches",
        shortcut: "/",
    },
    WorkspaceCommand {
        command: "pin",
        label: "Toggle Pin",
        description: "pin/unpin the current preview tab",
        shortcut: "",
    },
    WorkspaceCommand {
        command: "git worktrees",
        label: "Git Worktrees",
        description: "show known Git worktrees",
        shortcut: "",
    },
    WorkspaceCommand {
        command: "git branches",
        label: "Git Branches",
        description: "show Git branches",
        shortcut: "",
    },
    WorkspaceCommand {
        command: "git list",
        label: "Git List View",
        description: "show Git changes as a list",
        shortcut: "",
    },
    WorkspaceCommand {
        command: "git tree",
        label: "Git Tree View",
        description: "show Git changes as a tree",
        shortcut: "",
    },
    WorkspaceCommand {
        command: "stage",
        label: "Stage Current Change",
        description: "stage the exact focused changed-file snapshot",
        shortcut: "",
    },
    WorkspaceCommand {
        command: "unstage",
        label: "Unstage Current Change",
        description: "unstage the exact focused changed-file snapshot",
        shortcut: "",
    },
    WorkspaceCommand {
        command: "hunk-next",
        label: "Next Diff Hunk",
        description: "jump to the next hunk in the focused diff",
        shortcut: "",
    },
    WorkspaceCommand {
        command: "hunk-prev",
        label: "Previous Diff Hunk",
        description: "jump to the previous hunk in the focused diff",
        shortcut: "",
    },
    WorkspaceCommand {
        command: "reviewed",
        label: "Toggle Reviewed",
        description: "mark/unmark the current review snapshot as reviewed",
        shortcut: "",
    },
    WorkspaceCommand {
        command: "comment ",
        label: "Add Diff Comment…",
        description: "add a comment to the currently focused exact diff row",
        shortcut: "",
    },
    WorkspaceCommand {
        command: "send-review",
        label: "Send Review to Composer",
        description: "copy exact review feedback into the unsent conversation draft",
        shortcut: "",
    },
    WorkspaceCommand {
        command: "filter ",
        label: "Filter Current Navigator…",
        description: "set the focused Files/Git/Sessions filter",
        shortcut: "",
    },
    WorkspaceCommand {
        command: "file ",
        label: "Open File…",
        description: "open a rooted read-only file preview",
        shortcut: "",
    },
    WorkspaceCommand {
        command: "terminal-stop",
        label: "Stop Terminal",
        description: "terminate the focused terminal resource explicitly",
        shortcut: "",
    },
    WorkspaceCommand {
        command: "terminal-live",
        label: "Terminal Follow Live",
        description: "return the focused terminal view to live output",
        shortcut: "",
    },
    WorkspaceCommand {
        command: "terminal-scroll 20",
        label: "Terminal Scroll Back",
        description: "show older rows from aw's bounded terminal scrollback",
        shortcut: "",
    },
    WorkspaceCommand {
        command: "terminal-scroll -20",
        label: "Terminal Scroll Forward",
        description: "move back toward live terminal output",
        shortcut: "",
    },
    WorkspaceCommand {
        command: "close-pane",
        label: "Close Pane",
        description: "close the focused pane when layout invariants allow it",
        shortcut: "",
    },
    WorkspaceCommand {
        command: "recover-draft",
        label: "Recover Rejected Draft",
        description: "restore a preserved preflight-rejected input explicitly",
        shortcut: "",
    },
    WorkspaceCommand {
        command: "discard-recovery",
        label: "Discard Rejected Draft",
        description: "discard only the preserved rejected-input recovery buffer",
        shortcut: "",
    },
    WorkspaceCommand {
        command: "refresh",
        label: "Refresh",
        description: "refresh the focused view without running the model",
        shortcut: "Ctrl+R",
    },
    WorkspaceCommand {
        command: "layout-save",
        label: "Save Layout",
        description: "save only private view descriptors",
        shortcut: "",
    },
    WorkspaceCommand {
        command: "layout-load",
        label: "Load Layout",
        description: "restore saved view descriptors",
        shortcut: "",
    },
    WorkspaceCommand {
        command: "minimal",
        label: "Return to Agent",
        description: "leave fullscreen Workspace and preserve the configured Agent TUI mode",
        shortcut: "",
    },
    WorkspaceCommand {
        command: "help",
        label: "Keyboard Shortcuts",
        description: "show active workspace keymap; also available as /keymap",
        shortcut: "?",
    },
    WorkspaceCommand {
        command: "quit",
        label: "Quit",
        description: "quit only when drafts/recovery and terminal safety checks pass",
        shortcut: "",
    },
    WorkspaceCommand {
        command: "quit-discard",
        label: "Quit and Discard Draft",
        description: "explicitly discard unsent/recovery text; live terminals still block",
        shortcut: "",
    },
    WorkspaceCommand {
        command: "quit-terminals",
        label: "Quit and Terminate Terminals",
        description: "explicitly confirm terminal termination before application exit",
        shortcut: "",
    },
    WorkspaceCommand {
        command: "quit-all",
        label: "Quit, Discard Draft, and Terminate Terminals",
        description: "explicitly confirm both unsent-text discard and terminal termination",
        shortcut: "",
    },
];

pub fn cycle_selection(selection: usize, len: usize, forward: bool) -> usize {
    if len == 0 {
        return 0;
    }
    let selection = selection.min(len - 1);
    if forward {
        (selection + 1) % len
    } else if selection == 0 {
        len - 1
    } else {
        selection - 1
    }
}

fn fuzzy_score(candidate: &str, query: &str) -> Option<usize> {
    if query.is_empty() {
        return Some(0);
    }
    let candidate = candidate.to_ascii_lowercase();
    let query = query.to_ascii_lowercase();
    if candidate.starts_with(&query) {
        return Some(0);
    }
    if let Some(pos) = candidate.find(&query) {
        return Some(10 + pos);
    }
    let mut at = 0usize;
    let mut gaps = 0usize;
    for q in query.chars() {
        let rest = candidate.get(at..)?;
        let pos = rest.find(q)?;
        gaps = gaps.saturating_add(pos);
        at = at.saturating_add(pos + q.len_utf8());
    }
    Some(100 + gaps)
}

pub fn slash_query(text: &str) -> Option<&str> {
    if text.contains('\n') || !text.starts_with('/') || text.starts_with("//") {
        return None;
    }
    let body = &text[1..];
    Some(body.split_whitespace().next().unwrap_or(""))
}

pub fn exact_slash_command(text: &str, workspace: bool) -> Option<&'static SlashCommand> {
    let name = slash_query(text)?;
    SLASH_COMMANDS.iter().find(|spec| {
        spec.available(workspace)
            && (spec.name == name || slash_aliases(spec.name).contains(&name))
    })
}

pub fn slash_matches(text: &str, workspace: bool) -> Vec<&'static SlashCommand> {
    let Some(query) = slash_query(text) else {
        return Vec::new();
    };
    let mut matches = SLASH_COMMANDS
        .iter()
        .filter(|spec| spec.available(workspace))
        .filter_map(|spec| {
            let alias_score = slash_aliases(spec.name)
                .iter()
                .filter_map(|alias| fuzzy_score(alias, query))
                .min();
            fuzzy_score(spec.name, query)
                .into_iter()
                .chain(alias_score)
                .chain(fuzzy_score(spec.description, query).map(|score| score.saturating_add(20)))
                .min()
                .map(|score| (score, spec))
        })
        .collect::<Vec<_>>();
    matches.sort_by_key(|(score, _)| *score);
    matches.into_iter().map(|(_, spec)| spec).collect()
}

#[cfg(feature = "workspace")]
fn workspace_action_has_slash_equivalent(action: &WorkspaceCommand) -> bool {
    let verb = action
        .command
        .split_whitespace()
        .next()
        .unwrap_or("");
    canonical_slash_name(verb).is_some_and(|canonical| {
        SLASH_COMMANDS
            .iter()
            .any(|spec| spec.name == canonical && spec.available(true))
    })
}

#[cfg(feature = "workspace")]
pub fn workspace_matches(
    query: &str,
    context: WorkspaceCommandContext,
) -> Vec<&'static WorkspaceCommand> {
    let query = query.trim_start();
    let query_lower = query.to_ascii_lowercase();
    if let Some(command) = WORKSPACE_COMMANDS.iter().find(|command| {
        command.available(context)
            && !workspace_action_has_slash_equivalent(command)
            && command.command.ends_with(' ')
            && query_lower.starts_with(&command.command.to_ascii_lowercase())
    }) {
        return vec![command];
    }
    let mut matches = WORKSPACE_COMMANDS
        .iter()
        .filter(|spec| (**spec).available(context))
        .filter(|spec| !workspace_action_has_slash_equivalent(spec))
        .filter_map(|spec| {
            let score = [
                fuzzy_score(spec.command, query),
                fuzzy_score(spec.label, query).map(|score| score.saturating_add(10)),
                fuzzy_score(spec.description, query).map(|score| score.saturating_add(20)),
            ]
            .into_iter()
            .flatten()
            .min();
            score.map(|score| (score, spec))
        })
        .collect::<Vec<_>>();
    matches.sort_by_key(|(score, _)| *score);
    matches.into_iter().map(|(_, spec)| spec).collect()
}

#[cfg(feature = "durable")]
pub const DEFAULT_ACKNOWLEDGEMENT: &str =
    "User explicitly confirms inspection of uncertain effects/imported context";

#[cfg(feature = "durable")]
pub fn parse_resume_args(rest: &str) -> Option<(&str, &str)> {
    let mut parts = rest.split_whitespace();
    let session = parts.next()?;
    let branch = parts.next().unwrap_or("main");
    parts.next().is_none().then_some((session, branch))
}

#[cfg(feature = "durable")]
pub fn parse_fork_args(rest: &str) -> Option<(&str, Option<&str>)> {
    let mut parts = rest.split_whitespace();
    let name = parts.next()?;
    let at = parts.next();
    parts.next().is_none().then_some((name, at))
}

pub fn complete_slash(text: &str, spec: &SlashCommand) -> String {
    let prefix = format!("/{}", spec.name);
    let rest = text
        .find(char::is_whitespace)
        .map(|at| &text[at..])
        .unwrap_or("");
    if !rest.is_empty() {
        format!("{prefix}{rest}")
    } else if spec.supports_inline_args {
        format!("{prefix} ")
    } else {
        prefix
    }
}

pub fn slash_help(workspace: bool) -> String {
    let mut out = String::from("Commands\n\n");
    for spec in SLASH_COMMANDS
        .iter()
        .filter(|spec| spec.available(workspace))
    {
        use std::fmt::Write;
        let aliases = slash_aliases(spec.name);
        let alias_text = if aliases.is_empty() {
            String::new()
        } else {
            format!(
                " · aliases {}",
                aliases
                    .iter()
                    .map(|alias| format!("/{alias}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        };
        let _ = writeln!(
            out,
            "  {:<30} {}{}",
            spec.usage,
            spec.description,
            alias_text
        );
    }
    out.push_str("\nType / in the composer to search and complete commands.\n");
    out
}

pub const INLINE_KEYMAP_POPUP: &str = "Enter send · Shift/Alt+Enter or Ctrl+J newline
/ slash · Tab complete · ↑/↓ or Ctrl+P/N history
Ctrl/Alt+←/→ words · Ctrl+W/Alt+Backspace delete word
Ctrl+U/K kill · Ctrl+Y yank · Alt+Y rotate · Ctrl+_ undo
F1 / Ctrl+Shift+P palette · Ctrl+C clear/again quit
F6 thinking · F7 tool details · Esc close
";

pub const INLINE_KEYMAP: &str = "\
Enter        send prompt
Shift+Enter  newline
Alt+Enter    newline
Ctrl+J       newline
/            slash commands
Tab          complete selected slash command
↑ / ↓        slash selection; multiline cursor; otherwise history
Ctrl+P/N     previous/next prompt history
Ctrl+Shift+P open unified command palette, preserving the draft
o (Sessions) open selected branch as a read-only Agent view; Enter activates
F1           command palette; /keymap prints the full keymap
F6/F7        toggle thinking activity / tool details during live work
Ctrl+A/E     line start/end · Ctrl+B/F left/right
Ctrl/Alt+←/→ move by word · Alt+B/F move by word
Ctrl+W / Alt+Backspace  delete previous word
Alt+D / Alt+Delete      delete next word
Ctrl+U/K     delete to line start/end (kill ring)
Ctrl+Y       yank most recent kill · Alt+Y rotate kill ring
Ctrl+_ / Ctrl+-  undo last bounded editor mutation
Ctrl+D       delete next char; quit when input is empty
Ctrl+C       clear/arm quit; press again within 1s to quit
Left/Right   move cursor
Home/End     line start/end
";

#[cfg(feature = "workspace")]
pub const WORKSPACE_KEYMAP: &str = "\
Workspace
  Ctrl+Shift+P / F1   command palette
  :                   command palette (navigation mode)
  Ctrl+Shift+E         Files (outside terminal focus)
  Ctrl+Shift+G         Source Control (outside terminal focus)
  Alt+E               focus Explorer; Alt+W focus main content
  Alt+1..9            choose Explorer view or main tab by position
  Ctrl+`             focus existing terminal
  Ctrl+Tab            next tab
  Ctrl+Shift+Tab      previous tab
  Ctrl+W              close current non-terminal view
  Ctrl+\\\\             split active editor right
  Ctrl+R              refresh focused view
  Esc                 leave composer / close palette
  i                   edit focused Agent
  ?                   shortcuts/help (navigation mode)

Navigation surfaces
  ↑ ↓ ← →             navigate
  h j k l             Helix/Vim-style navigation
  Enter               open/select or expand/collapse
  t                   toggle Tree / List where supported
  mouse               click/focus/open; drag split borders to resize
  PageUp/PageDown     page content
  Home/End            top/bottom

Conversation composer
  Enter                       send
  Shift/Alt+Enter / Ctrl+J    newline
  Ctrl+A/E/B/F/U/K/W/D/Y      familiar readline editing
  Ctrl+P/N                    previous/next prompt history
  Ctrl/Alt+←/→, Alt+B/F       move by word
  Alt+Backspace / Alt+D       delete word backward/forward
  Alt+Y                       rotate kill ring · Ctrl+_/Ctrl+- undo
  /                   slash command autocomplete
  Tab                 complete selected slash command
  Esc                 return to navigation mode
  Ctrl+C              cancel active work; idle clear/again quit
  F6 / F7             toggle thinking activity / tool details

Terminal
  Ordinary keys go to the child.
  Ctrl+Shift+P / F1   command palette
  Ctrl+`       return to the previous workspace pane
";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_registries_have_unique_canonical_names() {
        let mut slash = std::collections::BTreeSet::new();
        for spec in SLASH_COMMANDS {
            assert!(
                slash.insert(spec.name),
                "duplicate slash command {}",
                spec.name
            );
        }
        #[cfg(feature = "workspace")]
        {
            let mut workspace = std::collections::BTreeSet::new();
            for spec in WORKSPACE_COMMANDS {
                assert!(
                    workspace.insert(spec.command),
                    "duplicate workspace command {}",
                    spec.command
                );
            }
        }
    }

    #[test]
    fn slash_registry_is_presentation_ordered_and_matches() {
        let matches = slash_matches("/mo", false);
        assert_eq!(matches.first().map(|m| m.name), Some("model"));
        assert_eq!(slash_matches("/tu",false).first().map(|m|m.name),Some("tui"));
        assert_eq!(slash_matches("/work",false).iter().any(|m|m.name=="workspace"),cfg!(feature="workspace"));
    }

    #[cfg(feature = "workspace")]
    #[test]
    fn exact_slash_lookup_does_not_leak_workspace_palette_commands() {
        assert_eq!(
            exact_slash_command("/model gpt", false).map(|spec| spec.name),
            Some("model")
        );
        assert!(exact_slash_command("/split right", true).is_none());
    }

    #[test]
    fn aliases_resolve_to_one_canonical_slash_command() {
        for (alias, canonical) in [
            ("clone", "fork"),
            ("open", "resume"),
            ("side", "btw"),
        ] {
            assert_eq!(canonical_slash_name(alias), Some(canonical));
            assert_eq!(
                exact_slash_command(&format!("/{alias}"), cfg!(feature = "workspace"))
                    .map(|spec| spec.name),
                Some(canonical)
            );
        }
        for name in ["think", "theme", "settings", "title-generate", "btw", "resume", "tui"] {
            assert_eq!(canonical_slash_name(name), Some(name));
        }
    }

    #[test]
    fn tui_modes_are_values_not_parallel_slash_commands() {
        assert_eq!(TuiTarget::parse("scrollback"),Some(TuiTarget::Scrollback));
        assert_eq!(TuiTarget::parse("inline"),Some(TuiTarget::Inline));
        assert_eq!(TuiTarget::parse("fullscreen"),Some(TuiTarget::Fullscreen));
        assert_eq!(TuiTarget::parse("workspace"),Some(TuiTarget::Workspace));
        for old in ["scrollback","inline","fullscreen","agent"] { assert!(SLASH_COMMANDS.iter().all(|spec|spec.name!=old)); }
        assert!(exact_slash_command("/tui inline",false).is_some());
        assert_eq!(tui_request("/tui"),Some(Ok(TuiRequest::Show)));
        assert_eq!(tui_request("/tui workspace"),Some(Ok(TuiRequest::Switch(TuiTarget::Workspace))));
        assert_eq!(tui_request("/workspace"),Some(Ok(TuiRequest::Switch(TuiTarget::Workspace))));
        assert_eq!(tui_request("/tui nope"),Some(Err(TUI_USAGE)));
        assert!(exact_slash_command("/minimal",false).is_none());
        #[cfg(feature="workspace")] {
            assert!(exact_slash_command("/minimal",true).is_some());
            assert!(exact_slash_command("/tui inline",true).is_none());
        }
    }

    #[test]
    fn generated_title_normalization_obeys_grapheme_and_store_byte_bounds() {
        assert_eq!(
            normalize_generated_title("\n  \"Concise title\"  \nignored").unwrap(),
            "Concise title"
        );

        let multilingual = format!("\"{}\"", "界".repeat(100));
        let title = normalize_generated_title(&multilingual).unwrap();
        assert!(title.len() <= 160);
        assert!(title.graphemes(true).count() <= 100);
        assert!(!title.is_empty());
    }

    #[test]
    fn command_registry_contains_requested_argument_contracts() {
        let usage = |name| {
            SLASH_COMMANDS
                .iter()
                .find(|spec| spec.name == name)
                .map(|spec| spec.usage)
                .unwrap()
        };
        assert_eq!(usage("think"), "/think instant|low|med|high|xhigh|max");
        assert_eq!(usage("theme"), "/theme auto|dark|light");
        assert_eq!(usage("title-generate"), "/title-generate");
        assert_eq!(usage("btw"), "/btw QUESTION");
        assert_eq!(usage("resume"), "/resume SESSION [BRANCH]");
    }

    #[test]
    fn cycle_selection_wraps_at_both_ends() {
        assert_eq!(cycle_selection(0, 5, false), 4);
        assert_eq!(cycle_selection(4, 5, true), 0);
        assert_eq!(cycle_selection(2, 5, false), 1);
        assert_eq!(cycle_selection(2, 5, true), 3);
        assert_eq!(cycle_selection(99, 5, true), 0);
        assert_eq!(cycle_selection(0, 0, true), 0);
    }

    #[test]
    fn slash_only_activates_at_start() {
        assert_eq!(slash_query("/model gpt"), Some("model"));
        assert_eq!(slash_query("//literal"), None);
        assert_eq!(slash_query("say /model"), None);
        assert_eq!(slash_query("/model\nmore"), None);
    }

    #[cfg(feature = "durable")]
    #[test]
    fn durable_multi_argument_parsers_are_exact() {
        assert_eq!(parse_resume_args("sess"), Some(("sess", "main")));
        assert_eq!(parse_resume_args("sess branch"), Some(("sess", "branch")));
        assert_eq!(parse_resume_args("sess branch extra"), None);
        assert_eq!(parse_fork_args("next"), Some(("next", None)));
        assert_eq!(
            parse_fork_args("next node_1"),
            Some(("next", Some("node_1")))
        );
        assert_eq!(parse_fork_args("next node_1 extra"), None);
    }

    #[test]
    fn completion_preserves_inline_arguments() {
        let model = SLASH_COMMANDS
            .iter()
            .find(|spec| spec.name == "model")
            .unwrap();
        assert_eq!(complete_slash("/mo", model), "/model ");
        assert_eq!(complete_slash("/model gpt-5", model), "/model gpt-5");
    }

    #[test]
    fn workspace_command_is_hidden_when_workspace_feature_is_absent() {
        let workspace = SLASH_COMMANDS
            .iter()
            .find(|spec| spec.name == "workspace")
            .unwrap();
        assert_eq!(workspace.available(false), cfg!(feature = "workspace"));
    }

    #[cfg(feature = "workspace")]
    #[test]
    fn workspace_palette_exposes_user_intentions_not_internal_ids() {
        for command in [
            "new",
            "status",
            "model ",
            "files",
            "git changes",
            "sessions",
            "terminal-start",
            "cancel-agent",
            "tree",
            "context",
            "split right",
            "quit",
        ] {
            assert!(
                WORKSPACE_COMMANDS
                    .iter()
                    .any(|spec| spec.command == command),
                "missing palette action {command}"
            );
        }
        for internal in [
            "context-action ",
            "terminal-open ",
            "resize ",
            "add-nav ",
            "move-pane ",
            "move-tab ",
            "focus ",
        ] {
            assert!(
                WORKSPACE_COMMANDS
                    .iter()
                    .all(|spec| spec.command != internal),
                "internal grammar leaked into palette: {internal}"
            );
        }
    }

    #[cfg(feature = "workspace")]
    #[test]
    fn workspace_actions_exclude_canonical_slash_commands() {
        let actions = workspace_matches("", WorkspaceCommandContext::default());
        for slash in SLASH_COMMANDS.iter().filter(|spec| spec.available(true)) {
            assert!(
                actions.iter().all(|action| {
                    action
                        .command
                        .split_whitespace()
                        .next()
                        .is_none_or(|verb| canonical_slash_name(verb) != Some(slash.name))
                }),
                "{} must appear under / Commands, not > Actions",
                slash.name
            );
        }
        assert!(
            actions.iter().any(|action| action.command == "conversation"),
            "direct Workspace actions remain discoverable"
        );
    }

    #[cfg(feature = "workspace")]
    #[test]
    fn workspace_palette_is_context_sensitive() {
        let default = WorkspaceCommandContext::default();
        assert!(workspace_matches("stage", default)
            .iter()
            .all(|command| command.command != "stage"));
        let diff = WorkspaceCommandContext {
            diff_focused: true,
            ..default
        };
        assert_eq!(
            workspace_matches("stage", diff)
                .first()
                .map(|command| command.command),
            Some("stage")
        );
        assert!(workspace_matches("recover rejected", default)
            .iter()
            .all(|command| command.command != "recover-draft"));
        let recovery = WorkspaceCommandContext {
            has_recovery: true,
            ..default
        };
        assert_eq!(
            workspace_matches("recover rejected", recovery)
                .first()
                .map(|command| command.command),
            Some("recover-draft")
        );

        assert!(
            workspace_matches("quit", default)
                .iter()
                .all(|command| command.command != "quit"),
            "canonical /quit belongs only to the slash-command category"
        );
        let unsent = WorkspaceCommandContext {
            has_unsent_text: true,
            ..default
        };
        assert_eq!(
            workspace_matches("quit", unsent)
                .first()
                .map(|command| command.command),
            Some("quit-discard")
        );
        let terminals = WorkspaceCommandContext {
            has_live_terminals: true,
            ..default
        };
        assert_eq!(
            workspace_matches("quit", terminals)
                .first()
                .map(|command| command.command),
            Some("quit-terminals")
        );
        let both = WorkspaceCommandContext {
            has_unsent_text: true,
            has_live_terminals: true,
            ..default
        };
        assert_eq!(
            workspace_matches("quit", both)
                .first()
                .map(|command| command.command),
            Some("quit-all")
        );
    }

    #[cfg(feature = "workspace")]
    #[test]
    fn workspace_search_is_descriptive() {
        assert_eq!(
            workspace_matches("terminal", WorkspaceCommandContext::default())
                .first()
                .map(|m| m.command),
            Some("terminal")
        );
        assert_eq!(
            workspace_matches("split right", WorkspaceCommandContext::default())
                .first()
                .map(|m| m.command),
            Some("split right")
        );
        assert_eq!(
            workspace_matches("file src/main.rs", WorkspaceCommandContext::default())
                .first()
                .map(|m| m.command),
            Some("file ")
        );
    }
}
