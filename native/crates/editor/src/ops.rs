//! Editing operations: each builds a [`Command`] from the current document,
//! ready for [`crate::history::History::execute`]. They allocate new ids
//! from the layout, so ids are never reused even after an undo.
use crate::history::{Command, Document};
use anyhow::{ensure, Context, Result};
use sa_scene::world_layout::{AssetRef, Node, NodeId, Transform};
use sa_scene::world_logic;
use std::collections::{HashMap, HashSet};

/// Place a new object at the end of the tree (or of `parent`'s children).
pub fn place(
    doc: &mut Document,
    asset: AssetRef,
    name: &str,
    transform: Transform,
    parent: Option<NodeId>,
) -> Result<(Command, NodeId)> {
    ensure!(transform.is_valid(), "that position is outside the world");
    if let Some(group) = parent {
        ensure!(
            doc.layout.node(group).is_some_and(Node::is_group),
            "objects can only be placed inside a group"
        );
    }
    let id = doc.layout.allocate_id();
    // Gameplay assets (a door, a checkpoint) come with their behaviour.
    let gameplay = match &asset {
        AssetRef::Builtin(builtin) => {
            world_logic::preset(builtin, world_logic::next_checkpoint(&doc.layout))
        }
        _ => None,
    };
    let mut node = Node::object(id, unique_name(doc, name), asset, transform);
    node.parent = parent;
    node.gameplay = gameplay.map(world_logic::Kept::Known);
    if matches!(node.asset.as_ref(), Some(AssetRef::Builtin(b)) if b == "terrain") {
        node.terrain = Some(sa_scene::builtin_assets::preset_terrain());
    }
    node.light = match node.asset.as_ref() {
        Some(AssetRef::Builtin(builtin)) => sa_scene::builtin_assets::light_preset(builtin),
        _ => None,
    };
    Ok((Command::Insert(vec![(doc.layout.nodes.len(), node)]), id))
}

/// `name`, or `name 2`, `name 3`, ... if an object already has that name.
pub fn unique_name(doc: &Document, name: &str) -> String {
    unique_name_besides(doc, name, &HashSet::new())
}

/// [`unique_name`], also avoiding `more` (names given earlier in the same
/// paste, which are not in the document yet).
fn unique_name_besides(doc: &Document, name: &str, more: &HashSet<String>) -> String {
    let mut taken: HashSet<&str> = doc.layout.nodes.iter().map(|n| n.name.as_str()).collect();
    taken.extend(more.iter().map(String::as_str));
    if !taken.contains(name) {
        return name.to_owned();
    }
    let base = name
        .trim_end_matches(|c: char| c.is_ascii_digit())
        .trim_end();
    (2..)
        .map(|i| format!("{base} {i}"))
        .find(|candidate| !taken.contains(candidate.as_str()))
        .unwrap()
}

/// `ids` plus everything inside any group among them, in tree order.
pub fn with_descendants(doc: &Document, ids: &[NodeId]) -> Vec<NodeId> {
    let mut all: HashSet<NodeId> = ids.iter().copied().collect();
    for id in ids {
        all.extend(doc.layout.descendants(*id));
    }
    doc.layout
        .nodes
        .iter()
        .map(|n| n.id)
        .filter(|id| all.contains(id))
        .collect()
}

