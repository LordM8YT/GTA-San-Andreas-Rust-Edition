//! SARE Build Mode core: everything the world editor does that needs no GPU
//! and no UI, so it can be tested on its own.
//!
//! - [`project`]: projects are content pack folders (`content.json` +
//!   `layout.json`); save, save as, autosave and recovery.
//! - [`history`]: the document and command-based undo/redo.
//! - [`ops`]: editing operations (place, delete, duplicate, group, ...).
//! - [`catalog`]: the asset catalog and its providers.
//! - [`viewport`]: picking, gizmo maths, snapping and the editor camera.
//! - [`thumbnail`]: Asset Browser pictures, drawn on the CPU.
//! - [`world_position`]: positions as server scripts write them.
//!
//! The runtime (`sa-runtime`, `build_mode` module) draws the editor with
//! egui and the existing wgpu renderer. See docs/build-mode.md.
pub mod catalog;
pub mod command_bar;
pub mod history;
pub mod ops;
pub mod project;
pub mod publish;
pub mod recent;
pub mod share;
pub mod thumbnail;
pub mod viewport;
pub mod world_position;

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
            info: WorldInfo::new("Test", "", [0.0, 0.0, 1.0]),
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
    fn history_keeps_only_the_newest_steps() {
        let mut doc = doc();
        let mut history = History::default();
        let total = crate::history::LIMIT + 5;
        for i in 0..total {
            let before = doc.info.clone();
            let mut after = before.clone();
            after.name = format!("World {i}");
            history
                .execute(&mut doc, Command::Info { before, after })
                .unwrap();
        }
        let mut undone = 0;
        while history.undo(&mut doc).unwrap().is_some() {
            undone += 1;
        }
        assert_eq!(undone, crate::history::LIMIT);
        // The five oldest steps were forgotten, so undo stops at "World 4".
        assert_eq!(doc.info.name, "World 4");
        assert!(history.redo(&mut doc).unwrap().is_some());
        assert_eq!(doc.info.name, "World 5");
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
    fn cut_and_paste_keeps_groups_and_lands_at_top_level() {
        let mut doc = doc();
        let mut history = History::default();
        let mut ids = Vec::new();
        for x in 0..2 {
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
        let (group, gid) = ops::group(&mut doc, &ids[..1], "Stack").unwrap();
        history.execute(&mut doc, group).unwrap();
        let (outer, outer_id) = ops::group(&mut doc, &[gid], "Outer").unwrap();
        history.execute(&mut doc, outer).unwrap();
        // Cut the inner group: copy, then delete.
        let copied = ops::copy(&doc, &[gid]).unwrap();
        assert_eq!(copied.len(), 2, "the group and the crate inside it");
        let command = ops::delete(&doc, &[gid]).unwrap();
        history.execute(&mut doc, command).unwrap();
        let (paste, roots) = ops::paste(&mut doc, &copied, [0.0, 0.0, 2.0]).unwrap();
        history.execute(&mut doc, paste).unwrap();
        assert_eq!(roots.len(), 1);
        let pasted = doc.layout.node(roots[0]).unwrap();
        assert!(pasted.is_group());
        // Its old group still exists, so it goes back in there.
        assert_eq!(pasted.parent, Some(outer_id));
        let inside = doc.layout.descendants(roots[0]);
        assert_eq!(inside.len(), 1);
        assert_eq!(
            doc.layout.node(inside[0]).unwrap().transform.position[2],
            2.0
        );
        // With the old group gone too, a paste lands at the top level.
        let command = ops::delete(&doc, &[outer_id]).unwrap();
        history.execute(&mut doc, command).unwrap();
        let (paste, roots) = ops::paste(&mut doc, &copied, [0.0; 3]).unwrap();
        history.execute(&mut doc, paste).unwrap();
        assert_eq!(doc.layout.node(roots[0]).unwrap().parent, None);
        doc.layout.validate().unwrap();
        assert!(ops::paste(&mut doc, &[], [0.0; 3]).is_err());
    }

    #[test]
    fn cut_paste_keeps_ids_and_pastes_stay_editable_and_uniquely_named() {
        let mut doc = doc();
        let mut history = History::default();
        let mut ids = Vec::new();
        for _ in 0..2 {
            let (place, id) =
                ops::place(&mut doc, crate_asset(), "Crate", Transform::default(), None).unwrap();
            history.execute(&mut doc, place).unwrap();
            ids.push(id);
        }
        // "Crate" and "Crate 2": pasting both must not name two "Crate 3".
        let copied = ops::copy(&doc, &ids).unwrap();
        let (paste, roots) = ops::paste(&mut doc, &copied, [1.0, 0.0, 0.0]).unwrap();
        history.execute(&mut doc, paste).unwrap();
        let names: Vec<&str> = roots
            .iter()
            .map(|id| doc.layout.node(*id).unwrap().name.as_str())
            .collect();
        assert_ne!(names[0], names[1]);
        // Cut and paste brings back the same ids, where they were.
        let cut = ops::copy(&doc, &[ids[0]]).unwrap();
        let command = ops::delete(&doc, &[ids[0]]).unwrap();
        history.execute(&mut doc, command).unwrap();
        let (restore, back) = ops::paste_cut(&doc, &cut).unwrap();
        history.execute(&mut doc, restore).unwrap();
        assert_eq!(back, vec![ids[0]]);
        assert_eq!(
            doc.layout.node(ids[0]).unwrap().transform.position,
            [0.0; 3]
        );
        // It exists again, so a second paste makes copies.
        assert!(ops::paste_cut(&doc, &cut).is_none());
        // A copy from a group that is now locked lands at the top level.
        let (group, gid) = ops::group(&mut doc, &[ids[1]], "Houses").unwrap();
        history.execute(&mut doc, group).unwrap();
        let copied = ops::copy(&doc, &[ids[1]]).unwrap();
        let lock = ops::modify(&doc, "Lock", &[gid], |n| n.locked = true).unwrap();
        history.execute(&mut doc, lock).unwrap();
        let (paste, roots) = ops::paste(&mut doc, &copied, [0.0; 3]).unwrap();
        history.execute(&mut doc, paste).unwrap();
        assert_eq!(doc.layout.node(roots[0]).unwrap().parent, None);
        doc.layout.validate().unwrap();
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
        // The copy's pivot moved; its contents stay relative to it.
        assert_eq!(doc.layout.world_transform(copies[0]).position[1], 5.0);
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
    fn outliner_drag_and_drop_reorders_and_regroups_with_undo() {
        use crate::ops::Drop;
        let mut doc = doc();
        let mut history = History::default();
        let mut add = |doc: &mut Document, name: &str| {
            let (command, id) =
                ops::place(doc, crate_asset(), name, Transform::default(), None).unwrap();
            history.execute(doc, command).unwrap();
            id
        };
        let a = add(&mut doc, "A");
        let b = add(&mut doc, "B");
        let c = add(&mut doc, "C");
        let names = |doc: &Document| {
            doc.layout
                .nodes
                .iter()
                .map(|n| n.name.clone())
                .collect::<Vec<_>>()
                .join(",")
        };
        let run = |doc: &mut Document, history: &mut History, ids: &[u64], to: Drop| {
            let command = ops::move_nodes(doc, ids, to).unwrap().unwrap();
            history.execute(doc, command).unwrap();
        };
        run(&mut doc, &mut history, &[c], Drop::Before(a));
        assert_eq!(names(&doc), "C,A,B");
        history.undo(&mut doc).unwrap();
        assert_eq!(names(&doc), "A,B,C", "one undo step");
        history.redo(&mut doc).unwrap();
        // A group, and B dragged into it, then A after B inside it.
        let (group, g) = ops::group(&mut doc, &[c], "G").unwrap();
        history.execute(&mut doc, group).unwrap();
        run(&mut doc, &mut history, &[b], Drop::Into(g));
        run(&mut doc, &mut history, &[a], Drop::After(b));
        let parent = |doc: &Document, id| doc.layout.node(id).unwrap().parent;
        assert_eq!((parent(&doc, a), parent(&doc, b)), (Some(g), Some(g)));
        let inside: Vec<_> = doc
            .layout
            .nodes
            .iter()
            .filter(|n| n.parent == Some(g))
            .map(|n| n.name.as_str())
            .collect();
        assert_eq!(inside, ["C", "B", "A"]);
        // The whole group moves to the end with its contents, past D.
        let (place, d) =
            ops::place(&mut doc, crate_asset(), "D", Transform::default(), None).unwrap();
        history.execute(&mut doc, place).unwrap();
        assert!(
            ops::move_nodes(&doc, &[d], Drop::End).unwrap().is_none(),
            "already last"
        );
        run(&mut doc, &mut history, &[g], Drop::End);
        assert_eq!(names(&doc), "D,G,C,B,A", "contents follow in order");
        assert_eq!(parent(&doc, a), Some(g));
        doc.layout.validate().unwrap();
        // A group cannot go into itself; a no-op is None.
        assert!(ops::move_nodes(&doc, &[g], Drop::Into(g)).is_err());
        assert!(ops::move_nodes(&doc, &[g], Drop::Before(a)).is_err());
        assert!(ops::move_nodes(&doc, &[a], Drop::After(b))
            .unwrap()
            .is_none());
        // Out to the top level.
        run(&mut doc, &mut history, &[a], Drop::End);
        assert_eq!(parent(&doc, a), None);
        while history.can_undo() {
            history.undo(&mut doc).unwrap();
        }
        assert!(doc.layout.nodes.is_empty());
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
    fn gameplay_assets_place_with_behaviour_save_and_play() {
        use sa_scene::world_logic::{self, Action, Component, Effect, Kept, Logic};
        let root = temp("gameplay");
        let folder = root.join("w");
        let (project, mut doc) = Project::create(&folder, "w", "W").unwrap();
        let mut history = History::default();
        let mut place = |doc: &mut Document, name: &str, at: [f32; 3]| {
            let (command, id) = ops::place(
                doc,
                AssetRef::Builtin(name.into()),
                name,
                Transform::at(at),
                None,
            )
            .unwrap();
            history.execute(doc, command).unwrap();
            id
        };
        let door = place(&mut doc, "door", [0.0, 5.0, 0.0]);
        let pad = place(&mut doc, "marker-teleport", [8.0, 0.0, 0.0]);
        let target = place(&mut doc, "marker-target", [-8.0, 0.0, 0.0]);
        let first = place(&mut doc, "marker-checkpoint", [0.0, -8.0, 0.0]);
        let second = place(&mut doc, "marker-checkpoint", [0.0, -14.0, 0.0]);
        let plain = place(&mut doc, "crate", [3.0, 3.0, 0.0]);
        let gameplay = |doc: &Document, id| doc.layout.node(id).unwrap().gameplay.clone();
        assert!(matches!(
            gameplay(&doc, door),
            Some(Kept::Known(Component::Interact(_)))
        ));
        assert!(
            gameplay(&doc, target).is_none(),
            "a destination is only a place"
        );
        assert!(gameplay(&doc, plain).is_none());
        let order = |doc: &Document, id| match gameplay(doc, id) {
            Some(Kept::Known(Component::Checkpoint(c))) => c.order,
            other => panic!("{other:?}"),
        };
        assert_eq!((order(&doc, first), order(&doc, second)), (1, 2));
        // The pad has no destination yet: the editor says so.
        assert!(world_logic::check(&doc.layout)
            .iter()
            .any(|p| p.node == pad && p.text.contains("no destination")));
        let point = ops::modify(&doc, "Edit gameplay", &[pad], |n| {
            if let Some(Kept::Known(Component::Trigger(t))) = &mut n.gameplay {
                t.on_enter = vec![Kept::Known(Action::Teleport {
                    target: Some(target),
                })];
            }
        })
        .unwrap();
        history.execute(&mut doc, point).unwrap();
        assert!(world_logic::check(&doc.layout).is_empty());
        // One undo takes the edit back.
        history.undo(&mut doc).unwrap();
        assert!(!world_logic::check(&doc.layout).is_empty());
        history.redo(&mut doc).unwrap();

        project.save(&doc).unwrap();
        let (_, reopened) = Project::open(&folder).unwrap();
        assert_eq!(reopened, doc, "gameplay survives save and open");
        // Played as a pack: markers are neither drawn nor solid, and the door
        // (gameplay opens it) is drawn and made solid by the game on its own.
        let mut world = PackWorld::open(&folder).unwrap();
        let origin = sa_scene::content_pack::ANCHOR;
        let scene = world.load(origin, origin, 300.0).unwrap();
        let markers: usize = ["marker-teleport", "marker-target", "marker-checkpoint"]
            .iter()
            .map(|m| sa_scene::builtin_assets::model(m).unwrap().triangles)
            .sum();
        let expected: usize = ["ground", "crate"]
            .iter()
            .map(|m| sa_scene::builtin_assets::model(m).unwrap().triangles)
            .sum();
        assert!(markers > 0);
        assert_eq!(scene.triangles, expected);
        // Duplicating the course end gives the next number; duplicating the
        // pad with its destination makes the copy lead to the copy.
        let (copy, new) = ops::duplicate(&mut doc, &[second], [2.0, 0.0, 0.0]).unwrap();
        history.execute(&mut doc, copy).unwrap();
        assert_eq!(order(&doc, new[0]), 3);
        let (group, gid) = ops::group(&mut doc, &[pad, target], "Pad").unwrap();
        history.execute(&mut doc, group).unwrap();
        let (copy, new) = ops::duplicate(&mut doc, &[gid], [0.0, 20.0, 0.0]).unwrap();
        history.execute(&mut doc, copy).unwrap();
        let inside = doc.layout.descendants(new[0]);
        let new_pad = *inside
            .iter()
            .find(|id| gameplay(&doc, **id).is_some())
            .unwrap();
        let new_target = *inside.iter().find(|id| **id != new_pad).unwrap();
        match gameplay(&doc, new_pad) {
            Some(Kept::Known(Component::Trigger(t))) => assert_eq!(
                t.on_enter,
                vec![Kept::Known(Action::Teleport {
                    target: Some(new_target)
                })]
            ),
            other => panic!("{other:?}"),
        }
        assert!(world_logic::check(&doc.layout).is_empty());
        let mut logic = Logic::new(&reopened.layout);
        logic.step([20.0, 20.0, 0.0], 0.1);
        logic.step([20.0, 20.0, 0.0], 0.1);
        let effects = logic.step([8.0, 0.0, 0.0], 0.1);
        assert!(matches!(&effects[..], [Effect::Teleport(p)] if p.position == [-8.0, 0.0, 0.0]));
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn the_gameplay_sample_is_clean_and_playable() {
        use sa_scene::world_logic::{self, Logic};
        let folder = PathBuf::from(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../../content/gameplay-sample"
        ));
        let (_, doc) = Project::open(&folder).unwrap();
        assert_eq!(world_logic::check(&doc.layout), Vec::new());
        let logic = Logic::new(&doc.layout);
        assert_eq!(logic.spawn().unwrap().position, [0.0, -6.0, 0.0]);
        assert!(logic.prompt([6.0, -6.0, 0.0]).is_some(), "the sign");
        let mut world = PackWorld::open(&folder).unwrap();
        let origin = sa_scene::content_pack::ANCHOR;
        let scene = world.load(origin, origin, 300.0).unwrap();
        let collision = scene.collision.unwrap();
        let [x, y, z] = world.spawn().unwrap();
        let point = glam::Vec3::new(x - origin[0], z, origin[1] - y);
        assert!(collision.standing_at(point, z + 2.0).is_some());
        // Its lamp posts glow: their coronas are part of the played world.
        let coronas = scene
            .batches
            .iter()
            .filter(|b| b.key == sa_scene::lights::CORONA_KEY)
            .map(|b| b.vertices.len() / 6)
            .sum::<usize>();
        assert_eq!(coronas, 3);
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
        let providers = Catalog::providers_for(&folder, "w", std::slice::from_ref(&root));
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
        // The same model again is the same asset; another model with the
        // same file name gets a numbered name.
        let again = mine
            .import_model(&other.folder.join("assets/signs/stop.glb"), "Street Signs")
            .unwrap();
        assert_eq!(again, imported);
        let taxi = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../../content/sare-rp-town/vehicles/taxi.glb"
        );
        std::fs::create_dir_all(root.join("elsewhere")).unwrap();
        std::fs::copy(taxi, root.join("elsewhere/stop.glb")).unwrap();
        let second = mine
            .import_model(&root.join("elsewhere/stop.glb"), "Street Signs")
            .unwrap();
        assert_eq!(second.to_string(), "self:assets/street-signs/stop-2.glb");
        let mut resolver = AssetResolver::with_dirs(&mine.folder, vec![root.clone()]);
        assert!(resolver.model(&imported).is_ok());
        let pack_ref = AssetRef::parse("pack:other:assets/signs/stop.glb").unwrap();
        assert_eq!(
            resolver.path(&pack_ref).unwrap().unwrap(),
            other.folder.join("assets/signs/stop.glb")
        );
        assert!(resolver.model(&pack_ref).is_ok());
        assert!(resolver.model(&crate_asset()).is_ok());
        let providers = Catalog::providers_for(&mine.folder, "mine", std::slice::from_ref(&root));
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

    #[test]
    fn thumbnails_show_the_model_on_a_clear_background() {
        use crate::thumbnail::render;
        let alpha =
            |t: &crate::thumbnail::Thumbnail, x: usize, y: usize| t.rgba[(y * t.size + x) * 4 + 3];
        // Set SARE_THUMBNAIL_DIR to look at the pictures.
        let dump = std::env::var_os("SARE_THUMBNAIL_DIR").map(PathBuf::from);
        let mut models: Vec<(String, sa_scene::gltf_model::Model)> = sa_scene::builtin_assets::ALL
            .iter()
            .map(|b| {
                (
                    b.name.to_owned(),
                    sa_scene::builtin_assets::model(b.name).unwrap(),
                )
            })
            .collect();
        let glb = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../../content/example-town/world.glb"
        );
        models.push((
            "example-town".into(),
            sa_scene::gltf_model::load(std::path::Path::new(glb), "town").unwrap(),
        ));
        for (name, model) in &models {
            let thumb = render(model, 64);
            assert_eq!(thumb.rgba.len(), 64 * 64 * 4);
            let opaque = thumb.rgba.chunks(4).filter(|p| p[3] == 255).count();
            assert!(opaque > 64 * 64 / 50, "{name}: only {opaque} opaque pixels");
            assert_eq!(alpha(&thumb, 0, 0), 0, "{name}: corner should be clear");
            if let Some(dir) = &dump {
                std::fs::create_dir_all(dir).unwrap();
                let file = std::fs::File::create(dir.join(format!("{name}.png"))).unwrap();
                let mut encoder = png::Encoder::new(file, 64, 64);
                encoder.set_color(png::ColorType::Rgba);
                encoder
                    .write_header()
                    .unwrap()
                    .write_image_data(&thumb.rgba)
                    .unwrap();
            }
        }
        let empty = render(&sa_scene::gltf_model::Model::default(), 8);
        assert!(empty.rgba.iter().all(|v| *v == 0));
    }

    #[test]
    fn a_placed_road_has_points_saves_and_draws_from_them() {
        let mut doc = doc();
        let at = Transform {
            position: [100.0, 50.0, 2.0],
            ..Transform::default()
        };
        let (command, id) = ops::place(
            &mut doc,
            AssetRef::Builtin("spline-road".into()),
            "Road",
            at,
            None,
        )
        .unwrap();
        command.apply(&mut doc).unwrap();
        let node = doc.layout.node(id).unwrap();
        let road = node.road.clone().expect("a placed road gets its points");
        assert!(road.is_valid() && road.points.len() >= 2);
        // It saves and loads with its points, and validates.
        let json = serde_json::to_string(&doc.layout).unwrap();
        let loaded: Layout = serde_json::from_str(&json).unwrap();
        loaded.validate().unwrap();
        assert_eq!(loaded.node(id).unwrap().road.as_ref(), Some(&road));
        // The model played and drawn is the road along its own points.
        let mut resolver = AssetResolver::new(std::path::Path::new("."));
        let model =
            sa_scene::content_pack::node_model(loaded.node(id).unwrap(), &mut resolver).unwrap();
        assert_eq!(model.triangles, sa_scene::road::model(&road).triangles);
        // A road needs two points: one alone does not validate.
        let mut broken = loaded.clone();
        broken.nodes.iter_mut().for_each(|n| {
            if let Some(road) = &mut n.road {
                road.points.truncate(1);
            }
        });
        assert!(broken.validate().is_err());
    }

    #[test]
    fn a_placed_part_keeps_its_shape_saves_and_plays() {
        use sa_scene::world_layout::{PartMaterial, PartShape};
        let mut doc = doc();
        let (command, id) = ops::place(
            &mut doc,
            AssetRef::Builtin("part-cylinder".into()),
            "Cylinder",
            Transform::default(),
            None,
        )
        .unwrap();
        command.apply(&mut doc).unwrap();
        let node = doc.layout.node(id).unwrap();
        // Every part refers to builtin:part; its look is in the part.
        assert_eq!(node.asset, Some(AssetRef::Builtin("part".into())));
        let mut part = node.part.clone().expect("a placed part gets its shape");
        assert_eq!(part.shape, PartShape::Cylinder);
        part.material = PartMaterial::Wood;
        part.color = [0.5, 0.3, 0.1];
        part.size = [1.0, 1.0, 6.0];
        ops::modify(&doc, "Edit part", &[id], |n| n.part = Some(part.clone()))
            .unwrap()
            .apply(&mut doc)
            .unwrap();
        let json = serde_json::to_string(&doc.layout).unwrap();
        let loaded: Layout = serde_json::from_str(&json).unwrap();
        loaded.validate().unwrap();
        assert_eq!(loaded.node(id).unwrap().part.as_ref(), Some(&part));
        // Played and drawn as the part, not as the asset.
        let mut resolver = AssetResolver::new(std::path::Path::new("."));
        let model =
            sa_scene::content_pack::node_model(loaded.node(id).unwrap(), &mut resolver).unwrap();
        assert_eq!(model.drawn[0].key, "builtin:material/wood");
        // A part with no size is refused on load.
        let mut broken = loaded.clone();
        broken.nodes.iter_mut().for_each(|n| {
            if let Some(part) = &mut n.part {
                part.size[2] = 0.0;
            }
        });
        assert!(broken.validate().is_err());
    }

    fn near(a: [f32; 3], b: [f32; 3]) -> bool {
        a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-3)
    }

    /// Two blocks, 4 m apart on X, grouped as a model.
    fn model_doc() -> (
        Document,
        sa_scene::world_layout::NodeId,
        [sa_scene::world_layout::NodeId; 2],
    ) {
        let mut doc = doc();
        let mut ids = [0; 2];
        for (i, x) in [10.0, 14.0].into_iter().enumerate() {
            let (command, id) = ops::place(
                &mut doc,
                AssetRef::Builtin("part-block".into()),
                "Block",
                Transform::at([x, 2.0, 1.0]),
                None,
            )
            .unwrap();
            command.apply(&mut doc).unwrap();
            ids[i] = id;
        }
        let (command, model) = ops::group(&mut doc, &ids, "House").unwrap();
        command.apply(&mut doc).unwrap();
        (doc, model, ids)
    }

    #[test]
    fn grouping_makes_a_model_with_a_pivot_and_moves_nothing() {
        let (doc, model, [a, b]) = model_doc();
        let group = doc.layout.node(model).unwrap();
        assert!(group.model && group.is_group());
        // In the middle of its contents, at their base.
        assert!(
            near(group.transform.position, [12.0, 2.0, 1.0]),
            "{:?}",
            group.transform
        );
        assert!(near(
            doc.layout.node(a).unwrap().transform.position,
            [-2.0, 0.0, 0.0]
        ));
        assert!(near(
            doc.layout.world_transform(a).position,
            [10.0, 2.0, 1.0]
        ));
        assert!(near(
            doc.layout.world_transform(b).position,
            [14.0, 2.0, 1.0]
        ));
    }

    #[test]
    fn turning_a_model_turns_its_contents_about_the_pivot() {
        let (mut doc, model, [a, _]) = model_doc();
        ops::modify(&doc, "Rotate", &[model], |n| {
            n.transform.rotation = [0.0, 0.0, 90.0]
        })
        .unwrap()
        .apply(&mut doc)
        .unwrap();
        let world = doc.layout.world_transform(a);
        assert!(near(world.position, [12.0, 0.0, 1.0]), "{world:?}");
        assert!(near(world.rotation, [0.0, 0.0, 90.0]));
        // Centre pivot moves only the pivot.
        ops::set_pivot(&doc, model, Transform::at([0.0, 0.0, 0.0]), "Pivot")
            .unwrap()
            .apply(&mut doc)
            .unwrap();
        assert!(near(
            doc.layout.world_transform(a).position,
            [12.0, 0.0, 1.0]
        ));
        ops::center_pivot(&doc, model)
            .unwrap()
            .apply(&mut doc)
            .unwrap();
        assert!(near(
            doc.layout.world_transform(a).position,
            [12.0, 0.0, 1.0]
        ));
        assert!(near(
            doc.layout.node(model).unwrap().transform.position,
            [12.0, 2.0, 1.0]
        ));
    }

    #[test]
    fn objects_keep_their_place_leaving_and_entering_models() {
        let (mut doc, model, [a, b]) = model_doc();
        ops::modify(&doc, "Rotate", &[model], |n| {
            n.transform.rotation = [0.0, 0.0, 90.0]
        })
        .unwrap()
        .apply(&mut doc)
        .unwrap();
        let world_a = doc.layout.world_transform(a);
        // Out of the model to the top level, and back in.
        ops::reparent(&doc, &[a], None)
            .unwrap()
            .apply(&mut doc)
            .unwrap();
        assert_eq!(doc.layout.node(a).unwrap().parent, None);
        assert!(near(
            doc.layout.world_transform(a).position,
            world_a.position
        ));
        ops::move_nodes(&doc, &[a], ops::Drop::Into(model))
            .unwrap()
            .unwrap()
            .apply(&mut doc)
            .unwrap();
        assert!(near(
            doc.layout.world_transform(a).position,
            world_a.position
        ));
        // Copied out of a model and pasted (at the top level when the
        // model is locked), a copy lands where the original is, plus the
        // offset.
        let copied = ops::copy(&doc, &[b]).unwrap();
        ops::modify(&doc, "Lock", &[model], |n| n.locked = true)
            .unwrap()
            .apply(&mut doc)
            .unwrap();
        let (command, pasted) = ops::paste(&mut doc, &copied, [0.0, 0.0, 5.0]).unwrap();
        command.apply(&mut doc).unwrap();
        let world_b = doc.layout.world_transform(b);
        let world_copy = doc.layout.world_transform(pasted[0]);
        assert_eq!(doc.layout.node(pasted[0]).unwrap().parent, None);
        assert!(near(
            world_copy.position,
            [
                world_b.position[0],
                world_b.position[1],
                world_b.position[2] + 5.0
            ]
        ));
        assert!(near(world_copy.rotation, world_b.rotation));
        // A copy of the whole model is a model with its own pivot.
        let copied = ops::copy(&doc, &[model]).unwrap();
        let (command, pasted) = ops::paste(&mut doc, &copied, [0.0, 10.0, 0.0]).unwrap();
        command.apply(&mut doc).unwrap();
        let inside = doc.layout.descendants(pasted[0]);
        assert_eq!(inside.len(), 2);
        let copy_a = inside
            .iter()
            .map(|id| doc.layout.world_transform(*id).position)
            .find(|p| {
                near(
                    *p,
                    [
                        world_a.position[0],
                        world_a.position[1] + 10.0,
                        world_a.position[2],
                    ],
                )
            });
        assert!(copy_a.is_some(), "the model's copy is not 10 m north");
        // Ungrouping keeps everything where it is.
        let before = doc.layout.world_transform(b);
        ops::ungroup(&doc, model).unwrap().apply(&mut doc).unwrap();
        assert!(near(
            doc.layout.world_transform(b).position,
            before.position
        ));
        assert!(near(
            doc.layout.world_transform(b).rotation,
            before.rotation
        ));
    }

    #[test]
    fn placing_into_a_turned_model_puts_it_where_clicked() {
        let (mut doc, model, _) = model_doc();
        ops::modify(&doc, "Rotate", &[model], |n| {
            n.transform.rotation = [0.0, 0.0, 45.0]
        })
        .unwrap()
        .apply(&mut doc)
        .unwrap();
        let (command, id) = ops::place(
            &mut doc,
            AssetRef::Builtin("cube".into()),
            "Cube",
            Transform::at([20.0, 20.0, 0.0]),
            Some(model),
        )
        .unwrap();
        command.apply(&mut doc).unwrap();
        assert!(near(
            doc.layout.world_transform(id).position,
            [20.0, 20.0, 0.0]
        ));
    }

    #[test]
    fn saved_model_copies_follow_the_file() {
        use sa_scene::saved_model::SavedModel;
        let (mut doc, model, [a, _]) = model_doc();
        let path = "models/house.model.json";
        let saved = SavedModel::capture(&doc.layout, model).unwrap();
        ops::link_saved(&doc, model, path)
            .unwrap()
            .apply(&mut doc)
            .unwrap();
        // Two more copies, one turned.
        let mut placed = Vec::new();
        for (x, turn) in [(40.0, 0.0), (80.0, 90.0)] {
            let mut at = Transform::at([x, 0.0, 0.0]);
            at.rotation[2] = turn;
            let (command, id) = ops::place_saved(&mut doc, &saved, path, at, None).unwrap();
            command.apply(&mut doc).unwrap();
            placed.push(id);
        }
        assert_eq!(ops::copies_of(&doc, path).len(), 3);
        let copy = placed[1];
        assert!(saved.matches(&doc.layout, copy));
        assert_ne!(
            doc.layout.node(copy).unwrap().name,
            "House",
            "copies get their own names"
        );
        // A block 2 m west of the pivot, turned 90°: 2 m south of it.
        let inside = doc.layout.descendants(copy);
        assert_eq!(inside.len(), 2);
        let west = inside
            .iter()
            .map(|id| doc.layout.world_transform(*id).position)
            .any(|p| near(p, [80.0, -2.0, 0.0]));
        assert!(west, "the copy's contents turn with it");

        // Change the first copy (move a block) and save it to all copies.
        ops::modify(&doc, "Move", &[a], |n| n.transform.position[2] = 3.0)
            .unwrap()
            .apply(&mut doc)
            .unwrap();
        assert!(!saved.matches(&doc.layout, model));
        let changed = SavedModel::capture(&doc.layout, model).unwrap();
        let mut history = History::default();
        let before = doc.clone();
        let others: Vec<_> = ops::copies_of(&doc, path)
            .into_iter()
            .filter(|id| *id != model)
            .collect();
        let command = ops::update_copies(&mut doc, &changed, &others)
            .unwrap()
            .expect("two copies to update");
        history.execute(&mut doc, command).unwrap();
        for id in ops::copies_of(&doc, path) {
            assert!(
                changed.matches(&doc.layout, id),
                "copy {id} was not updated"
            );
        }
        let node = doc.layout.node(copy).unwrap();
        assert_eq!(node.transform.rotation[2], 90.0, "copies keep their place");
        assert_eq!(node.saved_model.as_deref(), Some(path));
        assert!(doc.layout.validate().is_ok());
        // One undo step puts everything back.
        history.undo(&mut doc).unwrap();
        assert_eq!(doc.layout.nodes, before.layout.nodes);
        // Unlinked, a copy is left alone.
        ops::unlink_saved(&doc, copy)
            .unwrap()
            .apply(&mut doc)
            .unwrap();
        assert_eq!(ops::copies_of(&doc, path).len(), 2);
    }

    #[test]
    fn the_command_bar_changes_the_world_in_one_undo_step() {
        use crate::command_bar;
        let (mut doc, model, [a, b]) = model_doc();
        ops::modify(&doc, "Tag", &[a, b], |n| n.tags = vec!["Brick".into()])
            .unwrap()
            .apply(&mut doc)
            .unwrap();
        let before = doc.clone();
        let mut history = History::default();
        let outcome = command_bar::run(
            &mut doc,
            "for _, b in ipairs(world.tagged('Brick')) do b.color = rgb(1, 0, 0) end \
             world.find('Block').position = vector(1, 2, 3) \
             world.find('Block'):set('Price', 5) \
             world.find('Block'):addTag('Sold') \
             local c = world.find('Block 2'):clone(vector(9, 9, 0), 90) \
             print(#selection(), #world.tagged('Brick'))",
            &[model],
        )
        .unwrap();
        // The clone is tagged Brick too.
        assert_eq!(outcome.output, [("2  3".to_string(), false)]);
        history
            .execute(&mut doc, outcome.command.expect("changes"))
            .unwrap();
        let block = doc.layout.node(a).unwrap();
        assert_eq!(block.part.as_ref().unwrap().color, [1.0, 0.0, 0.0]);
        assert!(near(
            doc.layout.world_transform(a).position,
            [1.0, 2.0, 3.0]
        ));
        assert_eq!(
            block.attributes["Price"],
            sa_scene::world_layout::Attribute::Number(5.0)
        );
        assert!(block.tags.contains(&"Sold".to_string()));
        let copy = doc.layout.nodes.last().unwrap();
        assert!(
            copy.id > b && copy.part.is_some(),
            "the clone is a new part"
        );
        assert!(near(
            doc.layout.world_transform(copy.id).position,
            [9.0, 9.0, 0.0]
        ));
        assert!(doc.layout.validate().is_ok());
        history.undo(&mut doc).unwrap();
        assert_eq!(doc.layout.nodes, before.layout.nodes);
        // Errors are shown, and change nothing.
        let outcome = command_bar::run(&mut doc, "local x = nil + 1", &[]).unwrap();
        assert!(outcome.command.is_none());
        assert!(outcome.output[0].1, "{:?}", outcome.output);
    }
}
