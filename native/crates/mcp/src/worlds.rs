//! World tools: Build Mode worlds are content pack folders (`content.json` +
//! `layout.json`). Every edit opens the world with `sa_editor::project`,
//! changes it with the same operations as the editor's buttons, validates
//! the layout and saves atomically, so a failed call changes nothing.
use crate::protocol::Tool;
use crate::Server;
use anyhow::{bail, ensure, Context, Result};
use sa_editor::catalog::{AssetProvider, AssetStatus, Builtins, Catalog, Folder};
use sa_editor::history::{Command, Document};
use sa_editor::{ops, project};
use sa_scene::content_pack;
use sa_scene::world_layout::{AssetRef, Node, NodeId, Transform};
use serde_json::{json, Value};
use std::path::PathBuf;

/// Most objects one call may place, change or delete.
const MAX_PER_CALL: usize = 500;

pub fn tools() -> Vec<Tool> {
    vec![
        Tool {
            name: "list_worlds",
            description: "List the Build Mode worlds (content packs) in the SARE content folder, with their id, name and object count.",
            schema: || json!({ "type": "object", "properties": {} }),
            run: |s, _| list_worlds(s),
        },
        Tool {
            name: "create_world",
            description: "Create a new empty Build Mode world (a ground plate and a spawn point). Returns its id for the other world tools. Open it in SARE with the launcher's Build Mode or `sa-runtime --build <id>`.",
            schema: || json!({
                "type": "object",
                "properties": { "name": { "type": "string", "description": "Display name, e.g. \"Desert Race Track\"" } },
                "required": ["name"],
            }),
            run: create_world,
        },
        Tool {
            name: "get_world",
            description: "Read a world: name, spawn point and its objects (id, name, asset, transform, group). Coordinates are metres: x = east, y = north, z = up. Rotation is degrees around x, y, z (z turns left/right).",
            schema: || json!({
                "type": "object",
                "properties": {
                    "world": { "type": "string", "description": "World id from list_worlds" },
                    "offset": { "type": "integer", "minimum": 0, "description": "Skip this many objects" },
                    "limit": { "type": "integer", "minimum": 1, "maximum": 2000, "description": "Most objects to return (default 300)" },
                },
                "required": ["world"],
            }),
            run: get_world,
        },
        Tool {
            name: "list_assets",
            description: "List the models that can be placed in a world: SARE's built-in shapes (builtin:...), the world's own .glb files (self:...) and models from other installed packs (pack:<id>:...). Built-in models stand on their origin; sizes are in the label.",
            schema: || json!({
                "type": "object",
                "properties": {
                    "world": { "type": "string", "description": "World id; adds that world's own models" },
                    "search": { "type": "string", "description": "Text to look for in the name, category or source" },
                    "category": { "type": "string" },
                },
            }),
            run: list_assets,
        },
        Tool {
            name: "place_objects",
            description: "Place objects in a world and save it. Each object needs an asset from list_assets and a position [x east, y north, z up] in metres. Optional: name, rotation [x, y, z] degrees, scale [x, y, z], group (a group's id, or a name: a new group is made if none has it). All or nothing: if one object is invalid, nothing is placed.",
            schema: || json!({
                "type": "object",
                "properties": {
                    "world": { "type": "string" },
                    "objects": {
                        "type": "array",
                        "minItems": 1,
                        "maxItems": MAX_PER_CALL,
                        "items": {
                            "type": "object",
                            "properties": {
                                "asset": { "type": "string", "description": "e.g. builtin:house" },
                                "name": { "type": "string" },
                                "position": vec3_schema(),
                                "rotation": vec3_schema(),
                                "scale": vec3_schema(),
                                "group": { "type": ["string", "integer"] },
                            },
                            "required": ["asset", "position"],
                        },
                    },
                },
                "required": ["world", "objects"],
            }),
            run: place_objects,
        },
        Tool {
            name: "update_objects",
            description: "Change objects in a world by id and save it: name, position, rotation, scale, visible, collision, locked. Fields left out stay the same.",
            schema: || json!({
                "type": "object",
                "properties": {
                    "world": { "type": "string" },
                    "changes": {
                        "type": "array",
                        "minItems": 1,
                        "maxItems": MAX_PER_CALL,
                        "items": {
                            "type": "object",
                            "properties": {
                                "id": { "type": "integer" },
                                "name": { "type": "string" },
                                "position": vec3_schema(),
                                "rotation": vec3_schema(),
                                "scale": vec3_schema(),
                                "visible": { "type": "boolean" },
                                "collision": { "type": "boolean" },
                                "locked": { "type": "boolean" },
                            },
                            "required": ["id"],
                        },
                    },
                },
                "required": ["world", "changes"],
            }),
            run: update_objects,
        },
        Tool {
            name: "delete_objects",
            description: "Delete objects from a world by id and save it. Deleting a group deletes everything in it.",
            schema: || json!({
                "type": "object",
                "properties": {
                    "world": { "type": "string" },
                    "ids": { "type": "array", "items": { "type": "integer" }, "minItems": 1, "maxItems": MAX_PER_CALL },
                },
                "required": ["world", "ids"],
            }),
            run: delete_objects,
        },
        Tool {
            name: "set_spawn",
            description: "Set where players start in a world: [x east, y north, z up] in metres, a little above the ground.",
            schema: || json!({
                "type": "object",
                "properties": { "world": { "type": "string" }, "position": vec3_schema() },
                "required": ["world", "position"],
            }),
            run: set_spawn,
        },
    ]
}

