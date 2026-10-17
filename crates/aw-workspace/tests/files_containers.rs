use aw_workspace::*;
use serde_json::{json, Value};

fn scope(id: &str) -> ScopeId {
    ScopeId(id.into())
}

fn add_files(workspace: &mut Workspace, scope_id: ScopeId) -> (Id, Id, Id) {
    workspace
        .apply(Action::AddFiles {
            beside: 2,
            side: Side::Below,
            scope_id,
        })
        .unwrap();
    let navigator = workspace.focused;
    let container = workspace.files_container_for(navigator).unwrap();
    let group = workspace.files_group_for(navigator).unwrap();
    (container, navigator, group)
}

fn legacy(workspace: &Workspace, version: u32) -> Value {
    let mut value = serde_json::to_value(workspace).unwrap();
    value["version"] = json!(version);
    value.as_object_mut().unwrap().remove("containers");
    value.as_object_mut().unwrap().remove("next_container_id");
    value.as_object_mut().unwrap().remove("scope_labels");
    for tab in value["tabs"].as_object_mut().unwrap().values_mut() {
        if tab["target"]["kind"] == "file" {
            tab["target"].as_object_mut().unwrap().remove("scope_id");
        }
    }
    value
}

#[test]
fn equal_relative_paths_in_two_scopes_are_distinct_resources() {
    let mut workspace = Workspace::default();
    let original = workspace.tabs.clone();
    let (first, first_nav, first_group) = add_files(&mut workspace, scope("first"));
    let (second, second_nav, second_group) = add_files(&mut workspace, scope("second"));
    assert_ne!(first, second);
    for (group, scope_id) in [
        (first_group, scope("first")),
        (second_group, scope("second")),
    ] {
        workspace
            .apply(Action::Open {
                group,
                target: Target::File {
                    scope_id,
                    path: "src/lib.rs".into(),
                },
                pinned: true,
            })
            .unwrap();
    }
    assert_ne!(
        workspace.active(first_group).unwrap().target,
        workspace.active(second_group).unwrap().target
    );
    assert_eq!(workspace.files_scope_for(first_nav), Some(&scope("first")));
    assert_eq!(
        workspace.files_scope_for(second_nav),
        Some(&scope("second"))
    );
    assert_eq!(workspace.tabs[&3], original[&3]);
    assert_eq!(workspace.tabs[&4], original[&4]);
    workspace.validate().unwrap();
}

#[test]
fn split_move_and_roundtrip_keep_scope_container_and_exact_tab_identity() {
    let mut workspace = Workspace::default();
    let (container, navigator, group) = add_files(&mut workspace, scope("root-a"));
    workspace
        .apply(Action::Open {
            group,
            target: Target::File {
                scope_id: scope("root-a"),
                path: "src/lib.rs".into(),
            },
            pinned: true,
        })
        .unwrap();
    let tab = workspace.active(group).unwrap().clone();
    workspace
        .apply(Action::SplitGroup {
            pane: group,
            side: Side::Right,
        })
        .unwrap();
    let split = workspace.focused;
    assert_eq!(workspace.files_container_for(split), Some(container));
    workspace
        .apply(Action::MoveTab {
            tab: tab.id,
            group: split,
        })
        .unwrap();
    workspace
        .apply(Action::MovePane {
            pane: split,
            beside: 2,
            side: Side::Above,
        })
        .unwrap();
    assert_eq!(workspace.tabs[&tab.id], tab);
    assert_eq!(workspace.files_container_for(split), Some(container));
    assert_eq!(workspace.files_group_for(navigator), Some(group));
    workspace
        .apply(Action::MoveTab {
            tab: tab.id,
            group: 2,
        })
        .unwrap();
    assert_eq!(workspace.tabs[&tab.id], tab);
    assert_eq!(workspace.files_scope_for(2), None);
    let restored: Workspace =
        serde_json::from_slice(&serde_json::to_vec(&workspace).unwrap()).unwrap();
    assert_eq!(restored.migrate(&ScopeId::launch()).unwrap(), workspace);
}

