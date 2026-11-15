//! Editing operations: each builds a [`Command`] from the current document,
//! ready for [`crate::history::History::execute`]. They allocate new ids
//! from the layout, so ids are never reused even after an undo.
use crate::history::{Command, Document};
use anyhow::{ensure, Context, Result};
use sa_scene::world_layout::{AssetRef, Node, NodeId, Transform};
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
    let mut node = Node::object(id, unique_name(doc, name), asset, transform);
    node.parent = parent;
    Ok((Command::Insert(vec![(doc.layout.nodes.len(), node)]), id))
}

/// `name`, or `name 2`, `name 3`, ... if an object already has that name.
pub fn unique_name(doc: &Document, name: &str) -> String {
    let taken: HashSet<&str> = doc.layout.nodes.iter().map(|n| n.name.as_str()).collect();
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
    let all = with_descendants(doc, ids);
    ensure!(!all.is_empty(), "nothing selected");
    let mut remap = HashMap::new();
    for id in &all {
        remap.insert(*id, doc.layout.allocate_id());
    }
    let mut nodes = Vec::new();
    let mut roots = Vec::new();
    for id in &all {
        let mut copy = doc.layout.node(*id).context("object vanished")?.clone();
        copy.id = remap[id];
        let parent_copied = copy.parent.and_then(|p| remap.get(&p).copied());
        match parent_copied {
            Some(parent) => copy.parent = Some(parent),
            None => {
                roots.push(copy.id);
                copy.name = unique_name(doc, &copy.name);
            }
        }
        for (axis, delta) in offset.iter().enumerate() {
            copy.transform.position[axis] += delta;
        }
        nodes.push(copy);
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