fn vec3_schema() -> Value {
    json!({ "type": "array", "items": { "type": "number" }, "minItems": 3, "maxItems": 3 })
}

pub fn text<'a>(args: &'a Value, key: &str) -> Result<&'a str> {
    args[key]
        .as_str()
        .filter(|s| !s.trim().is_empty())
        .with_context(|| format!("{key} is required"))
}
fn vec3(value: &Value, key: &str) -> Result<Option<[f32; 3]>> {
    if value.is_null() {
        return Ok(None);
    }
    let parts = value
        .as_array()
        .filter(|a| a.len() == 3)
        .with_context(|| format!("{key} must be [x, y, z]"))?;
    let mut out = [0.0; 3];
    for (slot, part) in out.iter_mut().zip(parts) {
        let number = part
            .as_f64()
            .with_context(|| format!("{key} must hold numbers"))?;
        ensure!(number.is_finite(), "{key} must hold finite numbers");
        *slot = number as f32;
    }
    Ok(Some(out))
}

/// The folder of the world whose id (or folder name) is `world`.
fn world_folder(server: &Server, world: &str) -> Result<PathBuf> {
    let dir = server.content_dir()?;
    content_pack::list(dir)
        .into_iter()
        .find(|(folder, manifest)| {
            manifest.id == world || folder.file_name().is_some_and(|n| n == world)
        })
        .map(|(folder, _)| folder)
        .with_context(|| format!("no world {world:?}; call list_worlds to see the ids"))
}

fn open(server: &Server, args: &Value) -> Result<(project::Project, Document)> {
    project::Project::open(&world_folder(server, text(args, "world")?)?)
}

fn save(project: &project::Project, doc: &Document) -> Result<()> {
    doc.layout.validate()?;
    project.save(doc)
}

fn list_worlds(server: &Server) -> Result<Value> {
    let dir = server.content_dir()?;
    let worlds: Vec<Value> = content_pack::list(dir)
        .into_iter()
        .map(|(folder, manifest)| {
            let objects = project::Project::open(&folder)
                .map(|(_, doc)| json!(doc.layout.nodes.iter().filter(|n| !n.is_group()).count()))
                .unwrap_or_else(|e| json!(format!("cannot open: {e:#}")));
            json!({
                "id": manifest.id,
                "name": manifest.name,
                "description": manifest.description,
                "folder": folder.file_name().map(|n| n.to_string_lossy().into_owned()),
                "objects": objects,
                "has_base_model": manifest.world.scene.is_some(),
            })
        })
        .collect();
    Ok(json!({ "content_dir": dir.display().to_string(), "worlds": worlds }))
}

fn create_world(server: &Server, args: &Value) -> Result<Value> {
    let dir = server.content_dir()?;
    let name = text(args, "name")?.trim();
    ensure!(
        name.len() <= 64,
        "the name is too long (64 characters at most)"
    );
    let id = project::new_id(name, dir);
    let (project, doc) = project::Project::create(&dir.join(&id), &id, name)?;
    Ok(json!({
        "id": project.id,
        "name": doc.info.name,
        "spawn": doc.info.spawn,
        "objects": doc.layout.nodes.len(),
        "next": "Place objects with place_objects. Open it in SARE with Build Mode (launcher) or `sa-runtime --build <id>`.",
    }))
}