#[test]
fn explicit_binding_joins_same_scope_but_cannot_retarget_another_scope_group() {
    let mut workspace = Workspace::default();
    let (container, _, group) = add_files(&mut workspace, scope("root-a"));
    workspace
        .apply(Action::BindFiles {
            navigator: 1,
            group,
            scope_id: scope("root-a"),
        })
        .unwrap();
    assert_eq!(workspace.files_container_for(1), Some(container));
    assert_eq!(workspace.containers[&container].navigators.len(), 2);
    let before = workspace.clone();
    assert!(workspace
        .apply(Action::BindFiles {
            navigator: 1,
            group,
            scope_id: scope("root-b")
        })
        .is_err());
    assert_eq!(workspace, before);
}

#[test]
fn generic_launch_navigators_skip_nearer_groups_owned_by_other_scopes() {
    for add in [false, true] {
        let mut workspace = Workspace::default();
        workspace
            .apply(Action::AddFiles {
                beside: 2,
                side: Side::Left,
                scope_id: scope("external"),
            })
            .unwrap();
        let external = workspace.focused;
        let external_container = workspace.files_container_for(external).unwrap();
        let external_group = workspace.files_group_for(external).unwrap();
        assert!(
            workspace
                .ordered_panes()
                .iter()
                .position(|id| *id == external_group)
                < workspace.ordered_panes().iter().position(|id| *id == 2)
        );
        let action = if add {
            Action::AddNavigator {
                beside: 1,
                side: Side::Below,
                kind: Navigator::Files,
            }
        } else {
            Action::SetNavigator {
                pane: 1,
                kind: Navigator::Files,
            }
        };
        workspace.apply(action).unwrap();
        let launch = if add { workspace.focused } else { 1 };
        assert_eq!(workspace.files_group_for(launch), Some(2));
        assert_eq!(workspace.files_scope_for(launch), Some(&ScopeId::launch()));
        assert_eq!(
            workspace.files_container_for(external),
            Some(external_container)
        );
        assert_eq!(
            workspace.files_scope_for(external_group),
            Some(&scope("external"))
        );
    }
}

#[test]
fn switching_or_closing_navigators_preserves_document_tabs_and_container_lifetime() {
    let mut workspace = Workspace::default();
    let (container, navigator, group) = add_files(&mut workspace, scope("root-a"));
    workspace
        .apply(Action::BindFiles {
            navigator: 1,
            group,
            scope_id: scope("root-a"),
        })
        .unwrap();
    workspace
        .apply(Action::Open {
            group,
            target: Target::File {
                scope_id: scope("root-a"),
                path: "a.rs".into(),
            },
            pinned: true,
        })
        .unwrap();
    let tabs = workspace.tabs.clone();
    workspace
        .apply(Action::SetNavigator {
            pane: navigator,
            kind: Navigator::Git,
        })
        .unwrap();
    assert_eq!(workspace.files_container_for(1), Some(container));
    assert_eq!(workspace.containers[&container].navigators, vec![1]);
    assert_eq!(workspace.tabs, tabs);
    workspace.apply(Action::ClosePane(1)).unwrap();
    assert!(workspace.containers.is_empty());
    assert!(workspace.panes.contains_key(&group));
    assert_eq!(workspace.tabs, tabs);
}

#[test]
fn closing_last_owned_group_removes_only_its_navigators_and_rejects_final_global_group() {
    let mut workspace = Workspace::default();
    let (container, navigator, group) = add_files(&mut workspace, scope("root-a"));
    workspace.apply(Action::Focus(navigator)).unwrap();
    workspace.apply(Action::ClosePane(group)).unwrap();
    assert!(!workspace.panes.contains_key(&navigator));
    assert!(!workspace.containers.contains_key(&container));
    assert_eq!(workspace.focused, 2);
    assert!(workspace.tabs.contains_key(&3));
    workspace
        .apply(Action::BindFiles {
            navigator: 1,
            group: 2,
            scope_id: scope("root-a"),
        })
        .unwrap();
    let before = workspace.clone();
    assert!(workspace.apply(Action::ClosePane(2)).is_err());
    assert_eq!(workspace, before);
}

