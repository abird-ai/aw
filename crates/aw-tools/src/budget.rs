//! Zero-cost byte-budget types for semantically different boundaries.

use core::num::NonZeroUsize;

/// Raw payload bytes requested from a retained page.
///
/// This is not a serialized tool/MCP response ceiling.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct RawPageBytes(NonZeroUsize);

impl RawPageBytes {
    pub const fn new(bytes: usize) -> Option<Self> {
        match NonZeroUsize::new(bytes) {
            Some(bytes) => Some(Self(bytes)),
            None => None,
        }
    }

    pub const fn get(self) -> usize {
        self.0.get()
    }
}

/// Serialized model-visible result bytes available for an enclosing response.
///
/// This does not constrain the semantic size requested from a raw page.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct SerializedResultBytes(NonZeroUsize);

impl SerializedResultBytes {
    pub const fn new(bytes: usize) -> Option<Self> {
        match NonZeroUsize::new(bytes) {
            Some(bytes) => Some(Self(bytes)),
            None => None,
        }
    }

    pub const fn get(self) -> usize {
        self.0.get()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn budgets_are_distinct_nonzero_transparent_values() {
        assert_eq!(RawPageBytes::new(1).unwrap().get(), 1);
        assert_eq!(SerializedResultBytes::new(512).unwrap().get(), 512);
        assert!(RawPageBytes::new(0).is_none());
        assert!(SerializedResultBytes::new(0).is_none());
        assert_eq!(
            core::mem::size_of::<RawPageBytes>(),
            core::mem::size_of::<usize>()
        );
        assert_eq!(
            core::mem::size_of::<SerializedResultBytes>(),
            core::mem::size_of::<usize>()
        );
    }
}
