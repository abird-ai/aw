use super::{
    commands::{complete_slash, cycle_selection, exact_slash_command, slash_matches},
    editor::{edit_key, is_plain_enter, EditMode},
};
use aw::{Error, Result};
use aw_editor::Editor;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[derive(Clone, Debug, Default)]
pub struct Interaction {
    pub selected: usize,
    history_at: Option<usize>,
    scratch: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    Handled,
    OpenSettings,
    Submit(String),
    SubmitWhileBusy,
}

impl Interaction {
    pub fn reset_after_external_edit(&mut self) {
        self.selected = 0;
        self.history_at = None;
    }

    pub fn handle_live(
        &mut self,
        editor: &mut Editor,
        key: KeyEvent,
        workspace: bool,
    ) -> Result<Action> {
        let slash = slash_matches(editor.text(), workspace);
        if !slash.is_empty() {
            match key.code {
                KeyCode::Up => {
                    self.selected = cycle_selection(self.selected, slash.len(), false);
                    return Ok(Action::Handled);
                }
                KeyCode::Down => {
                    self.selected = cycle_selection(self.selected, slash.len(), true);
                    return Ok(Action::Handled);
                }
                KeyCode::Tab => {
                    if let Some(spec) = slash.get(self.selected.min(slash.len() - 1)) {
                        editor
                            .set(complete_slash(editor.text(), spec))
                            .map_err(|error| Error::Config(error.to_string()))?;
                    }
                    self.selected = 0;
                    return Ok(Action::Handled);
                }
                _ => {}
            }
        }

        if is_plain_enter(&key) && editor.text().trim() == "/settings" {
            editor.clear();
            self.selected = 0;
            return Ok(Action::OpenSettings);
        }

        if is_plain_enter(&key) {
            return Ok(Action::SubmitWhileBusy);
        }

        edit_key(editor, key, EditMode::Composer)
            .map_err(|error| Error::Config(error.to_string()))?;
        self.selected = 0;
        Ok(Action::Handled)
    }