fn node_json(node: &Node) -> Value {
    let mut value = json!({
        "id": node.id,
        "name": node.name,
        "position": node.transform.position,
        "rotation": node.transform.rotation,
        "scale": node.transform.scale,
    });
    match &node.asset {
        Some(asset) => value["asset"] = json!(asset.to_string()),
        None => value["group"] = json!(true),
    }
    if let Some(parent) = node.parent {
        value["parent"] = json!(parent);
    }
    for (key, on, default) in [
        ("visible", node.visible, true),
        ("collision", node.collision, true),
        ("locked", node.locked, false),
    ] {
        if on != default {
            value[key] = json!(on);
        }
    }
    value
}

fn get_world(server: &Server, args: &Value) -> Result<Value> {
    let (project, doc) = open(server, args)?;
    let offset = args["offset"].as_u64().unwrap_or(0) as usize;
    let limit = args["limit"].as_u64().unwrap_or(300).clamp(1, 2000) as usize;
    let nodes: Vec<Value> = doc
        .layout
        .nodes
        .iter()
        .skip(offset)
        .take(limit)
        .map(node_json)
        .collect();
    Ok(json!({
        "id": project.id,
        "name": doc.info.name,
        "description": doc.info.description,
        "spawn": doc.info.spawn,
        "total_nodes": doc.layout.nodes.len(),
        "offset": offset,
        "nodes": nodes,
    }))
}

fn catalog(server: &Server, world: Option<&(project::Project, Document)>) -> Result<Catalog> {
    let dir = server.content_dir()?;
    let providers: Vec<Box<dyn AssetProvider>> = match world {
        Some((project, _)) => {
            Catalog::providers_for(&project.folder, &project.id, &[dir.to_path_buf()])
        }
        None => {
            let mut providers: Vec<Box<dyn AssetProvider>> = vec![Box::new(Builtins)];
            for (folder, manifest) in content_pack::list(dir) {
                providers.push(Box::new(Folder {
                    root: folder,
                    pack_id: Some(manifest.id.clone()),
                    source: manifest.name.clone(),
                }));
            }
            providers
        }
    };
    Ok(Catalog::scan(&providers))
}

fn list_assets(server: &Server, args: &Value) -> Result<Value> {
    let world = match args["world"].as_str() {
        Some(_) => Some(open(server, args)?),
        None => None,
    };
    let catalog = catalog(server, world.as_ref())?;
    let search = args["search"].as_str().unwrap_or_default();
    let category = args["category"].as_str();
    let assets: Vec<Value> = catalog
        .search(search, category)
        .filter(|e| e.status == AssetStatus::Ready)
        .take(500)
        .map(|e| {
            json!({
                "asset": e.reference.to_string(),
                "label": e.label,
                "category": e.category,
                "source": e.source,
            })
        })
        .collect();
    Ok(json!({ "categories": catalog.categories(), "assets": assets }))
}

fn apply(doc: &mut Document, command: Command) -> Result<()> {
    command.apply(doc)
}

/// A group's id from an id or a name; a new group is made for a new name.
fn group_for(doc: &mut Document, group: &Value) -> Result<Option<NodeId>> {
    match group {
        Value::Null => Ok(None),
        Value::Number(n) => {
            let id = n.as_u64().context("group ids are whole numbers")?;
            ensure!(
                doc.layout.node(id).is_some_and(Node::is_group),
                "{id} is not a group"
            );
            Ok(Some(id))
        }
        Value::String(name) => {
            let name = name.trim();
            ensure!(!name.is_empty(), "a group name cannot be empty");
            if let Some(node) = doc
                .layout
                .nodes
                .iter()
                .find(|n| n.is_group() && n.name == name)
            {
                return Ok(Some(node.id));
            }
            let id = doc.layout.allocate_id();
            let node = Node::group(id, ops::unique_name(doc, name));
            apply(doc, Command::Insert(vec![(doc.layout.nodes.len(), node)]))?;
            Ok(Some(id))
        }
        _ => bail!("group must be a group id or a name"),
    }
}

