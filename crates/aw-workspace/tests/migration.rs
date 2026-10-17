use aw_workspace::{Action, Rect, ScopeId, Side, Target, Workspace, MAX_PANES, MAX_TABS};
use serde_json::{json, Value};

fn agent_view(store: &str, session: &str, branch: &str) -> Target {
    Target::AgentView {
        store_id: store.into(),
        session_id: session.into(),
        branch_id: branch.into(),
    }
}

fn legacy_layout() -> Value {
    json!({
        "version": 1,
        "root": {"type":"split", "axis":"rows", "ratio":3750,
            "first":{"type":"pane", "id":1},
            "second":{"type":"split", "axis":"columns", "ratio":6200,
                "first":{"type":"pane", "id":2}, "second":{"type":"pane", "id":4}}},
        "panes": {
            "1":{"type":"navigator", "kind":"git"},
            "2":{"type":"group", "tabs":[3,5], "active":5},
            "4":{"type":"group", "tabs":[6], "active":6}
        },
        "tabs": {
            "3":{"id":3, "target":{"kind":"conversation"}, "pinned":false},
            "5":{"id":5, "target":{"kind":"terminal", "terminal":"pty_fixture"}, "pinned":true},
            "6":{"id":6, "target":{"kind":"file", "path":"src/lib.rs"}, "pinned":true}
        },
        "focused":4, "next_id":7
    })
}

#[test]
fn v1_migration_keeps_geometry_and_binds_files_to_authorized_launch_scope() {
    let mut expected = legacy_layout();
    let legacy: Workspace = serde_json::from_value(expected.clone()).unwrap();
    assert!(legacy.validate().is_err());
    let migrated = legacy.migrate(&ScopeId::launch()).unwrap();
    expected["version"] = json!(4);
    expected["containers"] =
        json!({"1":{"git_contexts":{"1":"launch"},"navigators":[1],"groups":[2]}});
    expected["next_container_id"] = json!(2);
    expected["scope_labels"] = json!({});
    expected["git_labels"] = json!({});
    expected["tabs"]["6"]["target"]["scope_id"] = json!("launch");
    assert_eq!(serde_json::to_value(&migrated).unwrap(), expected);
    assert_eq!(migrated.tabs[&3].target, Target::Conversation);
    migrated
        .geometry(
            Rect {
                width: 160,
                height: 50,
                ..Rect::default()
            },
            false,
        )
        .unwrap();
}

#[test]
fn v4_is_stable_and_unsupported_versions_never_migrate() {
    let current = Workspace::default();
    assert_eq!(current.version, 4);
    assert_eq!(Workspace::conversation_only().version, 4);
    assert_eq!(
        current.clone().migrate(&ScopeId::launch()).unwrap(),
        current
    );
    for version in [0, 5, u32::MAX] {
        let mut unsupported = current.clone();
        unsupported.version = version;
        assert!(unsupported.clone().migrate(&ScopeId::launch()).is_err());
        assert!(unsupported.apply(Action::Focus(current.focused)).is_err());
    }
}

#[test]
fn legacy_new_targets_and_invalid_graphs_are_rejected() {
    let mut new_target = legacy_layout();
    new_target["tabs"]["3"]["target"] =
        serde_json::to_value(agent_view("store", "session", "branch")).unwrap();
    let mut invalid_focus = legacy_layout();
    invalid_focus["focused"] = json!(999);
    let mut invalid_path = legacy_layout();
    invalid_path["tabs"]["6"]["target"]["path"] = json!("../secret");
    for value in [new_target, invalid_focus, invalid_path] {
        let legacy: Workspace = serde_json::from_value(value).unwrap();
        assert!(legacy.migrate(&ScopeId::launch()).is_err());
    }
}

#[test]
fn exact_descriptor_roundtrip_is_opaque_and_rejects_authority_fields() {
    let target = agent_view(" store:α /repo ", "Session#42", "branch/.. main");
    let encoded = serde_json::to_value(&target).unwrap();
    assert_eq!(
        encoded,
        json!({"kind":"agent_view", "store_id":" store:α /repo ",
            "session_id":"Session#42", "branch_id":"branch/.. main"})
    );
    let mut workspace = Workspace::conversation_only();
    workspace
        .apply(Action::Open {
            group: 1,
            target: target.clone(),
            pinned: true,
        })
        .unwrap();
    let bytes = serde_json::to_vec(&workspace).unwrap();
    assert_eq!(
        serde_json::from_slice::<Workspace>(&bytes).unwrap(),
        workspace
    );
    assert_eq!(workspace.active(1).unwrap().target, target);
    let mut injected = encoded;
    injected["actor"] = json!("another-user");
    assert!(serde_json::from_value::<Target>(injected).is_err());
}

