//! Owner-side delegated browser read authority.
//!
//! This is intentionally separate from browser authentication and docking
//! correlation. A grant is a bounded, memory-only read capability for one exact
//! recipient and resource. Callers must still revalidate the parent resource
//! authority before publication.

use super::*;
use std::{
    collections::BTreeMap,
    time::{SystemTime, UNIX_EPOCH},
};

const MAX_GRANTS: usize = 128;
const MAX_GRANT_TTL: Duration = Duration::from_secs(10 * 60);

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Resource {
    AgentBranch {
        store_id: String,
        session_id: String,
        branch_id: String,
    },
    // Reserved by the accepted delegated-browser design; AgentBranch is the
    // first integrated resource and does not grant either capability implicitly.
    #[allow(dead_code)]
    File { scope_id: String, path: String },
    #[allow(dead_code)]
    Review { scope_id: String, review_id: String },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum ParentAuthority {
    Session {
        owner_membership_epoch: u64,
        audience_epoch: u64,
    },
    // File/Review parent evidence remains separate from the Agent branch path.
    #[allow(dead_code)]
    Surface { owner_epoch: String },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Binding {
    pub owner: PrincipalId,
    pub recipient: PrincipalId,
    pub source_id: String,
    pub site: String,
    pub resource: Resource,
    pub parent: ParentAuthority,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct IdentityStamp {
    principal: PrincipalId,
    user_id: String,
    security_epoch: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum State {
    Active,
    Revoked { at_unix_ms: u64 },
}

#[derive(Clone, Debug)]
struct Grant {
    id: String,
    generation: u64,
    owner: IdentityStamp,
    recipient: IdentityStamp,
    binding: Binding,
    issued_at_unix_ms: u64,
    expires_at_unix_ms: u64,
    deadline: Instant,
    state: State,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Issued {
    pub id: String,
    pub generation: u64,
    pub issued_at_unix_ms: u64,
    pub expires_at_unix_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Witness {
    grant_id: String,
    generation: u64,
    binding: Binding,
}

impl Witness {
    pub(super) const fn generation(&self) -> u64 {
        self.generation
    }
}

#[derive(Default)]
pub(super) struct Registry {
    grants: BTreeMap<String, Grant>,
    next_generation: u64,
}

fn unix_ms() -> Result<u64> {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| Error::Storage("system clock is before the Unix epoch"))?
        .as_millis();
    u64::try_from(millis)
        .map_err(|_| Error::Limit("system clock value exceeds delegated grant bounds"))
}

fn identity_stamp(store: &SessionStore, principal: &PrincipalId) -> Result<IdentityStamp> {
    // An arbitrary principal presented by a browser must not create a stable
    // user merely by being named at an authorization boundary.
    let user = store.known_user(principal)?;
    if user.status != "active" || user.security_epoch == 0 {
        return Err(Error::Denied(
            "delegated browser identity is inactive or changed",
        ));
    }
    Ok(IdentityStamp {
        principal: principal.clone(),
        user_id: user.user_id,
        security_epoch: user.security_epoch,
    })
}

fn bounded_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_.:-/@".contains(&byte))
}

fn guarded_relative_path(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 4096
        && !value.starts_with('/')
        && !value.contains('\\')
        && !value.chars().any(char::is_control)
        && value
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
}

fn valid_resource(resource: &Resource) -> bool {
    match resource {
        Resource::AgentBranch {
            store_id,
            session_id,
            branch_id,
        } => bounded_id(store_id) && bounded_id(session_id) && bounded_id(branch_id),
        Resource::File { scope_id, path } => bounded_id(scope_id) && guarded_relative_path(path),
        Resource::Review {
            scope_id,
            review_id,
        } => bounded_id(scope_id) && bounded_id(review_id),
    }
}

fn valid_parent(parent: &ParentAuthority) -> bool {
    match parent {
        ParentAuthority::Session {
            owner_membership_epoch,
            audience_epoch,
        } => *owner_membership_epoch > 0 && *audience_epoch > 0,
        ParentAuthority::Surface { owner_epoch } => bounded_id(owner_epoch),
    }
}

impl Registry {
    fn prune_for_admission(&mut self) {
        let now = Instant::now();
        self.grants
            .retain(|_, grant| matches!(grant.state, State::Active) && grant.deadline > now);
    }

    pub(super) fn grant(
        &mut self,
        store: &SessionStore,
        binding: Binding,
        ttl: Duration,
    ) -> Result<Issued> {
        if ttl.is_zero()
            || ttl > MAX_GRANT_TTL
            || !bounded_id(&binding.source_id)
            || !bounded_id(&binding.site)
            || !valid_resource(&binding.resource)
            || !valid_parent(&binding.parent)
        {
            return Err(Error::Denied("invalid delegated browser grant"));
        }
        let owner = identity_stamp(store, &binding.owner)?;
        let recipient = identity_stamp(store, &binding.recipient)?;
        if owner.principal == recipient.principal || owner.user_id == recipient.user_id {
            return Err(Error::Denied(
                "delegated browser grant requires a distinct recipient user",
            ));
        }

        self.prune_for_admission();
        if self.grants.len() >= MAX_GRANTS {
            return Err(Error::Limit("delegated browser grant limit reached"));
        }

        // At most one live grant exists for an exact owner/recipient/resource
        // binding. Reissue is a revoke+new generation, never an in-place refresh.
        let revoked_at = unix_ms()?;
        for grant in self.grants.values_mut() {
            if matches!(grant.state, State::Active)
                && grant.owner == owner
                && grant.recipient == recipient
                && grant.binding == binding
            {
                grant.state = State::Revoked {
                    at_unix_ms: revoked_at,
                };
            }
        }

        let generation = self
            .next_generation
            .checked_add(1)
            .ok_or(Error::Limit("delegated browser grant generation exhausted"))?;
        self.next_generation = generation;
        let id = random_token()?;
        let issued_at_unix_ms = unix_ms()?;
        let ttl_ms = u64::try_from(ttl.as_millis())
            .map_err(|_| Error::Limit("delegated browser grant TTL overflow"))?;
        let expires_at_unix_ms = issued_at_unix_ms
            .checked_add(ttl_ms)
            .ok_or(Error::Limit("delegated browser grant expiry overflow"))?;
        let deadline = Instant::now()
            .checked_add(ttl)
            .ok_or(Error::Limit("delegated browser grant deadline overflow"))?;
        self.grants.insert(
            id.clone(),
            Grant {
                id: id.clone(),
                generation,
                owner,
                recipient,
                binding,
                issued_at_unix_ms,
                expires_at_unix_ms,
                deadline,
                state: State::Active,
            },
        );
        Ok(Issued {
            id,
            generation,
            issued_at_unix_ms,
            expires_at_unix_ms,
        })
    }

    pub(super) fn authorize(
        &self,
        store: &SessionStore,
        grant_id: &str,
        binding: &Binding,
    ) -> Result<Witness> {
        let current_owner = identity_stamp(store, &binding.owner)?;
        let current_recipient = identity_stamp(store, &binding.recipient)?;
        let now_unix_ms = unix_ms()?;
        let grant = self
            .grants
            .get(grant_id)
            .ok_or(Error::Denied("delegated browser grant unavailable"))?;
        if !matches!(grant.state, State::Active)
            || grant.deadline <= Instant::now()
            || now_unix_ms < grant.issued_at_unix_ms
            || now_unix_ms >= grant.expires_at_unix_ms
            || grant.id != grant_id
            || grant.owner != current_owner
            || grant.recipient != current_recipient
            || grant.binding != *binding
        {
            return Err(Error::Denied(
                "delegated browser grant is expired, revoked, stale or mismatched",
            ));
        }
        Ok(Witness {
            grant_id: grant.id.clone(),
            generation: grant.generation,
            binding: grant.binding.clone(),
        })
    }

    /// Revalidate immediately before publishing protected content.
    ///
    /// Callers must supply freshly rechecked parent resource authority. A held
    /// response admitted under an older generation/security epoch cannot publish.
    pub(super) fn revalidate(
        &self,
        store: &SessionStore,
        witness: &Witness,
        current_parent: &ParentAuthority,
    ) -> Result<()> {
        if &witness.binding.parent != current_parent {
            return Err(Error::Denied("delegated browser parent authority changed"));
        }
        let current = self.authorize(store, &witness.grant_id, &witness.binding)?;
        if current.generation != witness.generation {
            return Err(Error::Denied("delegated browser grant generation changed"));
        }
        Ok(())
    }

    pub(super) fn revoke(
        &mut self,
        store: &SessionStore,
        owner: &PrincipalId,
        grant_id: &str,
    ) -> Result<()> {
        let current_owner = identity_stamp(store, owner)?;
        let grant = self
            .grants
            .get_mut(grant_id)
            .ok_or(Error::Denied("delegated browser grant unavailable"))?;
        if grant.owner.principal != *owner || grant.owner.user_id != current_owner.user_id {
            return Err(Error::Denied(
                "delegated browser grant belongs to another owner",
            ));
        }
        if matches!(grant.state, State::Active) {
            grant.state = State::Revoked {
                at_unix_ms: unix_ms()?,
            };
        }
        Ok(())
    }

    pub(super) fn revoke_recipient(&mut self, recipient: &PrincipalId) {
        let revoked_at = unix_ms().unwrap_or(0);
        for grant in self.grants.values_mut() {
            if matches!(grant.state, State::Active) && grant.recipient.principal == *recipient {
                grant.state = State::Revoked {
                    at_unix_ms: revoked_at,
                };
            }
        }
    }

    pub(super) fn revoke_all(&mut self) {
        let revoked_at = unix_ms().unwrap_or(0);
        for grant in self.grants.values_mut() {
            if matches!(grant.state, State::Active) {
                grant.state = State::Revoked {
                    at_unix_ms: revoked_at,
                };
            }
        }
    }

    #[cfg(test)]
    fn expire_for_test(&mut self, grant_id: &str) {
        self.grants.get_mut(grant_id).unwrap().deadline = Instant::now() - Duration::from_millis(1);
    }

    #[cfg(test)]
    fn revoked_at_for_test(&self, grant_id: &str) -> Option<u64> {
        match self.grants.get(grant_id).map(|grant| &grant.state) {
            Some(State::Revoked { at_unix_ms }) => Some(*at_unix_ms),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    struct Fixture {
        root: PathBuf,
        store: SessionStore,
        owner: PrincipalId,
        recipient: PrincipalId,
        other: PrincipalId,
    }

    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "aw-delegated-browser-test-{}",
                random_token().unwrap()
            ));
            std::fs::create_dir_all(&root).unwrap();
            let store = SessionStore::open(&root).unwrap();
            let owner = PrincipalId::new("unix:61001").unwrap();
            let recipient = PrincipalId::new("unix:61002").unwrap();
            let other = PrincipalId::new("unix:61003").unwrap();
            // Test setup deliberately materializes the stable identities. The
            // production grant path itself is read-only and refuses unknown users.
            store.current_user(&owner).unwrap();
            store.current_user(&recipient).unwrap();
            store.current_user(&other).unwrap();
            Self {
                root,
                store,
                owner,
                recipient,
                other,
            }
        }

        fn resource() -> Resource {
            Resource::File {
                scope_id: "scope_fixture".into(),
                path: "src/lib.rs".into(),
            }
        }

        fn parent() -> ParentAuthority {
            ParentAuthority::Surface {
                owner_epoch: "owner_epoch_1".into(),
            }
        }

        fn binding_for(&self, recipient: PrincipalId, resource: Resource) -> Binding {
            Binding {
                owner: self.owner.clone(),
                recipient,
                source_id: "surface_fixture".into(),
                site: "site_fixture".into(),
                resource,
                parent: Self::parent(),
            }
        }

        fn binding(&self) -> Binding {
            self.binding_for(self.recipient.clone(), Self::resource())
        }

        fn issue(&self, registry: &mut Registry) -> Issued {
            registry
                .grant(&self.store, self.binding(), Duration::from_secs(90))
                .unwrap()
        }

        fn authorize(&self, registry: &Registry, issued: &Issued) -> Witness {
            registry
                .authorize(&self.store, &issued.id, &self.binding())
                .unwrap()
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn grant_is_exact_recipient_and_resource_authority() {
        let fixture = Fixture::new();
        let mut registry = Registry::default();
        let issued = fixture.issue(&mut registry);
        let witness = fixture.authorize(&registry, &issued);
        registry
            .revalidate(&fixture.store, &witness, &Fixture::parent())
            .unwrap();

        assert!(registry
            .authorize(
                &fixture.store,
                &issued.id,
                &fixture.binding_for(fixture.other.clone(), Fixture::resource()),
            )
            .is_err());
        assert!(registry
            .authorize(
                &fixture.store,
                &issued.id,
                &fixture.binding_for(
                    fixture.recipient.clone(),
                    Resource::File {
                        scope_id: "scope_fixture".into(),
                        path: "src/other.rs".into(),
                    },
                ),
            )
            .is_err());
    }

    #[test]
    fn revoke_fences_held_publish_even_when_resource_is_unchanged() {
        let fixture = Fixture::new();
        let mut registry = Registry::default();
        let issued = fixture.issue(&mut registry);
        let held = fixture.authorize(&registry, &issued);

        registry
            .revoke(&fixture.store, &fixture.owner, &issued.id)
            .unwrap();
        assert!(registry.revoked_at_for_test(&issued.id).is_some());
        assert!(registry
            .revalidate(&fixture.store, &held, &Fixture::parent())
            .is_err());
        assert!(registry
            .authorize(&fixture.store, &issued.id, &fixture.binding())
            .is_err());
    }

    #[test]
    fn regrant_rotates_generation_and_old_witness_stays_stale() {
        let fixture = Fixture::new();
        let mut registry = Registry::default();
        let first = fixture.issue(&mut registry);
        let held = fixture.authorize(&registry, &first);
        let second = fixture.issue(&mut registry);
        assert!(second.generation > first.generation);
        assert!(registry
            .revalidate(&fixture.store, &held, &Fixture::parent())
            .is_err());
        let current = fixture.authorize(&registry, &second);
        registry
            .revalidate(&fixture.store, &current, &Fixture::parent())
            .unwrap();
    }

    #[test]
    fn recipient_security_epoch_change_invalidates_held_grant() {
        let fixture = Fixture::new();
        let mut registry = Registry::default();
        let issued = fixture.issue(&mut registry);
        let held = fixture.authorize(&registry, &issued);

        fixture
            .store
            .register_identity_device_key(
                &fixture.recipient,
                "1111111111111111111111111111111111111111111111111111111111111111",
            )
            .unwrap();

        assert!(registry
            .revalidate(&fixture.store, &held, &Fixture::parent())
            .is_err());
    }

    #[test]
    fn owner_security_epoch_change_invalidates_held_grant() {
        let fixture = Fixture::new();
        let mut registry = Registry::default();
        let issued = fixture.issue(&mut registry);
        let held = fixture.authorize(&registry, &issued);

        fixture
            .store
            .register_identity_device_key(
                &fixture.owner,
                "2222222222222222222222222222222222222222222222222222222222222222",
            )
            .unwrap();

        assert!(registry
            .revalidate(&fixture.store, &held, &Fixture::parent())
            .is_err());
    }

    #[test]
    fn logout_or_reload_does_not_restore_revoked_capability() {
        let fixture = Fixture::new();
        let mut registry = Registry::default();
        let issued = fixture.issue(&mut registry);
        let held = fixture.authorize(&registry, &issued);

        registry.revoke_all();
        assert!(registry
            .revalidate(&fixture.store, &held, &Fixture::parent())
            .is_err());

        // Browser/owner restart gets a fresh non-persistent registry. A copied
        // grant ID or old witness cannot silently regrant authority.
        let reloaded = Registry::default();
        assert!(reloaded
            .authorize(&fixture.store, &issued.id, &fixture.binding())
            .is_err());
    }

    #[test]
    fn parent_change_or_expiry_fences_publication_without_liveness_shortcuts() {
        let fixture = Fixture::new();
        let mut registry = Registry::default();
        let issued = fixture.issue(&mut registry);
        let held = fixture.authorize(&registry, &issued);
        assert!(registry
            .revalidate(
                &fixture.store,
                &held,
                &ParentAuthority::Surface {
                    owner_epoch: "owner_epoch_2".into(),
                },
            )
            .is_err());

        let current = fixture.issue(&mut registry);
        let held = fixture.authorize(&registry, &current);
        registry.expire_for_test(&current.id);
        // There is deliberately no receiver-ACK/liveness input to revalidation:
        // presentation heartbeats cannot extend resource authority.
        assert!(registry
            .revalidate(&fixture.store, &held, &Fixture::parent())
            .is_err());
    }

    #[test]
    fn unknown_recipient_is_not_created_by_grant_attempt() {
        let fixture = Fixture::new();
        let unknown = PrincipalId::new("unix:61999").unwrap();
        let mut registry = Registry::default();
        assert!(registry
            .grant(
                &fixture.store,
                fixture.binding_for(unknown.clone(), Fixture::resource()),
                Duration::from_secs(90),
            )
            .is_err());
        assert!(fixture.store.known_user(&unknown).is_err());
    }
}
