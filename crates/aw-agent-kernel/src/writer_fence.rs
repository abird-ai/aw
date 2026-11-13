//! Portable writer-generation evidence. A fence is an identity/check token,
//! never a grant. Durable or multi-writer owners require a non-zero epoch.

use core::num::NonZeroU64;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum WriterFence {
    #[default]
    NotApplicable,
    Epoch(NonZeroU64),
}

impl WriterFence {
    pub const fn epoch(value: NonZeroU64) -> Self {
        Self::Epoch(value)
    }

    pub const fn as_epoch(self) -> Option<NonZeroU64> {
        match self {
            Self::NotApplicable => None,
            Self::Epoch(value) => Some(value),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_cannot_be_a_writer_epoch() {
        assert!(NonZeroU64::new(0).is_none());
        let epoch = NonZeroU64::new(7).unwrap();
        assert_eq!(WriterFence::epoch(epoch).as_epoch(), Some(epoch));
        assert_eq!(WriterFence::NotApplicable.as_epoch(), None);
    }
}
