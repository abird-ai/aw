//! AW-P6-8B: versioned, strictly bounded *presentation-only* status snapshots
//! and event notices over the original UI extension Registry and semantic Span.
//! Not an Agent Message, TurnJournal event, Hook, queue, or execution grant.
use super::{Context, Contribution, Line, Registry, RegistryError, Request, Response, Slot};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const STATUS_PROTOCOL: u32 = 2;
pub const MAX_STATUS_ROWS: usize = 8;
pub const MAX_EVENT_ROWS: usize = 8;
pub const MAX_NOTICES: usize = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    StepEnd,
    TurnEnd,
    FinalOutput,
}
impl EventKind {
    pub const fn slot(self) -> Slot {
        match self {
            Self::StepEnd => Slot::AfterStep,
            Self::TurnEnd => Slot::AfterTurn,
            Self::FinalOutput => Slot::AfterFinalOutput,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EventContext {
    pub protocol_version: u32,
    pub kind: EventKind,
    pub session: String,
    pub turn_id: u64,
    pub sequence: u64,
    /// The original host supplies a sanitized metadata-only Context. Never
    /// put raw provider continuations, user prompt or tool bodies here.
    pub context: Context,
}
fn clean_metadata(value: &str, max: usize) -> bool {
    value.len() <= max
        && !value.chars().any(|c| {
            c.is_control() || matches!(c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
        })
}
impl EventContext {
    pub fn validate(&self) -> Result<(), RegistryError> {
        if self.protocol_version != STATUS_PROTOCOL
            || self.turn_id == 0
            || self.sequence == 0
            || self.session.is_empty()
            || self.session.len() > 128
            || !self
                .session
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"_-:.@".contains(&c))
            || self.context.session != self.session
            || !clean_metadata(&self.context.model, 256)
            || !clean_metadata(&self.context.profile, 256)
            || !clean_metadata(&self.context.provider, 256)
            || !clean_metadata(&self.context.effort, 256)
            || !clean_metadata(&self.context.directory, 1024)
            || !clean_metadata(&self.context.agent, 256)
            || !clean_metadata(&self.context.branch, 256)
            || !clean_metadata(&self.context.tools, 256)
            || !clean_metadata(&self.context.state, 256)
            || self
                .context
                .session_tps
                .is_some_and(|v| !v.is_finite() || v < 0.0)
            || self.context.performance.as_ref().is_some_and(|perf| {
                [
                    perf.elapsed_secs,
                    perf.first_token_secs.unwrap_or(0.0),
                    perf.output_tps.unwrap_or(0.0),
                ]
                .into_iter()
                .any(|v| !v.is_finite() || v < 0.0)
            })
        {
            return Err(RegistryError::InvalidHostResponse);
        }
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EventNotice {
    pub kind: EventKind,
    pub session: String,
    pub turn_id: u64,
    pub sequence: u64,
    pub lines: Vec<Line>,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StatusSnapshot {
    pub version: u64,
    pub regions: BTreeMap<Slot, Vec<Line>>,
}
/// Cached projection that increments a local monotonic version ONLY if the
/// bounded semantic status snapshot actually changes. Persisted UiConfig
/// remains the original atomic per-field CAS source of row layout data.
#[derive(Default)]
pub struct StatusSurface {
    snapshot: StatusSnapshot,
}
impl StatusSurface {
    pub fn snapshot(&self) -> &StatusSnapshot {
        &self.snapshot
    }
    pub fn refresh(
        &mut self,
        registry: &Registry,
        context: &Context,
    ) -> Result<&StatusSnapshot, RegistryError> {
        let mut regions = BTreeMap::new();
        for slot in [
            Slot::AboveComposer,
            Slot::BelowComposer,
            Slot::FullscreenStatus,
            Slot::LeftStatus,
            Slot::RightStatus,
            Slot::BelowStatus,
        ] {
            let lines = registry.project(slot, context)?;
            if lines.len() > MAX_STATUS_ROWS {
                return Err(RegistryError::InvalidContribution);
            }
            if !lines.is_empty() {
                regions.insert(slot, lines);
            }
        }
        if self.snapshot.version == 0 || self.snapshot.regions != regions {
            self.snapshot.version = self
                .snapshot
                .version
                .checked_add(1)
                .ok_or(RegistryError::InvalidContribution)?;
            self.snapshot.regions = regions;
        }
        Ok(&self.snapshot)
    }
}
/// Session-local, non-durable UI notice rail. No provider/tool/Actor calls.
/// Original factual journal remains authoritative; restart does not replay
/// notices or inject text into the assistant conversation.
#[derive(Clone, Debug)]
pub struct EventRail {
    session: String,
    last_sequence: u64,
    notices: Vec<EventNotice>,
}
impl EventRail {
    pub fn new(session: &str) -> Result<Self, RegistryError> {
        let empty = EventContext {
            protocol_version: STATUS_PROTOCOL,
            kind: EventKind::StepEnd,
            session: session.into(),
            turn_id: 1,
            sequence: 1,
            context: Context {
                session: session.into(),
                ..Context::default()
            },
        };
        empty.validate()?;
        Ok(Self {
            session: session.into(),
            last_sequence: 0,
            notices: Vec::new(),
        })
    }
    pub fn session(&self) -> &str {
        &self.session
    }
    pub fn notices(&self) -> &[EventNotice] {
        &self.notices
    }
    pub fn last_sequence(&self) -> u64 {
        self.last_sequence
    }
    pub fn publish(
        &mut self,
        registry: &Registry,
        event: &EventContext,
    ) -> Result<Option<&EventNotice>, RegistryError> {
        event.validate()?;
        if event.session != self.session || event.sequence <= self.last_sequence {
            return Err(RegistryError::InvalidHostResponse);
        }
        // Validate the entire contribution BEFORE advancing the cursor.
        let lines = registry.event_lines(event)?;
        self.last_sequence = event.sequence;
        if lines.is_empty() {
            return Ok(None);
        }
        self.notices.push(EventNotice {
            kind: event.kind,
            session: event.session.clone(),
            turn_id: event.turn_id,
            sequence: event.sequence,
            lines,
        });
        if self.notices.len() > MAX_NOTICES {
            self.notices.remove(0);
        }
        Ok(self.notices.last())
    }
    pub fn clear(&mut self) {
        self.notices.clear();
    }
}
impl Registry {
    /// Event processing uses v2-only already registered extension hosts.
    /// No arbitrary model/RPC extension registration, no persistence write.
    pub fn event_lines(&self, event: &EventContext) -> Result<Vec<Line>, RegistryError> {
        event.validate()?;
        let slot = event.kind.slot();
        let mut contributions = Vec::<Contribution>::new();
        for ext in &self.extensions {
            if ext.manifest.protocol_version != STATUS_PROTOCOL
                || !ext.manifest.slots.contains(&slot)
            {
                continue;
            }
            let answer = ext.host.request(Request::Event {
                event: Box::new(event.clone()),
            })?;
            let Response::Event {
                contributions: rows,
            } = answer
            else {
                return Err(RegistryError::InvalidHostResponse);
            };
            for row in rows {
                if row.extension != ext.manifest.id
                    || row.slot != slot
                    || row.lines.len() > MAX_EVENT_ROWS
                    || !valid_notice_lines(&row.lines)
                {
                    return Err(RegistryError::InvalidContribution);
                }
                contributions.push(row);
            }
        }
        contributions.sort_by(|a, b| {
            a.order
                .cmp(&b.order)
                .then_with(|| a.extension.cmp(&b.extension))
        });
        let lines = contributions
            .into_iter()
            .flat_map(|row| row.lines)
            .filter(|line| !line.is_empty())
            .collect::<Vec<_>>();
        if lines.len() > MAX_EVENT_ROWS {
            return Err(RegistryError::InvalidContribution);
        }
        Ok(lines)
    }
}
fn valid_notice_lines(lines: &[Line]) -> bool {
    lines.iter().all(|line| {
        line.spans.len() <= super::MAX_SPANS_PER_LINE
            && line.spans.iter().all(|span| {
                span.text.len() <= super::MAX_TEXT_BYTES
                    && clean_metadata(&span.text, super::MAX_TEXT_BYTES)
            })
    })
}

#[cfg(test)]
mod tests {
    use super::super::{Manifest, Span, Tone, UiExtension};
    use super::*;
    use std::sync::Arc;
    #[derive(Clone)]
    struct StatusExt {
        id: &'static str,
        version: u32,
        slot: Slot,
        order: i16,
        bad: bool,
    }
    impl UiExtension for StatusExt {
        fn manifest(&self) -> Manifest {
            Manifest {
                id: self.id.into(),
                protocol_version: self.version,
                slots: vec![self.slot],
                settings: Vec::new(),
            }
        }
        fn project(&self, slot: Slot, context: &Context) -> Vec<Contribution> {
            vec![Contribution {
                extension: self.id.into(),
                slot,
                order: self.order,
                lines: vec![Line::plain(
                    format!("{}={}", self.id, context.state),
                    Tone::State,
                )],
            }]
        }
        fn event(&self, event: &EventContext) -> Vec<Contribution> {
            vec![Contribution {
                extension: self.id.into(),
                slot: event.kind.slot(),
                order: self.order,
                lines: vec![Line {
                    spans: vec![Span::text(
                        if self.bad {
                            "bad\u{001b}ui".into()
                        } else {
                            format!("{}: turn {}", self.id, event.turn_id)
                        },
                        Tone::Normal,
                    )],
                }],
            }]
        }
    }
    fn fixture(session: &str, sequence: u64, kind: EventKind) -> EventContext {
        EventContext {
            protocol_version: STATUS_PROTOCOL,
            kind,
            session: session.into(),
            turn_id: 7,
            sequence,
            context: Context {
                session: session.into(),
                state: "ready".into(),
                ..Context::default()
            },
        }
    }
    #[test]
    fn aw_p6_8b_status_versions_change_only_on_actual_semantic_projection() {
        let reg = Registry::new([
            Arc::new(StatusExt {
                id: "left",
                version: 2,
                slot: Slot::LeftStatus,
                order: 5,
                bad: false,
            }) as Arc<dyn UiExtension>,
            Arc::new(StatusExt {
                id: "right",
                version: 2,
                slot: Slot::RightStatus,
                order: 1,
                bad: false,
            }) as Arc<dyn UiExtension>,
            Arc::new(StatusExt {
                id: "below",
                version: 2,
                slot: Slot::BelowStatus,
                order: 10,
                bad: false,
            }) as Arc<dyn UiExtension>,
        ])
        .unwrap();
        let mut surface = StatusSurface::default();
        let mut context = Context {
            state: "idle".into(),
            ..Context::default()
        };
        let first = surface.refresh(&reg, &context).unwrap();
        assert_eq!(first.version, 1);
        assert_eq!(
            first.regions[&Slot::LeftStatus][0].spans[0].text,
            "left=idle"
        );
        assert_eq!(
            first.regions[&Slot::RightStatus][0].spans[0].text,
            "right=idle"
        );
        assert_eq!(
            first.regions[&Slot::BelowStatus][0].spans[0].text,
            "below=idle"
        );
        assert_eq!(
            surface.refresh(&reg, &context).unwrap().version,
            1,
            "idle render cannot advance status revision or create layout noise"
        );
        context.state = "busy".into();
        assert_eq!(surface.refresh(&reg, &context).unwrap().version, 2);
        let json = format!("{:?}", surface.snapshot());
        assert!(json.contains("LeftStatus") && json.contains("RightStatus"));
        assert!(json.contains("version: 2"));
    }
    #[test]
    fn aw_p6_8b_event_rail_monotonic_bounded_not_journal_or_assistant_output() {
        let reg = Registry::new([
            Arc::new(StatusExt {
                id: "later",
                version: 2,
                slot: Slot::AfterTurn,
                order: 20,
                bad: false,
            }) as Arc<dyn UiExtension>,
            Arc::new(StatusExt {
                id: "first",
                version: 2,
                slot: Slot::AfterTurn,
                order: 1,
                bad: false,
            }) as Arc<dyn UiExtension>,
            Arc::new(StatusExt {
                id: "step",
                version: 2,
                slot: Slot::AfterStep,
                order: 0,
                bad: false,
            }) as Arc<dyn UiExtension>,
            Arc::new(StatusExt {
                id: "final",
                version: 2,
                slot: Slot::AfterFinalOutput,
                order: 0,
                bad: false,
            }) as Arc<dyn UiExtension>,
        ])
        .unwrap();
        let mut rail = EventRail::new("s_1").unwrap();
        let first = rail
            .publish(&reg, &fixture("s_1", 1, EventKind::StepEnd))
            .unwrap()
            .unwrap();
        assert_eq!(first.kind, EventKind::StepEnd);
        assert_eq!(first.lines[0].spans[0].text, "step: turn 7");
        assert!(rail
            .publish(&reg, &fixture("s_1", 1, EventKind::TurnEnd))
            .is_err());
        assert!(rail
            .publish(&reg, &fixture("other", 2, EventKind::TurnEnd))
            .is_err());
        let notice = rail
            .publish(&reg, &fixture("s_1", 2, EventKind::TurnEnd))
            .unwrap()
            .unwrap();
        assert_eq!(
            notice
                .lines
                .iter()
                .map(|row| row.spans[0].text.as_str())
                .collect::<Vec<_>>(),
            vec!["first: turn 7", "later: turn 7"]
        );
        for i in 3..45 {
            rail.publish(&reg, &fixture("s_1", i, EventKind::FinalOutput))
                .unwrap();
        }
        assert_eq!(rail.notices().len(), MAX_NOTICES);
        assert_eq!(rail.last_sequence(), 44);
        assert!(rail.notices().iter().all(|n| n.sequence >= 13));
        assert!(!format!("{:?}", rail.notices()).contains("assistant_response"));
        rail.clear();
        assert!(rail.notices().is_empty());
        assert!(
            rail.publish(&reg, &fixture("s_1", 44, EventKind::FinalOutput))
                .is_err(),
            "clearing presentation does not reset replay sequence"
        );
    }
    #[test]
    fn aw_p6_8b_v1_registry_is_unchanged_and_v2_events_are_sanitized() {
        assert!(Registry::new([Arc::new(StatusExt {
            id: "illegal_v1",
            version: 1,
            slot: Slot::LeftStatus,
            order: 0,
            bad: false
        }) as Arc<dyn UiExtension>,])
        .is_err());
        let old = Registry::new([Arc::new(StatusExt {
            id: "old",
            version: 1,
            slot: Slot::AfterTurn,
            order: 0,
            bad: false,
        }) as Arc<dyn UiExtension>])
        .unwrap();
        assert!(old
            .event_lines(&fixture("s_1", 1, EventKind::TurnEnd))
            .unwrap()
            .is_empty());
        let hostile = Registry::new([Arc::new(StatusExt {
            id: "hostile",
            version: 2,
            slot: Slot::AfterTurn,
            order: 0,
            bad: true,
        }) as Arc<dyn UiExtension>])
        .unwrap();
        assert!(
            hostile
                .event_lines(&fixture("s_1", 2, EventKind::TurnEnd))
                .is_err(),
            "escape/control characters cannot be rendered in TUI/browser"
        );
        let mut invalid = fixture("s_1", 1, EventKind::TurnEnd);
        invalid.protocol_version = 0;
        assert!(EventRail::new("s_1")
            .unwrap()
            .publish(&old, &invalid)
            .is_err());
        let mut cross = fixture("s_1", 2, EventKind::FinalOutput);
        cross.context.session = "forged_session".into();
        assert!(
            cross.validate().is_err(),
            "event context cannot spoof another Session"
        );
        assert!(EventRail::new("s_1")
            .unwrap()
            .publish(&old, &cross)
            .is_err());
        let mut unsafe_context = fixture("s_1", 3, EventKind::TurnEnd);
        unsafe_context.context.model = "bad\u{202e}bidi".into();
        assert!(
            unsafe_context.validate().is_err(),
            "model label bidi spoofing is not UI metadata"
        );
        let mut too_long = fixture("s_1", 4, EventKind::StepEnd);
        too_long.context.directory = "/".repeat(1300);
        assert!(
            too_long.validate().is_err(),
            "metadata size bound enforced before extension receives context"
        );
    }
}
