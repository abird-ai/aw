//! Host-neutral policy composition. Never issues tool, identity or payer grants.
//! Hosts authenticate scopes and recheck actual ToolPolicy/Envelope authority.

#![no_std]
#![forbid(unsafe_code)]

/// Host-normalized source identity used only for policy matching. This does not
/// register a source or imply a ToolPolicy grant.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceKind {
    Host,
    Mcp,
    Extension,
}

/// Policy-level effect vocabulary mirrors the semantic operation effect without
/// importing an Agent/tool executor into this pure crate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Effect {
    Read,
    Write,
    Process,
    Control,
    Unknown,
}

/// Exact semantic/resource identity after host semantic resolution and before
/// policy authorization. Strings are borrowed host evidence, never user-parsed
/// capabilities.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OperationContext<'a> {
    pub operation: &'a str,
    pub source_kind: SourceKind,
    pub source_id: &'a str,
    pub source_epoch: u64,
    pub effect: Effect,
    pub site: &'a str,
    pub target_domain: &'a str,
    pub resource_scope: &'a str,
}

/// One authenticated invocation context. The host/store authenticates every
/// identity and relation before constructing this value; validation here only
/// enforces bounded/canonical transport shape.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PolicyContext<'a> {
    pub actor: &'a str,
    pub domain: &'a str,
    pub active_team: Option<&'a str>,
    pub workspace: &'a str,
    pub session: Option<&'a str>,
    pub graph: Option<&'a str>,
    pub node: Option<u32>,
    pub invocation: &'a str,
    pub operation: OperationContext<'a>,
}

/// Positive authority subject proven by the host/store. A team-role path is
/// evidence about a verified membership relation; it is not inferred from a
/// Policy layer or from merely naming a team.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GrantSubject<'a> {
    DirectIdentity {
        identity: &'a str,
    },
    TeamRole {
        team: &'a str,
        role: &'a str,
        member: &'a str,
        membership_epoch: u64,
    },
}

/// Exact ceilings attached to one authenticated positive grant path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GrantRestrictions<'a> {
    pub operation: &'a str,
    pub source_kind: SourceKind,
    pub source_id: &'a str,
    pub source_epoch: u64,
    pub effect: Effect,
    pub site: &'a str,
    pub target_domain: &'a str,
    pub resource_scope: &'a str,
}

/// Sealed host/store evidence of an already-authenticated positive grant.
/// aw-policy validates and matches this evidence but can never create or
/// authenticate it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AuthenticatedGrantPath<'a> {
    pub issuer: &'a str,
    pub issuer_authority_epoch: u64,
    pub grant_scope: &'a str,
    pub proof_ref: &'a str,
    pub subject: GrantSubject<'a>,
    pub restrictions: GrantRestrictions<'a>,
}

/// Result of exact positive-path matching. The index is evidence selection
/// only; callers must still revalidate the authoritative grant before effects.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AuthorizedGrant {
    pub path_index: usize,
    pub issuer_authority_epoch: u64,
}

/// Combined pure admission result: exact positive authority exists and all
/// applicable constraints/preferences resolved. It is still not an executable
/// capability; ToolPolicy/Envelope/resource/payer owners remain authoritative.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AuthorizedPolicy {
    grant: AuthorizedGrant,
    effective: EffectivePolicy,
}

impl AuthorizedPolicy {
    pub const fn grant(self) -> AuthorizedGrant {
        self.grant
    }

    pub const fn effective(self) -> EffectivePolicy {
        self.effective
    }
}

/// One immutable guardian/policy decision shape. Advisory evidence can deny,
/// ask or narrow only inside an already-authorized admission envelope.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PolicyDecision<'a> {
    pub decision: Decision,
    pub revision: u64,
    pub route: Option<Strategy>,
    pub approval_scope: Option<&'a str>,
    pub narrow_ref: Option<&'a str>,
}

fn bounded_ascii(value: &str, max: usize) -> bool {
    !value.is_empty()
        && value.len() <= max
        && value.is_ascii()
        && !value.chars().any(char::is_control)
}

fn token(value: &str, max: usize) -> bool {
    bounded_ascii(value, max)
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._~:/@-".contains(&byte))
}

