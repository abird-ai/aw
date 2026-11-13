//! Small host-neutral Job event/wake value types.
//!
//! Sources own event revisions and rule generations. These values carry no
//! authority by themselves and never identify an AW Session or Agent.

use serde::{Deserialize, Serialize};

pub const MAX_EVENT_RULES: usize = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobEventKind {
    Result,
    Settled,
    Attention,
}

impl JobEventKind {
    pub const ALL: [Self; 3] = [Self::Result, Self::Settled, Self::Attention];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Result => "result",
            Self::Settled => "settled",
            Self::Attention => "attention",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobWakeMode {
    Auto,
    Queue,
    Steer,
    Interrupt,
}

impl JobWakeMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Queue => "queue",
            Self::Steer => "steer",
            Self::Interrupt => "interrupt",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EventRule {
    pub event: JobEventKind,
    pub mode: JobWakeMode,
}

impl EventRule {
    pub const fn new(event: JobEventKind, mode: JobWakeMode) -> Self {
        Self { event, mode }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct EventRuleSet(Vec<EventRule>);

impl EventRuleSet {
    pub fn new<I>(rules: I) -> Result<Self, &'static str>
    where
        I: IntoIterator<Item = EventRule>,
    {
        let mut rules = rules.into_iter().collect::<Vec<_>>();
        if rules.len() > MAX_EVENT_RULES {
            return Err("too many Job event rules");
        }
        rules.sort_by_key(|rule| rule.event);
        if rules.windows(2).any(|pair| pair[0].event == pair[1].event) {
            return Err("duplicate Job event rule");
        }
        Ok(Self(rules))
    }

    pub fn as_slice(&self) -> &[EventRule] {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn get(&self, event: JobEventKind) -> Option<JobWakeMode> {
        self.0
            .binary_search_by_key(&event, |rule| rule.event)
            .ok()
            .map(|index| self.0[index].mode)
    }
}

/// Immutable source-side event delivery identity. The receiver must revalidate
/// its own current wake/Input authority before admission.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JobEventDelivery {
    pub source_ref: String,
    pub event: JobEventKind,
    pub event_rev: u64,
    pub recipient_ref: String,
    /// Opaque host-issued receiver admission identity captured when the source
    /// rule was registered. Sources do not interpret this value.
    pub recipient_authority_ref: String,
    pub subscription_generation: u64,
    pub mode: JobWakeMode,
}

impl JobEventDelivery {
    pub fn validate(&self) -> Result<(), &'static str> {
        let bounded = |value: &str| {
            !value.is_empty()
                && value.len() <= 2048
                && value.is_ascii()
                && !value.chars().any(char::is_control)
        };
        if !bounded(&self.source_ref)
            || !bounded(&self.recipient_ref)
            || !bounded(&self.recipient_authority_ref)
            || self.event_rev == 0
            || self.subscription_generation == 0
        {
            return Err("invalid Job event delivery identity");
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SubscriptionUpdate {
    pub generation: u64,
    pub changed: bool,
    pub active: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Subscription {
    generation: u64,
    recipient_authority_ref: String,
    rules: EventRuleSet,
}

/// Pure source-side rule/event revision state. Callers serialize this with the
/// authoritative source transition; this type performs no I/O or authorization.
#[derive(Clone, Debug, Default)]
pub struct SourceEventBook {
    next_generation: u64,
    next_event_rev: u64,
    subscriptions: std::collections::BTreeMap<String, Subscription>,
}

impl SourceEventBook {
    pub fn replace(
        &mut self,
        recipient_ref: &str,
        recipient_authority_ref: &str,
        rules: EventRuleSet,
    ) -> Result<SubscriptionUpdate, &'static str> {
        if recipient_ref.is_empty()
            || recipient_ref.len() > 2048
            || !recipient_ref.is_ascii()
            || recipient_ref.chars().any(char::is_control)
            || recipient_authority_ref.is_empty()
            || recipient_authority_ref.len() > 2048
            || !recipient_authority_ref.is_ascii()
            || recipient_authority_ref.chars().any(char::is_control)
        {
            return Err("invalid Job event recipient admission");
        }
        if let Some(existing) = self.subscriptions.get(recipient_ref) {
            if existing.rules == rules
                && existing.recipient_authority_ref == recipient_authority_ref
            {
                return Ok(SubscriptionUpdate {
                    generation: existing.generation,
                    changed: false,
                    active: !rules.is_empty(),
                });
            }
        } else if rules.is_empty() {
            return Ok(SubscriptionUpdate {
                generation: self.next_generation,
                changed: false,
                active: false,
            });
        }
        self.next_generation = self
            .next_generation
            .checked_add(1)
            .ok_or("Job event subscription generation exhausted")?;
        let generation = self.next_generation;
        if rules.is_empty() {
            self.subscriptions.remove(recipient_ref);
        } else {
            self.subscriptions.insert(
                recipient_ref.to_owned(),
                Subscription {
                    generation,
                    recipient_authority_ref: recipient_authority_ref.to_owned(),
                    rules,
                },
            );
        }
        Ok(SubscriptionUpdate {
            generation,
            changed: true,
            active: self
                .subscriptions
                .get(recipient_ref)
                .is_some_and(|subscription| subscription.generation == generation),
        })
    }

    /// Commit one factual source checkpoint. All events in the slice share one
    /// event_rev so the receiver can coalesce result+settled when their effective
    /// modes agree.
    pub fn commit(
        &mut self,
        source_ref: &str,
        events: &[JobEventKind],
    ) -> Result<Vec<JobEventDelivery>, &'static str> {
        if events.is_empty() {
            return Ok(Vec::new());
        }
        if source_ref.is_empty()
            || source_ref.len() > 2048
            || !source_ref.is_ascii()
            || source_ref.chars().any(char::is_control)
        {
            return Err("invalid Job event source ref");
        }
        let mut canonical = events.to_vec();
        canonical.sort();
        canonical.dedup();
        self.next_event_rev = self
            .next_event_rev
            .checked_add(1)
            .ok_or("Job event revision exhausted")?;
        let event_rev = self.next_event_rev;
        let mut out = Vec::new();
        for (recipient_ref, subscription) in &self.subscriptions {
            for event in &canonical {
                if let Some(mode) = subscription.rules.get(*event) {
                    let delivery = JobEventDelivery {
                        source_ref: source_ref.to_owned(),
                        event: *event,
                        event_rev,
                        recipient_ref: recipient_ref.clone(),
                        recipient_authority_ref: subscription.recipient_authority_ref.clone(),
                        subscription_generation: subscription.generation,
                        mode,
                    };
                    delivery.validate()?;
                    out.push(delivery);
                }
            }
        }
        Ok(out)
    }

    pub fn generation_for(&self, recipient_ref: &str) -> Option<u64> {
        self.subscriptions
            .get(recipient_ref)
            .map(|subscription| subscription.generation)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_rules_are_unique_bounded_and_canonical() {
        let set = EventRuleSet::new([
            EventRule::new(JobEventKind::Attention, JobWakeMode::Steer),
            EventRule::new(JobEventKind::Result, JobWakeMode::Queue),
            EventRule::new(JobEventKind::Settled, JobWakeMode::Auto),
        ])
        .unwrap();
        assert_eq!(
            set.as_slice()
                .iter()
                .map(|rule| rule.event)
                .collect::<Vec<_>>(),
            vec![
                JobEventKind::Result,
                JobEventKind::Settled,
                JobEventKind::Attention
            ]
        );
        assert!(EventRuleSet::new([
            EventRule::new(JobEventKind::Result, JobWakeMode::Queue),
            EventRule::new(JobEventKind::Result, JobWakeMode::Steer),
        ])
        .is_err());
    }

    #[test]
    fn source_event_book_replacement_and_checkpoint_are_generation_fenced() {
        let mut book = SourceEventBook::default();
        let rules = EventRuleSet::new([
            EventRule::new(JobEventKind::Result, JobWakeMode::Queue),
            EventRule::new(JobEventKind::Settled, JobWakeMode::Queue),
        ])
        .unwrap();
        let first = book
            .replace(
                "/d:0/i:0/p:0/a:1",
                "jwa_fixture_generation_1",
                rules.clone(),
            )
            .unwrap();
        assert!(first.changed);
        assert_eq!(
            book.replace("/d:0/i:0/p:0/a:1", "jwa_fixture_generation_1", rules,)
                .unwrap()
                .generation,
            first.generation
        );
        let deliveries = book
            .commit(
                "/d:0/i:0/p:0/a:0/t:1/j:job",
                &[JobEventKind::Settled, JobEventKind::Result],
            )
            .unwrap();
        assert_eq!(deliveries.len(), 2);
        assert_eq!(deliveries[0].event_rev, deliveries[1].event_rev);
        assert_eq!(deliveries[0].subscription_generation, first.generation);
        let cleared = book
            .replace(
                "/d:0/i:0/p:0/a:1",
                "jwa_fixture_generation_1",
                EventRuleSet::default(),
            )
            .unwrap();
        assert!(cleared.changed);
        assert!(book
            .commit("/d:0/i:0/p:0/a:0/t:1/j:job", &[JobEventKind::Settled],)
            .unwrap()
            .is_empty());
    }
}
