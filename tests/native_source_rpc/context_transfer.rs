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

    // Admission fails before a Turn is started; neither the original
    // private read nor exact history copy grants inference disclosure.
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    for (session, branch) in [
        (destination, applied.branch.id.as_str()),
        (fork.id.as_str(), fork_branch.id.as_str()),
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