impl OperationContext<'_> {
    pub fn validate(&self) -> Result<(), PolicyError> {
        if !token(self.operation, 128)
            || !token(self.source_id, 128)
            || self.source_epoch == 0
            || !token(self.site, 128)
            || !token(self.target_domain, 128)
            || !bounded_ascii(self.resource_scope, 1024)
        {
            return Err(PolicyError::InvalidContext);
        }
        Ok(())
    }
}

impl PolicyContext<'_> {
    pub fn validate(&self) -> Result<(), PolicyError> {
        if !token(self.actor, 128)
            || !token(self.domain, 128)
            || !bounded_ascii(self.workspace, 512)
            || !token(self.invocation, 256)
            || self.active_team.is_some_and(|value| !token(value, 128))
            || self
                .session
                .is_some_and(|value| !bounded_ascii(value, 1024))
            || self.graph.is_some_and(|value| !bounded_ascii(value, 1024))
            || self.node == Some(0)
        {
            return Err(PolicyError::InvalidContext);
        }
        self.operation.validate()
    }
}

impl AuthenticatedGrantPath<'_> {
    pub fn validate(&self) -> Result<(), PolicyError> {
        if !token(self.issuer, 128)
            || self.issuer_authority_epoch == 0
            || !bounded_ascii(self.grant_scope, 1024)
            || !token(self.proof_ref, 256)
        {
            return Err(PolicyError::InvalidGrant);
        }
        match self.subject {
            GrantSubject::DirectIdentity { identity } => {
                if !token(identity, 128) {
                    return Err(PolicyError::InvalidGrant);
                }
            }
            GrantSubject::TeamRole {
                team,
                role,
                member,
                membership_epoch,
            } => {
                if !token(team, 128)
                    || !token(role, 128)
                    || !token(member, 128)
                    || membership_epoch == 0
                {
                    return Err(PolicyError::InvalidGrant);
                }
            }
        }
        let restriction = self.restrictions;
        OperationContext {
            operation: restriction.operation,
            source_kind: restriction.source_kind,
            source_id: restriction.source_id,
            source_epoch: restriction.source_epoch,
            effect: restriction.effect,
            site: restriction.site,
            target_domain: restriction.target_domain,
            resource_scope: restriction.resource_scope,
        }
        .validate()
        .map_err(|_| PolicyError::InvalidGrant)
    }
}

fn subject_matches(context: &PolicyContext<'_>, subject: GrantSubject<'_>) -> bool {
    match subject {
        GrantSubject::DirectIdentity { identity } => identity == context.actor,
        GrantSubject::TeamRole {
            team,
            member,
            membership_epoch: _,
            role: _,
        } => member == context.actor && context.active_team.is_none_or(|selected| selected == team),
    }
}

fn restrictions_match(
    operation: &OperationContext<'_>,
    restriction: &GrantRestrictions<'_>,
) -> bool {
    restriction.operation == operation.operation
        && restriction.source_kind == operation.source_kind
        && restriction.source_id == operation.source_id
        && restriction.source_epoch == operation.source_epoch
        && restriction.effect == operation.effect
        && restriction.site == operation.site
        && restriction.target_domain == operation.target_domain
        && restriction.resource_scope == operation.resource_scope
}

