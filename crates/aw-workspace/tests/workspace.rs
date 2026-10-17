use aw_workspace::*;
fn group(w: &Workspace) -> Id {
    w.groups()[0]
}
#[test]
fn default_is_valid() {
    Workspace::default().validate().unwrap();
}
#[test]
fn default_workspace_starts_with_sessions_explorer_chat_and_22_5_percent_sidebar() {
    let w = Workspace::default();
    assert_eq!(
        w.panes.get(&1),
        Some(&Pane::Navigator {
            kind: Navigator::Sessions
        })
    );
    assert!(matches!(
        w.active(2).map(|tab| &tab.target),
        Some(Target::Conversation)
    ));
    assert!(matches!(
        w.root,
        Node::Split {
            axis: Axis::Columns,
            ratio: 2250,
            ..
        }
    ));
}
#[test]
fn serde_round_trip() {
    let w = Workspace::default();
    let bytes = serde_json::to_vec(&w).unwrap();
    assert_eq!(serde_json::from_slice::<Workspace>(&bytes).unwrap(), w);
}
#[test]
fn schema_version_rejected() {
    let w = Workspace {
        version: 9,
        ..Workspace::default()
    };
    assert!(w.validate().is_err());
}
#[test]
fn dangling_active_rejected() {
    let mut w = Workspace::default();
    if let Some(Pane::Group { active, .. }) = w.panes.get_mut(&2) {
        *active = Some(999);
    }
    assert!(w.validate().is_err());
}
#[test]
fn duplicate_placement_rejected() {
    let mut w = Workspace::default();
    if let Some(Pane::Group { tabs, .. }) = w.panes.get_mut(&2) {
        tabs.push(3);
    }
    assert!(w.validate().is_err());
}
#[test]
fn invalid_operation_is_atomic() {
    let mut w = Workspace::default();
    let before = w.clone();
    assert!(w.apply(Action::ClosePane(2)).is_err());
    assert_eq!(w, before);
}
#[test]
fn navigator_switch_does_not_open_tab() {
    let mut w = Workspace::default();
    let tabs = w.tabs.clone();
    w.apply(Action::SetNavigator {
        pane: 1,
        kind: Navigator::Git,
    })
    .unwrap();
    assert_eq!(w.tabs, tabs);
}
#[test]
fn at_most_two_instances_of_kind() {
    let mut w = Workspace::default();
    // Add the first Files navigator; the default starts on Sessions.
    w.apply(Action::AddNavigator {
        beside: 1,
        side: Side::Below,
        kind: Navigator::Files,
    })
    .unwrap();
    w.apply(Action::AddNavigator {
        beside: 1,
        side: Side::Below,
        kind: Navigator::Files,
    })
    .unwrap();
    let before = w.clone();
    assert!(w
        .apply(Action::AddNavigator {
            beside: 1,
            side: Side::Below,
            kind: Navigator::Files
        })
        .is_err());
    assert_eq!(w, before);
}
#[test]
fn split_makes_agnostic_empty_group() {
    let mut w = Workspace::default();
    w.apply(Action::SplitGroup {
        pane: 2,
        side: Side::Right,
    })
    .unwrap();
    assert_eq!(w.groups().len(), 2);
    assert_eq!(w.tabs.len(), 2);
}
#[test]
fn tab_move_preserves_identity_target_pin() {
    let mut w = Workspace::default();
    w.apply(Action::SplitGroup {
        pane: 2,
        side: Side::Right,
    })
    .unwrap();
    let to = w.focused;
    let tab = w.tabs[&3].clone();
    w.apply(Action::MoveTab { tab: 3, group: to }).unwrap();
    assert_eq!(w.tabs[&3], tab);
    assert_eq!(w.group_for(3), Some(to));
}
#[test]
fn group_move_keeps_contents() {
    let mut w = Workspace::default();
    let panes = w.panes.clone();
    let tabs = w.tabs.clone();
    w.apply(Action::MovePane {
        pane: 1,
        beside: 2,
        side: Side::Right,
    })
    .unwrap();
    assert_eq!(w.panes, panes);
    assert_eq!(w.tabs, tabs);
    assert_eq!(w.ordered_panes(), vec![2, 1]);
}
#[test]
fn files_reuse_only_unpinned_file_preview() {
    let mut w = Workspace::default();
    for path in ["src/a.rs", "src/b.rs"] {
        w.apply(Action::Open {
            group: 2,
            target: Target::File {
                scope_id: ScopeId::launch(),
                path: path.into(),
            },
            pinned: false,
        })
        .unwrap();
    }
    assert_eq!(w.tabs.len(), 3);
    assert_eq!(w.tabs[&3].target, Target::Conversation);
    assert_eq!(w.tabs[&4].target, Target::Activity);
}
#[test]
fn pin_protects_file_preview() {
    let mut w = Workspace::default();
    w.apply(Action::Open {
        group: 2,
        target: Target::File {
            scope_id: ScopeId::launch(),
            path: "a".into(),
        },
        pinned: true,
    })
    .unwrap();
    w.apply(Action::Open {
        group: 2,
        target: Target::File {
            scope_id: ScopeId::launch(),
            path: "b".into(),
        },
        pinned: false,
    })
    .unwrap();
    assert_eq!(w.tabs.len(), 4);
}
#[test]
fn same_resource_open_focuses_existing() {
    let mut w = Workspace::default();
    w.apply(Action::Open {
        group: 2,
        target: Target::Activity,
        pinned: true,
    })
    .unwrap();
    assert_eq!(w.tabs.len(), 2);
    assert_eq!(w.active(2).unwrap().id, 4);
}
#[test]
fn closing_view_does_not_mutate_other_refs() {
    let mut w = Workspace::default();
    w.apply(Action::CloseTab { group: 2, tab: 3 }).unwrap();
    assert_eq!(w.active(2).unwrap().target, Target::Activity);
}
#[test]
fn empty_group_is_valid() {
    let mut w = Workspace::default();
    w.apply(Action::CloseTab { group: 2, tab: 3 }).unwrap();
    w.apply(Action::CloseTab { group: 2, tab: 4 }).unwrap();
    assert!(w.active(2).is_none());
    w.validate().unwrap();
}
#[test]
fn hiding_navigator_keeps_content() {
    let mut w = Workspace::default();
    w.apply(Action::ClosePane(1)).unwrap();
    assert_eq!(w.panes.len(), 1);
    assert_eq!(w.tabs.len(), 2);
}
#[test]
fn invalid_paths_cannot_be_restored() {
    for path in ["../secret", "/etc/passwd", "a//b", "a/./b", "x\\y", ""] {
        assert!(!valid_relative(path));
    }
    assert!(valid_relative("src/hello.rs"));
}
#[test]
fn narrow_geometry_is_temporary() {
    let w = Workspace::default();
    let g = w
        .geometry(
            Rect {
                x: 0,
                y: 0,
                width: 40,
                height: 20,
            },
            false,
        )
        .unwrap();
    assert_eq!(g.visible.len(), 1);
    assert_eq!(g.hidden, 1);
    assert_eq!(w.panes.len(), 2);
}
#[test]
fn geometry_tiles_wide_area() {
    let w = Workspace::default();
    let g = w
        .geometry(
            Rect {
                x: 2,
                y: 3,
                width: 160,
                height: 40,
            },
            false,
        )
        .unwrap();
    assert_eq!(
        g.visible
            .iter()
            .map(|p| u32::from(p.area.width) * u32::from(p.area.height))
            .sum::<u32>(),
        6400
    );
    assert_eq!(g.hidden, 0);
}
#[test]
fn resizing_changes_only_weight() {
    let mut w = Workspace::default();
    let panes = w.panes.clone();
    w.apply(Action::ResizeParent {
        pane: 1,
        delta: 500,
    })
    .unwrap();
    assert_eq!(w.panes, panes);
    assert!(matches!(w.root, Node::Split { ratio: 2750, .. }));
}
#[test]
fn focus_cycle_visits_hidden_panes() {
    let mut w = Workspace::default();
    w.focus_next().unwrap();
    assert_eq!(w.focused, 1);
    w.focus_next().unwrap();
    assert_eq!(w.focused, 2);
}
#[test]
fn focus_missing_is_atomic() {
    let mut w = Workspace::default();
    let old = w.clone();
    assert!(w.apply(Action::Focus(999)).is_err());
    assert_eq!(w, old);
}
#[test]
fn empty_dimensions_safe() {
    let w = Workspace::default();
    assert!(w
        .geometry(Rect::default(), false)
        .unwrap()
        .visible
        .is_empty());
}
#[test]
fn unicode_grapheme_deletion() {
    let mut e = Editor::default();
    e.insert("e\u{301}🐦").unwrap();
    e.backspace();
    assert_eq!(e.text(), "e\u{301}");
    e.backspace();
    assert_eq!(e.text(), "");
}
#[test]
fn paste_does_not_submit() {
    let mut e = Editor::default();
    e.insert("hello\n/quit\nworld").unwrap();
    assert_eq!(e.text(), "hello\n/quit\nworld");
}
#[test]
fn limit_preserves_editor() {
    let mut e = Editor::default();
    e.set("x".repeat(aw_workspace::editor::MAX_DRAFT_BYTES))
        .unwrap();
    let old = e.clone();
    assert!(e.insert("x").is_err());
    assert_eq!(e, old);
}
#[test]
fn multiline_motion() {
    let mut e = Editor::default();
    e.set("ab\ncdef").unwrap();
    e.home();
    assert_eq!(e.cursor(), 3);
    e.right();
    e.vertical(false);
    assert_eq!(e.cursor(), 1);
    e.vertical(true);
    assert_eq!(e.cursor(), 4);
}
#[test]
fn terminal_escape_is_never_emitted() {
    let mut e = Editor::default();
    e.set("\x1b]52;hello\x07").unwrap();
    let v = e.view(80, 4);
    assert!(!v.lines.join("").contains('\x1b'));
    assert!(!v.lines.join("").contains('\x07'));
    assert!(e.text().contains('\x1b'));
}
#[test]
fn cursor_always_inside_view() {
    let mut e = Editor::default();
    e.set("hello\n世界 🐦\n".repeat(30)).unwrap();
    for (w, h) in [(1, 1), (10, 3), (80, 10)] {
        let v = e.view(w, h);
        assert!(v.cursor_x < w && v.cursor_y < h);
        assert!(v.lines.len() <= h as usize);
    }
}
#[test]
fn new_group_can_show_any_existing_target() {
    let mut w = Workspace::default();
    let original = group(&w);
    w.apply(Action::SplitGroup {
        pane: original,
        side: Side::Below,
    })
    .unwrap();
    let to = w.focused;
    for target in [
        Target::Conversation,
        Target::Activity,
        Target::SessionTree,
        Target::Help,
        Target::File {
            scope_id: ScopeId::launch(),
            path: "readme".into(),
        },
    ] {
        w.apply(Action::Open {
            group: to,
            target,
            pinned: true,
        })
        .unwrap();
    }
    w.validate().unwrap();
    assert_eq!(w.groups().len(), 2);
}

