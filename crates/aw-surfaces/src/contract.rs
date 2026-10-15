//! Frontend-neutral, secret-free surface commands. A trusted host authenticates
//! the caller separately. These enums do not authenticate strings themselves.
use crate::{Error, Result};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Site {
    pub id: String,
    pub label: String,
    pub url: String,
}
impl Site {
    /// A conservative first website profile: dotted-name HTTPS, no userinfo,
    /// query, fragment, localhost or numeric address. No server-side fetch.
    pub fn new(id: impl Into<String>, label: impl Into<String>, value: &str) -> Result<Self> {
        let id = id.into();
        let label = label.into();
        id_check(&id)?;
        if label.trim().is_empty() || label.len() > 80 || label.chars().any(char::is_control) {
            return Err(Error::Invalid("site label"));
        }
        let url = url::Url::parse(value).map_err(|_| Error::Invalid("site URL"))?;
        let host = url.host_str().ok_or(Error::Invalid("site host"))?;
        if url.scheme() != "https"
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
            || !matches!(url.host(), Some(url::Host::Domain(_)))
            || !host.contains('.')
            || host.ends_with('.')
            || host.ends_with(".localhost")
            || host.ends_with(".local")
            || host.ends_with(".internal")
            || host == "localhost"
            || value.len() > 2048
            || value.chars().any(char::is_control)
        {
            return Err(Error::Invalid(
                "site requires HTTPS DNS host without credentials, query or fragment",
            ));
        }
        Ok(Self {
            id,
            label,
            url: url.to_string(),
        })
    }
    pub fn origin(&self) -> String {
        url::Url::parse(&self.url)
            .expect("validated Site")
            .origin()
            .ascii_serialization()
    }
    pub fn validate(&self) -> Result<()> {
        Self::new(&self.id, &self.label, &self.url).map(|_| ())
    }
}

