//! AW-P6-8B pure host/Sdk bridge between ORIGINAL factual AgentEvent
//! boundaries and bounded v2 presentation-only status/event notices.
//! This does not create an Agent Message, append a TurnJournal event, or
//! expose provider-private continuations or raw text to UI extensions.
use crate::{conversation::Outcome, AgentEvent};
use aw_ui_extension::{
    status_events::{EventContext, EventKind, STATUS_PROTOCOL},
    Context,
};

pub fn factual_event_contexts(
    event: &AgentEvent,
    context: &Context,
    first_sequence: u64,
) -> Vec<EventContext> {
    let kinds: Vec<(EventKind, u64)> = match event {
        AgentEvent::InvocationFinished { turn_id, .. } => vec![(EventKind::StepEnd, *turn_id)],
        AgentEvent::Finished { turn } => {
            let mut kinds = vec![(EventKind::TurnEnd, turn.id)];
            if turn.outcome == Outcome::Completed && !turn.answer.is_empty() {
                kinds.push((EventKind::FinalOutput, turn.id));
            }
            kinds
        }
        _ => Vec::new(),
    };
    kinds
        .into_iter()
        .enumerate()
        .filter_map(|(offset, (kind, turn_id))| {
            let sequence = first_sequence.checked_add(offset as u64)?;
            let event = EventContext {
                protocol_version: STATUS_PROTOCOL,
                kind,
                session: context.session.clone(),
                turn_id,
                sequence,
                context: context.clone(),
            };
            event.validate().ok().map(|()| event)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn aw_p6_8b_original_agent_factual_events_are_presentation_only() {
        let context = Context {
            session: "s_exact".into(),
            state: "ready".into(),
            ..Context::default()
        };
        assert!(factual_event_contexts(
            &AgentEvent::Started {
                turn_id: 1,
                profile: "profile".into(),
                model: "m".into()
            },
            &context,
            1
        )
        .is_empty());
        let finished = AgentEvent::Finished {
            turn: crate::conversation::Turn {
                id: 5,
                prompt: "private_input_never_exposed".into(),
                answer: "private_assistant_body_never_exposed".into(),
                model: "model".into(),
                profile: "fixture".into(),
                outcome: Outcome::Completed,
                meta: None,
                error: None,
                actor: None,
                tool_records: Vec::new(),
                invocations: Vec::new(),
                context_messages: Vec::new(),
                needs_reconciliation: false,
            },
        };
        let notices = factual_event_contexts(&finished, &context, 10);
        assert_eq!(notices.len(), 2);
        assert_eq!(notices[0].kind, EventKind::TurnEnd);
        assert_eq!(notices[1].kind, EventKind::FinalOutput);
        assert_eq!(notices[0].sequence, 10);
        assert_eq!(notices[1].sequence, 11);
        let text = serde_json::to_string(&notices).unwrap();
        assert!(!text.contains("private_input_never_exposed"));
        assert!(!text.contains("private_assistant_body_never_exposed"));
        let mut incomplete = finished;
        if let AgentEvent::Finished { turn } = &mut incomplete {
            turn.outcome = Outcome::Incomplete;
        }
        assert_eq!(
            factual_event_contexts(&incomplete, &context, 12).len(),
            1,
            "incomplete Turn cannot announce factual FinalOutput"
        );
        assert!(factual_event_contexts(&incomplete, &context, u64::MAX).len() == 1);
    }
}