#[test]
fn ratios_respect_leaf_minimums() {
    let mut w = Workspace::default();
    w.apply(Action::ResizeParent {
        pane: 1,
        delta: 6500,
    })
    .unwrap();
    let g = w
        .geometry(
            Rect {
                x: 0,
                y: 0,
                width: 70,
                height: 30,
            },
            false,
        )
        .unwrap();
    assert_eq!(g.visible[0].area.width, 30);
    assert_eq!(g.visible[1].area.width, 40);
}
#[test]
fn deleting_can_join_graphemes_but_cursor_stays_valid() {
    let mut e = Editor::default();
    e.set("a\n\u{301}").unwrap();
    e.home();
    e.backspace();
    assert!(e.text().is_char_boundary(e.cursor()));
    e.insert("x").unwrap();
    e.left();
    e.right();
}

#[test]
fn crlf_stays_a_single_linebreak() {
    let mut e = Editor::default();
    e.set("ab\r\ncd").unwrap();
    e.home();
    e.left();
    assert_eq!(e.cursor(), 2);
    let v = e.view(20, 4);
    assert_eq!(v.lines, vec!["ab", "cd"]);
}
#[test]
fn end_does_not_split_crlf() {
    let mut e = Editor::default();
    e.set("ab\r\ncd").unwrap();
    e.home();
    e.vertical(false);
    e.end();
    assert_eq!(e.cursor(), 2);
    e.delete();
    assert_eq!(e.text(), "abcd");
}
#[test]
fn bidi_format_controls_are_visible_not_executed() {
    let out = aw_workspace::editor::display("a\u{200f}b\u{061c}c");
    assert!(!out.contains('\u{200f}'));
    assert!(out.contains("\\u{200f}"));
}

