//! Explicit, bounded provider-attempt decisions. Retrying can bill the user
//! twice unless a TRUSTED provider adapter proves the first attempt never
//! dispatched. Transport timeouts, HTTP errors, partial streams and tool
//! effects do not meet that proof and are never automatically replayed.
use crate::{Error, Result};

pub const MAX_PRE_DISPATCH_RETRIES: u8 = 2;
pub const MAX_PROVIDER_ATTEMPTS_PER_TURN: u32 = 32;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RetryPolicy {
    /// Extra attempts beyond the first, not an implicit provider/tool grant.
    /// Zero by default; a trusted host can opt in to max 2.
    pub pre_dispatch_retries: u8,
}
impl RetryPolicy {
    pub fn new(retries: u8) -> Result<Self> {
        if retries > MAX_PRE_DISPATCH_RETRIES {
            return Err(Error::Config(
                "provider pre-dispatch retry ceiling must be 0..2".into(),
            ));
        }
        Ok(Self {
            pre_dispatch_retries: retries,
        })
    }
    /// The ONLY automatic retry predicate. No bytes can have been streamed,
    /// the adapter must explicitly attest BeforeDispatch, and the journaled
    /// attempt/turn budget must not be exceeded.
    pub fn allow(&self, error: &Error, streamed: bool, extra_used: u8, ordinal: u32) -> bool {
        matches!(error, Error::BeforeDispatch(_))
            && !streamed
            && extra_used < self.pre_dispatch_retries
            && ordinal < MAX_PROVIDER_ATTEMPTS_PER_TURN
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ambiguity_cannot_trigger_invisible_retry() {
        let policy = RetryPolicy::new(2).unwrap();
        assert!(policy.allow(&Error::BeforeDispatch("fixture"), false, 0, 1));
        assert!(!policy.allow(&Error::BeforeDispatch("fixture"), true, 0, 1));
        assert!(!policy.allow(&Error::BeforeDispatch("fixture"), false, 2, 1));
        assert!(!policy.allow(&Error::BeforeDispatch("fixture"), false, 0, 32));
        assert!(!policy.allow(&Error::Timeout, false, 0, 1));
        assert!(!policy.allow(
            &Error::Http {
                status: 429,
                hint: "throttle"
            },
            false,
            0,
            1
        ));
        assert!(!policy.allow(&Error::Transport("maybe sent"), false, 0, 1));
        assert!(!policy.allow(&Error::Cancelled, false, 0, 1));
        assert!(!RetryPolicy::default().allow(&Error::BeforeDispatch("fixture"), false, 0, 1));
        assert!(RetryPolicy::new(3).is_err());
    }
}
