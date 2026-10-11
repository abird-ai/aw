use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub type Id = u64;
pub type Result<T> = std::result::Result<T, LayoutError>;
pub const MAX_PANES: usize = 8;
pub const MAX_TABS: usize = 32;
pub const MAX_DEPTH: usize = 12;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum LayoutError {
    #[error("invalid workspace layout: {0}")]
    Invalid(&'static str),
    #[error("workspace limit: {0}")]
    Limit(&'static str),
    #[error("workspace target not available")]
    Missing,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Navigator {
    Files,
    Git,
    Sessions,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Axis {
    Columns,
    Rows,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    Left,
    Right,
    Above,
    Below,
}
impl Side {
    fn axis(self) -> Axis {
        match self {
            Self::Left | Self::Right => Axis::Columns,
            _ => Axis::Rows,
        }
    }
    fn before(self) -> bool {
        matches!(self, Self::Left | Self::Above)
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Node {
    Split {
        axis: Axis,
        ratio: u16,
        first: Box<Node>,
        second: Box<Node>,
    },
    Pane {
        id: Id,
    },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Pane {
    Navigator { kind: Navigator },
    Group { tabs: Vec<Id>, active: Option<Id> },
}
/// Phase 5 surface descriptors (resource IDs do not grant authority). The host resolves resources and authorizes
/// reads; a descriptor by itself grants no access and cannot execute a command.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Target {
    Conversation,
    SessionTree,
    ContextFlow,
    Activity,
    File { path: String },
    Diff { review: String },
    Terminal { terminal: String },
    Settings,
    Help,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Tab {
    pub id: Id,
    pub target: Target,
    pub pinned: bool,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Workspace {
    pub version: u32,
    pub root: Node,
    pub panes: BTreeMap<Id, Pane>,
    pub tabs: BTreeMap<Id, Tab>,
    pub focused: Id,
    pub next_id: Id,
}
#[derive(Clone, Debug)]
pub enum Action {
    Focus(Id),
    SetNavigator {
        pane: Id,
        kind: Navigator,
    },
    AddNavigator {
        beside: Id,
        side: Side,
        kind: Navigator,
    },
    SplitGroup {
        pane: Id,
        side: Side,
    },
    Open {
        group: Id,
        target: Target,
        pinned: bool,
    },
    Select {
        group: Id,
        tab: Id,
    },
    Pin {
        tab: Id,
        pinned: bool,
    },
    MoveTab {
        tab: Id,
        group: Id,
    },
    CycleTab {
        group: Id,
        forward: bool,
    },
    CloseTab {
        group: Id,
        tab: Id,
    },
    ClosePane(Id),
    MovePane {
        pane: Id,
        beside: Id,
        side: Side,
    },
    ResizeParent {
        pane: Id,
        delta: i16,
    },
    /// Resize the split separating two visible descendant panes. This is used by
    /// pointer dragging; it changes layout geometry only and never resource state.
    ResizeBetween {
        first: Id,
        second: Id,
        delta: i16,
    },
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rect {
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Placement {
    pub pane: Id,
    pub area: Rect,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Geometry {
    pub visible: Vec<Placement>,
    pub hidden: usize,
}

impl Default for Workspace {
    fn default() -> Self {
        Self {
            version: 1,
            root: Node::Split {
                axis: Axis::Columns,
                ratio: 2250,
                first: Box::new(Node::Pane { id: 1 }),
                second: Box::new(Node::Pane { id: 2 }),
            },
            panes: BTreeMap::from([
                (
                    1,
                    Pane::Navigator {
                        kind: Navigator::Sessions,
                    },
                ),
                (
                    2,
                    Pane::Group {
                        tabs: vec![3, 4],
                        active: Some(3),
                    },
                ),
            ]),
            tabs: BTreeMap::from([
                (
                    3,
                    Tab {
                        id: 3,
                        target: Target::Conversation,
                        pinned: true,
                    },
                ),
                (
                    4,
                    Tab {
                        id: 4,
                        target: Target::Activity,
                        pinned: true,
                    },
                ),
            ]),
            focused: 2,
            next_id: 5,
        }
    }
}
impl Workspace {
    /// Minimal fullscreen preset: one conversation tab, no explorer pane until
    /// the user explicitly opens Files/Git/Sessions. Saved layouts are never
    /// rewritten to this shape.
    pub fn conversation_only() -> Self {
        Self {
            version: 1,
            root: Node::Pane { id: 1 },
            panes: BTreeMap::from([(
                1,
                Pane::Group {
                    tabs: vec![2],
                    active: Some(2),
                },
            )]),
            tabs: BTreeMap::from([(
                2,
                Tab {
                    id: 2,
                    target: Target::Conversation,
                    pinned: true,
                },
            )]),
            focused: 1,
            next_id: 3,
        }
    }

    pub fn validate(&self) -> Result<()> {
        if self.version != 1 {
            return Err(LayoutError::Invalid("unsupported schema"));
        }
        if self.panes.is_empty() || self.panes.len() > MAX_PANES || self.tabs.len() > MAX_TABS {
            return Err(LayoutError::Limit("pane/tab count"));
        }
        let mut leaves = BTreeSet::new();
        validate_node(&self.root, 0, &mut leaves)?;
        if leaves != self.panes.keys().copied().collect() {
            return Err(LayoutError::Invalid("pane references"));
        }
        if !self.panes.contains_key(&self.focused) {
            return Err(LayoutError::Invalid("focused pane"));
        }
        let mut seen = BTreeSet::new();
        let mut groups = 0;
        let mut counts = BTreeMap::new();
        for (&id, pane) in &self.panes {
            if id == 0 || id >= self.next_id {
                return Err(LayoutError::Invalid("pane identity"));
            }
            match pane {
                Pane::Navigator { kind } => {
                    let n = counts.entry(*kind).or_insert(0);
                    *n += 1;
                    if *n > 2 {
                        return Err(LayoutError::Limit("at most two navigators of each kind"));
                    }
                }
                Pane::Group { tabs, active } => {
                    groups += 1;
                    if (tabs.is_empty() && active.is_some())
                        || (!tabs.is_empty() && active.is_none())
                        || active.is_some_and(|a| !tabs.contains(&a))
                    {
                        return Err(LayoutError::Invalid("active tab"));
                    }
                    for tab in tabs {
                        if !seen.insert(*tab) {
                            return Err(LayoutError::Invalid("duplicate tab placement"));
                        }
                    }
                }
            }
        }
        if groups == 0 {
            return Err(LayoutError::Invalid("at least one content group"));
        }
        if seen != self.tabs.keys().copied().collect() {
            return Err(LayoutError::Invalid("tab references"));
        }
        for (&id, tab) in &self.tabs {
            if id == 0 || id >= self.next_id || tab.id != id || self.panes.contains_key(&id) {
                return Err(LayoutError::Invalid("tab identity"));
            }
            match &tab.target {
                Target::File { path } if !valid_relative(path) => {
                    return Err(LayoutError::Invalid("file path"))
                }
                Target::Diff { review } if !valid_resource_id(review) => {
                    return Err(LayoutError::Invalid("review ID"))
                }
                Target::Terminal { terminal } if !valid_resource_id(terminal) => {
                    return Err(LayoutError::Invalid("terminal ID"))
                }
                _ => {}
            }
        }
        Ok(())
    }
    /// Reject invalid mutations atomically: original view state remains intact.
    pub fn apply(&mut self, action: Action) -> Result<()> {
        self.validate()?;
        let mut next = self.clone();
        next.mutate(action)?;
        next.validate()?;
        *self = next;
        Ok(())
    }
    fn id(&mut self) -> Result<Id> {
        let id = self.next_id;
        self.next_id = id
            .checked_add(1)
            .ok_or(LayoutError::Limit("ID exhaustion"))?;
        Ok(id)
    }
    pub fn groups(&self) -> Vec<Id> {
        self.ordered_panes()
            .into_iter()
            .filter(|id| matches!(self.panes.get(id), Some(Pane::Group { .. })))
            .collect()
    }
    pub fn ordered_panes(&self) -> Vec<Id> {
        let mut out = Vec::new();
        collect(&self.root, &mut out);
        out
    }
    pub fn active(&self, group: Id) -> Option<&Tab> {
        match self.panes.get(&group) {
            Some(Pane::Group {
                active: Some(id), ..
            }) => self.tabs.get(id),
            _ => None,
        }
    }
    pub fn group_for(&self, tab: Id) -> Option<Id> {
        self.panes.iter().find_map(|(id, p)| match p {
            Pane::Group { tabs, .. } if tabs.contains(&tab) => Some(*id),
            _ => None,
        })
    }
    pub fn focus_next(&mut self) -> Result<()> {
        let all = self.ordered_panes();
        let i = all
            .iter()
            .position(|x| *x == self.focused)
            .ok_or(LayoutError::Missing)?;
        self.apply(Action::Focus(all[(i + 1) % all.len()]))
    }
    fn mutate(&mut self, action: Action) -> Result<()> {
        match action {
            Action::Focus(id) => {
                if !self.panes.contains_key(&id) {
                    return Err(LayoutError::Missing);
                }
                self.focused = id;
            }
            Action::SetNavigator { pane, kind } => match self.panes.get_mut(&pane) {
                Some(Pane::Navigator { kind: k }) => *k = kind,
                _ => return Err(LayoutError::Missing),
            },
            Action::AddNavigator { beside, side, kind } => {
                if !self.panes.contains_key(&beside) {
                    return Err(LayoutError::Missing);
                }
                let id = self.id()?;
                self.panes.insert(id, Pane::Navigator { kind });
                split_at(&mut self.root, beside, Node::Pane { id }, side)?;
                self.focused = id;
            }
            Action::SplitGroup { pane, side } => {
                if !matches!(self.panes.get(&pane), Some(Pane::Group { .. })) {
                    return Err(LayoutError::Missing);
                }
                let id = self.id()?;
                self.panes.insert(
                    id,
                    Pane::Group {
                        tabs: vec![],
                        active: None,
                    },
                );
                split_at(&mut self.root, pane, Node::Pane { id }, side)?;
                self.focused = id;
            }
            Action::Open {
                group,
                target,
                pinned,
            } => {
                let existing = match self.panes.get(&group) {
                    Some(Pane::Group { tabs, .. }) => tabs
                        .iter()
                        .copied()
                        .find(|id| self.tabs.get(id).is_some_and(|t| t.target == target)),
                    _ => return Err(LayoutError::Missing),
                };
                if let Some(id) = existing {
                    if let Some(Pane::Group { active, .. }) = self.panes.get_mut(&group) {
                        *active = Some(id);
                    }
                    if pinned {
                        if let Some(t) = self.tabs.get_mut(&id) {
                            t.pinned = true;
                        }
                    }
                    self.focused = group;
                    return Ok(());
                }
                let preview =
                    if matches!(target, Target::File { .. } | Target::Diff { .. }) && !pinned {
                        match self.panes.get(&group) {
                            Some(Pane::Group { tabs, .. }) => tabs.iter().copied().find(|id| {
                                self.tabs.get(id).is_some_and(|t| {
                                    !t.pinned
                                        && matches!(
                                            (&target, &t.target),
                                            (Target::File { .. }, Target::File { .. })
                                                | (Target::Diff { .. }, Target::Diff { .. })
                                        )
                                })
                            }),
                            _ => None,
                        }
                    } else {
                        None
                    };
                let id = match preview {
                    Some(id) => id,
                    None => self.id()?,
                };
                self.tabs.insert(id, Tab { id, target, pinned });
                if let Some(Pane::Group { tabs, active }) = self.panes.get_mut(&group) {
                    if preview.is_none() {
                        tabs.push(id);
                    }
                    *active = Some(id);
                }
                self.focused = group;
            }
            Action::Select { group, tab } => match self.panes.get_mut(&group) {
                Some(Pane::Group { tabs, active }) if tabs.contains(&tab) => {
                    *active = Some(tab);
                    self.focused = group;
                }
                _ => return Err(LayoutError::Missing),
            },
            Action::Pin { tab, pinned } => {
                self.tabs.get_mut(&tab).ok_or(LayoutError::Missing)?.pinned = pinned
            }
            Action::MoveTab { tab, group } => {
                let from = self.group_for(tab).ok_or(LayoutError::Missing)?;
                if !matches!(self.panes.get(&group), Some(Pane::Group { .. })) {
                    return Err(LayoutError::Missing);
                }
                if from != group {
                    remove_tab(self.panes.get_mut(&from).ok_or(LayoutError::Missing)?, tab)?;
                    if let Some(Pane::Group { tabs, active }) = self.panes.get_mut(&group) {
                        tabs.push(tab);
                        *active = Some(tab);
                    }
                }
                self.focused = group;
            }
            Action::CycleTab { group, forward } => {
                if let Some(Pane::Group { tabs, active }) = self.panes.get_mut(&group) {
                    if !tabs.is_empty() {
                        let i = active
                            .and_then(|a| tabs.iter().position(|x| *x == a))
                            .unwrap_or(0);
                        let n = if forward {
                            (i + 1) % tabs.len()
                        } else {
                            (i + tabs.len() - 1) % tabs.len()
                        };
                        *active = Some(tabs[n]);
                    }
                    self.focused = group;
                } else {
                    return Err(LayoutError::Missing);
                }
            }
            Action::CloseTab { group, tab } => {
                remove_tab(self.panes.get_mut(&group).ok_or(LayoutError::Missing)?, tab)?;
                self.tabs.remove(&tab);
            }
            Action::ClosePane(id) => {
                let pane = self.panes.get(&id).ok_or(LayoutError::Missing)?.clone();
                if let Pane::Group { tabs, .. } = pane {
                    if self.groups().len() == 1 {
                        return Err(LayoutError::Invalid("cannot remove final content group"));
                    }
                    for tab in tabs {
                        self.tabs.remove(&tab);
                    }
                }
                self.root =
                    remove_pane(&self.root, id).ok_or(LayoutError::Invalid("empty layout"))?;
                self.panes.remove(&id);
                if self.focused == id {
                    self.focused = self.groups()[0];
                }
            }
            Action::MovePane { pane, beside, side } => {
                if pane == beside
                    || !self.panes.contains_key(&pane)
                    || !self.panes.contains_key(&beside)
                {
                    return Err(LayoutError::Missing);
                }
                self.root =
                    remove_pane(&self.root, pane).ok_or(LayoutError::Invalid("empty layout"))?;
                split_at(&mut self.root, beside, Node::Pane { id: pane }, side)?;
                self.focused = pane;
            }
            Action::ResizeParent { pane, delta } => {
                if !resize_parent(&mut self.root, pane, delta) {
                    return Err(LayoutError::Missing);
                }
            }
            Action::ResizeBetween {
                first,
                second,
                delta,
            } => {
                if !resize_between(&mut self.root, first, second, delta) {
                    return Err(LayoutError::Missing);
                }
            }
        }
        Ok(())
    }
    pub fn geometry(&self, area: Rect, maximize: bool) -> Result<Geometry> {
        self.validate()?;
        let mut visible = vec![];
        if area.width == 0 || area.height == 0 {
            return Ok(Geometry {
                visible,
                hidden: self.panes.len(),
            });
        }
        let min = minimum(&self.root, &self.panes);
        if maximize || u32::from(area.width) < min.0 || u32::from(area.height) < min.1 {
            visible.push(Placement {
                pane: self.focused,
                area,
            });
            return Ok(Geometry {
                visible,
                hidden: self.panes.len() - 1,
            });
        }
        place(&self.root, area, &self.panes, &mut visible);
        Ok(Geometry { visible, hidden: 0 })
    }
}
pub fn valid_relative(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= 4096
        && !path.contains('\\')
        && !path.chars().any(char::is_control)
        && path
            .split('/')
            .all(|p| !p.is_empty() && p != "." && p != "..")
}
fn remove_tab(pane: &mut Pane, tab: Id) -> Result<()> {
    if let Pane::Group { tabs, active } = pane {
        let i = tabs
            .iter()
            .position(|x| *x == tab)
            .ok_or(LayoutError::Missing)?;
        tabs.remove(i);
        if *active == Some(tab) {
            *active = tabs.get(i.min(tabs.len().saturating_sub(1))).copied();
        }
        Ok(())
    } else {
        Err(LayoutError::Missing)
    }
}
fn collect(node: &Node, out: &mut Vec<Id>) {
    match node {
        Node::Pane { id } => out.push(*id),
        Node::Split { first, second, .. } => {
            collect(first, out);
            collect(second, out);
        }
    }
}
fn validate_node(node: &Node, depth: usize, seen: &mut BTreeSet<Id>) -> Result<()> {
    if depth > MAX_DEPTH {
        return Err(LayoutError::Limit("split depth"));
    }
    match node {
        Node::Pane { id } => {
            if !seen.insert(*id) {
                return Err(LayoutError::Invalid("duplicate pane leaf"));
            }
        }
        Node::Split {
            ratio,
            first,
            second,
            ..
        } => {
            if !(1000..=9000).contains(ratio) {
                return Err(LayoutError::Invalid("split weight"));
            }
            validate_node(first, depth + 1, seen)?;
            validate_node(second, depth + 1, seen)?;
        }
    }
    Ok(())
}
fn contains(node: &Node, id: Id) -> bool {
    match node {
        Node::Pane { id: n } => *n == id,
        Node::Split { first, second, .. } => contains(first, id) || contains(second, id),
    }
}
fn split_at(node: &mut Node, id: Id, added: Node, side: Side) -> Result<()> {
    match node {
        Node::Pane { id: n } if *n == id => {
            let old = node.clone();
            let (first, second) = if side.before() {
                (added, old)
            } else {
                (old, added)
            };
            *node = Node::Split {
                axis: side.axis(),
                ratio: 5000,
                first: Box::new(first),
                second: Box::new(second),
            };
            Ok(())
        }
        Node::Split { first, second, .. } => {
            if contains(first, id) {
                split_at(first, id, added, side)
            } else {
                split_at(second, id, added, side)
            }
        }
        _ => Err(LayoutError::Missing),
    }
}
fn remove_pane(node: &Node, id: Id) -> Option<Node> {
    match node {
        Node::Pane { id: n } => {
            if *n == id {
                None
            } else {
                Some(node.clone())
            }
        }
        Node::Split {
            axis,
            ratio,
            first,
            second,
        } => match (remove_pane(first, id), remove_pane(second, id)) {
            (Some(a), Some(b)) => Some(Node::Split {
                axis: *axis,
                ratio: *ratio,
                first: Box::new(a),
                second: Box::new(b),
            }),
            (Some(n), None) | (None, Some(n)) => Some(n),
            (None, None) => None,
        },
    }
}
fn resize_parent(node: &mut Node, id: Id, delta: i16) -> bool {
    match node {
        Node::Pane { .. } => false,
        Node::Split {
            ratio,
            first,
            second,
            ..
        } => {
            if matches!(first.as_ref(),Node::Pane{id:n} if *n==id)
                || matches!(second.as_ref(),Node::Pane{id:n} if *n==id)
            {
                *ratio = (i32::from(*ratio) + i32::from(delta)).clamp(1000, 9000) as u16;
                true
            } else {
                resize_parent(first, id, delta) || resize_parent(second, id, delta)
            }
        }
    }
}

fn resize_between(node: &mut Node, a: Id, b: Id, delta: i16) -> bool {
    match node {
        Node::Pane { .. } => false,
        Node::Split {
            ratio,
            first,
            second,
            ..
        } => {
            let a_first = contains(first, a);
            let a_second = contains(second, a);
            let b_first = contains(first, b);
            let b_second = contains(second, b);
            if (a_first && b_second) || (b_first && a_second) {
                let signed = if a_first {
                    delta
                } else {
                    delta.saturating_neg()
                };
                *ratio = (i32::from(*ratio) + i32::from(signed)).clamp(1000, 9000) as u16;
                true
            } else if a_first && b_first {
                resize_between(first, a, b, delta)
            } else if a_second && b_second {
                resize_between(second, a, b, delta)
            } else {
                false
            }
        }
    }
}
fn minimum(node: &Node, panes: &BTreeMap<Id, Pane>) -> (u32, u32) {
    match node {
        Node::Pane { id } => match panes.get(id) {
            Some(Pane::Navigator { .. }) => (20, 6),
            _ => (40, 8),
        },
        Node::Split {
            axis,
            first,
            second,
            ..
        } => {
            let a = minimum(first, panes);
            let b = minimum(second, panes);
            match axis {
                Axis::Columns => (a.0 + b.0, a.1.max(b.1)),
                Axis::Rows => (a.0.max(b.0), a.1 + b.1),
            }
        }
    }
}
fn place(node: &Node, area: Rect, panes: &BTreeMap<Id, Pane>, out: &mut Vec<Placement>) {
    match node {
        Node::Pane { id } => out.push(Placement { pane: *id, area }),
        Node::Split {
            axis,
            ratio,
            first,
            second,
        } => {
            let a = minimum(first, panes);
            let b = minimum(second, panes);
            match axis {
                Axis::Columns => {
                    let width = u32::from(area.width);
                    let n = (width * u32::from(*ratio) / 10000).clamp(a.0, width - b.0) as u16;
                    place(first, Rect { width: n, ..area }, panes, out);
                    place(
                        second,
                        Rect {
                            x: area.x.saturating_add(n),
                            width: area.width - n,
                            ..area
                        },
                        panes,
                        out,
                    );
                }
                Axis::Rows => {
                    let height = u32::from(area.height);
                    let n = (height * u32::from(*ratio) / 10000).clamp(a.1, height - b.1) as u16;
                    place(first, Rect { height: n, ..area }, panes, out);
                    place(
                        second,
                        Rect {
                            y: area.y.saturating_add(n),
                            height: area.height - n,
                            ..area
                        },
                        panes,
                        out,
                    );
                }
            }
        }
    }
}

fn valid_resource_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}