#[test]
fn closing_one_split_group_keeps_other_members_and_stable_container_id() {
    let mut workspace = Workspace::default();
    let (container, navigator, group) = add_files(&mut workspace, scope("root-a"));
    workspace
        .apply(Action::SplitGroup {
            pane: group,
            side: Side::Below,
        })
        .unwrap();
    let other = workspace.focused;
    workspace.apply(Action::ClosePane(group)).unwrap();
    assert_eq!(workspace.files_container_for(navigator), Some(container));
    assert_eq!(workspace.files_group_for(navigator), Some(other));
}

#[test]
fn container_ids_are_monotonic_and_independent_of_reused_navigator_membership() {
    let mut workspace = Workspace::default();
    let (first, navigator, group) = add_files(&mut workspace, scope("root-a"));
    workspace
        .apply(Action::BindFiles {
            navigator: 1,
            group,
            scope_id: scope("root-a"),
        })
        .unwrap();
    workspace
        .apply(Action::SetNavigator {
            pane: navigator,
            kind: Navigator::Git,
        })
        .unwrap();
    workspace
        .apply(Action::BindFiles {
            navigator,
            group: 2,
            scope_id: scope("root-b"),
        })
        .unwrap();
    let second = workspace.files_container_for(navigator).unwrap();
    assert!(second > first);
    assert_eq!(workspace.files_container_for(1), Some(first));
    workspace.apply(Action::ClosePane(navigator)).unwrap();
    workspace
        .apply(Action::SetNavigator {
            pane: 1,
            kind: Navigator::Sessions,
        })
        .unwrap();
    workspace
        .apply(Action::BindFiles {
            navigator: 1,
            group: 2,
            scope_id: scope("root-c"),
        })
        .unwrap();
    assert!(workspace.files_container_for(1).unwrap() > second);
}

#[test]
fn all_global_limits_and_id_exhaustion_reject_container_creation_atomically() {
    let mut workspace = Workspace::default();
    let (_, _, group) = add_files(&mut workspace, scope("root-a"));
    add_files(&mut workspace, scope("root-b"));
    for _ in 0..2 {
        workspace
            .apply(Action::SplitGroup {
                pane: group,
                side: Side::Below,
            })
            .unwrap();
    }
    assert_eq!(workspace.panes.len(), MAX_PANES);
    let before = workspace.clone();
    assert!(workspace
        .apply(Action::AddFiles {
            beside: 2,
            side: Side::Right,
            scope_id: scope("overflow")
        })
        .is_err());
    assert_eq!(workspace, before);
    let mut exhausted = Workspace::default();
    exhausted.next_container_id = u64::MAX;
    let before = exhausted.clone();
    assert!(exhausted
        .apply(Action::BindFiles {
            navigator: 1,
            group: 2,
            scope_id: scope("root-a")
        })
        .is_err());
    assert_eq!(exhausted, before);
}

#[test]
fn malformed_membership_and_new_authority_fields_are_rejected() {
    let mut workspace = Workspace::default();
    let (container, navigator, group) = add_files(&mut workspace, scope("root-a"));
    for mutation in 0..8 {
        let mut invalid = workspace.clone();
        let entry = invalid.containers.get_mut(&container).unwrap();
        match mutation {
            0 => entry.navigators.push(navigator),
            1 => entry.groups.push(group),
            2 => entry.groups.push(999),
            3 => entry.groups.push(navigator),
            4 => entry.navigators.push(group),
            5 => entry.groups.clear(),
            6 => entry.navigators.clear(),
            _ => entry.scope_id = ScopeId::default(),
        }
        assert!(invalid.validate().is_err(), "mutation {mutation}");
    }
    let mut duplicate = workspace.clone();
    duplicate.containers.insert(
        duplicate.next_container_id,
        duplicate.containers[&container].clone(),
    );
    duplicate.next_container_id += 1;
    assert!(duplicate.validate().is_err());
    let mut orphan = workspace.clone();
    orphan.containers.clear();
    assert!(orphan.validate().is_err());
    let mut injected = serde_json::to_value(&workspace).unwrap();
    injected["containers"][container.to_string()]["root"] = json!("/secret");
    assert!(serde_json::from_value::<Workspace>(injected).is_err());
}

