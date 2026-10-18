//! Workspace command execution for the existing `Ui` state.
//! Resolved actions reuse the existing typed owner, layout, Git, file and terminal services.
use super::*;

impl Ui {
    pub(super) fn send_draft(&mut self, worker: &Worker, browser: &Browser) {
        if !self.can_submit {
            self.notice =
                "Resize/focus the visible Agent composer before Send; draft preserved".into();
            return;
        }
        let text = self.draft.text().to_owned();
        if text.trim().is_empty() {
            return;
        }
        if text.starts_with('/') && !text.starts_with("//") && !text.contains('\n') {
            if exact_slash_command(&text, true).is_none() {
                self.notice =
                    "Unknown slash command. Type / to search agent/session commands; use F1 for workspace actions."
                        .into();
                return;
            }
            self.draft.clear();
            self.composer.reset_after_external_edit();
            if !self.command(text.trim_start_matches('/'), worker, browser) {
                let _ = self.draft.set(text);
                self.composer.reset_after_external_edit();
            }
            return;
        }
        let text = if let Some(t) = text.strip_prefix("//") {
            format!("/{t}")
        } else {
            text
        };
        match worker.send(Command::Prompt(text)) {
            Ok(()) => {
                self.draft.clear();
                self.composer.reset_after_external_edit();
                self.snapshot = worker.projection.snapshot();
                self.notice.clear();
            }
            Err(e) => self.notice = e.to_string(),
        }
    }
    pub(super) fn return_to_agent(&mut self) -> bool {
        if !self.recovery.is_empty() {
            self.notice =
                "Recover or discard the rejected-input buffer before leaving; current draft preserved"
                    .into();
            return false;
        }
        self.exit = Some(Exit::Agent);
        self.notice = "Returning to the Agent application after current work settles; its configured TUI mode is preserved."
            .into();
        true
    }

