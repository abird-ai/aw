//! Host-neutral policy composition. Never issues tool, identity or payer grants.
//! Hosts authenticate scopes and recheck actual ToolPolicy/Envelope authority.

#![no_std]
#![forbid(unsafe_code)]

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Strategy {
    Local,
    DelegateAgent,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecisionScope {
    Graph,
    Node,
}

/// Ordered policy dimensions. The authenticated host selects applicable layers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum PolicyScope {
    Host,
    Domain,
    Identity,
    Team,
    Workspace,
    Session,
    Graph,
    Node,
    ToolCall,
}

/// Host-supplied policy reference; neither its revision nor its scope is a grant.
#[derive(Clone, Copy, Debug)]
pub struct PolicyLayer<'a> {
    pub scope: PolicyScope,
    pub revision: u64,
    pub policy: &'a Policy,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
    Allow,
    Deny,
    Ask,
    Narrow,
}

/// Candidate routes only. This is NOT execution authorization.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Routes(u8);

impl Routes {
    pub const NONE: Self = Self(0);
    pub const LOCAL: Self = Self(1);
    pub const DELEGATE_AGENT: Self = Self(2);
    pub const BOTH: Self = Self(3);

    pub const fn contains(self, strategy: Strategy) -> bool {
        let bit = match strategy {
            Strategy::Local => Self::LOCAL.0,
            Strategy::DelegateAgent => Self::DELEGATE_AGENT.0,
        };
        self.0 & bit != 0
    }

    pub const fn intersect(self, other: Self) -> Self {
        Self(self.0 & other.0)
    }
}

/// One trusted, already applicable layer. No human/team grant can originate here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Policy {
    pub deny: bool,
    pub routes: Routes,
    pub prefer: Option<Strategy>,
    pub require_preferred: bool,
    pub max_model_rounds: Option<u32>,
}

