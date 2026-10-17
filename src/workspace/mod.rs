//! Optional native workspace frontend. One owner thread uses the existing SDK;
//! neither a layout operation nor a read-only file view invokes a model.
mod agent_view;
mod background;
mod branch_view;
mod files;
mod git;
mod owner;
mod palette;
mod persistence;
mod session_nav;
mod syntax;
mod tab_strip;
pub(super) mod terminals;
mod ui;
mod viewport;

use crate::{
    runtime,
    tui::terminal_session::{Surface, TerminalSession},
    Host,
};
use aw::{Error, Result};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io::{self, IsTerminal};

pub(super) struct Returned {
    pub host: Host,
    pub draft: String,
    pub quit: bool,
    pub configure_provider: bool,
}

pub(super) fn run(mut host: Host, initial: Option<String>, draft: String) -> Result<Returned> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return Err(Error::Config(
            "aw workspace open requires a real terminal; use --output-print for redirected output".into(),
        ));
    }
    let root = host.tools.root.canonicalize()?;
    let layout_file = host.workspace_layout_file.clone();
    let saved_layout_in_memory = host.workspace_state.take();
    let should_load_layout = saved_layout_in_memory.is_none() && layout_file.is_some();
    let layout = saved_layout_in_memory.unwrap_or_default();
    layout
        .validate()
        .map_err(|e| Error::Config(e.to_string()))?;
    let background = background::Executor::new()?;
    let mut browser = files::Browser::new(root.clone(), background.clone())?;
    let terminals = host
        .workspace_terminals
        .get_or_insert_with(terminals::Pool::new)
        .clone();
    let shell = host
        .tools
        .bash
        .clone()
        .or_else(|| std::env::var_os("AW_BASH").map(std::path::PathBuf::from))
        .or_else(|| option_env!("AW_DEFAULT_BASH").map(std::path::PathBuf::from))
        .unwrap_or_else(|| std::path::PathBuf::from("/bin/bash"));
    let ui_config = host.ui.clone();
    let worker = owner::Worker::start(host)?;
    // This is the same live host, moved while idle from the inline presentation.
    // A request remains in this owner thread throughout changes to its views.
    let ui = ui::Ui::new_with_background(
        layout,
        draft,
        layout_file,
        root,
        worker.projection.snapshot(),
        ui_config,
        background,
    )
    .map(|mut ui| {
        ui.attach_terminals(terminals, shell);
        if should_load_layout {
            ui.schedule_initial_layout_load();
        }
        ui
    });
    let result = (|| -> Result<(ui::Ui, ui::Exit)> {
        let mut ui = ui?;
        let _session = TerminalSession::enter(Surface::Alternate { mouse: true })?;
        let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
        // Draw immediately into the fresh alternate-screen buffer. Avoid an
        // extra physical clear/flicker cycle between primary-screen Agent modes and the
        // first complete fullscreen frame.
        terminal.draw(|frame| ui.draw(frame))?;
        ui.schedule_visible_background_work();
        runtime()?.block_on(ui::event_loop(
            ui,
            &mut terminal,
            &worker,
            &mut browser,
            initial,
        ))
    })();
    // Above scope has restored the physical terminal. Await cooperative owner
    // settlement outside the UI runtime, on both success and error paths.
    let mut host = worker.finish()?;
    match result {
        Ok((ui, exit)) => {
            if ui.ui_config != host.ui {
                host.apply_ui(ui.ui_config.clone())?;
            }
            if exit == ui::Exit::Quit {
                if let Some(pool) = &host.workspace_terminals {
                    pool.shutdown();
                }
            }
            host.workspace_state = Some(ui.layout);
            Ok(Returned {
                host,
                draft: ui.draft.text().to_owned(),
                quit: exit == ui::Exit::Quit,
                configure_provider: exit == ui::Exit::ConfigureProvider,
            })
        }
        Err(error) => {
            drop(host);
            Err(error)
        }
    }
}
