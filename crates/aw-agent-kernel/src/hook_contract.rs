//! Exactly two hook verbs and ten typed stages. This pure vocabulary is
//! shared by hosted Rust and no_std ports; concrete hooks are host services.
use core::marker::PhantomData;
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Stage {
    Turn,
    Step,
    Input,
    Prompt,
    Message,
    Output,
    Tool,
    Signal,
    Event,
    Lifecycle,
}
impl Stage {
    pub const fn observe_only(self) -> bool {
        matches!(self, Self::Event | Self::Lifecycle)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Capability {
    Observe,
    Transform,
    Gate,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    InvalidCapability,
    StaleContinuation,
    Depth,
    Timeout,
    Cancelled,
    Denied,
    InvalidMessage,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Continuation {
    pub owner: super::actor::ActorKey,
    pub id: core::num::NonZeroU64,
    pub generation: core::num::NonZeroU64,
    pub envelope_revision: core::num::NonZeroU64,
}
/// Next(Some(m)) can observe/edit/replace; Next(None) consumes. Defer parks and
/// resumes exactly once via the bounded Runtime. No third branch/decision verb.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HookFlow<M> {
    Next(Option<M>),
    Defer(Continuation),
}
pub type HookOutcome<M> = core::result::Result<HookFlow<M>, Error>;

/// Stage admission is pure and fails closed: observe-only stages can never
/// receive a transform/gate capability. Host-specific guards are additional.
pub fn authorize_stage(stage: Stage, capability: Capability) -> Result<(), Error> {
    if stage.observe_only() && capability != Capability::Observe {
        Err(Error::InvalidCapability)
    } else {
        Ok(())
    }
}
/// Lifetime marker for a typed stage projection; no callbacks stored in core.
pub struct StageView<'a, M> {
    pub stage: Stage,
    pub message: &'a M,
    _borrow: PhantomData<&'a M>,
}
impl<'a, M> StageView<'a, M> {
    pub fn new(stage: Stage, message: &'a M) -> Self {
        Self {
            stage,
            message,
            _borrow: PhantomData,
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ten_stages_and_no_untrusted_lifecycle_rewrite() {
        let stages = [
            Stage::Turn,
            Stage::Step,
            Stage::Input,
            Stage::Prompt,
            Stage::Message,
            Stage::Output,
            Stage::Tool,
            Stage::Signal,
            Stage::Event,
            Stage::Lifecycle,
        ];
        assert_eq!(stages.len(), 10);
        for stage in stages {
            assert_eq!(
                authorize_stage(stage, Capability::Transform).is_ok(),
                !stage.observe_only()
            );
        }
        let m = "text";
        assert_eq!(StageView::new(Stage::Input, &m).message, &m);
        let next: HookFlow<&str> = HookFlow::Next(None);
        assert_eq!(next, HookFlow::Next(None));
    }
}
