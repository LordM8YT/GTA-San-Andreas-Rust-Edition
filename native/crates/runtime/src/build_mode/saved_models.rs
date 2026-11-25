//! My Models in Build Mode: saving a model to the world's folder, placing
//! copies of it from the Toolbox, and keeping the copies the same (see
//! [`sa_scene::saved_model`]).
use super::BuildMode;
use sa_editor::history::Command;
use sa_editor::ops;
use sa_scene::saved_model::{self, SavedModel};
use sa_scene::world_layout::{AssetRef, NodeId, Transform};

impl BuildMode {
    /// The saved model at `path` (inside the world's folder), read once.
    pub fn saved_model(&mut self, path: &str) -> Result<SavedModel, String> {
        if let Some(read) = self.saved_models.get(path) {
            return read.clone();
        }
        let read = sa_scene::content_pack::inside(&self.project.folder, path)
            .and_then(|file| SavedModel::read(&file))
            .map_err(|e| format!("{e:#}"));
        self.saved_models.insert(path.to_owned(), read.clone());
        read
    }

    /// Place a copy of the saved model at `path` where `transform` says.
    pub fn place_saved(
        &mut self,
        path: &str,
        transform: Transform,
        parent: Option<NodeId>,
    ) -> anyhow::Result<(Command, NodeId)> {
        let saved = self.saved_model(path).map_err(anyhow::Error::msg)?;
        ops::place_saved(&mut self.doc, &saved, path, transform, parent)
    }

    /// Save the model `group` to My Models as a new saved model, and link
    /// it to the file: it becomes the first copy.
    pub fn save_to_my_models(&mut self, group: NodeId) {
        let Some(node) = self.doc.layout.node(group).cloned() else {
            return;
        };
        if !node.is_group() {
            return self.warn("Group objects into a model first (Ctrl+G), then save the model");
        }
        // Copies are placed by their pivot: it must be where the model is.
        if node.transform.is_unmoved() {
            match ops::center_pivot(&self.doc, group) {
                Ok(command) => self.execute(command),
                Err(error) => return self.error(format!("{error:#}")),
            }
        }
        let path = self.new_saved_path(&node.name);
        let saved = match SavedModel::capture(&self.doc.layout, group) {
            Ok(saved) => saved,
            Err(error) => return self.error(format!("Could not save {}: {error:#}", node.name)),
        };
        if let Err(error) = self.write_saved(&path, &saved) {
            return self.error(format!("Could not save {}: {error:#}", node.name));
        }
        match ops::link_saved(&self.doc, group, &path) {
            Ok(command) => self.execute(command),
            Err(error) => return self.error(format!("{error:#}")),
        }
        self.info(format!(
            "Saved {} to My Models ({path}): place it again from the Toolbox",
            node.name
        ));
    }

    /// Write this copy's contents to its saved model and give every other
    /// copy the same.
    pub fn save_to_all_copies(&mut self, copy: NodeId) {
        let Some(path) = self.linked(copy) else {
            return;
        };
        let mut saved = match SavedModel::capture(&self.doc.layout, copy) {
            Ok(saved) => saved,
            Err(error) => return self.error(format!("{error:#}")),
        };
        // The saved model keeps its name; this copy may have another.
        if let Ok(before) = self.saved_model(&path) {
            saved.layout.nodes[0].name = before.name().to_owned();
        }
        if let Err(error) = self.write_saved(&path, &saved) {
            return self.error(format!("Could not save {}: {error:#}", saved.name()));
        }
        let others: Vec<NodeId> = ops::copies_of(&self.doc, &path)
            .into_iter()
            .filter(|id| *id != copy)
            .collect();
        match ops::update_copies(&mut self.doc, &saved, &others) {
            Ok(Some(command)) => self.execute(command),
            Ok(None) => {}
            Err(error) => return self.error(format!("{error:#}")),
        }
        self.info(format!(
            "Saved {} to My Models and updated {} other cop{} (Undo puts the copies back; the saved file keeps the change)",
            saved.name(),
            others.len(),
            if others.len() == 1 { "y" } else { "ies" }
        ));
    }

    /// Throw away this copy's changes: the saved model's contents again.
    pub fn revert_copy(&mut self, copy: NodeId) {
        let Some(path) = self.linked(copy) else {
            return;
        };
        let saved = match self.saved_model(&path) {
            Ok(saved) => saved,
            Err(error) => return self.error(error),
        };
        match ops::update_copies(&mut self.doc, &saved, &[copy]) {
            Ok(Some(command)) => self.execute(command),
            Ok(None) => {}
            Err(error) => self.error(format!("{error:#}")),
        }
    }

    pub fn unlink_copy(&mut self, copy: NodeId) {
        match ops::unlink_saved(&self.doc, copy) {
            Ok(command) => self.execute(command),
            Err(error) => self.error(format!("{error:#}")),
        }
    }

    fn linked(&self, copy: NodeId) -> Option<String> {
        self.doc.layout.node(copy)?.saved_model.clone()
    }

    /// `models/<name>.model.json`, numbered when that file exists.
    fn new_saved_path(&self, name: &str) -> String {
        let stem = sa_editor::project::sanitize(name);
        let stem = if stem.is_empty() {
            "model".into()
        } else {
            stem
        };
        (1..)
            .map(|n| match n {
                1 => format!("{}/{stem}{}", saved_model::DIR, saved_model::EXTENSION),
                n => format!("{}/{stem}-{n}{}", saved_model::DIR, saved_model::EXTENSION),
            })
            .find(|path| !self.project.folder.join(path).exists())
            .unwrap()
    }

    /// Write a saved model and show the new version everywhere: the
    /// Toolbox, its picture and the placement preview.
    fn write_saved(&mut self, path: &str, saved: &SavedModel) -> anyhow::Result<()> {
        let file = sa_scene::content_pack::inside(&self.project.folder, path)?;
        saved.write(&file)?;
        let asset = AssetRef::Own(path.to_owned());
        self.saved_models.insert(path.to_owned(), Ok(saved.clone()));
        self.resolver.forget(&asset);
        self.gpu.forget(&asset);
        self.ui.thumbnails.remove(&asset);
        self.refresh_catalog();
        Ok(())
    }
}
