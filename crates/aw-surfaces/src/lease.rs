//! Memory-scoped terminal input ownership. Input acknowledgements mean queued,
//! not executed, and are not durable after owner failure.
use crate::{Error, Result};
use serde_json::{json, Value};
use std::time::{Duration, Instant};
pub(crate) const TTL: Duration = Duration::from_secs(15);
pub(crate) struct Lease {
    pub client: String,
    token: String,
    expires: Instant,
    next: u64,
    last: Option<(u64, [u8; 32])>,
}
impl Lease {
    pub fn new(client: String, token: String, now: Instant) -> Self {
        Self {
            client,
            token,
            expires: now + TTL,
            next: 1,
            last: None,
        }
    }
    pub fn expired(&self, now: Instant) -> bool {
        now >= self.expires
    }
    pub fn renew(&mut self, now: Instant) {
        self.expires = now + TTL;
    }
    pub fn reply(&self) -> Value {
        json!({"lease":self.token,"next_sequence":self.next,"ttl_seconds":TTL.as_secs()})
    }
    pub fn check(&self, client: &str, token: &str, now: Instant) -> Result<()> {
        if self.expired(now) || client != self.client || token != self.token {
            Err(Error::Conflict(
                "terminal control lease expired or owned by another client",
            ))
        } else {
            Ok(())
        }
    }
    pub fn duplicate(&self, sequence: u64, digest: [u8; 32]) -> Result<bool> {
        if sequence == self.next {
            return Ok(false);
        }
        if self.last == Some((sequence, digest)) {
            return Ok(true);
        }
        Err(Error::Conflict(
            "input sequence changed; do not replay or skip uncertain input",
        ))
    }
    pub fn accept(&mut self, sequence: u64, digest: [u8; 32], now: Instant) -> Result<()> {
        if sequence != self.next {
            return Err(Error::Conflict("input sequence"));
        }
        self.next = self
            .next
            .checked_add(1)
            .ok_or(Error::Limit("input sequence exhausted"))?;
        self.last = Some((sequence, digest));
        self.renew(now);
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn different_client_cannot_write() {
        let n = Instant::now();
        let l = Lease::new("a".into(), "secret".into(), n);
        assert!(l.check("b", "secret", n).is_err());
    }
    #[test]
    fn old_token_denied() {
        let n = Instant::now();
        let l = Lease::new("a".into(), "secret".into(), n);
        assert!(l.check("a", "old", n).is_err());
    }
    #[test]
    fn expiry_boundary() {
        let n = Instant::now();
        let l = Lease::new("a".into(), "secret".into(), n);
        assert!(l.check("a", "secret", n + TTL).is_err());
    }
    #[test]
    fn only_exact_duplicate_is_idempotent() {
        let n = Instant::now();
        let mut l = Lease::new("a".into(), "s".into(), n);
        assert!(!l.duplicate(1, [1; 32]).unwrap());
        l.accept(1, [1; 32], n).unwrap();
        assert!(l.duplicate(1, [1; 32]).unwrap());
        assert!(l.duplicate(1, [2; 32]).is_err());
    }
    #[test]
    fn out_of_order_rejected() {
        let l = Lease::new("a".into(), "s".into(), Instant::now());
        assert!(l.duplicate(2, [1; 32]).is_err());
    }
    #[test]
    fn old_different_sequences_do_not_execute_twice() {
        let n = Instant::now();
        let mut l = Lease::new("a".into(), "s".into(), n);
        l.accept(1, [1; 32], n).unwrap();
        l.accept(2, [2; 32], n).unwrap();
        assert!(l.duplicate(1, [1; 32]).is_err());
    }
}