fn place_objects(server: &Server, args: &Value) -> Result<Value> {
    let world = open(server, args)?;
    let catalog = catalog(server, Some(&world))?;
    let (project, mut doc) = world;
    let objects = args["objects"]
        .as_array()
        .filter(|a| !a.is_empty())
        .context("objects must be a list of objects to place")?;
    ensure!(
        objects.len() <= MAX_PER_CALL,
        "place at most {MAX_PER_CALL} objects per call"
    );
    let mut placed = Vec::new();
    for (index, object) in objects.iter().enumerate() {
        let at = || format!("object {index}");
        let asset = AssetRef::parse(text(object, "asset").with_context(at)?).with_context(at)?;
        match catalog.find(&asset) {
            Some(entry) if entry.status == AssetStatus::Ready => {}
            _ => bail!("{}: there is no asset {asset}; call list_assets", at()),
        }
        let transform = Transform {
            position: vec3(&object["position"], "position")
                .with_context(at)?
                .with_context(|| format!("{}: position is required", at()))?,
            rotation: vec3(&object["rotation"], "rotation")
                .with_context(at)?
                .unwrap_or([0.0; 3]),
            scale: vec3(&object["scale"], "scale")
                .with_context(at)?
                .unwrap_or([1.0; 3]),
        };
        ensure!(
            transform.is_valid(),
            "{}: position must be within 20 km of the origin and scale between 0.001 and 1000",
            at()
        );
        let name = match object["name"].as_str().map(str::trim) {
            Some(name) if !name.is_empty() => name.to_owned(),
            _ => catalog
                .find(&asset)
                .map_or_else(|| asset.to_string(), |e| e.label.clone()),
        };
        let parent = group_for(&mut doc, &object["group"]).with_context(at)?;
        let (command, id) = ops::place(&mut doc, asset, &name, transform, parent)?;
        apply(&mut doc, command)?;
        placed.push(json!({ "id": id, "name": doc.layout.node(id).map(|n| n.name.clone()) }));
    }
    save(&project, &doc)?;
    Ok(json!({ "world": project.id, "placed": placed, "total_nodes": doc.layout.nodes.len() }))
}

fn update_objects(server: &Server, args: &Value) -> Result<Value> {
    let (project, mut doc) = open(server, args)?;
    let changes = args["changes"]
        .as_array()
        .filter(|a| !a.is_empty())
        .context("changes must be a list")?;
    ensure!(
        changes.len() <= MAX_PER_CALL,
        "change at most {MAX_PER_CALL} objects per call"
    );
    for change in changes {
        let id = change["id"].as_u64().context("each change needs an id")?;
        ensure!(
            doc.layout.node(id).is_some(),
            "no object {id}; call get_world"
        );
        let position = vec3(&change["position"], "position")?;
        let rotation = vec3(&change["rotation"], "rotation")?;
        let scale = vec3(&change["scale"], "scale")?;
        let name = match change["name"].as_str() {
            Some(name) => {
                let name = name.trim();
                ensure!(
                    !name.is_empty() && name.len() <= 256,
                    "{id}: names are 1-256 characters"
                );
                Some(name.to_owned())
            }
            None => None,
        };
        let flag = |key: &str| change[key].as_bool();
        let (visible, collision, locked) = (flag("visible"), flag("collision"), flag("locked"));
        let command = ops::modify(&doc, "Edit", &[id], |node| {
            if let Some(p) = position {
                node.transform.position = p;
            }
            if let Some(r) = rotation {
                node.transform.rotation = r;
            }
            if let Some(s) = scale {
                node.transform.scale = s;
            }
            if let Some(name) = &name {
                node.name = name.clone();
            }
            if let Some(v) = visible {
                node.visible = v;
            }
            if let Some(c) = collision {
                node.collision = c;
            }
            if let Some(l) = locked {
                node.locked = l;
            }
        })?;
        apply(&mut doc, command)?;
        ensure!(
            doc.layout.node(id).is_some_and(|n| n.transform.is_valid()),
            "{id}: position must be within 20 km of the origin and scale between 0.001 and 1000"
        );
    }
    save(&project, &doc)?;
    Ok(json!({ "world": project.id, "changed": changes.len() }))
}

fn delete_objects(server: &Server, args: &Value) -> Result<Value> {
    let (project, mut doc) = open(server, args)?;
    let ids: Vec<NodeId> = args["ids"]
        .as_array()
        .context("ids must be a list of object ids")?
        .iter()
        .map(|v| v.as_u64().context("ids are whole numbers"))
        .collect::<Result<_>>()?;
    ensure!(
        !ids.is_empty() && ids.len() <= MAX_PER_CALL,
        "delete 1-{MAX_PER_CALL} ids per call"
    );
    for id in &ids {
        ensure!(
            doc.layout.node(*id).is_some(),
            "no object {id}; call get_world"
        );
    }
    let before = doc.layout.nodes.len();
    let command = ops::delete(&doc, &ids)?;
    apply(&mut doc, command)?;
    save(&project, &doc)?;
    Ok(json!({ "world": project.id, "deleted": before - doc.layout.nodes.len() }))
}

