use aw_editor::{Editor, Result};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditMode {
    Composer,
    SingleLine,
}

pub fn is_plain_enter(key: &KeyEvent) -> bool {
    key.code == KeyCode::Enter
        && !key
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SHIFT)
}

pub fn edit_key(editor: &mut Editor, key: KeyEvent, mode: EditMode) -> Result<()> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    let shift = key.modifiers.contains(KeyModifiers::SHIFT);

    match key.code {
        // Familiar readline/editor navigation that is safe in both composers
        // and single-line command inputs.
        KeyCode::Char('a') if ctrl => editor.home(),
        KeyCode::Char('e') if ctrl => editor.end(),
        KeyCode::Char('b') if ctrl => editor.left(),
        KeyCode::Char('f') if ctrl => editor.right(),
        KeyCode::Char('u') if ctrl => editor.delete_to_line_start(),
        KeyCode::Char('k') if ctrl => editor.delete_to_line_end(),
        KeyCode::Char('w') if ctrl => editor.delete_word_backward(),
        KeyCode::Char('d') if ctrl => editor.delete(),
        KeyCode::Char('h') if ctrl => editor.backspace(),
        KeyCode::Char('y') if ctrl => editor.yank()?,
        KeyCode::Char('-' | '_') if ctrl => editor.undo(),

        // Readline/shell-style word navigation. Terminals differ in how they
        // encode these chords, so accept the common Alt and Ctrl forms.
        KeyCode::Char('b') if alt => editor.word_left(),
        KeyCode::Char('f') if alt => editor.word_right(),
        KeyCode::Char('d') if alt => editor.delete_word_forward(),
        KeyCode::Char('y') if alt => editor.yank_pop()?,
        KeyCode::Left if ctrl || alt => editor.word_left(),
        KeyCode::Right if ctrl || alt => editor.word_right(),
        KeyCode::Backspace if ctrl || alt => editor.delete_word_backward(),
        KeyCode::Delete if ctrl || alt => editor.delete_word_forward(),

        // Multiline insertion is deliberately composer-only. Command palette
        // inputs are always one logical line.
        KeyCode::Char('j') if ctrl && mode == EditMode::Composer => editor.insert("\n")?,
        KeyCode::Enter if mode == EditMode::Composer && (alt || shift) => editor.insert("\n")?,

        KeyCode::Char(c) if !ctrl && !alt => {
            let mut buf = [0; 4];
            editor.insert(c.encode_utf8(&mut buf))?;
        }
        KeyCode::Backspace => editor.backspace(),
        KeyCode::Delete => editor.delete(),
        KeyCode::Left => editor.left(),
        KeyCode::Right => editor.right(),
        KeyCode::Home => editor.home(),
        KeyCode::End => editor.end(),
        KeyCode::Up if mode == EditMode::Composer => editor.vertical(false),
        KeyCode::Down if mode == EditMode::Composer => editor.vertical(true),
        _ => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn composer_keeps_plain_navigation_letters_as_text() {
        let mut editor = Editor::default();
        edit_key(
            &mut editor,
            KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE),
            EditMode::Composer,
        )
        .unwrap();
        assert_eq!(editor.text(), "j");
    }

    #[test]
    fn single_line_palette_never_accepts_newline_chords() {
        let mut editor = Editor::default();
        edit_key(
            &mut editor,
            KeyEvent::new(KeyCode::Char('j'), KeyModifiers::CONTROL),
            EditMode::SingleLine,
        )
        .unwrap();
        edit_key(
            &mut editor,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT),
            EditMode::SingleLine,
        )
        .unwrap();
        assert!(editor.is_empty());
    }

    #[test]
    fn ctrl_u_deletes_only_to_current_line_start() {
        let mut editor = Editor::default();
        editor.set("first\nsecond").unwrap();
        edit_key(
            &mut editor,
            KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL),
            EditMode::Composer,
        )
        .unwrap();
        assert_eq!(editor.text(), "first\n");
    }

    #[test]
    fn ctrl_enter_is_not_an_undocumented_newline_alias() {
        let mut editor = Editor::default();
        edit_key(
            &mut editor,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL),
            EditMode::Composer,
        )
        .unwrap();
        assert!(editor.is_empty());
    }

    #[test]
    fn familiar_control_navigation_is_shared() {
        let mut editor = Editor::default();
        editor.set("ab").unwrap();
        edit_key(
            &mut editor,
            KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL),
            EditMode::Composer,
        )
        .unwrap();
        edit_key(
            &mut editor,
            KeyEvent::new(KeyCode::Char('X'), KeyModifiers::NONE),
            EditMode::Composer,
        )
        .unwrap();
        assert_eq!(editor.text(), "Xab");
        edit_key(
            &mut editor,
            KeyEvent::new(KeyCode::Char('e'), KeyModifiers::CONTROL),
            EditMode::Composer,
        )
        .unwrap();
        edit_key(
            &mut editor,
            KeyEvent::new(KeyCode::Char('Y'), KeyModifiers::NONE),
            EditMode::Composer,
        )
        .unwrap();
        assert_eq!(editor.text(), "XabY");
    }

    #[test]
    fn word_navigation_and_deletion_accept_common_terminal_chords() {
        let mut editor = Editor::default();
        editor.set("alpha beta-gamma").unwrap();
        edit_key(
            &mut editor,
            KeyEvent::new(KeyCode::Left, KeyModifiers::CONTROL),
            EditMode::Composer,
        )
        .unwrap();
        assert_eq!(editor.cursor(), "alpha beta-".len());
        edit_key(
            &mut editor,
            KeyEvent::new(KeyCode::Backspace, KeyModifiers::ALT),
            EditMode::Composer,
        )
        .unwrap();
        assert_eq!(
            editor.text(),
            "alpha betagamma",
            "backward word deletion crosses exactly one punctuation run"
        );
        edit_key(
            &mut editor,
            KeyEvent::new(KeyCode::Right, KeyModifiers::CONTROL),
            EditMode::Composer,
        )
        .unwrap();
        edit_key(
            &mut editor,
            KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL),
            EditMode::Composer,
        )
        .unwrap();
        assert_eq!(editor.text(), "alpha ");
    }

    #[test]
    fn readline_kill_yank_yank_pop_and_undo_mappings_reach_shared_editor_state() {
        let mut editor = Editor::default();
        editor.set("alpha beta gamma").unwrap();
        edit_key(
            &mut editor,
            KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL),
            EditMode::Composer,
        )
        .unwrap();
        edit_key(
            &mut editor,
            KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL),
            EditMode::Composer,
        )
        .unwrap();
        assert_eq!(editor.text(), "alpha ");
        edit_key(
            &mut editor,
            KeyEvent::new(KeyCode::Char('y'), KeyModifiers::CONTROL),
            EditMode::Composer,
        )
        .unwrap();
        assert_eq!(editor.text(), "alpha beta ");
        edit_key(
            &mut editor,
            KeyEvent::new(KeyCode::Char('y'), KeyModifiers::ALT),
            EditMode::Composer,
        )
        .unwrap();
        assert_eq!(editor.text(), "alpha gamma");
        edit_key(
            &mut editor,
            KeyEvent::new(KeyCode::Char('_'), KeyModifiers::CONTROL),
            EditMode::Composer,
        )
        .unwrap();
        assert_eq!(editor.text(), "alpha beta ");
    }

    #[test]
    fn ctrl_k_and_ctrl_d_match_readline_line_editing() {
        let mut editor = Editor::default();
        editor.set("first second\nthird").unwrap();
        editor.home();
        edit_key(
            &mut editor,
            KeyEvent::new(KeyCode::Char('k'), KeyModifiers::CONTROL),
            EditMode::Composer,
        )
        .unwrap();
        assert_eq!(editor.text(), "first second\n");

        editor.set("third").unwrap();
        editor.home();
        edit_key(
            &mut editor,
            KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL),
            EditMode::Composer,
        )
        .unwrap();
        assert_eq!(editor.text(), "hird");
    }
}
