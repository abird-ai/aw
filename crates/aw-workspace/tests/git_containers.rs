use aw_workspace::*;
use serde_json::{json, Value};

fn context(name: &str) -> GitContextId {
    GitContextId(name.into())
}

fn add_git(workspace: &mut Workspace, name: &str) -> (Id, Id, Id) {
    workspace
        .apply(Action::AddGit {
            beside: 2,
            side: Side::Below,
            context_id: context(name),
        })
        .unwrap();
    let navigator = workspace.focused;
    (
        workspace.container_for(navigator).unwrap(),
        navigator,
        workspace.container_group_for(navigator).unwrap(),
    )
}

fn v3(workspace: &Workspace) -> Value {
    let mut value = serde_json::to_value(workspace).unwrap();
    value["version"] = json!(3);
    value.as_object_mut().unwrap().remove("git_labels");
    for container in value["containers"].as_object_mut().unwrap().values_mut() {
        container.as_object_mut().unwrap().remove("git_contexts");
    }
    value
}

#[test]
fn two_git_containers_keep_exact_contexts_diff_ids_and_group_membership() {
    let mut workspace = Workspace::default();
    let agent = workspace.tabs[&3].clone();
    let (first, nav_a, group_a) = add_git(&mut workspace, "checkout-a");
    let (second, nav_b, group_b) = add_git(&mut workspace, "checkout-b");
    assert_ne!(first, second);
    assert_eq!(
        workspace.git_context_for(nav_a),
        Some(&context("checkout-a"))
    );
    assert_eq!(
        workspace.git_context_for(nav_b),
        Some(&context("checkout-b"))
    );
    assert_eq!(workspace.files_scope_for(nav_a), None);
    assert_eq!(workspace.files_scope_for(group_a), None);
    assert_eq!(workspace.git_context_for(group_a), None);
    for (group, review) in [(group_a, "review_a"), (group_b, "review_b")] {
        workspace
            .apply(Action::Open {
                group,
                target: Target::Diff {
                    review: review.into(),
                },
                pinned: false,
            })
            .unwrap();
    }
    let diff = workspace.active(group_a).unwrap().clone();
    workspace
        .apply(Action::SplitGroup {
            pane: group_a,
            side: Side::Right,
        })
        .unwrap();
    let split = workspace.focused;
    assert_eq!(workspace.container_for(split), Some(first));
    workspace
        .apply(Action::MoveTab {
            tab: diff.id,
            group: group_b,
        })
        .unwrap();
    assert_eq!(workspace.tabs[&diff.id], diff);
    workspace
        .apply(Action::MovePane {
            pane: split,
            beside: group_b,
            side: Side::Below,
        })
        .unwrap();
    assert_eq!(workspace.container_for(split), Some(first));
    assert_eq!(workspace.tabs[&3], agent);
    assert_eq!(
        serde_json::from_slice::<Workspace>(&serde_json::to_vec(&workspace).unwrap()).unwrap(),
        workspace
    );
}

#[test]
fn mixed_navigators_rebind_only_the_selected_checkout_and_keep_dormant_bindings() {
    let mut workspace = Workspace::default();
    workspace
        .apply(Action::SetNavigator {
            pane: 1,
            kind: Navigator::Files,
        })
        .unwrap();
    let owner = workspace.container_for(1).unwrap();
    for _ in 0..2 {
        workspace
            .apply(Action::AddNavigator {
                beside: 1,
                side: Side::Below,
                kind: Navigator::Git,
            })
            .unwrap();
    }
    let git = workspace.containers[&owner]
        .git_contexts
        .keys()
        .copied()
        .collect::<Vec<_>>();
    let before = workspace.tabs.clone();
    workspace
        .apply(Action::SetGitContext {
            navigator: git[0],
            context_id: context("selected-checkout"),
        })
        .unwrap();
    assert_eq!(
        workspace.git_context_for(git[1]),
        Some(&GitContextId::launch())
    );
    for kind in [Navigator::Sessions, Navigator::Files, Navigator::Git] {
        workspace
            .apply(Action::SetNavigator { pane: git[0], kind })
            .unwrap();
        assert_eq!(
            workspace.containers[&owner].git_contexts[&git[0]],
            context("selected-checkout")
        );
        assert_eq!(
            workspace.containers[&owner].files_scope,
            Some(ScopeId::launch())
        );
        assert_eq!(workspace.container_for(git[0]), Some(owner));
    }
    assert_eq!(
        workspace.git_context_for(git[0]),
        Some(&context("selected-checkout"))
    );
    assert_eq!(workspace.tabs, before);
}