    pub fn handle(
        &mut self,
        editor: &mut Editor,
        key: KeyEvent,
        history: &[String],
        workspace: bool,
    ) -> Result<Action> {
        if key.modifiers.contains(KeyModifiers::CONTROL)
            && !key.modifiers.contains(KeyModifiers::SHIFT)
            && matches!(key.code, KeyCode::Char('p') | KeyCode::Char('n'))
        {
            self.history(
                editor,
                history,
                key.code == KeyCode::Char('p'),
            )?;
            self.selected = 0;
            return Ok(Action::Handled);
        }

        let slash = slash_matches(editor.text(), workspace);
        if !slash.is_empty() {
            match key.code {
                KeyCode::Up => {
                    self.selected = cycle_selection(self.selected, slash.len(), false);
                    return Ok(Action::Handled);
                }
                KeyCode::Down => {
                    self.selected = cycle_selection(self.selected, slash.len(), true);
                    return Ok(Action::Handled);
                }
                KeyCode::Tab => {
                    if let Some(spec) = slash.get(self.selected.min(slash.len() - 1)) {
                        editor
                            .set(complete_slash(editor.text(), spec))
                            .map_err(|error| Error::Config(error.to_string()))?;
                    }
                    self.selected = 0;
                    return Ok(Action::Handled);
                }
                KeyCode::Esc => {
                    if editor.text() == "/" {
                        editor.clear();
                    }
                    self.selected = 0;
                    return Ok(Action::Handled);
                }
                KeyCode::Enter if is_plain_enter(&key) => {
                    if let Some(spec) = slash.get(self.selected.min(slash.len() - 1)) {
                        let exact = exact_slash_command(editor.text(), workspace)
                            .is_some_and(|exact| exact.name == spec.name);
                        if !exact {
                            editor
                                .set(complete_slash(editor.text(), spec))
                                .map_err(|error| Error::Config(error.to_string()))?;
                            self.selected = 0;
                            if spec.supports_inline_args {
                                return Ok(Action::Handled);
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        if is_plain_enter(&key) && editor.text().trim() == "/settings" {
            editor.clear();
            self.selected = 0;
            self.history_at = None;
            return Ok(Action::OpenSettings);
        }

        if is_plain_enter(&key) {
            let value = editor.text().to_owned();
            if !value.trim().is_empty() {
                return Ok(Action::Submit(value));
            }
            return Ok(Action::Handled);
        }

        if slash.is_empty() {
            match key.code {
                KeyCode::Up if editor.text().contains('\n') => {
                    editor.vertical(false);
                    self.history_at = None;
                    return Ok(Action::Handled);
                }
                KeyCode::Down if editor.text().contains('\n') => {
                    editor.vertical(true);
                    self.history_at = None;
                    return Ok(Action::Handled);
                }
                KeyCode::Up if !history.is_empty() => {
                    self.history(editor, history, true)?;
                    return Ok(Action::Handled);
                }
                KeyCode::Down if self.history_at.is_some() => {
                    self.history(editor, history, false)?;
                    return Ok(Action::Handled);
                }
                _ => {}
            }
        }

        edit_key(editor, key, EditMode::Composer)
            .map_err(|error| Error::Config(error.to_string()))?;
        self.selected = 0;
        self.history_at = None;
        Ok(Action::Handled)
    }

    fn history(
        &mut self,
        editor: &mut Editor,
        history: &[String],
        previous: bool,
    ) -> Result<bool> {
        if previous {
            if history.is_empty() {
                return Ok(false);
            }
            if self.history_at.is_none() {
                self.scratch = editor.text().to_owned();
                self.history_at = Some(history.len());
            }
            let next = self.history_at.unwrap_or(history.len()).saturating_sub(1);
            self.history_at = Some(next);
            editor
                .set(history[next].clone())
                .map_err(|error| Error::Config(error.to_string()))?;
            return Ok(true);
        }

        let Some(at) = self.history_at else {
            return Ok(false);
        };
        if at + 1 < history.len() {
            self.history_at = Some(at + 1);
            editor
                .set(history[at + 1].clone())
                .map_err(|error| Error::Config(error.to_string()))?;
        } else {
            self.history_at = None;
            editor
                .set(self.scratch.clone())
                .map_err(|error| Error::Config(error.to_string()))?;
        }
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn slash_and_history_selection_wrap_and_share_one_state_machine() {
        let mut state = Interaction::default();
        let mut editor = Editor::default();
        editor.set("/").unwrap();
        state.handle(&mut editor, key(KeyCode::Up), &[], false).unwrap();
        assert!(state.selected > 0);
        state.handle(&mut editor, key(KeyCode::Down), &[], false).unwrap();

        editor.set("draft").unwrap();
        let history = vec!["first".into(), "second".into()];
        state
            .handle(
                &mut editor,
                KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL),
                &history,
                false,
            )
            .unwrap();
        assert_eq!(editor.text(), "second");
        state
            .handle(
                &mut editor,
                KeyEvent::new(KeyCode::Char('n'), KeyModifiers::CONTROL),
                &history,
                false,
            )
            .unwrap();
        assert_eq!(editor.text(), "draft");
    }

    #[test]
    fn settings_and_submit_are_semantic_actions() {
        let mut state = Interaction::default();
        let mut editor = Editor::default();
        editor.set("/settings").unwrap();
        assert_eq!(
            state.handle(&mut editor, key(KeyCode::Enter), &[], false).unwrap(),
            Action::OpenSettings
        );
        assert!(editor.is_empty());

        editor.set("hello").unwrap();
        assert_eq!(
            state.handle(&mut editor, key(KeyCode::Enter), &[], false).unwrap(),
            Action::Submit("hello".into())
        );
    }
}
