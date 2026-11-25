//! The command bar: a line of Lua run against the world being edited, the
//! way world scripts see it (`world.find`, `world.tagged`, `object:set`,
//! `object.position = ...`), plus `selection()`. What it changes becomes
//! one undo step. It runs once, at once: `wait`, `every` and events do
//! nothing here.
use crate::history::{Command, Document};
use crate::ops;
use anyhow::Result;
use sa_lua::world::{self as host, Request};
use sa_scene::world_layout::{Node, NodeId};
use sa_scene::world_scripts;
use std::collections::HashMap;

/// What a command did.
#[derive(Debug, Default)]
pub struct Outcome {
    /// The changes, one undo step (None: it changed nothing).
    pub command: Option<Command>,
    /// What it printed, its error, and notes on what it could not do here.
    pub output: Vec<(String, bool)>,
}

/// Run `line` against `doc`. `selected` are the selected nodes (groups
/// stand for what is inside them).
pub fn run(doc: &mut Document, line: &str, selected: &[NodeId]) -> Result<Outcome> {
    let world = doc.layout.resolved();
    let mut objects = world_scripts::host_objects(&world);
    // While editing, everything can be changed.
    for object in &mut objects {
        object.changeable = true;
        object.paintable = true;
    }
    let before: HashMap<u64, host::ObjectInfo> =
        objects.iter().map(|o| (o.id, o.clone())).collect();
    let picked: Vec<String> = ops::with_descendants(doc, selected)
        .into_iter()
        .filter(|id| before.contains_key(id))
        .map(|id| id.to_string())
        .collect();
    let source = format!(
        // On the command's first line, so errors give its own line numbers.
        "local __picked = {{ {} }} function selection() local list = {{}} \
         for _, id in ipairs(__picked) do for _, o in ipairs(world.objects()) do \
         if o.id == id then list[#list + 1] = o end end end return list end {line}",
        picked.join(", ")
    );
    let first_copy = doc.layout.next_id.max(1) + 1_000_000;
    let mut scripts = host::WorldScripts::new(host::Config {
        scripts: vec![host::Source {
            name: "Command".into(),
            source,
            object: None,
            language: host::Language::Lua,
        }],
        objects,
        weathers: Vec::new(),
        hour: None,
        weather: None,
        first_copy,
        max_copies: 200,
        copies: Vec::new(),
    });
    let requests = scripts.tick(host::Frame::default(), Vec::new());
    let mut outcome = Outcome::default();
    for line in scripts.take_output() {
        outcome.output.push((line.text, line.error));
    }
    let after = scripts.objects();

    // Copies first, so later changes can find them.
    let mut scratch = doc.clone();
    let mut commands = Vec::new();
    let mut made: HashMap<u64, NodeId> = HashMap::new();
    for request in &requests {
        if let Request::Copy {
            source,
            id,
            position,
            heading,
        } = request
        {
            let source = made.get(source).copied().unwrap_or(*source);
            let copied = ops::copy(&scratch, &[source])?;
            let (command, roots) = ops::paste(&mut scratch, &copied, [0.0; 3])?;
            command.apply(&mut scratch)?;
            commands.push(command);
            let new = roots[0];
            made.insert(*id, new);
            place(&mut scratch, &mut commands, new, *position, *heading)?;
        }
    }
    let node_of = |made: &HashMap<u64, NodeId>, id: u64| made.get(&id).copied().unwrap_or(id);
    let mut edits: Vec<Node> = Vec::new();
    for request in requests {
        match request {
            Request::Visible { object, visible } => {
                edit(&mut edits, &scratch, node_of(&made, object), &mut |n, _| {
                    n.visible = visible
                })
            }
            Request::Move {
                object,
                position,
                heading,
                ..
            } => edit(
                &mut edits,
                &scratch,
                node_of(&made, object),
                &mut |n, doc| {
                    let mut world = doc.layout.world_transform(n.id);
                    world.position = position;
                    world.rotation[2] = heading;
                    n.transform = doc.layout.local_in(n.parent, &world);
                },
            ),
            Request::Color { object, color } => {
                let id = node_of(&made, object);
                let is_part = scratch.layout.node(id).is_some_and(|n| n.part.is_some());
                if is_part {
                    edit(&mut edits, &scratch, id, &mut |n, _| {
                        if let (Some(part), Some(c)) = (n.part.as_mut(), color) {
                            part.color = c.map(|v| v.clamp(0.0, 1.0));
                        }
                    });
                } else {
                    outcome.output.push((
                        "colour stays as it is: while editing, only parts keep a colour".into(),
                        false,
                    ));
                }
            }
            Request::Destroy { object } => {
                if let Some(id) = made.remove(&object) {
                    let command = ops::delete(&scratch, &[id])?;
                    command.apply(&mut scratch)?;
                    commands.push(command);
                    edits.retain(|n| n.id != id);
                }
            }
            Request::Message { text, .. } => {
                outcome.output.push((format!("message: {text}"), false))
            }
            Request::Hud { key, text, .. } => outcome
                .output
                .push((format!("hud {key}: {}", text.unwrap_or_default()), false)),
            Request::Hour(_) | Request::Weather(_) => outcome.output.push((
                "the hour and weather are set in Lighting while editing".into(),
                false,
            )),
            Request::Teleport { .. } | Request::Copy { .. } => {}
        }
    }
    // Attributes and tags the command changed go on the objects themselves.
    for object in after {
        let Some(was) = before.get(&object.id) else {
            continue;
        };
        if was.attributes == object.attributes && was.tags == object.tags {
            continue;
        }
        let id = node_of(&made, object.id);
        edit(&mut edits, &scratch, id, &mut |n, _| {
            for (name, value) in &object.attributes {
                if was.attributes.get(name) != Some(value) {
                    n.attributes
                        .insert(name.clone(), world_scripts::layout_attribute(value));
                }
            }
            for name in was.attributes.keys() {
                if !object.attributes.contains_key(name) {
                    n.attributes.remove(name);
                }
            }
            for tag in &object.tags {
                if !was.tags.contains(tag) && !n.tags.contains(tag) {
                    n.tags.push(tag.clone());
                }
            }
            n.tags.retain(|t| object.tags.contains(t));
        });
    }
    let changes: Vec<(Node, Node)> = edits
        .into_iter()
        .filter_map(|after| {
            let before = scratch.layout.node(after.id)?.clone();
            (before != after).then_some((before, after))
        })
        .collect();
    if !changes.is_empty() {
        commands.push(Command::Modify {
            label: "Command".into(),
            changes,
        });
    }
    doc.layout.next_id = doc.layout.next_id.max(scratch.layout.next_id);
    if !commands.is_empty() {
        outcome.command = Some(Command::Batch {
            label: format!("Command: {}", short(line)),
            commands,
        });
    }
    Ok(outcome)
}

