//! Independent no_std consumer: no std, alloc, TLS, OS or Agent loop.
//! Uses exactly the same authority predicate as hosted aw Agent.
#![no_std]
#![forbid(unsafe_code)]

use aw_agent_kernel::capability::{Capability, Effect, Grant, Source, SourceKind};

pub fn exactly_permits(actor: &str, site: &str, epoch: u64, effect: Effect) -> bool {
    let source = match Source::new(SourceKind::Mcp, "remote-files", epoch) {
        Ok(source) => source,
        Err(_) => return false,
    };
    let capability = match Capability::new("remote_read", source, Effect::Read) {
        Ok(capability) => capability,
        Err(_) => return false,
    };
    let grant = match Grant::new("local:user", "local", capability) {
        Ok(grant) => grant,
        Err(_) => return false,
    };
    let proposed = match Capability::new("remote_read", source, effect) {
        Ok(capability) => capability,
        Err(_) => return false,
    };
    grant.permits(actor, site, &proposed)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn no_std_consumer_cannot_upgrade_grants() {
        assert!(exactly_permits("local:user", "local", 3, Effect::Read));
        assert!(!exactly_permits("local:user", "local", 3, Effect::Write));
        assert!(!exactly_permits("different", "local", 3, Effect::Read));
        assert!(!exactly_permits("local:user", "elsewhere", 3, Effect::Read));
        assert!(!exactly_permits("local:user", "local", 0, Effect::Read));
    }
}
