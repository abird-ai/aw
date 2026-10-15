use super::owner::{SavedBranch, SavedSession, SavedSubagent};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Scope {
    #[default]
    User,
    Autonomous,
    Archived,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Target {
    Unsaved,
    Root { session: String },
    Branch { session: String, branch: String },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Row {
    pub(super) label: String,
    pub(super) target: Target,
}

pub(super) struct Projection<'a> {
    pub(super) sessions: &'a [SavedSession],
    pub(super) subagents: &'a [SavedSubagent],
    pub(super) active_session: Option<&'a str>,
    pub(super) active_branch: &'a str,
    pub(super) scope: Scope,
    pub(super) list: bool,
    pub(super) query: &'a str,
    pub(super) collapsed: &'a BTreeSet<String>,
    pub(super) include_unsaved: bool,
}

pub(super) fn rows(input: Projection<'_>) -> Vec<Row> {
    let query = input.query.to_lowercase();
    let want_archived = input.scope == Scope::Archived;
    let want_autonomous = input.scope == Scope::Autonomous;
    let autonomous_filter = (!want_archived).then_some(want_autonomous);
    let included_ids = input
        .sessions
        .iter()
        .filter(|session| {
            session.archived == want_archived
                && (want_archived || session.autonomous == want_autonomous)
        })
        .map(|session| session.id.as_str())
        .collect::<BTreeSet<_>>();
    let child_ids = input
        .subagents
        .iter()
        .filter(|relation| {
            included_ids.contains(relation.parent_session.as_str())
                && included_ids.contains(relation.child_session.as_str())
        })
        .map(|relation| relation.child_session.as_str())
        .collect::<BTreeSet<_>>();

    let mut out = Vec::new();
    if input.include_unsaved
        && !want_autonomous
        && !want_archived
        && (query.is_empty() || "new".contains(&query))
    {
        out.push(Row {
            label: "New ○".into(),
            target: Target::Unsaved,
        });
    }

    for session in input.sessions {
        if session.archived != want_archived
            || (!want_archived && session.autonomous != want_autonomous)
        {
            continue;
        }
        let title_matches = session.title.to_lowercase().contains(&query)
            || session.directory.to_lowercase().contains(&query);

        if input.list {
            push_list_row(&mut out, session, title_matches, &query, &input);
            continue;
        }

        if child_ids.contains(session.id.as_str()) {
            continue;
        }
        let child_matches = if query.is_empty() {
            input
                .subagents
                .iter()
                .any(|relation| relation.parent_session == session.id)
        } else {
            let mut visited = BTreeSet::new();
            subtree_matches(
                input.sessions,
                input.subagents,
                &session.id,
                &query,
                autonomous_filter,
                want_archived,
                &mut visited,
            )
        };
        let descendant_matches = session
            .branches
            .iter()
            .any(|branch| branch.name.to_lowercase().contains(&query))
            || child_matches;
        if !query.is_empty() && !title_matches && !descendant_matches {
            continue;
        }

        let collapsed = input.collapsed.contains(&session.id);
        let active = input.active_session == Some(session.id.as_str());
        out.push(Row {
            label: format!(
                "{}{} {}",
                if active { "*" } else { " " },
                if collapsed { ">" } else { "v" },
                session.title
            ),
            target: Target::Root {
                session: session.id.clone(),
            },
        });
        if collapsed {
            continue;
        }

        push_branch_tree(&mut out, session, active, input.active_branch, &query);
        let mut subagent_seen = BTreeSet::new();
        push_subagents(
            &mut out,
            &input,
            &session.id,
            1,
            &query,
            autonomous_filter,
            want_archived,
            title_matches,
            &mut subagent_seen,
        );
    }
    out
}

fn push_list_row(
    out: &mut Vec<Row>,
    session: &SavedSession,
    title_matches: bool,
    query: &str,
    input: &Projection<'_>,
) {
    let branch_matches = session
        .branches
        .iter()
        .any(|branch| branch.name.to_lowercase().contains(query));
    if !title_matches && !branch_matches {
        return;
    }
    let branch = if !title_matches && branch_matches {
        session
            .branches
            .iter()
            .find(|branch| branch.name.to_lowercase().contains(query))
    } else if input.active_session == Some(session.id.as_str()) {
        session
            .branches
            .iter()
            .find(|branch| branch.name == input.active_branch)
    } else {
        session
            .branches
            .iter()
            .find(|branch| branch.name == "main")
            .or_else(|| session.branches.first())
    };
    if let Some(branch) = branch {
        out.push(Row {
            label: session.title.clone(),
            target: Target::Branch {
                session: session.id.clone(),
                branch: branch.name.clone(),
            },
        });
    }
}

fn push_branch_tree(
    out: &mut Vec<Row>,
    session: &SavedSession,
    active_session: bool,
    active_branch: &str,
    query: &str,
) {
    let names = session
        .branches
        .iter()
        .map(|branch| branch.name.as_str())
        .collect::<BTreeSet<_>>();
    let mut children = BTreeMap::<String, Vec<&SavedBranch>>::new();
    for branch in &session.branches {
        let parent = branch
            .parent
            .as_deref()
            .filter(|parent| names.contains(parent))
            .unwrap_or("");
        children.entry(parent.to_owned()).or_default().push(branch);
    }

    struct BranchWalk<'a, 'b> {
        out: &'a mut Vec<Row>,
        children: &'a BTreeMap<String, Vec<&'b SavedBranch>>,
        session_id: &'b str,
        active_session: bool,
        active_branch: &'b str,
        query: &'b str,
        visited: &'a mut BTreeSet<String>,
    }

    impl BranchWalk<'_, '_> {
        fn walk(&mut self, parent: &str, depth: usize) {
            let Some(branches) = self.children.get(parent) else {
                return;
            };
            for (index, branch) in branches.iter().enumerate() {
                if !self.visited.insert(branch.name.clone()) {
                    continue;
                }
                let matches = self.query.is_empty()
                    || branch.name.to_lowercase().contains(self.query)
                    || self.children.get(&branch.name).is_some_and(|nested| {
                        nested
                            .iter()
                            .any(|child| child.name.to_lowercase().contains(self.query))
                    });
                if !matches {
                    continue;
                }
                let connector = if index + 1 == branches.len() { "`-" } else { "|-" };
                let active = self.active_session && self.active_branch == branch.name;
                self.out.push(Row {
                    label: format!(
                        "  {}{connector} {}{}",
                        "  ".repeat(depth),
                        if active { "* " } else { "" },
                        branch.name
                    ),
                    target: Target::Branch {
                        session: self.session_id.to_owned(),
                        branch: branch.name.clone(),
                    },
                });
                self.walk(&branch.name, depth + 1);
            }
        }
    }

    let mut visited = BTreeSet::new();
    BranchWalk {
        out,
        children: &children,
        session_id: &session.id,
        active_session,
        active_branch,
        query,
        visited: &mut visited,
    }
    .walk("", 0);
    for branch in &session.branches {
        if visited.insert(branch.name.clone())
            && (query.is_empty() || branch.name.to_lowercase().contains(query))
        {
            out.push(Row {
                label: format!("  ?- {}", branch.name),
                target: Target::Branch {
                    session: session.id.clone(),
                    branch: branch.name.clone(),
                },
            });
        }
    }
}

fn subtree_matches(
    sessions: &[SavedSession],
    subagents: &[SavedSubagent],
    parent_session: &str,
    query: &str,
    autonomous_filter: Option<bool>,
    archived: bool,
    visited: &mut BTreeSet<String>,
) -> bool {
    for relation in subagents
        .iter()
        .filter(|relation| relation.parent_session == parent_session)
    {
        if !visited.insert(relation.child_session.clone()) {
            continue;
        }
        let Some(child) = sessions
            .iter()
            .find(|candidate| candidate.id == relation.child_session)
        else {
            continue;
        };
        if child.archived != archived
            || autonomous_filter.is_some_and(|want| child.autonomous != want)
        {
            continue;
        }
        let direct = query.is_empty()
            || child.title.to_lowercase().contains(query)
            || child.directory.to_lowercase().contains(query)
            || child
                .branches
                .iter()
                .any(|branch| branch.name.to_lowercase().contains(query));
        if direct
            || subtree_matches(
                sessions,
                subagents,
                &child.id,
                query,
                autonomous_filter,
                archived,
                visited,
            )
        {
            return true;
        }
    }
    false
}

#[allow(clippy::too_many_arguments)]
fn push_subagents(
    out: &mut Vec<Row>,
    input: &Projection<'_>,
    parent_session: &str,
    depth: usize,
    query: &str,
    autonomous_filter: Option<bool>,
    archived: bool,
    ancestor_matches: bool,
    visited: &mut BTreeSet<String>,
) {
    for relation in input
        .subagents
        .iter()
        .filter(|relation| relation.parent_session == parent_session)
    {
        if !visited.insert(relation.child_session.clone()) {
            continue;
        }
        let Some(child) = input
            .sessions
            .iter()
            .find(|candidate| candidate.id == relation.child_session)
        else {
            continue;
        };
        if child.archived != archived
            || autonomous_filter.is_some_and(|want| child.autonomous != want)
        {
            continue;
        }
        let direct_matches = query.is_empty()
            || child.title.to_lowercase().contains(query)
            || child.directory.to_lowercase().contains(query)
            || child
                .branches
                .iter()
                .any(|branch| branch.name.to_lowercase().contains(query));
        let descendant_matches = if query.is_empty() {
            true
        } else {
            let mut search_seen = visited.clone();
            subtree_matches(
                input.sessions,
                input.subagents,
                &child.id,
                query,
                autonomous_filter,
                archived,
                &mut search_seen,
            )
        };
        if !query.is_empty() && !ancestor_matches && !direct_matches && !descendant_matches {
            continue;
        }

        let child_active = input.active_session == Some(child.id.as_str());
        let collapsed = input.collapsed.contains(&child.id);
        let state = match relation.state.as_str() {
            "running" => "●",
            "completed" => "✓",
            "failed" | "interrupted" => "!",
            "cancelled" => "×",
            _ => "○",
        };
        let authority = if relation.tool_mode == "off" {
            relation.mode.clone()
        } else {
            format!("{} · isolated", relation.tool_mode)
        };
        out.push(Row {
            label: format!(
                "{} `- {}{} {} · d{} · {}",
                "  ".repeat(depth),
                if child_active {
                    "*"
                } else if collapsed {
                    ">"
                } else {
                    "v"
                },
                state,
                child.title,
                relation.depth,
                authority,
            ),
            target: Target::Root {
                session: child.id.clone(),
            },
        });
        if collapsed {
            continue;
        }
        for branch in &child.branches {
            if query.is_empty()
                || ancestor_matches
                || direct_matches
                || branch.name.to_lowercase().contains(query)
            {
                out.push(Row {
                    label: format!(
                        "{}   `- {}{}",
                        "  ".repeat(depth),
                        if child_active && input.active_branch == branch.name {
                            "* "
                        } else {
                            ""
                        },
                        branch.name,
                    ),
                    target: Target::Branch {
                        session: child.id.clone(),
                        branch: branch.name.clone(),
                    },
                });
            }
        }
        push_subagents(
            out,
            input,
            &child.id,
            depth + 1,
            query,
            autonomous_filter,
            archived,
            ancestor_matches || direct_matches,
            visited,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(id: &str, archived: bool) -> SavedSession {
        SavedSession {
            id: id.into(),
            title: id.into(),
            directory: format!("/tmp/{id}"),
            autonomous: false,
            archived,
            updated_at: 0,
            turn_count: 1,
            branches: vec![SavedBranch {
                name: "main".into(),
                parent: None,
            }],
        }
    }

    #[test]
    fn visible_child_becomes_root_when_parent_is_archived() {
        let sessions = vec![session("parent", true), session("child", false)];
        let subagents = vec![SavedSubagent {
            parent_session: "parent".into(),
            child_session: "child".into(),
            state: "completed".into(),
            mode: "background".into(),
            tool_mode: "off".into(),
            depth: 1,
        }];
        let collapsed = BTreeSet::new();
        let visible = rows(Projection {
            sessions: &sessions,
            subagents: &subagents,
            active_session: None,
            active_branch: "main",
            scope: Scope::User,
            list: false,
            query: "",
            collapsed: &collapsed,
            include_unsaved: false,
        });
        assert!(visible.iter().any(|row| {
            matches!(&row.target, Target::Root { session } if session == "child")
        }));
        assert!(!visible.iter().any(|row| row.label.contains("parent")));
    }

    #[test]
    fn list_branch_query_targets_the_matching_branch() {
        let mut value = session("work", false);
        value.branches.push(SavedBranch {
            name: "feature-x".into(),
            parent: Some("main".into()),
        });
        let sessions = vec![value];
        let collapsed = BTreeSet::new();
        let visible = rows(Projection {
            sessions: &sessions,
            subagents: &[],
            active_session: None,
            active_branch: "main",
            scope: Scope::User,
            list: true,
            query: "feature-x",
            collapsed: &collapsed,
            include_unsaved: false,
        });
        assert!(matches!(
            &visible[0].target,
            Target::Branch { branch, .. } if branch == "feature-x"
        ));
    }
}