/// Change a node (once more if it was changed already).
fn edit(
    edits: &mut Vec<Node>,
    scratch: &Document,
    id: NodeId,
    f: &mut dyn FnMut(&mut Node, &Document),
) {
    if let Some(i) = edits.iter().position(|n| n.id == id) {
        f(&mut edits[i], scratch);
    } else if let Some(node) = scratch.layout.node(id) {
        let mut node = node.clone();
        f(&mut node, scratch);
        edits.push(node);
    }
}

/// Put a fresh copy where the script asked.
fn place(
    scratch: &mut Document,
    commands: &mut Vec<Command>,
    id: NodeId,
    position: [f32; 3],
    heading: f32,
) -> Result<()> {
    let mut world = scratch.layout.world_transform(id);
    world.position = position;
    world.rotation[2] = heading;
    let parent = scratch.layout.node(id).and_then(|n| n.parent);
    let local = scratch.layout.local_in(parent, &world);
    let command = ops::modify(scratch, "Place copy", &[id], |n| n.transform = local)?;
    command.apply(scratch)?;
    commands.push(command);
    Ok(())
}

fn short(line: &str) -> String {
    let line = line.trim();
    match line.char_indices().nth(40) {
        Some((at, _)) => format!("{}…", &line[..at]),
        None => line.to_owned(),
    }
}
