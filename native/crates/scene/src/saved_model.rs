//! Saved models ("My Models"): a model saved to a file in the world's
//! folder and placed as many times as wanted, like a reusable building
//! block.
//!
//! The file (`models/<name>.model.json`) is a world layout whose one
//! top-level node is the model group, with its pivot at the origin. Each
//! copy placed in the world is a model group whose `saved_model` names the
//! file; what is inside a copy are ordinary objects, so everything that
//! plays a layout (packs, servers, scripts, older SARE versions) plays
//! copies unchanged. Saving the changes made to one copy writes the file,
//! and the editor then gives every other copy the same contents.
use crate::world_layout::{AssetRef, Layout, NodeId, Transform};
use crate::world_logic::{Action, Kept};
use anyhow::{ensure, Context, Result};
use std::collections::HashMap;
use std::path::Path;

/// Folder inside the world that holds saved models.
pub const DIR: &str = "models";
/// File name ending of a saved model.
pub const EXTENSION: &str = ".model.json";

/// A path (inside the world's folder) that names a saved model.
pub fn is_saved_model(path: &str) -> bool {
    path.to_ascii_lowercase().ends_with(EXTENSION)
}

/// The reference a saved model has in the Toolbox, when `asset` is one.
pub fn path_of(asset: &AssetRef) -> Option<&str> {
    match asset {
        AssetRef::Own(path) if is_saved_model(path) => Some(path),
        _ => None,
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct SavedModel {
    /// The model group first (its pivot at the origin), then everything in
    /// it, in tree order, with ids from 1.
    pub layout: Layout,
}

impl SavedModel {
    pub fn from_json(text: &str) -> Result<Self> {
        let layout = Layout::from_json(text)?;
        let roots: Vec<_> = layout.nodes.iter().filter(|n| n.parent.is_none()).collect();
        ensure!(
            roots.len() == 1 && roots[0].is_group() && layout.nodes[0].parent.is_none(),
            "a saved model holds one model group with everything inside it"
        );
        Ok(Self { layout })
    }
    pub fn to_json(&self) -> Result<String> {
        self.layout.to_json()
    }
    pub fn read(path: &Path) -> Result<Self> {
        let text =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        Self::from_json(&text).with_context(|| format!("in {}", path.display()))
    }
    pub fn write(&self, path: &Path) -> Result<()> {
        if let Some(folder) = path.parent() {
            std::fs::create_dir_all(folder)
                .with_context(|| format!("creating {}", folder.display()))?;
        }
        crate::world_layout::write_atomic(path, self.to_json()?.as_bytes())
    }
    /// The model's name (its group's).
    pub fn name(&self) -> &str {
        &self.layout.nodes[0].name
    }

    /// The group `root` of `layout` with everything in it, as a saved
    /// model: the pivot at the origin (contents keep their place relative
    /// to it), ids from 1 in tree order. Behaviour that points at objects
    /// inside (a teleport, a door to show) points at them in the file;
    /// pointing outside the model is dropped, as the file has nothing there.
    /// Copies of other saved models inside become plain models.
    pub fn capture(layout: &Layout, root: NodeId) -> Result<Self> {
        let group = layout.node(root).context("that model is gone")?;
        ensure!(group.is_group(), "only a group can be saved as a model");
        let mut inside = layout.descendants(root);
        inside.insert(0, root);
        let order: Vec<NodeId> = layout
            .nodes
            .iter()
            .map(|n| n.id)
            .filter(|id| inside.contains(id))
            .collect();
        // The root first, the rest in tree order.
        let order: Vec<NodeId> = std::iter::once(root)
            .chain(order.into_iter().filter(|id| *id != root))
            .collect();
        let remap: HashMap<NodeId, NodeId> = order
            .iter()
            .enumerate()
            .map(|(i, id)| (*id, i as NodeId + 1))
            .collect();
        let mut nodes = Vec::with_capacity(order.len());
        for id in &order {
            let mut node = layout.node(*id).context("object vanished")?.clone();
            node.id = remap[id];
            node.parent = node.parent.and_then(|p| remap.get(&p).copied());
            node.saved_model = None;
            if *id == root {
                node.parent = None;
                node.transform = Transform::default();
                node.model = true;
                node.visible = true;
                node.locked = false;
            }
            if let Some(Kept::Known(component)) = &mut node.gameplay {
                for list in component.action_lists_mut() {
                    for action in list.iter_mut().filter_map(Kept::known_mut) {
                        let target = match action {
                            Action::Teleport { target } => target,
                            Action::Show { node }
                            | Action::Hide { node }
                            | Action::Toggle { node } => node,
                            _ => continue,
                        };
                        *target = target.and_then(|t| remap.get(&t).copied());
                    }
                }
            }
            nodes.push(node);
        }
        let layout = Layout {
            next_id: nodes.len() as NodeId + 1,
            nodes,
            ..Layout::default()
        };
        layout.validate()?;
        Ok(Self { layout })
    }

    /// The copy `root` in `layout` has the same contents as this file:
    /// the same objects, looks and behaviour, in the same places relative
    /// to the pivot. The copy's own name, place, visibility and lock do
    /// not count.
    pub fn matches(&self, layout: &Layout, root: NodeId) -> bool {
        let Ok(mut copy) = Self::capture(layout, root) else {
            return false;
        };
        copy.layout.nodes[0].name = self.name().to_owned();
        let normal = |nodes: &[crate::world_layout::Node]| {
            let mut nodes = nodes.to_vec();
            nodes[0].visible = true;
            nodes[0].locked = false;
            nodes
        };
        normal(&copy.layout.nodes) == normal(&self.layout.nodes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world_layout::Node;

    fn house() -> Layout {
        let mut layout = Layout::default();
        let mut group = Node::group(5, "House");
        group.model = true;
        group.transform = Transform {
            position: [100.0, 50.0, 2.0],
            rotation: [0.0, 0.0, 90.0],
            scale: [1.0; 3],
        };
        let mut wall = Node::object(
            7,
            "Wall",
            AssetRef::Builtin("part".into()),
            Transform::at([3.0, 0.0, 0.0]),
        );
        wall.parent = Some(5);
        let other = Node::object(
            2,
            "Tree",
            AssetRef::Builtin("tree".into()),
            Transform::default(),
        );
        layout.nodes = vec![other, wall, group];
        layout.next_id = 8;
        layout
    }

    #[test]
    fn a_saved_model_keeps_its_contents_about_the_pivot() {
        let layout = house();
        let saved = SavedModel::capture(&layout, 5).unwrap();
        assert_eq!(saved.name(), "House");
        assert_eq!(saved.layout.nodes.len(), 2, "the tree outside is not saved");
        let root = &saved.layout.nodes[0];
        assert_eq!((root.id, root.parent), (1, None));
        assert!(root.transform.is_unmoved() && root.model);
        let wall = &saved.layout.nodes[1];
        assert_eq!((wall.id, wall.parent), (2, Some(1)));
        assert_eq!(wall.transform.position, [3.0, 0.0, 0.0]);
        let read = SavedModel::from_json(&saved.to_json().unwrap()).unwrap();
        assert_eq!(read, saved);
        assert!(read.matches(&layout, 5));
    }

    #[test]
    fn a_changed_copy_no_longer_matches() {
        let mut layout = house();
        let saved = SavedModel::capture(&layout, 5).unwrap();
        // Moving, renaming or locking the copy itself is not a change.
        let group = layout.nodes.iter_mut().find(|n| n.id == 5).unwrap();
        group.transform.position = [0.0; 3];
        group.name = "House 2".into();
        group.locked = true;
        group.saved_model = Some("models/house.model.json".into());
        assert!(saved.matches(&layout, 5));
        // Moving a wall inside it is.
        layout.nodes[1].transform.position[0] = 4.0;
        assert!(!saved.matches(&layout, 5));
    }

    #[test]
    fn a_file_with_loose_objects_is_refused() {
        let text = house().to_json().unwrap();
        assert!(SavedModel::from_json(&text).is_err());
    }

    #[test]
    fn a_saved_model_draws_as_one_model() {
        let folder = std::env::temp_dir().join(format!("sare-saved-model-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        let mut layout = house();
        layout.nodes[1].asset = Some(AssetRef::Builtin("cube".into()));
        let saved = SavedModel::capture(&layout, 5).unwrap();
        saved
            .write(&folder.join("models/house.model.json"))
            .unwrap();
        let mut assets = crate::content_pack::AssetResolver::with_dirs(&folder, Vec::new());
        let asset = AssetRef::Own("models/house.model.json".into());
        let model = assets.model(&asset).unwrap();
        assert!(model.triangles > 0 && !model.collision.is_empty());
        // A model that holds a copy of itself is refused, not looped.
        let mut looped = saved.clone();
        looped.layout.nodes[1].asset = Some(asset.clone());
        looped
            .write(&folder.join("models/house.model.json"))
            .unwrap();
        assets.forget(&asset);
        assert!(assets.model(&asset).is_err());
        let _ = std::fs::remove_dir_all(&folder);
    }
}