/// Delete objects (and the contents of deleted groups).
pub fn delete(doc: &Document, ids: &[NodeId]) -> Result<Command> {
    let ids = with_descendants(doc, ids);
    ensure!(!ids.is_empty(), "nothing selected");
    let nodes = ids
        .iter()
        .map(|id| {
            let index = doc.layout.index_of(*id).context("object vanished")?;
            Ok((index, doc.layout.nodes[index].clone()))
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(Command::Remove(nodes))
}

/// Copy objects (groups with their contents), moved by `offset` metres in
/// Blender axes. Returns the command and the new ids of the copied roots.
pub fn duplicate(
    doc: &mut Document,
    ids: &[NodeId],
    offset: [f32; 3],
) -> Result<(Command, Vec<NodeId>)> {
    let nodes = copy(doc, ids)?;
    paste(doc, &nodes, offset)
}

/// The objects `ids` (groups with their contents) as they are now, in tree
/// order: what Copy puts on the editor's clipboard.
pub fn copy(doc: &Document, ids: &[NodeId]) -> Result<Vec<Node>> {
    let all = with_descendants(doc, ids);
    ensure!(!all.is_empty(), "nothing selected");
    all.iter()
        .map(|id| doc.layout.node(*id).cloned().context("object vanished"))
        .collect()
}

/// Where a pasted object whose group was not pasted with it goes: back into
/// that group while it exists and can be edited (shown, not locked),
/// otherwise the top level, so a paste is never born hidden or locked.
fn paste_parent(doc: &Document, parent: Option<NodeId>) -> Option<NodeId> {
    parent.filter(|p| {
        doc.layout.node(*p).is_some_and(Node::is_group)
            && doc.layout.effective_visible(*p)
            && !doc.layout.effective_locked(*p)
    })
}

/// Put cut objects (from [`copy`], then deleted) back with their own ids,
/// so links to them from other objects (teleport destinations, shown or
/// hidden objects) and checkpoint numbers still hold. None when any of them
/// exists again (an undo brought it back): paste copies then.
pub fn paste_cut(doc: &Document, cut: &[Node]) -> Option<(Command, Vec<NodeId>)> {
    if cut.is_empty() || cut.iter().any(|n| doc.layout.node(n.id).is_some()) {
        return None;
    }
    let ids: HashSet<NodeId> = cut.iter().map(|n| n.id).collect();
    let mut names = HashSet::new();
    let mut roots = Vec::new();
    let start = doc.layout.nodes.len();
    let inserted = cut
        .iter()
        .enumerate()
        .map(|(i, node)| {
            let mut node = node.clone();
            if !node.parent.is_some_and(|p| ids.contains(&p)) {
                roots.push(node.id);
                node.parent = paste_parent(doc, node.parent);
                node.name = unique_name_besides(doc, &node.name, &names);
                names.insert(node.name.clone());
            }
            (start + i, node)
        })
        .collect();
    Some((Command::Insert(inserted), roots))
}

/// Insert copies of `copied` (from [`copy`]) with new ids, moved by
/// `offset` metres in Blender axes. A copied object goes back into its
/// group when that still exists and can be edited, else the top level.
/// Returns the command and the new ids of the pasted roots.
pub fn paste(
    doc: &mut Document,
    copied: &[Node],
    offset: [f32; 3],
) -> Result<(Command, Vec<NodeId>)> {
    ensure!(!copied.is_empty(), "nothing to paste");
    let mut remap = HashMap::new();
    for node in copied {
        remap.insert(node.id, doc.layout.allocate_id());
    }
    let mut nodes = Vec::new();
    let mut roots = Vec::new();
    let mut names = HashSet::new();
    for node in copied {
        let mut copy = node.clone();
        copy.id = remap[&node.id];
        let parent_copied = copy.parent.and_then(|p| remap.get(&p).copied());
        match parent_copied {
            Some(parent) => copy.parent = Some(parent),
            None => {
                roots.push(copy.id);
                copy.name = unique_name_besides(doc, &copy.name, &names);
                names.insert(copy.name.clone());
                copy.parent = paste_parent(doc, copy.parent);
            }
        }
        for (axis, delta) in offset.iter().enumerate() {
            copy.transform.position[axis] += delta;
        }
        nodes.push(copy);
    }
    // Gameplay of the copies: actions point at the copies of objects copied
    // along with them, and checkpoints take the next free numbers.
    let mut next_order: HashMap<String, u32> = HashMap::new();
    for node in &mut nodes {
        let Some(world_logic::Kept::Known(component)) = &mut node.gameplay else {
            continue;
        };
        if let world_logic::Component::Checkpoint(checkpoint) = component {
            let next = next_order
                .entry(checkpoint.race.clone())
                .or_insert_with(|| {
                    doc.layout
                        .nodes
                        .iter()
                        .filter_map(|n| match n.gameplay.as_ref()?.known()? {
                            world_logic::Component::Checkpoint(c) if c.race == checkpoint.race => {
                                Some(c.order)
                            }
                            _ => None,
                        })
                        .max()
                        .unwrap_or(0)
                        + 1
                });
            checkpoint.order = *next;
            *next += 1;
        }
        for list in component.action_lists_mut() {
            for action in list.iter_mut().filter_map(world_logic::Kept::known_mut) {
                let target = match action {
                    world_logic::Action::Teleport { target } => target,
                    world_logic::Action::Show { node }
                    | world_logic::Action::Hide { node }
                    | world_logic::Action::Toggle { node } => node,
                    _ => continue,
                };
                if let Some(copied) = target.and_then(|t| remap.get(&t)) {
                    *target = Some(*copied);
                }
            }
        }
    }
    let start = doc.layout.nodes.len();
    let inserted = nodes
        .into_iter()
        .enumerate()
        .map(|(i, node)| (start + i, node))
        .collect();
    Ok((Command::Insert(inserted), roots))
}

/// One change to several nodes: `edit` is called on a copy of each.
pub fn modify(
    doc: &Document,
    label: &str,
    ids: &[NodeId],
    mut edit: impl FnMut(&mut Node),
) -> Result<Command> {
    let mut changes = Vec::new();
    for id in ids {
        let before = doc.layout.node(*id).context("object vanished")?.clone();
        let mut after = before.clone();
        edit(&mut after);
        ensure!(after.id == before.id, "an edit may not change ids");
        if after != before {
            changes.push((before, after));
        }
    }
    Ok(Command::Modify {
        label: label.into(),
        changes,
    })
}

pub fn rename(doc: &Document, id: NodeId, name: &str) -> Result<Command> {
    let name = name.trim();
    ensure!(!name.is_empty(), "a name cannot be empty");
    ensure!(name.len() <= 256, "that name is too long");
    let name = name.to_owned();
    modify(doc, &format!("Rename to {name}"), &[id], |n| {
        n.name = name.clone()
    })
}

/// Move `ids` into a new group placed where the first of them was.
pub fn group(doc: &mut Document, ids: &[NodeId], name: &str) -> Result<(Command, NodeId)> {
    ensure!(!ids.is_empty(), "select objects to group");
    // Only the topmost selected nodes move; their children come along.
    let selected: HashSet<NodeId> = ids.iter().copied().collect();
    let roots: Vec<NodeId> = ids
        .iter()
        .copied()
        .filter(|id| {
            !doc.layout
                .ancestry(*id)
                .skip(1)
                .any(|ancestor| selected.contains(&ancestor.id))
        })
        .collect();
    let first = roots
        .iter()
        .filter_map(|id| doc.layout.index_of(*id))
        .min()
        .context("object vanished")?;
    let parent = doc.layout.nodes[first].parent;
    // Every root must share the new group's parent's subtree or be moved out
    // of its own; moving keeps them where they are in the tree order.
    let id = doc.layout.allocate_id();
    let mut group = Node::group(id, unique_name(doc, name));
    group.parent = parent;
    let insert = Command::Insert(vec![(first, group)]);
    let mut probe = doc.clone();
    insert.apply(&mut probe)?;
    let reparent = modify(&probe, "", &roots, |n| n.parent = Some(id))?;
    Ok((
        Command::Batch {
            label: format!("Group {} objects", roots.len()),
            commands: vec![insert, reparent],
        },
        id,
    ))
}

/// Dissolve a group: its children move up one level and the group goes.
pub fn ungroup(doc: &Document, group: NodeId) -> Result<Command> {
    let node = doc.layout.node(group).context("group vanished")?;
    ensure!(node.is_group(), "{} is not a group", node.name);
    let parent = node.parent;
    let children: Vec<NodeId> = doc
        .layout
        .nodes
        .iter()
        .filter(|n| n.parent == Some(group))
        .map(|n| n.id)
        .collect();
    let lift = modify(doc, "", &children, |n| n.parent = parent)?;
    let mut probe = doc.clone();
    lift.apply(&mut probe)?;
    let index = probe.layout.index_of(group).context("group vanished")?;
    Ok(Command::Batch {
        label: format!("Ungroup {}", node.name),
        commands: vec![
            lift,
            Command::Remove(vec![(index, probe.layout.nodes[index].clone())]),
        ],
    })
}

/// Move `ids` into `parent` (a group) or to the top level.
/// Where dragged objects land in the World Outliner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Drop {
    /// Just before this object, in its group.
    Before(NodeId),
    /// Just after this object, in its group.
    After(NodeId),
    /// Last inside this group.
    Into(NodeId),
    /// Last at the top level.
    End,
}

/// Move objects (groups with their contents) to a new place in the tree:
/// reorder, or move between groups. One undo step. None when nothing would
/// change.
pub fn move_nodes(doc: &Document, ids: &[NodeId], to: Drop) -> Result<Option<Command>> {
    let moving = with_descendants(doc, ids);
    ensure!(!moving.is_empty(), "nothing selected");
    let moving_set: HashSet<NodeId> = moving.iter().copied().collect();
    let target = match to {
        Drop::Before(t) | Drop::After(t) | Drop::Into(t) => Some(t),
        Drop::End => None,
    };
    if let Some(t) = target {
        let node = doc.layout.node(t).context("that object no longer exists")?;
        ensure!(
            !moving_set.contains(&t),
            "objects cannot be moved next to or into themselves"
        );
        if matches!(to, Drop::Into(_)) {
            ensure!(node.is_group(), "objects can only go inside a group");
        }
    }
    let parent = match to {
        Drop::Before(t) | Drop::After(t) => doc.layout.node(t).and_then(|n| n.parent),
        Drop::Into(g) => Some(g),
        Drop::End => None,
    };
    let remaining: Vec<&Node> = doc
        .layout
        .nodes
        .iter()
        .filter(|n| !moving_set.contains(&n.id))
        .collect();
    let position_of = |id: NodeId| remaining.iter().position(|n| n.id == id);
    let at = match to {
        Drop::Before(t) => position_of(t).context("target vanished")?,
        Drop::After(t) => position_of(t).context("target vanished")? + 1,
        Drop::Into(g) => {
            // After the group's last member still in place, else after it.
            let inside: HashSet<NodeId> = doc.layout.descendants(g).into_iter().collect();
            remaining
                .iter()
                .rposition(|n| inside.contains(&n.id) || n.id == g)
                .context("target vanished")?
                + 1
        }
        Drop::End => remaining.len(),
    };
    // Roots of the moved trees take the new parent; their contents keep theirs.
    let moved: Vec<Node> = moving
        .iter()
        .filter_map(|id| doc.layout.node(*id))
        .map(|node| {
            let mut node = node.clone();
            if !node.parent.is_some_and(|p| moving_set.contains(&p)) {
                node.parent = parent;
            }
            node
        })
        .collect();
    let mut after: Vec<Node> = remaining[..at].iter().map(|n| (*n).clone()).collect();
    after.extend(moved);
    after.extend(remaining[at..].iter().map(|n| (*n).clone()));
    if after == doc.layout.nodes {
        return Ok(None);
    }
    let removed = moving
        .iter()
        .map(|id| {
            let index = doc.layout.index_of(*id).context("object vanished")?;
            Ok((index, doc.layout.nodes[index].clone()))
        })
        .collect::<Result<Vec<_>>>()?;
    let inserted = after
        .iter()
        .enumerate()
        .filter(|(_, n)| moving_set.contains(&n.id))
        .map(|(index, n)| (index, n.clone()))
        .collect();
    Ok(Some(Command::Batch {
        label: if moving.len() == 1 {
            "Move object".into()
        } else {
            format!("Move {} objects", moving.len())
        },
        commands: vec![Command::Remove(removed), Command::Insert(inserted)],
    }))
}

pub fn reparent(doc: &Document, ids: &[NodeId], parent: Option<NodeId>) -> Result<Command> {
    if let Some(group) = parent {
        ensure!(
            doc.layout.node(group).is_some_and(Node::is_group),
            "objects can only go inside a group"
        );
        for id in ids {
            ensure!(
                *id != group && !doc.layout.descendants(*id).contains(&group),
                "a group cannot go inside itself"
            );
        }
    }
    modify(doc, "Move to group", ids, |n| n.parent = parent)
}