#[test]
fn scope_ids_are_bounded_opaque_and_never_normalized() {
    let mut workspace = Workspace::default();
    for invalid in [
        String::new(),
        "x".repeat(129),
        "é".repeat(65),
        "root\nnext".into(),
        "root\0next".into(),
    ] {
        let before = workspace.clone();
        assert!(workspace
            .apply(Action::AddFiles {
                beside: 2,
                side: Side::Below,
                scope_id: ScopeId(invalid)
            })
            .is_err());
        assert_eq!(workspace, before);
    }
    let exact = scope(" root:/α ../ ");
    let (_, navigator, group) = add_files(&mut workspace, exact.clone());
    assert_eq!(workspace.files_scope_for(navigator), Some(&exact));
    workspace
        .apply(Action::Open {
            group,
            target: Target::File {
                scope_id: exact,
                path: "src/lib.rs".into(),
            },
            pinned: false,
        })
        .unwrap();
}

#[test]
fn legacy_full_layout_with_two_files_navigators_preserves_geometry_ids_focus_and_limits() {
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
            kind: Navigator::Files,
        })
        .unwrap();
    for _ in 0..5 {
        workspace
            .apply(Action::SplitGroup {
                pane: 2,
                side: Side::Below,
            })
            .unwrap();
    }
    workspace.next_id = u64::MAX;
    for version in [1, 2] {
        let old = legacy(&workspace, version);
        let decoded: Workspace = serde_json::from_value(old.clone()).unwrap();
        let migrated = decoded.migrate(&scope("authorized-launch")).unwrap();
        let current = serde_json::to_value(&migrated).unwrap();
        for field in ["root", "panes", "tabs", "focused", "next_id"] {
            assert_eq!(current[field], old[field], "{field}, schema {version}");
        }
        assert_eq!(migrated.panes.len(), MAX_PANES);
        assert_eq!(migrated.next_id, u64::MAX);
        assert_eq!(migrated.containers.len(), 1);
        assert_eq!(
            migrated
                .containers
                .values()
                .next()
                .unwrap()
                .navigators
                .len(),
            2
        );
        assert_eq!(
            migrated.containers.values().next().unwrap().scope_id,
            scope("authorized-launch")
        );
    }
}

#[test]
fn legacy_migration_owns_file_groups_without_adding_panes_and_current_missing_scope_fails() {
    let mut workspace = Workspace::default();
    workspace
        .apply(Action::SetNavigator {
            pane: 1,
            kind: Navigator::Files,
        })
        .unwrap();
    workspace
        .apply(Action::SplitGroup {
            pane: 2,
            side: Side::Right,
        })
        .unwrap();
    let split = workspace.focused;
    workspace
        .apply(Action::Open {
            group: split,
            target: Target::File {
                scope_id: ScopeId::launch(),
                path: "src/lib.rs".into(),
            },
            pinned: true,
        })
        .unwrap();
    let old = legacy(&workspace, 2);
    let migrated: Workspace = serde_json::from_value(old).unwrap();
    let migrated = migrated.migrate(&scope("authorized-launch")).unwrap();
    assert_eq!(migrated.panes.len(), workspace.panes.len());
    assert_eq!(
        migrated.containers.values().next().unwrap().groups,
        vec![split]
    );
    assert!(
        matches!(&migrated.active(split).unwrap().target, Target::File { scope_id, .. } if scope_id == &scope("authorized-launch"))
    );
    let mut missing = serde_json::to_value(&migrated).unwrap();
    let tab = migrated.active(split).unwrap().id.to_string();
    missing["tabs"][tab]["target"]
        .as_object_mut()
        .unwrap()
        .remove("scope_id");
    let missing: Workspace = serde_json::from_value(missing).unwrap();
    assert!(missing.migrate(&ScopeId::launch()).is_err());
}