pub(crate) fn id_check(id: &str) -> Result<()> {
    if id.is_empty()
        || id.len() > 128
        || !id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"_-".contains(&c))
    {
        Err(Error::Invalid("opaque identifier"))
    } else {
        Ok(())
    }
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GitMode {
    Changes,
    Worktrees,
    Branches,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum TerminalInput {
    Key {
        key: String,
        #[serde(default)]
        ctrl: bool,
        #[serde(default)]
        alt: bool,
        #[serde(default)]
        shift: bool,
    },
    Text {
        text: String,
    },
    Paste {
        text: String,
    },
}
fn default_file_search_limit() -> usize {
    80
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    Capabilities,
    Files {
        #[serde(default)]
        path: String,
    },
    FileSearch {
        query: String,
        #[serde(default = "default_file_search_limit")]
        limit: usize,
    },
    File {
        path: String,
    },
    GitList {
        context: String,
        mode: GitMode,
    },
    GitOpen {
        item_id: String,
    },
    Review {
        review_id: String,
    },
    ReviewFeedback {
        review_id: String,
        row: usize,
        text: String,
    },
    Terminals,
    TerminalStart {
        request_id: String,
        context: String,
        confirmed: bool,
        rows: u16,
        cols: u16,
    },
    TerminalClaim {
        terminal_id: String,
        client_id: String,
    },
    TerminalRelease {
        terminal_id: String,
        client_id: String,
        lease: String,
    },
    TerminalScreen {
        terminal_id: String,
        #[serde(default)]
        scrollback: usize,
        #[serde(default)]
        client_id: Option<String>,
        #[serde(default)]
        lease: Option<String>,
    },
    TerminalInput {
        terminal_id: String,
        client_id: String,
        lease: String,
        sequence: u64,
        input: TerminalInput,
    },
    TerminalResize {
        terminal_id: String,
        client_id: String,
        lease: String,
        rows: u16,
        cols: u16,
    },
    TerminalStop {
        terminal_id: String,
        confirmed: bool,
    },
    TerminalForget {
        terminal_id: String,
    },
}

pub fn input_bytes(input: &TerminalInput, application: bool, bracketed: bool) -> Result<Vec<u8>> {
    use aw_terminal::{Key, Modifiers};
    match input {
        TerminalInput::Text { text } => {
            if text.len() > 4096 || text.chars().any(char::is_control) {
                return Err(Error::Invalid("typed text must be bounded printable UTF-8"));
            }
            Ok(text.as_bytes().to_vec())
        }
        TerminalInput::Paste { text } => aw_terminal::encode_paste(text, bracketed)
            .map_err(|_| Error::Invalid("paste rejected by current terminal mode or size")),
        TerminalInput::Key {
            key,
            ctrl,
            alt,
            shift,
        } => {
            let k = match key.as_str() {
                "Enter" => Key::Enter,
                "Tab" if *shift => Key::BackTab,
                "Tab" => Key::Tab,
                "Backspace" => Key::Backspace,
                "Escape" => Key::Escape,
                "ArrowUp" => Key::Up,
                "ArrowDown" => Key::Down,
                "ArrowLeft" => Key::Left,
                "ArrowRight" => Key::Right,
                "Home" => Key::Home,
                "End" => Key::End,
                "Delete" => Key::Delete,
                "Insert" => Key::Insert,
                "PageUp" => Key::PageUp,
                "PageDown" => Key::PageDown,
                other if other.starts_with('F') && other.len() <= 3 => Key::Function(
                    other[1..]
                        .parse()
                        .map_err(|_| Error::Invalid("function key"))?,
                ),
                other => {
                    let mut chars = other.chars();
                    let c = chars.next().ok_or(Error::Invalid("key"))?;
                    if chars.next().is_some() || c.is_control() {
                        return Err(Error::Invalid("key"));
                    }
                    Key::Char(c)
                }
            };
            aw_terminal::encode_key(
                k,
                Modifiers {
                    ctrl: *ctrl,
                    alt: *alt,
                    shift: *shift,
                },
                application,
            )
            .map_err(|_| Error::Invalid("unsupported key"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn public_https_site() {
        assert!(Site::new("docs", "Docs", "https://example.org/docs").is_ok());
    }
    #[test]
    fn no_local_or_token_sites() {
        for u in [
            "http://example.org",
            "https://127.0.0.1/",
            "https://[::1]/",
            "https://localhost/",
            "https://a.local/",
            "https://a.internal/",
            "https://u:p@example.org/",
            "https://example.org/?token=x",
            "https://example.org/#key",
            "javascript:alert(1)",
        ] {
            assert!(Site::new("x", "x", u).is_err(), "{u}");
        }
    }
    #[test]
    fn unknown_command_fields_fail() {
        assert!(
            serde_json::from_str::<Command>(r#"{"op":"file","path":"x","api_key":"secret"}"#)
                .is_err()
        );
    }
    #[test]
    fn ordinary_key_and_ctrl_c() {
        assert_eq!(
            input_bytes(
                &TerminalInput::Key {
                    key: "c".into(),
                    ctrl: true,
                    alt: false,
                    shift: false
                },
                false,
                false
            )
            .unwrap(),
            vec![3]
        );
    }
    #[test]
    fn raw_text_cannot_inject_controls() {
        assert!(input_bytes(
            &TerminalInput::Text {
                text: "\x1b]52;c;bad".into()
            },
            false,
            false
        )
        .is_err());
    }
    #[test]
    fn unicode_text_survives() {
        assert_eq!(
            input_bytes(
                &TerminalInput::Text {
                    text: "a\u{301}界".into()
                },
                false,
                false
            )
            .unwrap(),
            "a\u{301}界".as_bytes()
        );
    }
    #[test]
    fn paste_needs_child_permission_for_newline() {
        assert!(input_bytes(
            &TerminalInput::Paste {
                text: "a\nb".into()
            },
            false,
            false
        )
        .is_err());
    }
    #[test]
    fn invalid_keys_rejected() {
        assert!(input_bytes(
            &TerminalInput::Key {
                key: "F19".into(),
                ctrl: false,
                alt: false,
                shift: false
            },
            false,
            false
        )
        .is_err());
    }
}