    pub(super) fn command(&mut self, line: &str, worker: &Worker, browser: &Browser) -> bool {
        let (raw_verb, rest) = line.trim().split_once(' ').unwrap_or((line.trim(), ""));
        let verb = crate::tui::commands::canonical_slash_name(raw_verb).unwrap_or(raw_verb);
        let rest = rest.trim();
        if let Some(accepted) = self.extra_command(verb, rest, worker) {
            return accepted;
        }
        let mut accepted = true;
        let pane = self.layout.focused;
        let side = |s: &str| match s {
            "left" => Some(Side::Left),
            "right" => Some(Side::Right),
            "above" => Some(Side::Above),
            "below" => Some(Side::Below),
            _ => None,
        };
        match verb {
            "settings" => {
                self.open_settings();
                self.notice.clear();
            }
            "theme" => {
                if rest.is_empty() {
                    self.notice = format!(
                        "Theme: {}",
                        match self.ui_config.theme {
                            aw::config::ThemeMode::System => "auto",
                            aw::config::ThemeMode::Dark => "dark",
                            aw::config::ThemeMode::Light => "light",
                        }
                    );
                } else {
                    let theme = match rest {
                        "auto" | "system" => Some(aw::config::ThemeMode::System),
                        "dark" => Some(aw::config::ThemeMode::Dark),
                        "light" => Some(aw::config::ThemeMode::Light),
                        _ => None,
                    };
                    if let Some(theme) = theme {
                        if self.ui_config.theme != theme {
                            self.ui_config.theme = theme;
                            self.refresh_file_highlights_for_theme();
                            if let Err(error) = worker.persist_ui(self.ui_config.clone()) {
                                self.notice = error.to_string();
                            }
                        }
                        self.notice = format!("Theme: {rest}");
                    } else {
                        self.notice = "/theme auto|dark|light".into();
                        accepted = false;
                    }
                }
            }
            "think" => {
                if rest.is_empty() {
                    self.notice = if self.snapshot.effort.is_empty() {
                        "Thinking effort: provider default".into()
                    } else {
                        format!("Thinking effort: {}", self.snapshot.effort)
                    };
                } else if let Some(effort) = aw::config::ReasoningEffort::parse(rest) {
                    accepted = self.owner(worker, Command::Think(effort), false);
                } else {
                    self.notice = "/think instant|low|med|high|xhigh|max".into();
                    accepted = false;
                }
            }
            "title-generate" => {
                if !rest.is_empty() {
                    self.notice = "/title-generate takes no arguments".into();
                    accepted = false;
                } else {
                    accepted = self.owner(worker, Command::GenerateTitle, false);
                }
            }
            "btw" => {
                if rest.is_empty() {
                    self.notice = "/btw QUESTION".into();
                    accepted = false;
                } else {
                    accepted = self.owner(worker, Command::Side(rest.into()), false);
                }
            }
            "status" => {
                self.notice = format!(
                    "{} · branch:{} · user:{} · profile:{} · model:{} · tools:{}",
                    self.snapshot.session.as_deref().unwrap_or("memory"),
                    self.snapshot.branch,
                    self.snapshot.actor,
                    self.snapshot.profile,
                    self.snapshot.model,
                    self.snapshot.tools
                );
            }
            "title" => {
                if rest.is_empty() {
                    self.notice = self
                        .snapshot
                        .session_title
                        .as_deref()
                        .map(|title| format!("Agent: {title}"))
                        .unwrap_or_else(|| "Current Agent is not durable yet".into());
                } else {
                    accepted = self.owner(worker, Command::Title(rest.into()), false);
                }
            }
            "open-agent-view" => self.open_selected_agent_view(),
            "keymap" => self.open(Target::Help),
            "context" => self.open(Target::ContextFlow),
            "context-action" => match serde_json::from_str::<aw::context_flow::Command>(rest) {
                Ok(command) => {
                    let moving = command.mutates_view();
                    self.open(Target::ContextFlow);
                    self.owner(worker, Command::Context(command), moving);
                }
                Err(_) => {
                    self.notice =
                        "context-action requires the strict JSON command; no actor/key/endpoint fields"
                            .into();
                    accepted = false;
                }
            },
            "help" => self.open(Target::Help),
            "conversation" => self.open(Target::Conversation),
            "activity" => self.open(Target::Activity),
            "tree" => {
                self.open(Target::SessionTree);
                self.owner(worker, Command::Tree, false);
            }
            "files" => self.set_navigator(Navigator::Files),
            "sessions" => {
                self.set_navigator(Navigator::Sessions);
                self.owner(worker, Command::Refresh, false);
            }
            "git-open" | "git-reconnect" => {
                accepted = self.authorize_git(rest, verb == "git-reconnect");
            }
            "git" => {
                self.set_navigator(Navigator::Git);
                let p = self.layout.focused;
                let result = match rest {
                    "changes" => self.git.set_mode(p, git::Mode::Changes),
                    "worktrees" => self.git.set_mode(p, git::Mode::Worktrees),
                    "branches" => self.git.set_mode(p, git::Mode::Branches),
                    "list" | "tree" => {
                        let nav = self.git.panes.entry(p).or_default();
                        if nav.mode != git::Mode::Changes {
                            Err("Git List/Tree applies only to Changes".into())
                        } else {
                            nav.set_display(if rest == "list" {
                                git::Display::List
                            } else {
                                git::Display::Tree
                            });
                            Ok(())
                        }
                    }
                    "" => Ok(()),
                    _ => Err("git changes|worktrees|branches|list|tree".into()),
                };
                if let Err(e) = result {
                    self.notice = e;
                }
            }
            "split" => {
                if let Some(side) = side(rest) {
                    self.apply(Action::SplitGroup {
                        pane: self.group(),
                        side,
                    });
                    self.input = false;
                } else {
                    self.notice = "split left|right|above|below".into();
                    accepted = false;
                }
            }
            "add-nav" => {
                let mut parts = rest.split_whitespace();
                let kind = match parts.next() {
                    Some("files") => Some(Navigator::Files),
                    Some("git") => Some(Navigator::Git),
                    Some("sessions") => Some(Navigator::Sessions),
                    _ => None,
                };
                if let (Some(kind), Some(side)) = (kind, side(parts.next().unwrap_or("left"))) {
                    self.apply(Action::AddNavigator {
                        beside: pane,
                        side,
                        kind,
                    });
                    self.input = false;
                } else {
                    self.notice = "add-nav files|git|sessions left|right|above|below".into();
                    accepted = false;
                }
            }
            "focus" => {
                if let Ok(id) = rest.parse() {
                    self.apply(Action::Focus(id));
                    self.input = false;
                } else {
                    self.notice = "focus PANE_ID".into();
                    accepted = false;
                }
            }
            "move-pane" => {
                let p = rest.split_whitespace().collect::<Vec<_>>();
                if p.len() == 4 && p[1] == "beside" {
                    if let (Ok(id), Ok(beside), Some(side)) =
                        (p[0].parse(), p[2].parse(), side(p[3]))
                    {
                        self.apply(Action::MovePane {
                            pane: id,
                            beside,
                            side,
                        });
                    } else {
                        self.notice = "Invalid pane IDs/side".into();
                        accepted = false;
                    }
                } else {
                    self.notice = "move-pane ID beside ID left|right|above|below".into();
                    accepted = false;
                }
            }
            "move-tab" => {
                if let (Some(tab), Ok(group)) =
                    (self.layout.active(pane).map(|t| t.id), rest.parse())
                {
                    self.apply(Action::MoveTab { tab, group });
                } else {
                    self.notice = "Focus a content tab; move-tab GROUP_ID".into();
                    accepted = false;
                }
            }
            "close-tab" => {
                if let Some(t) = self.layout.active(pane) {
                    self.apply(Action::CloseTab {
                        group: pane,
                        tab: t.id,
                    });
                }
            }
            "close-pane" => self.apply(Action::ClosePane(pane)),
            "resize" => {
                if let Ok(delta) = rest.parse::<i16>() {
                    self.apply(Action::ResizeParent { pane, delta });
                } else {
                    self.notice = "resize +500 or -500 basis points of first child".into();
                    accepted = false;
                }
            }
            "maximize" => self.toggle_maximize(pane),
            "explorer-left" => self.set_explorer_placement(ExplorerPlacement::Left),
            "explorer-right" => self.set_explorer_placement(ExplorerPlacement::Right),
            "explorer-hide" => self.set_explorer_placement(ExplorerPlacement::Hidden),
            "explorer-show" => {
                let placement = if self.explorer_placement == ExplorerPlacement::Hidden {
                    ExplorerPlacement::Left
                } else {
                    self.explorer_placement
                };
                self.set_explorer_placement(placement);
            }
            "pin" => {
                if let Some(t) = self.layout.active(pane) {
                    self.apply(Action::Pin {
                        tab: t.id,
                        pinned: !t.pinned,
                    });
                }
            }
            "find" => {
                self.start_search();
            }
            "filter" => {
                if matches!(
                    self.layout.panes.get(&pane),
                    Some(Pane::Navigator {
                        kind: Navigator::Git
                    })
                ) {
                    let n = self.git.panes.entry(pane).or_default();
                    n.filter = rest.into();
                    n.selected = 0;
                } else if let Some(Pane::Navigator { kind }) = self.layout.panes.get(&pane).cloned()
                {
                    let n = self.nav.entry((pane, kind)).or_default();
                    n.filter = rest.into();
                    n.selected = 0;
                    if kind == Navigator::Sessions {
                        n.selected_sessions.clear();
                    }
                    if kind == Navigator::Files {
                        self.search_file_filter(browser, pane, rest);
                    }
                } else {
                    self.notice = "Focus Files, Git or Sessions before filtering".into();
                    accepted = false;
                }
            }
            "files-open" => accepted = self.authorize_files(browser, rest, false),
            "files-reconnect" => accepted = self.authorize_files(browser, rest, true),
            "file" => {
                if rest.is_empty() {
                    self.notice = "file RELATIVE_PATH".into();
                    accepted = false;
                } else {
                    self.read_file(browser, rest.into());
                }
            }
            "refresh" => {
                if matches!(
                    self.layout.panes.get(&pane),
                    Some(Pane::Navigator {
                        kind: Navigator::Git
                    })
                ) {
                    if let Err(e) = self.git.refresh(pane) {
                        self.notice = e;
                    }
                } else if let Some(Pane::Navigator {
                    kind: Navigator::Files,
                }) = self.layout.panes.get(&pane)
                {
                    let path = self
                        .nav
                        .entry((pane, Navigator::Files))
                        .or_default()
                        .directory
                        .clone();
                    self.refresh_files_navigator(browser, pane, path);
                } else if let Some(Target::File { scope_id, path }) =
                    self.layout.active(pane).map(|t| t.target.clone())
                {
                    self.refresh_file(browser, FileKey::new(scope_id, path));
                } else {
                    self.owner(worker, Command::Refresh, false);
                }
            }
            "new" => accepted = self.owner(worker, Command::New, true),
            "resume" => {
                if rest.is_empty() {
                    self.open_palette();
                    if let Some(editor) = self.palette.as_mut() {
                        let _ = editor.set("#");
                    }
                    self.notice = "Choose a saved Session to resume".into();
                } else if let Some((session, branch)) =
                    crate::tui::commands::parse_resume_args(rest)
                {
                    accepted = self.owner(
                        worker,
                        Command::Resume {
                            session: session.into(),
                            branch: branch.into(),
                        },
                        true,
                    );
                } else {
                    self.notice = "/resume SESSION [BRANCH]".into();
                    accepted = false;
                }
            }
            "switch" => {
                if rest.is_empty() {
                    self.notice = "/switch BRANCH".into();
                    accepted = false;
                } else {
                    accepted = self.owner(worker, Command::Switch(rest.into()), true);
                }
            }
            "fork" => {
                if let Some((name, at)) = crate::tui::commands::parse_fork_args(rest) {
                    accepted = self.owner(
                        worker,
                        Command::Fork {
                            name: name.into(),
                            at: at.map(str::to_owned),
                        },
                        true,
                    );
                } else {
                    self.notice = "/fork NAME [NODE]".into();
                    accepted = false;
                }
            }
            "checkpoint" => {
                if rest.is_empty() {
                    self.notice = "/checkpoint SUMMARY".into();
                    accepted = false;
                } else {
                    accepted = self.owner(worker, Command::Checkpoint(rest.into()), true);
                }
            }
            "model" => {
                if rest.is_empty() {
                    self.notice = format!("Current model: {}", self.snapshot.model);
                } else if let Err(e) = aw::config::validate_model(rest) {
                    self.notice = e.to_string();
                    accepted = false;
                } else {
                    accepted = self.owner(worker, Command::Model(rest.into()), false);
                }
            }
            "acknowledge" => {
                let note = if rest.is_empty() {
                    crate::tui::commands::DEFAULT_ACKNOWLEDGEMENT
                } else {
                    rest
                };
                accepted = self.owner(worker, Command::Acknowledge(note.into()), false);
            }
            "layout-save" | "layout-load" => {
                if self.layout_pending {
                    self.notice = "A layout operation is still running".into();
                    return false;
                }
                let Some(path) = self.layout_file.clone() else {
                    self.notice = "Set --workspace-layout PATH to save/reload view state".into();
                    return false;
                };
                let root = self.root.clone();
                let layout = self.layout.clone();
                let base_generation = self.layout_generation;
                let save = verb == "layout-save";
                self.notice = "Layout I/O pending; agent/context/draft remain unchanged".into();
                self.layout_pending = true;
                let out = self.background_tx.clone();
                let panic_out = out.clone();
                if let Err(error) = self.background.try_submit_with_panic(
                    "layout",
                    move || {
                        let result = if save {
                            persistence::save(&path, &root, &layout)
                                .map(|_| None)
                                .map_err(|e| e.to_string())
                        } else {
                            persistence::load(&path, &root, &ScopeId::launch())
                                .and_then(|layout| {
                                    layout.map(Some).ok_or_else(|| {
                                        Error::Config("No saved layout exists".into())
                                    })
                                })
                                .map_err(|e| e.to_string())
                        };
                        let _ = out.blocking_send(UiBackgroundResult::Layout {
                            base_generation,
                            result,
                        });
                    },
                    move || {
                        let _ = panic_out.blocking_send(UiBackgroundResult::Layout {
                            base_generation,
                            result: Err("layout background job panicked; retry explicitly".into()),
                        });
                    },
                ) {
                    self.layout_pending = false;
                    self.notice = error.to_string();
                    accepted = false;
                }
            }
            "recover-draft" => {
                if !self.draft.is_empty() {
                    self.notice="Your newer draft is preserved; clear it first to recover the rejected prompt".into();
                    accepted = false;
                } else {
                    let recovered = std::mem::take(&mut self.recovery);
                    if let Err(e) = self.draft.set(recovered) {
                        self.notice = e.to_string();
                    }
                    self.composer.reset_after_external_edit();
                    self.open(Target::Conversation);
                }
            }
            "discard-recovery" => {
                self.recovery.clear();
                self.notice = "Discarded the unsent rejected-input recovery buffer only".into();
            }
            "minimal" => accepted = self.return_to_agent(),
            "quit" | "quit-discard" | "quit-terminals" | "quit-all" => {
                if self.terminal_pending {
                    self.notice = "Wait for terminal launch before quitting".into();
                    return false;
                }
                let discard_text = matches!(verb, "quit-discard" | "quit-all");
                let terminate_terminals = matches!(verb, "quit-terminals" | "quit-all");
                if !terminate_terminals && self.terminal_pool.as_ref().is_some_and(|p| p.running())
                {
                    self.notice="Live terminals remain. Hide/return inline keeps them; use quit-terminals to confirm termination.".into();
                    return false;
                }
                if !discard_text && (!self.draft.is_empty() || !self.recovery.is_empty()) {
                    self.notice="Unsent text remains. Use quit-discard to explicitly discard and exit, or keep editing.".into();
                    return false;
                }
                self.draft.clear();
                self.composer.reset_after_external_edit();
                self.recovery.clear();
                self.exit = Some(Exit::Quit);
                worker.projection.cancel();
                self.notice =
                    "Cancellation requested; waiting for native journal settlement before exiting."
                        .into();
            }
            _ => {
                self.notice =
                    "Unknown workspace command. Ctrl+Shift+P/F1 opens commands; ? shows shortcuts."
                        .into();
                accepted = false;
            }
        }
        accepted
    }
    pub(super) fn palette_item_allowed(&self, item: &PaletteItem) -> bool {
        if !self.snapshot.busy && !self.snapshot.state_pending {
            return true;
        }
        match &item.target {
            PaletteTarget::File(_) | PaletteTarget::Window { .. } | PaletteTarget::Terminal(_) => {
                true
            }
            PaletteTarget::Session { session, branch } => {
                self.snapshot.session.as_deref() == Some(session.as_str())
                    && self.snapshot.branch_id.as_deref() == Some(branch.as_str())
            }
            PaletteTarget::Command(command) | PaletteTarget::Slash(command) => {
                let raw = command.split_whitespace().next().unwrap_or("");
                let verb = crate::tui::commands::canonical_slash_name(raw).unwrap_or(raw);
                matches!(
                    verb,
                    "conversation"
                        | "open-agent-view"
                        | "settings"
                        | "status"
                        | "theme"
                        | "keymap"
                        | "help"
                        | "activity"
                        | "tree"
                        | "files"
                        | "files-open"
                        | "files-reconnect"
                        | "sessions"
                        | "git"
                        | "split"
                        | "add-nav"
                        | "focus"
                        | "move-pane"
                        | "move-tab"
                        | "close-tab"
                        | "close-pane"
                        | "resize"
                        | "maximize"
                        | "explorer-left"
                        | "explorer-right"
                        | "explorer-hide"
                        | "explorer-show"
                        | "pin"
                        | "find"
                        | "filter"
                        | "file"
                        | "terminal"
                        | "terminal-open"
                        | "terminals"
                        | "terminal-dock"
                        | "terminal-dock-cancel"
                        | "terminal-scroll"
                        | "terminal-live"
                        | "hunk-next"
                        | "hunk-prev"
                        | "reviewed"
                        | "cancel-agent"
                )
            }
        }
    }
    pub(super) fn execute_palette_item(
        &mut self,
        item: PaletteItem,
        worker: &Worker,
        browser: &Browser,
    ) {
        if !self.palette_item_allowed(&item) {
            self.notice =
                "This command is available after the current turn settles; no command queued"
                    .into();
            return;
        }
        match item.target {
            PaletteTarget::Command(command) => {
                let _ = self.command(command.trim_end(), worker, browser);
            }
            PaletteTarget::Slash(command) => {
                let _ = self.command(command.trim_end(), worker, browser);
            }
            PaletteTarget::File(key) => {
                let group = self.group();
                self.read_file_key(browser, key.clone());
                self.focus_palette_file(group, &key);
            }
            PaletteTarget::Session { session, branch } => {
                if !self.focus_current_agent(&session, &branch)
                    && self.owner(worker, Command::Resume { session, branch }, true)
                {
                    self.open(Target::Conversation);
                }
            }
            PaletteTarget::Window { group, tab } => {
                self.apply(Action::Select { group, tab });
                self.input = matches!(
                    self.layout.active(group).map(|tab| &tab.target),
                    Some(Target::Conversation)
                );
            }
            PaletteTarget::Terminal(terminal) => self.open(Target::Terminal { terminal }),
        }
    }

