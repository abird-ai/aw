use crate::{Error, Result, MAX_PASTE};
#[derive(Clone, Copy, Debug, Default)]
pub struct Modifiers {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
}
#[derive(Clone, Copy, Debug)]
pub enum Key {
    Char(char),
    Enter,
    Tab,
    BackTab,
    Backspace,
    Escape,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    Delete,
    Insert,
    PageUp,
    PageDown,
    Function(u8),
}
pub fn encode_key(key: Key, m: Modifiers, application_cursor: bool) -> Result<Vec<u8>> {
    let modifier = 1 + u8::from(m.shift) + 2 * u8::from(m.alt) + 4 * u8::from(m.ctrl);
    let mut s = match key {
        Key::Char(c) => {
            if m.ctrl {
                let c = c.to_ascii_lowercase();
                let byte = match c {
                    'a'..='z' => (c as u8) - b'a' + 1,
                    ' ' | '@' => 0,
                    '[' => 27,
                    '\\' => 28,
                    ']' => 29,
                    '^' => 30,
                    '_' | '/' => 31,
                    '?' => 127,
                    _ => return Err(Error::Unavailable("unsupported control-key encoding")),
                };
                vec![byte]
            } else {
                let mut buf = [0; 4];
                c.encode_utf8(&mut buf).as_bytes().to_vec()
            }
        }
        Key::Enter => vec![b'\r'],
        Key::Tab => vec![b'\t'],
        Key::BackTab => b"\x1b[Z".to_vec(),
        Key::Backspace => vec![127],
        Key::Escape => vec![27],
        Key::Up | Key::Down | Key::Left | Key::Right | Key::Home | Key::End => {
            let c = match key {
                Key::Up => 'A',
                Key::Down => 'B',
                Key::Right => 'C',
                Key::Left => 'D',
                Key::Home => 'H',
                _ => 'F',
            };
            if modifier != 1 {
                format!("\x1b[1;{modifier}{c}").into_bytes()
            } else {
                format!("\x1b{}{c}", if application_cursor { 'O' } else { '[' }).into_bytes()
            }
        }
        Key::Insert | Key::Delete | Key::PageUp | Key::PageDown => {
            let n = match key {
                Key::Insert => 2,
                Key::Delete => 3,
                Key::PageUp => 5,
                _ => 6,
            };
            if modifier == 1 {
                format!("\x1b[{n}~").into_bytes()
            } else {
                format!("\x1b[{n};{modifier}~").into_bytes()
            }
        }
        Key::Function(n) => {
            if (1..=4).contains(&n) {
                let c = char::from(b'P' + n - 1);
                if modifier == 1 {
                    format!("\x1bO{c}").into_bytes()
                } else {
                    format!("\x1b[1;{modifier}{c}").into_bytes()
                }
            } else {
                let code = match n {
                    5 => 15,
                    6 => 17,
                    7 => 18,
                    8 => 19,
                    9 => 20,
                    10 => 21,
                    11 => 23,
                    12 => 24,
                    _ => return Err(Error::Unavailable("function key outside F1–F12")),
                };
                if modifier == 1 {
                    format!("\x1b[{code}~").into_bytes()
                } else {
                    format!("\x1b[{code};{modifier}~").into_bytes()
                }
            }
        }
    };
    if m.alt
        && matches!(
            key,
            Key::Char(_) | Key::Enter | Key::Tab | Key::Backspace | Key::Escape
        )
    {
        s.insert(0, 27);
    }
    Ok(s)
}
pub fn encode_paste(text: &str, bracketed: bool) -> Result<Vec<u8>> {
    if text.len() > MAX_PASTE {
        return Err(Error::Unavailable("paste exceeds 64 KiB"));
    }
    if text
        .chars()
        .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
    {
        return Err(Error::Unavailable("terminal paste contains control bytes"));
    }
    if !bracketed && text.contains(['\r', '\n']) {
        return Err(Error::Unavailable(
            "child has not enabled bracketed paste; multiline paste refused",
        ));
    }
    let mut out = Vec::new();
    if bracketed {
        out.extend_from_slice(b"\x1b[200~");
    }
    out.extend_from_slice(text.as_bytes());
    if bracketed {
        out.extend_from_slice(b"\x1b[201~");
    }
    Ok(out)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ctrl_c_is_child_input() {
        assert_eq!(
            encode_key(
                Key::Char('c'),
                Modifiers {
                    ctrl: true,
                    ..Modifiers::default()
                },
                false
            )
            .unwrap(),
            [3]
        );
    }
    #[test]
    fn app_cursor_and_normal_modes() {
        assert_eq!(
            encode_key(Key::Up, Modifiers::default(), false).unwrap(),
            b"\x1b[A"
        );
        assert_eq!(
            encode_key(Key::Up, Modifiers::default(), true).unwrap(),
            b"\x1bOA"
        );
    }
    #[test]
    fn modified_arrow_has_xterm_modifier() {
        assert_eq!(
            encode_key(
                Key::Left,
                Modifiers {
                    ctrl: true,
                    ..Modifiers::default()
                },
                false
            )
            .unwrap(),
            b"\x1b[1;5D"
        );
    }
    #[test]
    fn paste_never_injects_escape() {
        assert!(encode_paste("\x1b[201~oops", true).is_err());
        assert!(encode_paste("a\nb", false).is_err());
    }
    #[test]
    fn bracketed_paste_wraps_once() {
        assert_eq!(
            encode_paste("a\nb", true).unwrap(),
            b"\x1b[200~a\nb\x1b[201~"
        );
    }
    #[test]
    fn unicode_is_utf8() {
        assert_eq!(
            encode_key(Key::Char('界'), Modifiers::default(), false).unwrap(),
            "界".as_bytes()
        );
    }
}