#[test]
fn closing_navigators_releases_only_their_context_until_last_member_closes() {
    let mut workspace = Workspace::default();
    let (owner, navigator, group) = add_git(&mut workspace, "checkout-a");
    workspace
        .apply(Action::BindFiles {
            navigator: 1,
            group,
            scope_id: ScopeId::launch(),
        })
        .unwrap();
    workspace
        .apply(Action::SetGitLabel {
            context_id: context("checkout-a"),
            label: "display only".into(),
        })
        .unwrap();
    workspace.apply(Action::ClosePane(navigator)).unwrap();
    assert_eq!(workspace.container_for(1), Some(owner));
    assert!(workspace.containers[&owner].git_contexts.is_empty());
    assert!(workspace.git_labels.is_empty());
    workspace.apply(Action::ClosePane(1)).unwrap();
    assert!(workspace.containers.is_empty());
    assert!(workspace.panes.contains_key(&group));
    assert!(workspace.tabs.contains_key(&3));
}

#[test]
fn closing_last_git_group_removes_all_mixed_navigators_but_preserves_other_groups() {
    let mut workspace = Workspace::default();
    let (owner, git, group) = add_git(&mut workspace, "checkout-a");
    workspace
        .apply(Action::BindFiles {
            navigator: 1,
            group,
            scope_id: ScopeId::launch(),
        })
        .unwrap();
    workspace.apply(Action::Focus(git)).unwrap();
    workspace.apply(Action::ClosePane(group)).unwrap();
    assert!(!workspace.containers.contains_key(&owner));
    assert!(!workspace.panes.contains_key(&git));
    assert!(!workspace.panes.contains_key(&1));
    assert_eq!(workspace.focused, 2);
    assert!(workspace.tabs.contains_key(&3));
}

#[test]
fn context_ids_labels_and_malformed_membership_are_rejected_atomically() {
    let mut workspace = Workspace::default();
    let (owner, navigator, _) = add_git(&mut workspace, "checkout-a");
    for bad in [
        String::new(),
        "x".repeat(129),
        "é".repeat(65),
        "a\nb".into(),
        "a\0b".into(),
    ] {
        let before = workspace.clone();
        assert!(workspace
            .apply(Action::SetGitContext {
                navigator,
                context_id: GitContextId(bad)
            })
            .is_err());
        assert_eq!(workspace, before);
    }
    for bad in [String::new(), "é".repeat(513), "a\nb".into()] {
        let before = workspace.clone();
        assert!(workspace
            .apply(Action::SetGitLabel {
                context_id: context("checkout-a"),
                label: bad
            })
            .is_err());
        assert_eq!(workspace, before);
    }
    workspace
        .apply(Action::SetGitLabel {
            context_id: context("checkout-a"),
            label: "é".repeat(512),
        })
        .unwrap();
    let mut malformed = workspace.clone();
    malformed
        .containers
        .get_mut(&owner)
        .unwrap()
        .git_contexts
        .insert(999, context("orphan"));
    assert!(malformed.validate().is_err());
    let mut unbound = workspace.clone();
    unbound
        .containers
        .get_mut(&owner)
        .unwrap()
        .git_contexts
        .clear();
    assert!(unbound.validate().is_err());
    let mut injected = serde_json::to_value(&workspace).unwrap();
    injected["containers"][owner.to_string()]["root"] = json!("/ungranted");
    assert!(serde_json::from_value::<Workspace>(injected).is_err());
    workspace
        .apply(Action::SetGitContext {
            navigator,
            context_id: context("checkout-b"),
        })
        .unwrap();
    assert!(workspace.git_labels.is_empty());
}