#[test]
fn each_descriptor_id_is_bounded_and_control_free_without_normalizing() {
    for index in 0..3 {
        for invalid in [
            String::new(),
            "x".repeat(129),
            "é".repeat(65),
            "a\0b".into(),
            "a\nb".into(),
            "a\x1bb".into(),
            "a\u{85}b".into(),
        ] {
            let mut ids = ["store".to_string(), "session".into(), "branch".into()];
            ids[index] = invalid;
            let mut workspace = Workspace::conversation_only();
            let before = workspace.clone();
            assert!(workspace
                .apply(Action::Open {
                    group: 1,
                    target: agent_view(&ids[0], &ids[1], &ids[2]),
                    pinned: true,
                })
                .is_err());
            assert_eq!(workspace, before);
        }
        let mut ids = ["store".to_string(), "session".into(), "branch".into()];
        ids[index] = "é".repeat(64);
        let target = agent_view(&ids[0], &ids[1], &ids[2]);
        let mut workspace = Workspace::conversation_only();
        workspace
            .apply(Action::Open {
                group: 1,
                target: target.clone(),
                pinned: true,
            })
            .unwrap();
        assert_eq!(workspace.active(1).unwrap().target, target);
    }
}

#[test]
fn descriptor_identity_includes_store_session_and_branch() {
    let mut workspace = Workspace::conversation_only();
    for (store, session, branch) in [
        ("store", "session", "branch"),
        ("other-store", "session", "branch"),
        ("store", "other-session", "branch"),
        ("store", "session", "other-branch"),
    ] {
        workspace
            .apply(Action::Open {
                group: 1,
                target: agent_view(store, session, branch),
                pinned: false,
            })
            .unwrap();
    }
    assert_eq!(workspace.tabs.len(), 5);
    let same = agent_view("store", "session", "branch");
    let first = workspace
        .tabs
        .values()
        .find(|tab| tab.target == same)
        .unwrap()
        .id;
    workspace
        .apply(Action::Open {
            group: 1,
            target: same,
            pinned: true,
        })
        .unwrap();
    assert_eq!(workspace.tabs.len(), 5);
    assert_eq!(workspace.active(1).unwrap().id, first);
    assert!(workspace.tabs[&first].pinned);
}

#[test]
fn split_duplicate_views_keep_distinct_ids_through_move_and_close() {
    let mut workspace = Workspace::conversation_only();
    let follow = workspace.tabs[&2].clone();
    let target = agent_view("store", "session", "branch");
    workspace
        .apply(Action::Open {
            group: 1,
            target: target.clone(),
            pinned: true,
        })
        .unwrap();
    let first = workspace.active(1).unwrap().id;
    workspace
        .apply(Action::SplitGroup {
            pane: 1,
            side: Side::Right,
        })
        .unwrap();
    let second_group = workspace.focused;
    workspace
        .apply(Action::Open {
            group: second_group,
            target,
            pinned: false,
        })
        .unwrap();
    let second = workspace.active(second_group).unwrap().clone();
    assert_ne!(first, second.id);
    assert_eq!(workspace.tabs[&first].target, second.target);
    workspace
        .apply(Action::MoveTab {
            tab: second.id,
            group: 1,
        })
        .unwrap();
    assert_eq!(workspace.tabs[&second.id], second);
    assert_eq!(workspace.group_for(first), Some(1));
    assert_eq!(workspace.group_for(second.id), Some(1));
    workspace
        .apply(Action::CloseTab {
            group: 1,
            tab: first,
        })
        .unwrap();
    assert_eq!(workspace.tabs[&second.id], second);
    assert_eq!(workspace.tabs[&2], follow);
}

#[test]
fn agent_views_count_against_existing_global_limits_atomically() {
    let mut workspace = Workspace::conversation_only();
    for n in 1..MAX_TABS {
        workspace
            .apply(Action::Open {
                group: 1,
                target: agent_view("store", "session", &format!("branch-{n}")),
                pinned: true,
            })
            .unwrap();
    }
    let before = workspace.clone();
    assert!(workspace
        .apply(Action::Open {
            group: 1,
            target: agent_view("store", "session", "overflow"),
            pinned: true,
        })
        .is_err());
    assert_eq!(workspace, before);
    for _ in 1..MAX_PANES {
        workspace
            .apply(Action::SplitGroup {
                pane: workspace.focused,
                side: Side::Right,
            })
            .unwrap();
    }
    let before = workspace.clone();
    assert!(workspace
        .apply(Action::SplitGroup {
            pane: workspace.focused,
            side: Side::Below,
        })
        .is_err());
    assert_eq!(workspace, before);
}