#[test]
fn legacy_version_cannot_smuggle_current_scopes_or_container_graphs() {
    let mut workspace = Workspace::default();
    add_files(&mut workspace, scope("requested"));
    workspace.version = 2;
    assert!(workspace.migrate(&ScopeId::launch()).is_err());
    let mut workspace = Workspace::default();
    workspace
        .apply(Action::Open {
            group: 2,
            target: Target::File {
                scope_id: scope("requested"),
                path: "src/lib.rs".into(),
            },
            pinned: true,
        })
        .unwrap();
    workspace.version = 2;
    assert!(workspace.migrate(&ScopeId::launch()).is_err());
}

#[test]
fn scope_labels_are_display_only_and_pruned_only_after_the_last_resource_reference() {
    let mut workspace = Workspace::default();
    let (_, navigator, group) = add_files(&mut workspace, scope("root-a"));
    workspace
        .apply(Action::Open {
            group,
            target: Target::File {
                scope_id: scope("root-a"),
                path: "src/lib.rs".into(),
            },
            pinned: true,
        })
        .unwrap();
    let tab = workspace.active(group).unwrap().clone();
    for label in ["C:\\work\\alpha 雪", "C:\\work\\renamed label"] {
        workspace
            .apply(Action::SetScopeLabel {
                scope_id: scope("root-a"),
                label: label.into(),
            })
            .unwrap();
        assert_eq!(workspace.tabs[&tab.id], tab);
        assert_eq!(workspace.scope_labels[&scope("root-a")], label);
    }
    let restored: Workspace =
        serde_json::from_slice(&serde_json::to_vec(&workspace).unwrap()).unwrap();
    assert_eq!(restored, workspace);
    workspace.apply(Action::ClosePane(navigator)).unwrap();
    assert!(workspace.scope_labels.contains_key(&scope("root-a")));
    workspace
        .apply(Action::CloseTab { group, tab: tab.id })
        .unwrap();
    assert!(workspace.scope_labels.is_empty());
}

#[test]
fn labels_are_bounded_and_cannot_be_orphaned_or_smuggled_into_legacy_layouts() {
    let mut workspace = Workspace::default();
    add_files(&mut workspace, scope("root-a"));
    for label in [
        String::new(),
        "x".repeat(MAX_SCOPE_LABEL_BYTES + 1),
        "é".repeat(MAX_SCOPE_LABEL_BYTES / 2 + 1),
        "root\nnext".into(),
        "root\0next".into(),
    ] {
        let before = workspace.clone();
        assert!(workspace
            .apply(Action::SetScopeLabel {
                scope_id: scope("root-a"),
                label
            })
            .is_err());
        assert_eq!(workspace, before);
    }
    workspace
        .apply(Action::SetScopeLabel {
            scope_id: scope("root-a"),
            label: "é".repeat(MAX_SCOPE_LABEL_BYTES / 2),
        })
        .unwrap();
    let before = workspace.clone();
    assert!(workspace
        .apply(Action::SetScopeLabel {
            scope_id: scope("unknown"),
            label: "ungranted path".into()
        })
        .is_err());
    assert_eq!(workspace, before);
    let mut invalid = Workspace::default();
    invalid
        .scope_labels
        .insert(scope("unreferenced"), "display only".into());
    assert!(invalid.validate().is_err());
    let mut too_many = workspace.clone();
    for n in 0..=MAX_SCOPE_LABELS {
        too_many
            .scope_labels
            .insert(scope(&format!("extra-{n}")), "display".into());
    }
    assert!(too_many.validate().is_err());
    let mut old = legacy(&workspace, 2);
    old["scope_labels"] = json!({"root-a":"ungranted path"});
    let old: Workspace = serde_json::from_value(old).unwrap();
    assert!(old.migrate(&ScopeId::launch()).is_err());
}
