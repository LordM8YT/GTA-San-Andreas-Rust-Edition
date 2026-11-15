//! Command-based editing with undo and redo.
//!
//! Every change to a [`Document`] is a [`Command`] that knows how to apply
//! itself and how to take itself back. UI code never edits the document
//! directly; it builds a command (usually through [`crate::ops`]) and hands
//! it to [`History::execute`]. Live previews such as a gizmo drag change the
//! document without history and then record one command with the state from
//! before the drag, so a whole drag is one undo step.
use anyhow::{bail, ensure, Context, Result};
use sa_scene::world_layout::{Layout, Node, NodeId, WorldSettings};

/// World facts kept in `content.json`, edited like everything else.
#[derive(Clone, Debug, PartialEq)]
pub struct WorldInfo {
    pub name: String,
    pub description: String,
    /// Spawn point [east, north, up] in metres.
    pub spawn: [f32; 3],
}

/// Everything Build Mode edits.
#[derive(Clone, Debug, PartialEq)]
pub struct Document {
    pub layout: Layout,
    pub info: WorldInfo,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    /// Insert nodes; each index is the node's position after insertion.
    Insert(Vec<(usize, Node)>),
    /// Remove nodes; each index is where the node was, ascending.
    Remove(Vec<(usize, Node)>),
    /// Replace nodes (matched by id): (before, after) pairs.
    Modify {
        label: String,
        changes: Vec<(Node, Node)>,
    },
    Info {
        before: WorldInfo,
        after: WorldInfo,
    },
    Settings {
        before: WorldSettings,
        after: WorldSettings,
    },
    /// Several commands as one undo step, applied in order.
    Batch {
        label: String,
        commands: Vec<Command>,
    },
}
impl Command {
    pub fn label(&self) -> String {
        match self {
            Self::Insert(nodes) if nodes.len() == 1 => format!("Add {}", nodes[0].1.name),
            Self::Insert(nodes) => format!("Add {} objects", nodes.len()),
            Self::Remove(nodes) if nodes.len() == 1 => format!("Delete {}", nodes[0].1.name),
            Self::Remove(nodes) => format!("Delete {} objects", nodes.len()),
            Self::Modify { label, .. } | Self::Batch { label, .. } => label.clone(),
            Self::Info { .. } => "World settings".into(),
            Self::Settings { .. } => "Environment".into(),
        }
    }
    /// The nodes this command creates, changes or removes.
    pub fn touched(&self) -> Vec<NodeId> {
        match self {
            Self::Insert(nodes) | Self::Remove(nodes) => nodes.iter().map(|(_, n)| n.id).collect(),
            Self::Modify { changes, .. } => changes.iter().map(|(b, _)| b.id).collect(),
            Self::Batch { commands, .. } => commands.iter().flat_map(Self::touched).collect(),
            Self::Info { .. } | Self::Settings { .. } => Vec::new(),
        }
    }
    pub fn apply(&self, doc: &mut Document) -> Result<()> {
        match self {
            Self::Insert(nodes) => insert(&mut doc.layout, nodes),
            Self::Remove(nodes) => remove(&mut doc.layout, nodes),
            Self::Modify { changes, .. } => {
                replace(&mut doc.layout, changes.iter().map(|(b, a)| (b, a)))
            }
            Self::Info { after, .. } => {
                doc.info = after.clone();
                Ok(())
            }
            Self::Settings { after, .. } => {
                doc.layout.settings = after.clone();
                Ok(())
            }
            Self::Batch { commands, .. } => {
                for (done, command) in commands.iter().enumerate() {
                    if let Err(error) = command.apply(doc) {
                        // Leave the document as it was.
                        for earlier in commands[..done].iter().rev() {
                            earlier.revert(doc).ok();
                        }
                        return Err(error);
                    }
                }
                Ok(())
            }
        }
    }
    pub fn revert(&self, doc: &mut Document) -> Result<()> {
        match self {
            Self::Insert(nodes) => remove(&mut doc.layout, nodes),
            Self::Remove(nodes) => insert(&mut doc.layout, nodes),
            Self::Modify { changes, .. } => {
                replace(&mut doc.layout, changes.iter().map(|(b, a)| (a, b)))
            }
            Self::Info { before, .. } => {
                doc.info = before.clone();
                Ok(())
            }
            Self::Settings { before, .. } => {
                doc.layout.settings = before.clone();
                Ok(())
            }
            Self::Batch { commands, .. } => {
                for command in commands.iter().rev() {
                    command.revert(doc)?;
                }
                Ok(())
            }
        }
    }
}