#[test]
fn terminal_move_preserves_target_and_id() {
    let mut w = Workspace::default();
    w.apply(Action::Open {
        group: 2,
        target: Target::Terminal {
            terminal: "pty_fixture".into(),
        },
        pinned: true,
    })
    .unwrap();
    let id = w.active(2).unwrap().id;
    w.apply(Action::SplitGroup {
        pane: 2,
        side: Side::Right,
    })
    .unwrap();
    let group = w.focused;
    w.apply(Action::MoveTab { tab: id, group }).unwrap();
    assert_eq!(w.active(group).unwrap().id, id);
    assert_eq!(
        w.tabs[&id].target,
        Target::Terminal {
            terminal: "pty_fixture".into()
        }
    );
}
#[test]
fn file_and_diff_have_separate_preview_slots() {
    let mut w = Workspace::default();
    for n in 0..3 {
        w.apply(Action::Open {
            group: 2,
            target: Target::File {
                scope_id: ScopeId::launch(),
                path: format!("f{n}"),
            },
            pinned: false,
        })
        .unwrap();
        w.apply(Action::Open {
            group: 2,
            target: Target::Diff {
                review: format!("review_{n}"),
            },
            pinned: false,
        })
        .unwrap();
    }
    assert_eq!(w.tabs.len(), 4);
}
#[test]
fn invalid_resource_identifier_is_atomic() {
    let mut w = Workspace::default();
    let before = w.clone();
    assert!(w
        .apply(Action::Open {
            group: 2,
            target: Target::Terminal {
                terminal: "../bad".into()
            },
            pinned: true
        })
        .is_err());
    assert_eq!(w, before);
}
#[test]
fn unknown_live_resource_is_only_descriptor() {
    let mut w = Workspace::default();
    w.apply(Action::Open {
        group: 2,
        target: Target::Terminal {
            terminal: "old_owner".into(),
        },
        pinned: true,
    })
    .unwrap();
    let bytes = serde_json::to_vec(&w).unwrap();
    let restored: Workspace = serde_json::from_slice(&bytes).unwrap();
    restored.validate().unwrap();
    assert_eq!(restored, w);
}

#[test]
fn ctrl_u_editor_primitive_preserves_prior_lines() {
    let mut editor = Editor::default();
    editor.set("alpha\nβeta").unwrap();
    editor.delete_to_line_start();
    assert_eq!(editor.text(), "alpha\n");
    assert_eq!(editor.cursor(), "alpha\n".len());
}
