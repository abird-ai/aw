//! Model text is data, not terminal control. No ANSI/OSC pass-through.
pub use crate::presentation::{safe_text, tail_text, tool_title, tool_tone, ToolTone};
use crate::Result;
use crossterm::{
    queue,
    style::{Color, ResetColor, SetBackgroundColor, SetForegroundColor},
};
use std::io::{self, IsTerminal, Write};

pub fn delta(text: &str) -> Result<()> {
    let mut out = io::stdout().lock();
    out.write_all(safe_text(text).as_bytes())?;
    out.flush()?;
    Ok(())
}

fn write_tinted_lines(
    mut out: impl Write,
    lines: &[String],
    background: Color,
    foreground: Color,
) -> Result<()> {
    let width = crossterm::terminal::size().map_or(80usize, |(width, _)| usize::from(width));
    let terminal = io::stderr().is_terminal();
    for line in lines {
        if terminal {
            queue!(
                out,
                SetForegroundColor(foreground),
                SetBackgroundColor(background)
            )?;
        }
        let safe = safe_text(line);
        write!(out, "{safe}")?;
        if terminal {
            let used = safe.chars().count().min(width);
            if width > used {
                write!(out, "{}", " ".repeat(width - used))?;
            }
            queue!(out, ResetColor)?;
        }
        writeln!(out)?;
    }
    out.flush()?;
    Ok(())
}

fn tool_terminal_colors(tone: ToolTone) -> (Color, Color) {
    match tone {
        ToolTone::Success => (
            Color::Rgb {
                r: 35,
                g: 48,
                b: 39,
            },
            Color::Rgb {
                r: 194,
                g: 213,
                b: 199,
            },
        ),
        ToolTone::Failure => (
            Color::Rgb {
                r: 58,
                g: 38,
                b: 41,
            },
            Color::Rgb {
                r: 244,
                g: 184,
                b: 188,
            },
        ),
        ToolTone::Neutral => (
            Color::Rgb {
                r: 43,
                g: 48,
                b: 45,
            },
            Color::Rgb {
                r: 188,
                g: 196,
                b: 191,
            },
        ),
    }
}

pub fn tool_started(record: &aw_agent_core::ToolRecord) -> Result<()> {
    let line = tool_title(record);
    let mut out = io::stderr().lock();
    let (background, foreground) = tool_terminal_colors(ToolTone::Neutral);
    write_tinted_lines(&mut out, &[line], background, foreground)
}

pub fn tool_finished(record: &aw_agent_core::ToolRecord) -> Result<()> {
    use aw_agent_core::ToolStatus;
    let (background, foreground) = tool_terminal_colors(tool_tone(Some(record)));
    let mut lines = Vec::new();
    if let Some(output) = &record.output {
        let safe = safe_text(&output.content);
        let all = safe.lines().collect::<Vec<_>>();
        const MAX_VISIBLE: usize = 12;
        if all.len() > MAX_VISIBLE {
            lines.push(format!(
                "… ({} earlier lines omitted)",
                all.len() - MAX_VISIBLE
            ));
            lines.extend(
                all[all.len() - MAX_VISIBLE..]
                    .iter()
                    .map(|line| (*line).to_owned()),
            );
        } else {
            lines.extend(all.into_iter().map(str::to_owned));
        }
        if output.truncated {
            lines.push("… output truncated by tool bound".into());
        }
    }
    if lines.is_empty() {
        lines.push(match record.status {
            ToolStatus::Succeeded => "✓ done".into(),
            ToolStatus::Failed => "failed".into(),
            ToolStatus::Denied => "denied".into(),
            ToolStatus::Interrupted => "interrupted".into(),
            ToolStatus::Running => "running".into(),
        });
    }
    let mut out = io::stderr().lock();
    write_tinted_lines(&mut out, &lines, background, foreground)
}

pub fn user_prompt(text: &str, padding: u8) -> Result<()> {
    let safe = safe_text(text);
    let pad = " ".repeat(usize::from(padding));
    let mut out = io::stdout().lock();
    for (index, line) in safe.split('\n').enumerate() {
        write!(out, "{pad}")?;
        if index == 0 {
            queue!(out, SetForegroundColor(Color::Cyan))?;
            write!(out, "›")?;
            queue!(out, ResetColor)?;
            write!(out, " {line}")?;
        } else {
            write!(out, "  {line}")?;
        }
        writeln!(out)?;
    }
    out.flush()?;
    Ok(())
}

#[derive(Debug, PartialEq, Eq)]
pub enum Input<'a> {
    Prompt(&'a str),
    Help,
    New,
    History,
    Quit,
    Retry,
    Title(Option<&'a str>),
    Model(Option<&'a str>),
    Profile(Option<&'a str>),
    Unknown,
}
pub fn parse_input(line: &str) -> Input<'_> {
    if line.starts_with("//") {
        return Input::Prompt(&line[1..]);
    }
    if !line.starts_with('/') {
        return Input::Prompt(line);
    }
    let mut parts = line.splitn(2, char::is_whitespace);
    let command = parts.next().unwrap_or("");
    let value = parts.next().map(str::trim).filter(|s| !s.is_empty());
    match (command, value) {
        ("/help", None) => Input::Help,
        ("/new", None) => Input::New,
        ("/history", None) => Input::History,
        ("/quit" | "/exit", None) => Input::Quit,
        ("/retry", None) => Input::Retry,
        ("/title", v) => Input::Title(v),
        ("/model", v) => Input::Model(v),
        ("/profile", v) => Input::Profile(v),
        _ => Input::Unknown,
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn no_terminal_injection() {
        let s = safe_text("ok\x1b]52;c;secret\x07\n");
        assert!(!s.contains('\x1b'));
        assert!(!s.contains('\x07'));
        assert!(s.ends_with('\n'));
    }
    #[test]
    fn unicode_and_newlines_preserved() {
        assert_eq!(safe_text("héllo 🐦\n\t世界"), "héllo 🐦\n\t世界");
    }
    #[test]
    fn commands_and_escape() {
        assert_eq!(parse_input("/model local"), Input::Model(Some("local")));
        assert_eq!(parse_input("//help"), Input::Prompt("/help"));
        assert_eq!(parse_input("/whatever"), Input::Unknown);
        assert_eq!(parse_input("/new"), Input::New);
        assert_eq!(parse_input("/title bird"), Input::Title(Some("bird")));
    }
    #[test]
    fn embedded_command_is_only_text() {
        assert_eq!(parse_input("please /quit"), Input::Prompt("please /quit"));
    }
}
