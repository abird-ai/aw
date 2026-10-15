use aw::conversation::Turn;
use std::time::Duration;

/// Raw measured performance data. Formatting/projection belongs to the built-in
/// performance UI extension so other frontends/extension hosts receive the
/// same semantic data without duplicating TPS/TTFT presentation logic.
#[derive(Clone, Debug, Default)]
pub struct PerformanceStats {
    pub elapsed: Duration,
    pub first_token: Option<Duration>,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    /// True only when elapsed/first-token timing belongs to one model round
    /// with no tool execution interleaved. Multi-round Agent wall time is not
    /// model generation throughput.
    pub throughput_eligible: bool,
}

impl PerformanceStats {
    pub fn generation_duration(&self) -> Option<Duration> {
        if !self.throughput_eligible {
            return None;
        }
        let first = self.first_token?;
        let generation = self.elapsed.saturating_sub(first);
        (!generation.is_zero()).then_some(generation)
    }

    pub fn output_tps(&self) -> Option<f64> {
        let output = self.output_tokens?;
        let generation = self.generation_duration()?;
        Some(output as f64 / generation.as_secs_f64())
    }
}

#[cfg(any(feature = "durable", test))]
#[derive(Clone, Debug, Default)]
pub struct SessionPerformance {
    output_tokens: u64,
    generation: Duration,
    measured_turns: u64,
}

#[cfg(any(feature = "durable", test))]
impl SessionPerformance {
    pub fn record(&mut self, stats: &PerformanceStats) {
        let Some(output) = stats.output_tokens else {
            return;
        };
        let Some(generation) = stats.generation_duration() else {
            return;
        };
        self.output_tokens = self.output_tokens.saturating_add(output);
        self.generation = self.generation.saturating_add(generation);
        self.measured_turns = self.measured_turns.saturating_add(1);
    }

    pub fn average_output_tps(&self) -> Option<f64> {
        (self.measured_turns > 0 && !self.generation.is_zero()).then(|| {
            self.output_tokens as f64 / self.generation.as_secs_f64()
        })
    }

    pub fn clear(&mut self) {
        *self = Self::default();
    }
}

fn total_tokens(
    turn: &Turn,
    field: impl Fn(&aw_agent_core::CompletionMeta) -> Option<u64>,
) -> Option<u64> {
    let mut total = 0u64;
    for invocation in &turn.invocations {
        let meta = invocation.meta.as_ref()?;
        total = total.checked_add(field(meta)?)?;
    }
    Some(total)
}

impl PerformanceStats {
    pub fn from_turn(turn: &Turn, elapsed: Duration, first_token: Option<Duration>) -> Self {
        Self {
            elapsed,
            first_token,
            input_tokens: total_tokens(turn, |meta| meta.usage.input_tokens),
            output_tokens: total_tokens(turn, |meta| meta.usage.output_tokens),
            throughput_eligible: turn.invocations.len() == 1 && turn.tool_records.is_empty(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_stats_are_plain_measurements() {
        let stats = PerformanceStats {
            elapsed: Duration::from_secs(2),
            first_token: Some(Duration::from_millis(500)),
            input_tokens: Some(2_000),
            output_tokens: Some(100),
            throughput_eligible: true,
        };
        assert_eq!(stats.elapsed, Duration::from_secs(2));
        assert_eq!(stats.first_token, Some(Duration::from_millis(500)));
        assert_eq!(stats.input_tokens, Some(2_000));
        assert_eq!(stats.output_tokens, Some(100));
    }

    #[test]
    fn session_average_uses_only_measured_generation_time() {
        let mut session = SessionPerformance::default();
        session.record(&PerformanceStats {
            elapsed: Duration::from_secs(2),
            first_token: Some(Duration::from_secs(1)),
            output_tokens: Some(100),
            throughput_eligible: true,
            ..PerformanceStats::default()
        });
        session.record(&PerformanceStats {
            elapsed: Duration::from_secs(3),
            first_token: Some(Duration::from_secs(1)),
            output_tokens: Some(100),
            throughput_eligible: true,
            ..PerformanceStats::default()
        });
        assert_eq!(session.average_output_tps(), Some(200.0 / 3.0));

        session.record(&PerformanceStats {
            elapsed: Duration::from_secs(30),
            first_token: Some(Duration::from_secs(1)),
            output_tokens: Some(1_000),
            throughput_eligible: false,
            ..PerformanceStats::default()
        });
        assert_eq!(
            session.average_output_tps(),
            Some(200.0 / 3.0),
            "tool/multi-round wall time must not pollute model throughput"
        );

        session.clear();
        assert_eq!(session.average_output_tps(), None);
    }
}