/// Require at least one currently authenticated exact positive path. Invalid
/// evidence fails closed rather than being silently skipped.
pub fn authorize_exact(
    context: &PolicyContext<'_>,
    grants: &[AuthenticatedGrantPath<'_>],
) -> Result<AuthorizedGrant, PolicyError> {
    context.validate()?;
    let mut matched = None;
    for (index, grant) in grants.iter().enumerate() {
        grant.validate()?;
        if subject_matches(context, grant.subject)
            && restrictions_match(&context.operation, &grant.restrictions)
        {
            matched.get_or_insert(AuthorizedGrant {
                path_index: index,
                issuer_authority_epoch: grant.issuer_authority_epoch,
            });
        }
    }
    matched.ok_or(PolicyError::MissingGrant)
}

/// Resolve all applicable pure constraints and require a separate exact
/// authenticated positive grant. Preferences can therefore never mint missing
/// authority, while any applicable deny still wins.
pub fn resolve_authorized(
    context: &PolicyContext<'_>,
    grants: &[AuthenticatedGrantPath<'_>],
    layers: &[PolicyLayer<'_>],
) -> Result<AuthorizedPolicy, PolicyError> {
    context.validate()?;
    let effective = resolve_scoped(layers)?;
    let grant = authorize_exact(context, grants)?;
    Ok(AuthorizedPolicy { grant, effective })
}

impl PolicyDecision<'_> {
    /// Structural decision validation inside one already-resolved effective
    /// route envelope. This never authenticates the guardian or an approver.
    pub fn validate(&self, effective: EffectivePolicy) -> Result<(), PolicyError> {
        if self.revision == 0
            || self
                .route
                .is_some_and(|strategy| !effective.routes().contains(strategy))
        {
            return Err(PolicyError::InvalidDecision);
        }
        match self.decision {
            Decision::Allow => {
                if self.approval_scope.is_some() || self.narrow_ref.is_some() {
                    return Err(PolicyError::InvalidDecision);
                }
            }
            Decision::Deny => {
                if self.route.is_some()
                    || self.approval_scope.is_some()
                    || self.narrow_ref.is_some()
                {
                    return Err(PolicyError::InvalidDecision);
                }
            }
            Decision::Ask => {
                if self
                    .approval_scope
                    .is_none_or(|scope| !bounded_ascii(scope, 1024))
                    || self.narrow_ref.is_some()
                {
                    return Err(PolicyError::InvalidDecision);
                }
            }
            Decision::Narrow => {
                if self
                    .narrow_ref
                    .is_none_or(|reference| !token(reference, 256))
                    || self.approval_scope.is_some()
                {
                    return Err(PolicyError::InvalidDecision);
                }
            }
        }
        Ok(())
    }
}

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
    InvalidContext,
    InvalidGrant,
    MissingGrant,
    InvalidDecision,
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

    fn policy_context<'a>(active_team: Option<&'a str>) -> PolicyContext<'a> {
        PolicyContext {
            actor: "i:alice",
            domain: "d:acme",
            active_team,
            workspace: "/d:acme/i:service/p:repo",
            session: Some("/d:acme/i:alice/p:repo/a:0"),
            graph: None,
            node: None,
            invocation: "turn:1/call:read-1",
            operation: OperationContext {
                operation: "tools.fs.read",
                source_kind: SourceKind::Host,
                source_id: "trusted-host",
                source_epoch: 7,
                effect: Effect::Read,
                site: "local",
                target_domain: "d:acme",
                resource_scope: "/d:acme/i:service/p:repo/path:README.md",
            },
        }
    }

    fn direct_grant<'a>() -> AuthenticatedGrantPath<'a> {
        AuthenticatedGrantPath {
            issuer: "i:admin",
            issuer_authority_epoch: 11,
            grant_scope: "/d:acme/i:service/p:repo",
            proof_ref: "grant-read-readme-v11",
            subject: GrantSubject::DirectIdentity {
                identity: "i:alice",
            },
            restrictions: GrantRestrictions {
                operation: "tools.fs.read",
                source_kind: SourceKind::Host,
                source_id: "trusted-host",
                source_epoch: 7,
                effect: Effect::Read,
                site: "local",
                target_domain: "d:acme",
                resource_scope: "/d:acme/i:service/p:repo/path:README.md",
            },
        }
    }

    #[test]
    fn preference_never_mints_missing_positive_authority() {
        let context = policy_context(None);
        let delegate = Policy {
            prefer: Some(Strategy::DelegateAgent),
            ..Policy::default()
        };
        let layers = [PolicyLayer {
            scope: PolicyScope::Session,
            revision: 1,
            policy: &delegate,
        }];
        assert_eq!(
            resolve_authorized(&context, &[], &layers),
            Err(PolicyError::MissingGrant)
        );
    }

    #[test]
    fn exact_authenticated_grant_path_requires_every_semantic_dimension() {
        let context = policy_context(None);
        let grant = direct_grant();
        assert_eq!(
            authorize_exact(&context, &[grant]).unwrap(),
            AuthorizedGrant {
                path_index: 0,
                issuer_authority_epoch: 11,
            }
        );

        let mut wrong = grant;
        wrong.restrictions.source_epoch = 8;
        assert_eq!(
            authorize_exact(&context, &[wrong]),
            Err(PolicyError::MissingGrant)
        );
        wrong = grant;
        wrong.restrictions.effect = Effect::Write;
        assert_eq!(
            authorize_exact(&context, &[wrong]),
            Err(PolicyError::MissingGrant)
        );
        wrong = grant;
        wrong.restrictions.site = "remote";
        assert_eq!(
            authorize_exact(&context, &[wrong]),
            Err(PolicyError::MissingGrant)
        );
        wrong = grant;
        wrong.restrictions.resource_scope = "/d:acme/i:service/p:repo/path:OTHER.md";
        assert_eq!(
            authorize_exact(&context, &[wrong]),
            Err(PolicyError::MissingGrant)
        );
    }

    #[test]
    fn selected_team_context_excludes_unrelated_team_grant_paths() {
        let context = policy_context(Some("team:engineering"));
        let direct = direct_grant();
        let engineering = AuthenticatedGrantPath {
            subject: GrantSubject::TeamRole {
                team: "team:engineering",
                role: "developer",
                member: "i:alice",
                membership_epoch: 4,
            },
            proof_ref: "grant-team-eng-v4",
            ..direct
        };
        let finance = AuthenticatedGrantPath {
            subject: GrantSubject::TeamRole {
                team: "team:finance",
                role: "developer",
                member: "i:alice",
                membership_epoch: 9,
            },
            proof_ref: "grant-team-fin-v9",
            ..direct
        };

        assert_eq!(
            authorize_exact(&context, &[finance, engineering])
                .unwrap()
                .path_index,
            1
        );
        // An independently valid direct identity path remains eligible under an
        // explicitly selected team, as required by the unified design.
        assert_eq!(
            authorize_exact(&context, &[finance, direct])
                .unwrap()
                .path_index,
            1
        );
    }

    #[test]
    fn invalid_or_stale_shaped_grant_evidence_fails_closed() {
        let context = policy_context(None);
        let mut grant = direct_grant();
        grant.issuer_authority_epoch = 0;
        assert_eq!(
            authorize_exact(&context, &[grant]),
            Err(PolicyError::InvalidGrant)
        );
        grant = direct_grant();
        grant.subject = GrantSubject::TeamRole {
            team: "team:engineering",
            role: "developer",
            member: "i:alice",
            membership_epoch: 0,
        };
        assert_eq!(
            authorize_exact(&context, &[grant]),
            Err(PolicyError::InvalidGrant)
        );
    }

    #[test]
    fn applicable_deny_wins_even_with_valid_positive_grant() {
        let context = policy_context(None);
        let grant = direct_grant();
        let deny = Policy {
            deny: true,
            ..Policy::default()
        };
        assert_eq!(
            resolve_authorized(
                &context,
                &[grant],
                &[PolicyLayer {
                    scope: PolicyScope::Workspace,
                    revision: 3,
                    policy: &deny,
                }],
            ),
            Err(PolicyError::Denied)
        );
    }

    #[test]
    fn guardian_decision_cannot_widen_routes_or_mix_ask_and_narrow_evidence() {
        let local = resolve(&[Policy {
            routes: Routes::LOCAL,
            ..Policy::default()
        }])
        .unwrap();
        assert!(PolicyDecision {
            decision: Decision::Allow,
            revision: 1,
            route: Some(Strategy::Local),
            approval_scope: None,
            narrow_ref: None,
        }
        .validate(local)
        .is_ok());
        assert_eq!(
            PolicyDecision {
                decision: Decision::Allow,
                revision: 1,
                route: Some(Strategy::DelegateAgent),
                approval_scope: None,
                narrow_ref: None,
            }
            .validate(local),
            Err(PolicyError::InvalidDecision)
        );
        assert_eq!(
            PolicyDecision {
                decision: Decision::Ask,
                revision: 2,
                route: Some(Strategy::Local),
                approval_scope: Some("/d:acme/i:alice"),
                narrow_ref: Some("narrow-1"),
            }
            .validate(local),
            Err(PolicyError::InvalidDecision)
        );
        assert!(PolicyDecision {
            decision: Decision::Narrow,
            revision: 3,
            route: Some(Strategy::Local),
            approval_scope: None,
            narrow_ref: Some("narrow-1"),
        }
        .validate(local)
        .is_ok());
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