impl Default for Policy {
    fn default() -> Self {
        Self {
            deny: false,
            routes: Routes::BOTH,
            prefer: None,
            require_preferred: false,
            max_model_rounds: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EffectivePolicy {
    /// A selected path, not a tool/provider/payer grant.
    strategy: Strategy,
    routes: Routes,
    max_model_rounds: Option<u32>,
}

impl EffectivePolicy {
    pub const fn local() -> Self {
        Self {
            strategy: Strategy::Local,
            routes: Routes::LOCAL,
            max_model_rounds: None,
        }
    }

    pub const fn strategy(self) -> Strategy {
        self.strategy
    }

    pub const fn routes(self) -> Routes {
        self.routes
    }

    pub const fn max_model_rounds(self) -> Option<u32> {
        self.max_model_rounds
    }

    /// Restrict an already selected route; never retarget or restore ceilings.
    /// This is a pure constraint merge, never an authority grant.
    pub fn narrow(self, next: Self) -> Result<Self, PolicyError> {
        let routes = self.routes.intersect(next.routes);
        if self.strategy != next.strategy || !routes.contains(self.strategy) {
            return Err(PolicyError::RequiredRouteUnavailable);
        }
        let max_model_rounds = match (self.max_model_rounds, next.max_model_rounds) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (Some(a), None) | (None, Some(a)) => Some(a),
            (None, None) => None,
        };
        Ok(Self {
            strategy: self.strategy,
            routes,
            max_model_rounds,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PolicyError {
    Denied,
    NoRoute,
    RequiredRouteUnavailable,
    InvalidLimit,
    ConflictingRequiredRoutes,
    InvalidLayer,
}

/// The host supplies already authenticated/applicable layers, broad to narrow.
/// Denies win, ceilings intersect, nearest permissible preference wins.
/// The result never substitutes for ToolPolicy/Envelope or actual resource grants.
pub fn resolve<'a>(
    layers: impl IntoIterator<Item = &'a Policy>,
) -> Result<EffectivePolicy, PolicyError> {
    let mut routes = Routes::BOTH;
    let mut preferred = Strategy::Local;
    let mut required: Option<Strategy> = None;
    let mut max_model_rounds: Option<u32> = None;

    for layer in layers {
        if layer.deny {
            return Err(PolicyError::Denied);
        }
        if layer.max_model_rounds == Some(0) {
            return Err(PolicyError::InvalidLimit);
        }
        routes = routes.intersect(layer.routes);
        if let Some(limit) = layer.max_model_rounds {
            max_model_rounds = Some(max_model_rounds.map_or(limit, |ceiling| ceiling.min(limit)));
        }
        if let Some(strategy) = layer.prefer {
            preferred = strategy;
            if layer.require_preferred {
                if required.is_some_and(|existing| existing != strategy) {
                    return Err(PolicyError::ConflictingRequiredRoutes);
                }
                required = Some(strategy);
            }
        } else if layer.require_preferred {
            return Err(PolicyError::RequiredRouteUnavailable);
        }
    }
    if routes == Routes::NONE {
        return Err(PolicyError::NoRoute);
    }
    let strategy = if let Some(required) = required {
        if !routes.contains(required) {
            return Err(PolicyError::RequiredRouteUnavailable);
        }
        // A narrower *soft preference* cannot override a mandatory route.
        required
    } else if routes.contains(preferred) {
        preferred
    } else if routes.contains(Strategy::Local) {
        Strategy::Local
    } else {
        // Never implicitly start paid Agent inference in place of Local.
        return Err(PolicyError::RequiredRouteUnavailable);
    };
    Ok(EffectivePolicy {
        strategy,
        routes,
        max_model_rounds,
    })
}

/// Scope precedence is checked, while grant/membership validity remains the
/// responsibility of the host's authenticated policy-resource owner.
pub fn resolve_scoped(layers: &[PolicyLayer<'_>]) -> Result<EffectivePolicy, PolicyError> {
    let mut previous: Option<PolicyScope> = None;
    let mut same_scope_preference = None;
    for layer in layers {
        if layer.revision == 0 || previous.is_some_and(|p| layer.scope < p) {
            return Err(PolicyError::InvalidLayer);
        }
        if previous != Some(layer.scope) {
            same_scope_preference = None;
        }
        if let Some(preference) = layer.policy.prefer {
            if same_scope_preference.is_some_and(|existing| existing != preference) {
                // Several domain/team/resource layers may all constrain the same
                // request, but their equal-precedence preferences cannot race.
                return Err(PolicyError::InvalidLayer);
            }
            same_scope_preference = Some(preference);
        }
        previous = Some(layer.scope);
    }
    resolve(layers.iter().map(|layer| layer.policy))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn userless_default_is_local_without_guardian_or_grants() {
        let result = resolve(&[]).unwrap();
        assert_eq!(result.strategy, Strategy::Local);
        assert_eq!(result.routes, Routes::BOTH);
    }

    #[test]
    fn explicit_delegate_is_allowed_only_within_candidate_ceiling() {
        let host = Policy {
            routes: Routes::LOCAL,
            ..Policy::default()
        };
        let session = Policy {
            prefer: Some(Strategy::DelegateAgent),
            ..Policy::default()
        };
        let result = resolve(&[host, session]).unwrap();
        assert_eq!(result.strategy, Strategy::Local);

        let required = Policy {
            require_preferred: true,
            ..session
        };
        assert_eq!(
            resolve(&[host, required]),
            Err(PolicyError::RequiredRouteUnavailable)
        );
    }

    #[test]
    fn authenticated_scope_order_is_explicit_and_bounded() {
        let host = Policy::default();
        let session = Policy {
            max_model_rounds: Some(2),
            ..Policy::default()
        };
        let layers = [
            PolicyLayer {
                scope: PolicyScope::Host,
                revision: 1,
                policy: &host,
            },
            PolicyLayer {
                scope: PolicyScope::Session,
                revision: 4,
                policy: &session,
            },
        ];
        assert_eq!(resolve_scoped(&layers).unwrap().max_model_rounds, Some(2));
        assert_eq!(
            resolve_scoped(&[layers[1], layers[0]]),
            Err(PolicyError::InvalidLayer)
        );
    }

    #[test]
    fn required_outer_route_survives_inner_soft_preference() {
        let host = Policy {
            prefer: Some(Strategy::Local),
            require_preferred: true,
            ..Policy::default()
        };
        let session = Policy {
            prefer: Some(Strategy::DelegateAgent),
            ..Policy::default()
        };
        assert_eq!(
            resolve(&[host, session]).unwrap().strategy(),
            Strategy::Local
        );
    }

    #[test]
    fn constraining_a_live_selection_cannot_retarget_or_widen() {
        let initial = resolve(&[Policy {
            max_model_rounds: Some(2),
            ..Policy::default()
        }])
        .unwrap();
        let wider = resolve(&[Policy::default()]).unwrap();
        let narrowed = initial.narrow(wider).unwrap();
        assert_eq!(narrowed.max_model_rounds(), Some(2));
        assert_eq!(narrowed.strategy(), Strategy::Local);
        assert_eq!(narrowed.routes(), Routes::BOTH);
        let delegate = resolve(&[Policy {
            prefer: Some(Strategy::DelegateAgent),
            ..Policy::default()
        }])
        .unwrap();
        assert_eq!(
            narrowed.narrow(delegate),
            Err(PolicyError::RequiredRouteUnavailable)
        );
    }

    #[test]
    fn equal_scope_preference_conflicts_fail_closed() {
        let local = Policy {
            prefer: Some(Strategy::Local),
            ..Policy::default()
        };
        let delegate = Policy {
            prefer: Some(Strategy::DelegateAgent),
            ..Policy::default()
        };
        let layers = [
            PolicyLayer {
                scope: PolicyScope::Domain,
                revision: 1,
                policy: &local,
            },
            PolicyLayer {
                scope: PolicyScope::Domain,
                revision: 2,
                policy: &delegate,
            },
        ];
        assert_eq!(resolve_scoped(&layers), Err(PolicyError::InvalidLayer));
        assert!(resolve_scoped(&layers[..1]).is_ok());
    }

    #[test]
    fn deny_and_tightest_ceiling_win() {
        let domain = Policy {
            max_model_rounds: Some(8),
            ..Policy::default()
        };
        let session = Policy {
            max_model_rounds: Some(3),
            ..Policy::default()
        };
        assert_eq!(
            resolve(&[domain, session]).unwrap().max_model_rounds,
            Some(3)
        );
        assert_eq!(
            resolve(&[
                domain,
                Policy {
                    deny: true,
                    ..session
                }
            ]),
            Err(PolicyError::Denied)
        );
    }
}
