use aw_agent_core::{conversation::Turn, Role, ToolRecord, ToolStatus};

pub fn safe_text(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    for c in input.chars() {
        match c {
            '\n' | '\t' => output.push(c),
            '\r' => {}
            c if c.is_control()
                || matches!(c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}') =>
            {
                use std::fmt::Write;
                let _ = write!(output, "\\u{{{:x}}}", c as u32);
            }
            c => output.push(c),
        }
    }
    output
}

pub fn tail_text(text: &str, max_bytes: usize) -> &str {
    if text.len() <= max_bytes {
        return text;
    }
    let mut start = text.len() - max_bytes;
    while !text.is_char_boundary(start) {
        start += 1;
    }
    &text[start..]
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolTone {
    Success,
    Failure,
    Neutral,
}

pub fn tool_tone(record: Option<&ToolRecord>) -> ToolTone {
    let Some(record) = record else {
        return ToolTone::Neutral;
    };
    if matches!(
        record.status,
        ToolStatus::Failed | ToolStatus::Denied | ToolStatus::Interrupted
    ) || record.output.as_ref().is_some_and(|output| output.is_error)
    {
        ToolTone::Failure
    } else if matches!(record.status, ToolStatus::Succeeded) {
        ToolTone::Success
    } else {
        ToolTone::Neutral
    }
}

pub fn tool_title(record: &ToolRecord) -> String {
    let title = if record.call.name == "bash" {
        record
            .call
            .arguments
            .get("command")
            .and_then(serde_json::Value::as_str)
            .map(|command| format!("$ {command}"))
    } else {
        ["path", "query", "url"].into_iter().find_map(|key| {
            record
                .call
                .arguments
                .get(key)
                .and_then(serde_json::Value::as_str)
                .map(|value| format!("{}  {}", record.call.name, value))
        })
    };
    if let Some(title) = title {
        return safe_text(&title);
    }
    let args = serde_json::to_string(&record.call.arguments).unwrap_or_else(|_| "{}".into());
    let title = if args == "{}" {
        record.call.name.clone()
    } else {
        format!("{}  {}", record.call.name, args)
    };
    safe_text(&title)
}

pub fn settled_answer_fallback(turn: &Turn) -> Option<String> {
    if turn.answer.trim().is_empty() {
        return None;
    }

    let projected = turn
        .context_messages
        .iter()
        .skip(1)
        .filter(|message| message.role == Role::Assistant && !message.content.is_empty())
        .map(|message| message.content.as_str())
        .collect::<Vec<_>>()
        .join("\n\n");

    if projected == turn.answer || projected.ends_with(&turn.answer) {
        return None;
    }

    if let Some(suffix) = turn.answer.strip_prefix(&projected) {
        if let Some(missing_rounds) = suffix.strip_prefix("\n\n") {
            if !missing_rounds.trim().is_empty() {
                return Some(missing_rounds.to_owned());
            }
        }
    }

    Some(turn.answer.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn turn(answer: &str, messages: Vec<aw_agent_core::Message>) -> Turn {
        Turn {
            id: 1,
            prompt: "hello".into(),
            answer: answer.into(),
            model: "fixture".into(),
            profile: "fixture".into(),
            outcome: aw_agent_core::conversation::Outcome::Completed,
            meta: None,
            error: None,
            actor: None,
            tool_records: Vec::new(),
            invocations: Vec::new(),
            context_messages: messages,
            needs_reconciliation: false,
        }
    }

    #[test]
    fn safe_text_preserves_unicode_and_exposes_controls() {
        assert_eq!(safe_text("héllo 🐦\n\t世界"), "héllo 🐦\n\t世界");
        let escaped = safe_text("ok\u{1b}]52;c;secret\u{7}");
        assert!(!escaped.contains('\u{1b}'));
        assert!(!escaped.contains('\u{7}'));
        assert!(escaped.contains("\\u{1b}"));
        let bidi = safe_text("left\u{202e}right");
        assert!(!bidi.contains('\u{202e}'));
        assert!(bidi.contains("\\u{202e}"));
    }

    #[test]
    fn tail_text_stays_on_utf8_boundary() {
        assert_eq!(tail_text("ab世界", 4), "界");
        assert_eq!(tail_text("short", 99), "short");
    }

    #[test]
    fn tool_presentation_is_sanitized_and_semantic() {
        let record = |status, command: &str| ToolRecord {
            call: aw_agent_core::ToolCall {
                id: "call-1".into(),
                name: "bash".into(),
                arguments: serde_json::json!({"command": command}),
            },
            effect: aw_agent_core::ToolEffect::Process,
            execution_site: "local".into(),
            status,
            output: None,
        };
        let running = record(ToolStatus::Running, "printf '\u{1b}]52;bad'");
        assert_eq!(tool_tone(Some(&running)), ToolTone::Neutral);
        assert!(!tool_title(&running).contains('\u{1b}'));

        let succeeded = record(ToolStatus::Succeeded, "true");
        assert_eq!(tool_tone(Some(&succeeded)), ToolTone::Success);

        let failed = record(ToolStatus::Failed, "false");
        assert_eq!(tool_tone(Some(&failed)), ToolTone::Failure);
        assert_eq!(tool_tone(None), ToolTone::Neutral);
    }

    #[test]
    fn settled_answer_fallback_covers_complete_partial_and_stale_traces() {
        let user = aw_agent_core::Message::text(1024, Role::User, "hello");

        let mut first = aw_agent_core::Message::text(1025, Role::Assistant, "checking");
        first.tool_calls.push(aw_agent_core::ToolCall {
            id: "call-1".into(),
            name: "read".into(),
            arguments: serde_json::json!({"path":"README.md"}),
        });
        let tool = {
            let mut message = aw_agent_core::Message::text(1026, Role::Tool, "result");
            message.tool_call_id = Some("call-1".into());
            message
        };
        let mut empty_tool_round = aw_agent_core::Message::text(1027, Role::Assistant, "");
        empty_tool_round.tool_calls.push(aw_agent_core::ToolCall {
            id: "call-2".into(),
            name: "read".into(),
            arguments: serde_json::json!({"path":"Cargo.toml"}),
        });
        let second_tool = {
            let mut message = aw_agent_core::Message::text(1028, Role::Tool, "result");
            message.tool_call_id = Some("call-2".into());
            message
        };
        let final_message = aw_agent_core::Message::text(1029, Role::Assistant, "done");

        let complete = turn(
            "checking\n\ndone",
            vec![
                user.clone(),
                first.clone(),
                tool.clone(),
                empty_tool_round,
                second_tool,
                final_message,
            ],
        );
        assert_eq!(settled_answer_fallback(&complete), None);

        let partial_prior_round = turn("checking\n\ndone", vec![user.clone(), first, tool]);
        assert_eq!(
            settled_answer_fallback(&partial_prior_round).as_deref(),
            Some("done")
        );

        let partial_same_round = turn(
            "hello world",
            vec![
                user.clone(),
                aw_agent_core::Message::text(1025, Role::Assistant, "hello"),
            ],
        );
        assert_eq!(
            settled_answer_fallback(&partial_same_round).as_deref(),
            Some("hello world")
        );

        let partial_suffix = turn(
            "hello world",
            vec![
                user.clone(),
                aw_agent_core::Message::text(1025, Role::Assistant, "world"),
            ],
        );
        assert_eq!(
            settled_answer_fallback(&partial_suffix).as_deref(),
            Some("hello world")
        );

        let stale = turn(
            "authoritative",
            vec![
                user,
                aw_agent_core::Message::text(1025, Role::Assistant, "stale"),
            ],
        );
        assert_eq!(
            settled_answer_fallback(&stale).as_deref(),
            Some("authoritative")
        );
    }
}