fn set_spawn(server: &Server, args: &Value) -> Result<Value> {
    let (project, mut doc) = open(server, args)?;
    let position = vec3(&args["position"], "position")?.context("position is required")?;
    ensure!(
        Transform::at(position).is_valid(),
        "the spawn must be within 20 km of the origin"
    );
    doc.info.spawn = position;
    save(&project, &doc)?;
    Ok(json!({ "world": project.id, "spawn": position }))
}

#[cfg(test)]
mod tests {
    use super::*;

    pub fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("sare-mcp-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }
    fn run(server: &Server, name: &str, args: Value) -> Result<Value> {
        let tool = tools().into_iter().find(|t| t.name == name).unwrap();
        (tool.run)(server, &args)
    }

    #[test]
    fn build_a_world_that_opens_in_build_mode() {
        let dir = temp("worlds");
        let server = Server {
            content: Some(dir.clone()),
            resources: None,
        };
        let made = run(&server, "create_world", json!({ "name": "Race Track" })).unwrap();
        assert_eq!(made["id"], "race-track");
        let again = run(&server, "create_world", json!({ "name": "Race Track" })).unwrap();
        assert_eq!(again["id"], "race-track-2", "never overwrites a world");
        let assets = run(&server, "list_assets", json!({ "search": "house" })).unwrap();
        assert!(assets["assets"]
            .as_array()
            .unwrap()
            .iter()
            .any(|a| a["asset"] == "builtin:house"));

        let placed = run(
            &server,
            "place_objects",
            json!({ "world": "race-track", "objects": [
                { "asset": "builtin:house", "position": [10, 5, 0], "rotation": [0, 0, 90], "group": "Town" },
                { "asset": "builtin:tree", "name": "Oak", "position": [14, 5, 0], "group": "Town" },
                { "asset": "builtin:road", "position": [0, 20, 0], "scale": [1, 2, 1] },
            ]}),
        )
        .unwrap();
        let ids: Vec<u64> = placed["placed"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["id"].as_u64().unwrap())
            .collect();
        assert_eq!(ids.len(), 3);

        // All or nothing: an unknown asset places nothing.
        let error = run(
            &server,
            "place_objects",
            json!({ "world": "race-track", "objects": [
                { "asset": "builtin:crate", "position": [0, 0, 0] },
                { "asset": "builtin:spaceship", "position": [0, 0, 0] },
            ]}),
        )
        .unwrap_err();
        assert!(format!("{error:#}").contains("list_assets"));
        let bad = run(
            &server,
            "place_objects",
            json!({ "world": "race-track", "objects": [{ "asset": "builtin:crate", "position": [1e9, 0, 0] }]}),
        );
        assert!(bad.is_err());

        run(
            &server,
            "update_objects",
            json!({ "world": "race-track", "changes": [{ "id": ids[1], "position": [20, 5, 0], "collision": false }]}),
        )
        .unwrap();
        run(
            &server,
            "set_spawn",
            json!({ "world": "race-track", "position": [0, -5, 1] }),
        )
        .unwrap();
        run(
            &server,
            "delete_objects",
            json!({ "world": "race-track", "ids": [ids[2]] }),
        )
        .unwrap();

        // The saved world is what Build Mode opens.
        let (_, doc) = project::Project::open(&dir.join("race-track")).unwrap();
        doc.layout.validate().unwrap();
        assert_eq!(doc.info.spawn, [0.0, -5.0, 1.0]);
        let oak = doc.layout.node(ids[1]).unwrap();
        assert_eq!(
            (oak.name.as_str(), oak.transform.position, oak.collision),
            ("Oak", [20.0, 5.0, 0.0], false)
        );
        let town = doc.layout.node(oak.parent.unwrap()).unwrap();
        assert!(town.is_group() && town.name == "Town");
        assert_eq!(
            doc.layout.node(ids[0]).unwrap().parent,
            oak.parent,
            "same group"
        );
        assert!(doc.layout.node(ids[2]).is_none());
        // ground + group + house + oak
        assert_eq!(doc.layout.nodes.len(), 4);

        let world = run(&server, "get_world", json!({ "world": "race-track" })).unwrap();
        assert_eq!(world["total_nodes"], 4);
        let listed = run(&server, "list_worlds", json!({})).unwrap();
        assert_eq!(listed["worlds"].as_array().unwrap().len(), 2);
        assert!(run(&server, "get_world", json!({ "world": "nope" })).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
