//! SARE Build Mode core: everything the world editor does that needs no GPU
//! and no UI, so it can be tested on its own.
//!
//! - [`project`]: projects are content pack folders (`content.json` +
//!   `layout.json`); save, save as, autosave and recovery.
//! - [`history`]: the document and command-based undo/redo.
//! - [`ops`]: editing operations (place, delete, duplicate, group, ...).
//! - [`catalog`]: the asset catalog and its providers.
//! - [`viewport`]: picking, gizmo maths, snapping and the editor camera.
//!
//! The runtime (`sa-runtime`, `build_mode` module) draws the editor with
//! egui and the existing wgpu renderer. See docs/build-mode.md.
pub mod catalog;
pub mod history;
pub mod ops;
pub mod project;
pub mod viewport;

#[cfg(test)]
mod tests {
    use crate::catalog::{AssetStatus, Catalog};
    use crate::history::{Command, Document, History, WorldInfo};
    use crate::{ops, project::Project};
    use sa_scene::content::WorldSource;
    use sa_scene::content_pack::{AssetResolver, PackWorld};
    use sa_scene::world_layout::{AssetRef, Layout, Transform};
    use std::path::PathBuf;

    fn doc() -> Document {
        Document {
            layout: Layout::default(),
            info: WorldInfo {
                name: "Test".into(),
                description: String::new(),
                spawn: [0.0, 0.0, 1.0],
            },
        }
    }
    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("sare-editor-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }
    fn crate_asset() -> AssetRef {
        AssetRef::Builtin("crate".into())
    }

    #[test]
    fn create_delete_and_undo_redo() {
        let mut doc = doc();
        let mut history = History::default();
        let (place, id) = ops::place(
            &mut doc,
            crate_asset(),
            "Crate",
            Transform::at([1.0, 2.0, 0.0]),
            None,
        )
        .unwrap();
        history.execute(&mut doc, place).unwrap();
        assert_eq!(doc.layout.nodes.len(), 1);
        assert!(history.dirty());
        {
            let command = ops::delete(&doc, &[id]).unwrap();
            history.execute(&mut doc, command).unwrap();
        }
        assert!(doc.layout.nodes.is_empty());
        assert_eq!(
            history.undo(&mut doc).unwrap().as_deref(),
            Some("Delete Crate")
        );
        assert_eq!(
            doc.layout.node(id).unwrap().transform.position,
            [1.0, 2.0, 0.0]
        );
        history.undo(&mut doc).unwrap();
        assert!(doc.layout.nodes.is_empty());
        assert!(!history.dirty(), "undoing everything is the saved state");
        history.redo(&mut doc).unwrap();
        history.redo(&mut doc).unwrap();
        assert!(doc.layout.nodes.is_empty());
        assert!(!history.can_redo());
        // Ids are never reused, even after undo.
        let (place, again) =
            ops::place(&mut doc, crate_asset(), "Crate", Transform::default(), None).unwrap();
        assert_ne!(again, id);
        history.execute(&mut doc, place).unwrap();
        doc.layout.validate().unwrap();
    }

    #[test]
    fn transforms_renames_and_flags_undo_as_one_step_each() {
        let mut doc = doc();
        let mut history = History::default();
        let (place, id) =
            ops::place(&mut doc, crate_asset(), "Crate", Transform::default(), None).unwrap();
        history.execute(&mut doc, place).unwrap();
        // A gizmo drag: many previews, one recorded command.
        let before = doc.layout.node(id).unwrap().clone();
        for step in 1..=10 {
            doc.layout.node_mut(id).unwrap().transform.position[0] = step as f32;
        }
        let after = doc.layout.node(id).unwrap().clone();
        history.record(Command::Modify {
            label: "Move".into(),
            changes: vec![(before, after)],
        });
        {
            let command = ops::rename(&doc, id, "Box").unwrap();
            history.execute(&mut doc, command).unwrap();
        }
        let hide = ops::modify(&doc, "Hide", &[id], |n| n.visible = false).unwrap();
        history.execute(&mut doc, hide).unwrap();
        let scale = ops::modify(&doc, "Scale", &[id], |n| {
            n.transform.scale = [2.0, 2.0, 2.0]
        })
        .unwrap();
        history.execute(&mut doc, scale).unwrap();
        let node = doc.layout.node(id).unwrap();
        assert_eq!(
            (node.name.as_str(), node.visible, node.transform.position[0]),
            ("Box", false, 10.0)
        );
        history.undo(&mut doc).unwrap();
        history.undo(&mut doc).unwrap();
        assert!(doc.layout.node(id).unwrap().visible);
        history.undo(&mut doc).unwrap();
        assert_eq!(doc.layout.node(id).unwrap().name, "Crate");
        history.undo(&mut doc).unwrap();
        assert_eq!(doc.layout.node(id).unwrap().transform.position[0], 0.0);
        assert!(ops::rename(&doc, id, "  ").is_err());
    }

    #[test]
    fn duplicate_group_and_ungroup() {
        let mut doc = doc();
        let mut history = History::default();
        let mut ids = Vec::new();
        for x in 0..3 {
            let (place, id) = ops::place(
                &mut doc,
                crate_asset(),
                "Crate",
                Transform::at([x as f32, 0.0, 0.0]),
                None,
            )
            .unwrap();
            history.execute(&mut doc, place).unwrap();
            ids.push(id);
        }
        assert_eq!(doc.layout.node(ids[1]).unwrap().name, "Crate 2");
        let (group, gid) = ops::group(&mut doc, &ids[..2], "Stack").unwrap();
        history.execute(&mut doc, group).unwrap();
        assert_eq!(doc.layout.nodes[0].id, gid);
        assert_eq!(doc.layout.descendants(gid).len(), 2);
        // Duplicating a group copies its contents with fresh ids.
        let (dup, roots) = ops::duplicate(&mut doc, &[gid], [0.0, 5.0, 0.0]).unwrap();
        history.execute(&mut doc, dup).unwrap();
        assert_eq!(roots.len(), 1);
        let copies = doc.layout.descendants(roots[0]);
        assert_eq!(copies.len(), 2);
        assert!(copies.iter().all(|id| !ids.contains(id)));
        assert_eq!(
            doc.layout.node(copies[0]).unwrap().transform.position[1],
            5.0
        );
        doc.layout.validate().unwrap();
        // Deleting a group deletes its contents; undo brings all back.
        let count = doc.layout.nodes.len();
        {
            let command = ops::delete(&doc, &[roots[0]]).unwrap();
            history.execute(&mut doc, command).unwrap();
        }
        assert_eq!(doc.layout.nodes.len(), count - 3);
        history.undo(&mut doc).unwrap();
        assert_eq!(doc.layout.nodes.len(), count);
        {
            let command = ops::ungroup(&doc, gid).unwrap();
            history.execute(&mut doc, command).unwrap();
        }
        assert!(doc.layout.node(gid).is_none());
        assert!(doc.layout.node(ids[0]).unwrap().parent.is_none());
        history.undo(&mut doc).unwrap();
        assert_eq!(doc.layout.node(ids[0]).unwrap().parent, Some(gid));
        assert!(ops::reparent(&doc, &[gid], Some(gid)).is_err());
        doc.layout.validate().unwrap();
    }

    #[test]
    fn projects_save_reopen_and_play_without_game_files() {
        let root = temp("project");
        let folder = root.join("my-world");
        let (project, mut doc) = Project::create(&folder, "my-world", "My World").unwrap();
        let mut history = History::default();
        let (place, id) = ops::place(
            &mut doc,
            crate_asset(),
            "Crate",
            Transform {
                position: [2.0, 3.0, 0.0],
                rotation: [0.0, 0.0, 45.0],
                scale: [1.0, 2.0, 1.0],
            },
            None,
        )
        .unwrap();
        history.execute(&mut doc, place).unwrap();
        doc.info.spawn = [0.0, 0.0, 0.5];
        project.save(&doc).unwrap();
        history.mark_saved();
        assert!(!history.dirty());

        let (reopened, again) = Project::open(&folder).unwrap();
        assert_eq!(again, doc);
        assert_eq!(reopened.id, "my-world");
        // The saved project is a normal content pack: it plays as a world.
        let mut world = PackWorld::open(&folder).unwrap();
        let origin = sa_scene::content_pack::ANCHOR;
        let scene = world.load(origin, origin, 300.0).unwrap();
        assert!(scene.triangles > 12, "ground and crate are drawn");
        let collision = scene.collision.unwrap();
        let [x, y, z] = world.spawn().unwrap();
        let point = glam::Vec3::new(x - origin[0], z, origin[1] - y);
        assert!(collision.standing_at(point, z + 2.0).is_some());
        // Save As copies it under a new id.
        let copy = reopened
            .save_as(&again, &root.join("copy"), "copy")
            .unwrap();
        assert_eq!(Project::open(&copy.folder).unwrap().0.id, "copy");
        assert!(again.layout.node(id).is_some());
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn autosave_recovers_unsaved_work() {
        let root = temp("autosave");
        let (project, mut doc) = Project::create(&root.join("w"), "w", "W").unwrap();
        assert!(project.recovery(&doc).unwrap().is_none());
        let (place, _) =
            ops::place(&mut doc, crate_asset(), "Crate", Transform::default(), None).unwrap();
        place.apply(&mut doc).unwrap();
        project.autosave(&doc).unwrap();
        // A crash: reopen the saved files and find the autosave.
        let (project, saved) = Project::open(&project.folder).unwrap();
        let recovered = project.recovery(&saved).unwrap().expect("autosave found");
        assert_eq!(recovered.document, doc);
        project.save(&recovered.document).unwrap();
        assert!(
            project.recovery(&recovered.document).unwrap().is_none(),
            "a save clears it"
        );
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn missing_assets_are_reported_and_kept() {
        let root = temp("missing");
        let folder = root.join("w");
        let (project, mut doc) = Project::create(&folder, "w", "W").unwrap();
        for asset in [
            "self:assets/props/gone.glb",
            "pack:not-installed:x.glb",
            "builtin:no-such",
        ] {
            let (place, _) = ops::place(
                &mut doc,
                AssetRef::parse(asset).unwrap(),
                "Thing",
                Transform::default(),
                None,
            )
            .unwrap();
            place.apply(&mut doc).unwrap();
        }
        project.save(&doc).unwrap();
        // The world still opens; the missing objects stay in the layout.
        let (_, reopened) = Project::open(&folder).unwrap();
        assert_eq!(reopened.layout.nodes.len(), 4);
        assert!(PackWorld::open(&folder).is_ok());
        let mut resolver = AssetResolver::with_dirs(&folder, vec![root.clone()]);
        let mut model = sa_scene::gltf_model::Model::default();
        let report =
            sa_scene::content_pack::place_layout(&mut model, &reopened.layout, &mut resolver);
        assert_eq!(report.placed, 1);
        assert_eq!(report.problems.len(), 3);
        let providers = Catalog::providers_for(&folder, "w", &[root.clone()]);
        let mut catalog = Catalog::scan(&providers);
        catalog.add_missing(&reopened.layout);
        let missing: Vec<_> = catalog
            .entries
            .iter()
            .filter(|e| matches!(e.status, AssetStatus::Missing(_)))
            .collect();
        assert_eq!(missing.len(), 3);
        assert!(catalog.categories().contains(&"Missing".to_string()));
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn asset_references_resolve_to_built_ins_own_files_and_other_packs() {
        let root = temp("resolve");
        let (other, _) = Project::create(&root.join("other-pack"), "other", "Other").unwrap();
        // A real model: the example pack's world, shipped in the repository.
        let glb = std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../../content/example-town/world.glb"
        ))
        .unwrap();
        std::fs::create_dir_all(other.folder.join("assets/signs")).unwrap();
        std::fs::write(other.folder.join("assets/signs/stop.glb"), &glb).unwrap();
        let (mine, _) = Project::create(&root.join("mine"), "mine", "Mine").unwrap();
        // Importing copies the model into the project.
        let imported = mine
            .import_model(&other.folder.join("assets/signs/stop.glb"), "Street Signs")
            .unwrap();
        assert_eq!(imported.to_string(), "self:assets/street-signs/stop.glb");
        let mut resolver = AssetResolver::with_dirs(&mine.folder, vec![root.clone()]);
        assert!(resolver.model(&imported).is_ok());
        let pack_ref = AssetRef::parse("pack:other:assets/signs/stop.glb").unwrap();
        assert_eq!(
            resolver.path(&pack_ref).unwrap().unwrap(),
            other.folder.join("assets/signs/stop.glb")
        );
        assert!(resolver.model(&pack_ref).is_ok());
        assert!(resolver.model(&crate_asset()).is_ok());
        let providers = Catalog::providers_for(&mine.folder, "mine", &[root.clone()]);
        let catalog = Catalog::scan(&providers);
        let signs = catalog
            .find(&pack_ref)
            .expect("other pack's asset is listed");
        assert_eq!(signs.category, "Signs");
        assert_eq!(catalog.find(&imported).unwrap().source, "This world");
        assert!(catalog.search("crate", None).count() >= 1);
        assert!(catalog
            .search("", Some("Nature"))
            .all(|e| e.category == "Nature"));
        std::fs::remove_dir_all(&root).unwrap();
    }
}