fn insert(layout: &mut Layout, nodes: &[(usize, Node)]) -> Result<()> {
    for (_, node) in nodes {
        ensure!(
            layout.node(node.id).is_none(),
            "object {} already exists",
            node.id
        );
    }
    let mut sorted: Vec<_> = nodes.iter().collect();
    sorted.sort_by_key(|(index, _)| *index);
    for (index, node) in sorted {
        let at = (*index).min(layout.nodes.len());
        layout.nodes.insert(at, node.clone());
        layout.next_id = layout.next_id.max(node.id + 1);
    }
    Ok(())
}
fn remove(layout: &mut Layout, nodes: &[(usize, Node)]) -> Result<()> {
    for (_, node) in nodes {
        ensure!(
            layout.node(node.id).is_some(),
            "object {} does not exist",
            node.id
        );
    }
    for (_, node) in nodes {
        let index = layout.index_of(node.id).context("object vanished")?;
        layout.nodes.remove(index);
    }
    Ok(())
}
fn replace<'a>(
    layout: &mut Layout,
    changes: impl Iterator<Item = (&'a Node, &'a Node)> + Clone,
) -> Result<()> {
    for (from, _) in changes.clone() {
        ensure!(
            layout.node(from.id).is_some(),
            "object {} does not exist",
            from.id
        );
    }
    for (from, to) in changes {
        if from.id != to.id {
            bail!("a change may not alter an object's id");
        }
        *layout.node_mut(from.id).context("object vanished")? = to.clone();
    }
    Ok(())
}

/// Undo and redo stacks plus "unsaved changes" tracking.
#[derive(Default)]
pub struct History {
    undo: Vec<(u64, Command)>,
    redo: Vec<(u64, Command)>,
    serial: u64,
    /// Serial of the newest applied command when the project was saved.
    saved: u64,
    /// Bumped on every change, including undo and redo, so views can
    /// notice that the document changed.
    pub revision: u64,
}
/// Undo steps kept; older ones are forgotten.
pub const LIMIT: usize = 1000;

impl History {
    pub fn execute(&mut self, doc: &mut Document, command: Command) -> Result<()> {
        command.apply(doc)?;
        self.record(command);
        Ok(())
    }
    /// Remember a command whose effect is already in the document (the end
    /// of a live preview such as a gizmo drag).
    pub fn record(&mut self, command: Command) {
        self.serial += 1;
        self.undo.push((self.serial, command));
        if self.undo.len() > LIMIT {
            self.undo.remove(0);
        }
        self.redo.clear();
        self.revision += 1;
    }
    /// Undo the newest command; returns its label.
    pub fn undo(&mut self, doc: &mut Document) -> Result<Option<String>> {
        let Some((serial, command)) = self.undo.pop() else {
            return Ok(None);
        };
        if let Err(error) = command.revert(doc) {
            self.undo.push((serial, command));
            return Err(error);
        }
        let label = command.label();
        self.redo.push((serial, command));
        self.revision += 1;
        Ok(Some(label))
    }
    pub fn redo(&mut self, doc: &mut Document) -> Result<Option<String>> {
        let Some((serial, command)) = self.redo.pop() else {
            return Ok(None);
        };
        if let Err(error) = command.apply(doc) {
            self.redo.push((serial, command));
            return Err(error);
        }
        let label = command.label();
        self.undo.push((serial, command));
        self.revision += 1;
        Ok(Some(label))
    }
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
    pub fn undo_label(&self) -> Option<String> {
        self.undo.last().map(|(_, c)| c.label())
    }
    pub fn redo_label(&self) -> Option<String> {
        self.redo.last().map(|(_, c)| c.label())
    }
    fn current(&self) -> u64 {
        self.undo.last().map_or(0, |(serial, _)| *serial)
    }
    pub fn mark_saved(&mut self) {
        self.saved = self.current();
    }
    /// Changes since the last save (undoing back to the saved state counts
    /// as no changes).
    pub fn dirty(&self) -> bool {
        self.current() != self.saved
    }
}