    pub(super) fn send_review_feedback(&mut self) -> bool {
        let Some(id) = self.review_id() else {
            self.notice = "Focus a diff before sending review feedback".into();
            return false;
        };
        let Some(agent_root) = self.snapshot.agent_root.as_deref() else {
            self.notice = "Current Agent root unavailable; feedback remains local.".into();
            return false;
        };
        match self.git.feedback(&id, agent_root) {
            Ok(text) => {
                let addition = if self.draft.is_empty() {
                    text
                } else {
                    format!("\n\n{text}")
                };
                if let Err(error) = self.draft.insert(&addition) {
                    self.notice = error.to_string();
                    return false;
                }
                self.open(Target::Conversation);
                self.notice =
                    "Feedback inserted into unsent composer. Inspect and Send explicitly.".into();
                true
            }
            Err(error) => {
                self.notice = error;
                false
            }
        }
    }

    pub(super) fn extra_command(
        &mut self,
        verb: &str,
        rest: &str,
        worker: &Worker,
    ) -> Option<bool> {
        let mut accepted = true;
        match verb {
            "cancel-agent" => {
                worker.projection.cancel();
                self.notice = "Agent cancellation requested, separate from terminal input".into();
            }
            "terminal" => self.toggle_terminal(),
            #[cfg(unix)]
            "terminal-dock" => {
                let id = self.terminal().map(|terminal| terminal.id.clone());
                match (&self.terminal_pool, id) {
                    (Some(pool), Some(id)) => match super::super::native_dock::invite(pool.clone(), &id) {
                        Ok(invite) => self.notice = format!(
                            "Dock: aw workspace attach {}",
                            invite.display()
                        ),
                        Err(error) => { self.notice = error.to_string(); accepted = false; }
                    },
                    _ => { self.notice = "Focus a running terminal before docking".into(); accepted = false; }
                }
            }
            #[cfg(unix)]
            "terminal-dock-cancel" => {
                let id = self.terminal().map(|terminal| terminal.id.clone());
                match (&self.terminal_pool, id) {
                    (Some(pool), Some(id)) => match pool.cancel_dock(&id) {
                        Ok(()) => self.notice = "Unused native PTY invitation revoked; shell remains owned here".into(),
                        Err(error) => { self.notice = error.to_string(); accepted = false; }
                    },
                    _ => { self.notice = "Focus a terminal with a pending invitation".into(); accepted = false; }
                }
            }
            "terminal-start" => {
                if self.terminal_pending {
                    self.notice = "Terminal launch already pending".into();
                    return Some(false);
                }
                let Some(pool) = self.terminal_pool.clone() else {
                    self.notice = "Terminal service not attached".into();
                    return Some(false);
                };
                let root = match self.terminal_git_root() {
                    Ok(root) => root,
                    Err(error) => {
                        self.notice = error;
                        return Some(false);
                    }
                };
                let shell = self.shell.clone();
                let owner = self.snapshot.actor.clone();
                self.notice = format!(
                    "Launching trusted shell at {} — agent binding unchanged",
                    root.path.display()
                );
                self.terminal_pending = true;
                let out = self.background_tx.clone();
                let panic_out = out.clone();
                if let Err(error) = self.background.try_submit_with_panic(
                    "terminal",
                    move || {
                        let result = root.launch(pool, shell, owner);
                        let _ = out.blocking_send(UiBackgroundResult::Terminal(result));
                    },
                    move || {
                        let _ = panic_out.blocking_send(UiBackgroundResult::Terminal(Err(
                            "terminal launch background job panicked; retry explicitly".into(),
                        )));
                    },
                ) {
                    self.terminal_pending = false;
                    self.notice = error.to_string();
                    accepted = false;
                }
            }
            "terminal-open" => {
                let id = self.terminal_pool.as_ref().and_then(|p| {
                    p.get(rest).map(|t| t.id.clone()).or_else(|| {
                        rest.parse::<usize>()
                            .ok()
                            .and_then(|n| n.checked_sub(1))
                            .and_then(|n| p.all().get(n).map(|t| t.id.clone()))
                    })
                });
                if let Some(terminal) = id {
                    self.open(Target::Terminal { terminal });
                } else {
                    self.notice="Unknown or restored terminal ID. Only explicit terminal-start creates a shell.".into();
                    accepted = false;
                }
            }
            "terminals" => {
                let lines = self
                    .terminal_pool
                    .as_ref()
                    .map(|p| {
                        p.all()
                            .iter()
                            .enumerate()
                            .map(|(i, t)| {
                                format!("#{} {} {:?} {}", i + 1, t.id, t.status(), t.root.display())
                            })
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                self.notice = if lines.is_empty() {
                    "No terminals started".into()
                } else {
                    lines.join(" | ")
                };
            }
            "terminal-stop" => {
                if let (Some(pool), Some(t)) = (&self.terminal_pool, self.terminal()) {
                    match pool.terminate_local(&t.id) {
                        Ok(()) => self.notice =
                            "PTY termination requested; inspect commands with uncertain effects".into(),
                        Err(error) => {
                            self.notice = error.to_string();
                            accepted = false;
                        }
                    }
                } else {
                    self.notice = "Focus a terminal tab".into();
                    accepted = false;
                }
            }
            "terminal-forget" => {
                let current = self.terminal().map(|t| t.id.clone());
                let id = if rest.is_empty() {
                    current.as_deref().unwrap_or("")
                } else {
                    rest
                };
                if let Some(pool) = &self.terminal_pool {
                    match pool.forget(id) {
                        Ok(()) => {
                            self.notice =
                                "Forgot exited resource; existing views show unavailable".into();
                        }
                        Err(e) => {
                            self.notice = e.to_string();
                            accepted = false;
                        }
                    }
                } else {
                    self.notice = "Terminal service not attached".into();
                    accepted = false;
                }
            }
            "terminal-scroll" | "terminal-live" => {
                if let Some(t) = self.layout.active(self.layout.focused) {
                    if matches!(t.target, Target::Terminal { .. }) {
                        let offset = self.terminal_scroll.entry(t.id).or_default();
                        if verb == "terminal-live" {
                            *offset = 0;
                        } else if let Ok(delta) = rest.parse::<isize>() {
                            *offset = offset
                                .saturating_add_signed(delta)
                                .min(aw_terminal::SCROLLBACK);
                        } else {
                            self.notice =
                                "terminal-scroll -20|20; positive means older rows".into();
                            accepted = false;
                        }
                    } else {
                        self.notice = "Focus a terminal tab".into();
                        accepted = false;
                    }
                } else {
                    self.notice = "Focus a terminal tab".into();
                    accepted = false;
                }
            }
            "stage" | "unstage" => {
                if let Some(id) = self.review_id() {
                    let action = if verb == "stage" {
                        aw_git::IndexAction::Stage
                    } else {
                        aw_git::IndexAction::Unstage
                    };
                    match self.git.index_request(&id, action) {
                        Ok(request) => {
                            accepted = self.owner(worker, Command::GitIndex(request), false)
                        }
                        Err(error) => {
                            self.notice = error;
                            accepted = false;
                        }
                    }
                } else {
                    self.notice =
                        "Focus an exact changed-file diff. No stage-all operation.".into();
                    accepted = false;
                }
            }
            "comment" => {
                if let (Some(id), Some((number, text))) = (self.review_id(), rest.split_once(' ')) {
                    match number.parse::<usize>().ok().and_then(|n| n.checked_sub(1)) {
                        Some(row) => match self.git.comment(&id, row, text) {
                            Ok(()) => {
                                self.notice = "Comment attached to this exact snapshot; send-review copies to composer".into();
                            }
                            Err(e) => {
                                self.notice = e;
                                accepted = false;
                            }
                        },
                        None => {
                            self.notice = "comment DIFF_ROW TEXT (rows start at 1)".into();
                            accepted = false;
                        }
                    }
                } else {
                    self.notice = "Focus diff; comment DIFF_ROW TEXT".into();
                    accepted = false;
                }
            }
            "send-review" => accepted = self.send_review_feedback(),
            "reviewed" => {
                if let Some(id) = self.review_id() {
                    if !self.git.reviewed.remove(&id) {
                        self.git.reviewed.insert(id);
                    }
                    self.git.revision = self.git.revision.wrapping_add(1);
                } else {
                    self.notice = "Focus a diff before marking it reviewed".into();
                    accepted = false;
                }
            }
            "hunk-next" | "hunk-prev" => {
                let mut moved = false;
                if let Some(id) = self.review_id() {
                    if let (Some((_, review)), Some(tab)) = (
                        self.git.reviews.get(&id),
                        self.layout.active(self.layout.focused),
                    ) {
                        let at = self.scroll.get(&tab.id).map_or(0, Viewport::offset);
                        let starts = review
                            .lines
                            .iter()
                            .enumerate()
                            .filter(|(_, l)| l.kind == aw_git::LineKind::Hunk)
                            .map(|(i, _)| i + 2)
                            .collect::<Vec<_>>();
                        let next = if verb == "hunk-next" {
                            starts.iter().copied().find(|i| *i > at)
                        } else {
                            starts.iter().rev().copied().find(|i| *i < at)
                        };
                        if let Some(row) = next {
                            self.jump_to_row(tab.id, row);
                            moved = true;
                        }
                    }
                }
                if !moved {
                    self.notice = "No matching diff hunk from the current position".into();
                    accepted = false;
                }
            }
            _ => return None,
        }
        Some(accepted)
    }
}
