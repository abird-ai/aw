//! Reuse the actual native-source fixture's retained Tool/Turn custody.
use aw_agent_core::{
    AccountOwner, BillingOwner, CredentialCustody, InvocationIdentity, PrincipalId, TurnJournal,
    TurnStart,
};
use aw_store::{
    AgentHistoryForkAccess, ApplyContext, ApplyMode, PlanKind, PrepareContext, Selection,
    SessionStore, SourceSelection,
};
use std::path::Path;

pub(super) fn exercise(
    store: &SessionStore,
    owner: &PrincipalId,
    source: &str,
    branch: &str,
    destination: &str,
    root: &Path,
) {
    let other = PrincipalId::new("internal:fixture:context-transfer-other").unwrap();
    let target = store.context_target(owner, destination, "main").unwrap();
    let prepare = PrepareContext {
        target: target.clone(),
        sources: vec![SourceSelection {
            session_id: source.into(),
            branch: branch.into(),
            head: None,
            selection: Selection::Standalone,
        }],
        kind: PlanKind::Import,
        focus: "private context".into(),
        max_input_bytes: aw_agent_core::context::MAX_SUMMARY_INPUT,
        max_output_bytes: 8192,
        timeout_seconds: 30,
    };

    // The legacy and Agent ACL audiences must both remain private.
    store
        .grant(owner, destination, &other, Some(false))
        .unwrap();
    assert!(store.context_prepare(owner, prepare.clone()).is_err());
    store.grant(owner, destination, &other, None).unwrap();
    let plan = store.context_prepare(owner, prepare.clone()).unwrap();
    assert!(plan.evidence.iter().any(|entry| entry.kind == "turn"));
    assert!(store.context_plan(&other, &plan.id).is_err());
    assert!(store
        .summary_journal(owner, &plan.id, "protected-summary")
        .is_err());
    let artifact = store
        .context_manual(
            owner,
            &plan.id,
            "private-manual",
            "private source summary",
            None,
        )
        .unwrap();
    let edited = store
        .context_manual(
            owner,
            &plan.id,
            "private-edit",
            "private edited summary",
            Some(artifact.id.clone()),
        )
        .unwrap();
    assert!(store.context_artifact(&other, &edited.id).is_err());
    let apply = ApplyContext {
        artifact_id: edited.id.clone(),
        target: target.clone(),
        mode: ApplyMode::Append,
        request_id: "private-append".into(),
        confirmed_disclosure: true,
    };
    for mode in [
        ApplyMode::NextPrompt,
        ApplyMode::NewSession {
            title: "not approved".into(),
        },
        ApplyMode::Fork {
            name: "not-approved".into(),
        },
    ] {
        assert!(store
            .context_apply(
                owner,
                ApplyContext {
                    mode,
                    ..apply.clone()
                }
            )
            .is_err());
    }
    store
        .grant(owner, destination, &other, Some(false))
        .unwrap();
    assert!(store.context_apply(owner, apply.clone()).is_err());
    store.grant(owner, destination, &other, None).unwrap();
    let applied = store.context_apply(owner, apply.clone()).unwrap();
    assert_eq!(
        store.context_apply(owner, apply).unwrap().node_id,
        applied.node_id
    );
    let history = store
        .read_agent_branch_view(owner, destination, &applied.branch.id)
        .unwrap()
        .history;
    assert_eq!(history.context_blocks.len(), 1);
    assert_eq!(history.context_blocks[0].text, edited.text);
    assert!(store
        .grant(owner, destination, &other, Some(false))
        .is_err());
    assert!(store.export_jsonl(owner, destination).is_err());

    // The carried manifest must also constrain a new exact history fork
    // even though that new Agent has no source bindings.
    let layout = root.join("private-context-fork");
    aw_platform::ensure_private_dir(&layout).unwrap();
    let mut binding = store.session(owner, destination).unwrap().binding;
    binding.tool_root = layout.clone();
    binding.working_directory = layout;
    let (fork, fork_branch) = store
        .create_agent_from_agent_history(
            owner,
            destination,
            &applied.branch.id,
            "private context fork",
            binding,
            AgentHistoryForkAccess::Private,
        )
        .unwrap();
    assert_eq!(
        store
            .read_agent_branch_view(owner, &fork.id, &fork_branch.id)
            .unwrap()
            .history
            .context_blocks[0]
            .text,
        edited.text
    );
    assert!(store.grant(owner, &fork.id, &other, Some(false)).is_err());
    assert!(store.export_jsonl(owner, &fork.id).is_err());

    // Select actual protected Turn facts through a structurally shared
    // inherited prefix. Neither copying local nodes nor resealing the
    // backing Agent's current history can stand in for the original grant.
    let fresh_layout = |name: &str| {
        let path = root.join(name);
        aw_platform::ensure_private_dir(&path).unwrap();
        let mut binding = store.session(owner, destination).unwrap().binding;
        binding.tool_root = path.clone();
        binding.working_directory = path;
        binding
    };
    let (inherited, inherited_branch) = store
        .create_agent_from_agent_history(
            owner,
            source,
            branch,
            "native history fork",
            fresh_layout("native-inherited-fork"),
            AgentHistoryForkAccess::Private,
        )
        .unwrap();
    let inherited_before = store
        .read_agent_branch_view(owner, &inherited.id, &inherited_branch.id)
        .unwrap();
    assert_eq!(inherited_before.history.turns.len(), 6);
    let inherited_rev = store
        .agent_public_revision(owner, &inherited.id, &inherited_branch.id)
        .unwrap();
    let (target_session, target_branch) = store
        .create(
            owner,
            "inherited transfer destination",
            fresh_layout("inherited-context-destination"),
        )
        .unwrap();
    assert_eq!(inherited.workspace_id, target_session.workspace_id);
    let inherited_target = store
        .context_target(owner, &target_session.id, &target_branch.id)
        .unwrap();
    let mut inherited_prepare = PrepareContext {
        target: inherited_target.clone(),
        sources: vec![SourceSelection {
            session_id: inherited.id.clone(),
            branch: inherited_branch.id.clone(),
            head: None,
            selection: Selection::Inherited,
        }],
        ..prepare.clone()
    };
    let whole = store
        .context_prepare(owner, inherited_prepare.clone())
        .unwrap();
    assert_eq!(
        serde_json::to_value(&whole.evidence).unwrap(),
        serde_json::to_value(&plan.evidence).unwrap()
    );
    assert!(whole.sources[0].inherited_history.is_some());
    assert_eq!(
        store
            .agent_public_revision(owner, &inherited.id, &inherited_branch.id)
            .unwrap(),
        inherited_rev,
        "selecting a retained prefix must not reseal or mutate its Agent",
    );
    // Range endpoints must be real settled inherited Turns, not local
    // nodes, an override head, or an approximate ordinal boundary.
    for selection in [
        Selection::InheritedTurns { first: 0, last: 2 },
        Selection::InheritedTurns { first: 3, last: 2 },
        Selection::InheritedTurns { first: 2, last: 7 },
    ] {
        let mut invalid = inherited_prepare.clone();
        invalid.sources[0].selection = selection;
        assert!(store.context_prepare(owner, invalid).is_err());
    }
    let mut invalid = inherited_prepare.clone();
    invalid.sources[0].head = Some(inherited_branch.head.clone());
    assert!(store.context_prepare(owner, invalid).is_err());
    let mut invalid = inherited_prepare.clone();
    invalid.sources[0].branch = "main".into();
    assert!(store.context_prepare(owner, invalid).is_err());
    let mut invalid = inherited_prepare.clone();
    invalid.sources[0].session_id = source.into();
    invalid.sources[0].branch = branch.into();
    assert!(store.context_prepare(owner, invalid).is_err());
    let mut invalid = inherited_prepare.clone();
    invalid.sources[0].selection = Selection::Standalone;
    assert!(store.context_prepare(owner, invalid).is_err());
    let mut invalid = inherited_prepare.clone();
    invalid.sources.push(prepare.sources[0].clone());
    assert!(store.context_prepare(owner, invalid).is_err());

    inherited_prepare.sources[0].selection = Selection::InheritedTurns { first: 2, last: 3 };
    let subset = store
        .context_prepare(owner, inherited_prepare.clone())
        .unwrap();
    assert_eq!(
        serde_json::to_value(&subset.evidence).unwrap(),
        serde_json::to_value(&whole.evidence[1..3]).unwrap()
    );
    assert_eq!(
        subset.sources[0].inherited_history,
        whole.sources[0].inherited_history
    );
    assert!(subset
        .evidence
        .iter()
        .all(|entry| entry.source.session_id == source));
    assert_eq!(subset.dependencies.len(), 2);
    assert!(store.context_plan(&other, &subset.id).is_err());
    assert!(store
        .summary_journal(owner, &subset.id, "inherited-paid")
        .is_err());
    let mut duplicate = inherited_prepare.clone();
    duplicate.sources.push(duplicate.sources[0].clone());
    let dedup = store.context_prepare(owner, duplicate).unwrap();
    assert_eq!(dedup.duplicate_nodes_omitted, 2);
    assert_eq!(
        serde_json::to_value(&dedup.evidence).unwrap(),
        serde_json::to_value(&subset.evidence).unwrap()
    );

    // A later local divergent tail does not change the inherited selection
    // or the retained original grant. A nested fork carries both spans.
    let local_target = store
        .context_target(owner, &inherited.id, &inherited_branch.id)
        .unwrap();
    let local_plan = store
        .context_prepare(
            owner,
            PrepareContext {
                target: local_target.clone(),
                ..inherited_prepare.clone()
            },
        )
        .unwrap();
    let local_artifact = store
        .context_manual(
            owner,
            &local_plan.id,
            "inherited-local-manual",
            "local divergent context",
            None,
        )
        .unwrap();
    store
        .context_apply(
            owner,
            ApplyContext {
                artifact_id: local_artifact.id,
                target: local_target,
                mode: ApplyMode::Append,
                request_id: "inherited-local-append".into(),
                confirmed_disclosure: true,
            },
        )
        .unwrap();
    let after_tail = store
        .context_prepare(
            owner,
            PrepareContext {
                sources: whole
                    .sources
                    .iter()
                    .map(|source| SourceSelection {
                        session_id: source.session_id.clone(),
                        branch: source.branch_id.clone(),
                        head: None,
                        selection: source.selection.clone(),
                    })
                    .collect(),
                ..inherited_prepare.clone()
            },
        )
        .unwrap();
    assert_eq!(
        after_tail.sources[0].inherited_history,
        whole.sources[0].inherited_history
    );
    assert_ne!(after_tail.sources[0].head, whole.sources[0].head);
    assert_eq!(
        serde_json::to_value(&after_tail.evidence).unwrap(),
        serde_json::to_value(&whole.evidence).unwrap()
    );

    let (nested, nested_branch) = store
        .create_agent_from_agent_history(
            owner,
            &inherited.id,
            &inherited_branch.id,
            "nested protected history",
            fresh_layout("nested-inherited-fork"),
            AgentHistoryForkAccess::Private,
        )
        .unwrap();
    let nested_plan = store
        .context_prepare(
            owner,
            PrepareContext {
                sources: vec![SourceSelection {
                    session_id: nested.id,
                    branch: nested_branch.id,
                    head: None,
                    selection: Selection::Inherited,
                }],
                ..inherited_prepare.clone()
            },
        )
        .unwrap();
    assert_eq!(nested_plan.evidence.len(), whole.evidence.len() + 1);
    assert_eq!(
        serde_json::to_value(&nested_plan.evidence[..whole.evidence.len()]).unwrap(),
        serde_json::to_value(&whole.evidence).unwrap()
    );
    assert_eq!(nested_plan.evidence.last().unwrap().kind, "context_import");
    assert_eq!(
        nested_plan.evidence.last().unwrap().source.session_id,
        inherited.id
    );
    assert!(store
        .summary_journal(owner, &nested_plan.id, "nested-paid")
        .is_err());

    let outside = store
        .create_workspace(owner, None, "outside protected transfer")
        .unwrap();
    let (outside_session, outside_branch) = store
        .create_in_workspace(
            owner,
            "outside context",
            fresh_layout("outside-context"),
            None,
            Some(&outside.id),
            None,
        )
        .unwrap();
    assert_ne!(outside_session.workspace_id, inherited.workspace_id);
    assert!(store
        .context_prepare(
            owner,
            PrepareContext {
                target: store
                    .context_target(owner, &outside_session.id, &outside_branch.id)
                    .unwrap(),
                ..inherited_prepare.clone()
            }
        )
        .is_err());

    // A carried manual context block in a different inherited prefix must
    // retain its own original provenance, not become ordinary local data.
    let carried = store
        .context_prepare(
            owner,
            PrepareContext {
                sources: vec![SourceSelection {
                    session_id: fork.id.clone(),
                    branch: fork_branch.id.clone(),
                    head: None,
                    selection: Selection::Inherited,
                }],
                ..inherited_prepare.clone()
            },
        )
        .unwrap();
    assert_eq!(carried.evidence.len(), 1);
    assert_eq!(carried.evidence[0].kind, "context_import");
    assert_eq!(carried.evidence[0].source.session_id, destination);
    assert!(carried
        .dependencies
        .iter()
        .any(|entry| entry.session_id == source));
    assert!(store
        .summary_journal(owner, &carried.id, "carried-paid")
        .is_err());

    let inherited_artifact = store
        .context_manual(
            owner,
            &subset.id,
            "inherited-manual",
            "two inherited source Turns",
            None,
        )
        .unwrap();
    let inherited_edit = store
        .context_manual(
            owner,
            &subset.id,
            "inherited-edit",
            "edited inherited source context",
            Some(inherited_artifact.id),
        )
        .unwrap();
    assert_eq!(
        inherited_edit.plan.sources[0].inherited_history,
        subset.sources[0].inherited_history
    );
    let inherited_apply = ApplyContext {
        artifact_id: inherited_edit.id.clone(),
        target: inherited_target,
        mode: ApplyMode::Append,
        request_id: "inherited-append".into(),
        confirmed_disclosure: true,
    };
    assert!(store
        .context_apply(
            owner,
            ApplyContext {
                mode: ApplyMode::NextPrompt,
                ..inherited_apply.clone()
            }
        )
        .is_err());
    store
        .grant(owner, &target_session.id, &other, Some(false))
        .unwrap();
    assert!(store.context_apply(owner, inherited_apply.clone()).is_err());
    store
        .grant(owner, &target_session.id, &other, None)
        .unwrap();
    let inherited_applied = store.context_apply(owner, inherited_apply.clone()).unwrap();
    assert_eq!(
        store.context_apply(owner, inherited_apply).unwrap().node_id,
        inherited_applied.node_id
    );
    assert!(store
        .grant(owner, &target_session.id, &other, Some(false))
        .is_err());
    assert!(store.export_jsonl(owner, &target_session.id).is_err());
    let transferred = store
        .read_agent_branch_view(owner, &target_session.id, &target_branch.id)
        .unwrap();
    assert_eq!(
        transferred.history.context_blocks[0].text,
        inherited_edit.text
    );
    assert_eq!(
        store.context_plan(owner, &subset.id).unwrap().sources[0].inherited_history,
        subset.sources[0].inherited_history
    );

    // Admission fails before a Turn is started; neither the original
    // private read nor exact history copy grants inference disclosure.
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    for (session, branch) in [
        (destination, applied.branch.id.as_str()),
        (fork.id.as_str(), fork_branch.id.as_str()),
        (target_session.id.as_str(), target_branch.id.as_str()),
    ] {
        let (view, handle) = store.activate(owner, session, branch).unwrap();
        let identity = InvocationIdentity {
            actor: owner.clone(),
            account_id: "fixture".into(),
            account_owner: AccountOwner::User(owner.clone()),
            payer: BillingOwner::User(owner.clone()),
            credential_custody: CredentialCustody::UserDevice("fixture".into()),
            credential_version: None,
            inference_executor: "local".into(),
            model: "fixture".into(),
            grant_revision: 1,
        };
        let result = runtime.block_on(handle.begin(TurnStart {
            turn_id: view.history.next_turn_id,
            prompt: "must not dispatch".into(),
            identity,
        }));
        assert!(
            matches!(result, Err(aw_agent_core::Error::Denied(_))),
            "{result:?}"
        );
        assert_eq!(
            store
                .read_agent_branch_view(owner, session, branch)
                .unwrap()
                .history
                .next_turn_id,
            view.history.next_turn_id
        );
    }
}