#[test]
fn git_containers_share_existing_global_limits_and_never_reuse_exhausted_ids() {
    let mut workspace = Workspace::default();
    let (_, _, group) = add_git(&mut workspace, "checkout-a");
    add_git(&mut workspace, "checkout-b");
    for _ in 0..2 {
        workspace
            .apply(Action::SplitGroup {
                pane: group,
                side: Side::Below,
            })
            .unwrap();
    }
    let before = workspace.clone();
    assert_eq!(workspace.panes.len(), MAX_PANES);
    assert!(workspace
        .apply(Action::AddGit {
            beside: 2,
            side: Side::Below,
            context_id: context("overflow")
        })
        .is_err());
    assert_eq!(workspace, before);
    let mut exhausted = Workspace::default();
    exhausted.next_container_id = Id::MAX;
    let before = exhausted.clone();
    assert!(matches!(
        exhausted.apply(Action::SetNavigator {
            pane: 1,
            kind: Navigator::Git
        }),
        Err(LayoutError::Limit("container ID exhaustion"))
    ));
    assert_eq!(exhausted, before);
}

#[test]
fn v3_migration_joins_files_owner_without_changing_geometry_ids_or_exhausted_counters() {
    let mut workspace = Workspace::default();
    workspace
        .apply(Action::SetNavigator {
            pane: 1,
            kind: Navigator::Files,
        })
        .unwrap();
    workspace
        .apply(Action::AddNavigator {
            beside: 1,
            side: Side::Below,
            kind: Navigator::Sessions,
        })
        .unwrap();
    let git = workspace.focused;
    let mut old = v3(&workspace);
    old["panes"][git.to_string()]["kind"] = json!("git");
    old["next_id"] = json!(Id::MAX);
    old["next_container_id"] = json!(Id::MAX);
    let decoded: Workspace = serde_json::from_value(old.clone()).unwrap();
    let migrated = decoded.migrate(&ScopeId::launch()).unwrap();
    let current = serde_json::to_value(&migrated).unwrap();
    for field in [
        "root",
        "panes",
        "tabs",
        "focused",
        "next_id",
        "next_container_id",
    ] {
        assert_eq!(current[field], old[field], "{field}");
    }
    assert_eq!(migrated.containers.len(), 1);
    assert_eq!(migrated.container_for(git), migrated.container_for(1));
    assert_eq!(migrated.files_scope_for(1), Some(&ScopeId::launch()));
    assert_eq!(migrated.git_context_for(git), Some(&GitContextId::launch()));
}

#[test]
fn exhausted_legacy_allocator_reuses_existing_owner_or_refuses_without_partial_state() {
    let mut workspace = Workspace::default();
    workspace
        .apply(Action::AddFiles {
            beside: 2,
            side: Side::Right,
            scope_id: ScopeId("external-files".into()),
        })
        .unwrap();
    let mut old = v3(&workspace);
    old["panes"]["1"]["kind"] = json!("git");
    old["next_container_id"] = json!(Id::MAX);
    let decoded: Workspace = serde_json::from_value(old.clone()).unwrap();
    let migrated = decoded.migrate(&ScopeId::launch()).unwrap();
    assert_eq!(migrated.containers.len(), 1);
    assert_eq!(migrated.next_container_id, Id::MAX);
    assert_eq!(
        serde_json::to_value(&migrated).unwrap()["root"],
        old["root"]
    );
    assert_eq!(
        migrated.containers.values().next().unwrap().files_scope,
        Some(ScopeId("external-files".into()))
    );
    let mut impossible = v3(&Workspace::default());
    impossible["panes"]["1"]["kind"] = json!("git");
    impossible["next_container_id"] = json!(Id::MAX);
    let decoded: Workspace = serde_json::from_value(impossible).unwrap();
    assert!(matches!(
        decoded.migrate(&ScopeId::launch()),
        Err(LayoutError::Limit("container ID exhaustion"))
    ));
}

#[test]
fn mixed_or_malformed_v3_bindings_cannot_be_healed_by_migration() {
    let mut workspace = Workspace::default();
    workspace
        .apply(Action::SetNavigator {
            pane: 1,
            kind: Navigator::Files,
        })
        .unwrap();
    let owner = workspace.container_for(1).unwrap();
    let old = v3(&workspace);
    for modification in 0..4 {
        let mut invalid = old.clone();
        match modification {
            0 => invalid["containers"][owner.to_string()]["git_contexts"] = json!({"1":"injected"}),
            1 => invalid["git_labels"] = json!({"injected":"display"}),
            2 => invalid["containers"][owner.to_string()]["navigators"] = json!([]),
            _ => invalid["containers"][owner.to_string()]["groups"] = json!([]),
        }
        let decoded: Workspace = serde_json::from_value(invalid).unwrap();
        assert!(
            decoded.migrate(&ScopeId::launch()).is_err(),
            "modification {modification}"
        );
    }
}
