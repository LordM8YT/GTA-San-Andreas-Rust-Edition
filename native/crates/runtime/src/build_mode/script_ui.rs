//! World scripts in Build Mode: the script editor window, the Explorer's
//! script rows, the Scripts page (every script in the world) and the
//! Scripts section of an object's Properties, with ready-made examples to
//! start from. Scripts are kept in the layout (see
//! `sa_scene::world_layout::Script`) and run when the world is played
//! (`sa_scene::world_scripts`).
use super::panels::{section, ERROR, WARNING};
use super::BuildMode;
use crate::ui_theme::{ACCENT, MUTED, SUCCESS, TEXT};
use egui::text::{LayoutJob, TextFormat};
use egui::{Color32, FontId, RichText, Ui};
use sa_editor::history::Command;
use sa_scene::world_layout::{Layout, Node, NodeId, Script};
use std::time::{Duration, Instant};

/// Where a script is: the world's own list, or an object's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScriptAt {
    World(usize),
    Object(NodeId, usize),
}
impl ScriptAt {
    fn owner(self) -> Option<NodeId> {
        match self {
            Self::World(_) => None,
            Self::Object(id, _) => Some(id),
        }
    }
    fn index(self) -> usize {
        match self {
            Self::World(i) | Self::Object(_, i) => i,
        }
    }
    fn of(owner: Option<NodeId>, index: usize) -> Self {
        match owner {
            None => Self::World(index),
            Some(id) => Self::Object(id, index),
        }
    }
}

/// The script open in the editor window.
pub struct Editor {
    pub at: ScriptAt,
    /// The text being typed; written to the world after a pause, when the
    /// window closes, and before saving or playing.
    draft: String,
    dirty: bool,
    typed: Option<Instant>,
    /// The compile error of the draft, if any.
    problem: Option<String>,
    checked: String,
    help: bool,
}

/// Seconds of no typing before the text becomes an undo step.
const PAUSE: Duration = Duration::from_millis(1200);

/// A ready-made script to start from.
pub struct Example {
    pub name: &'static str,
    pub about: &'static str,
    pub source: &'static str,
}

pub const EXAMPLES: &[Example] = &[
    Example {
        name: "Empty",
        about: "Runs when the world starts",
        source: "-- Runs once when the world starts. Open Help for what scripts can do.\nprint('Hello from ' .. script.name)\n",
    },
    Example {
        name: "Door that opens on E",
        about: "Give the object an Interaction behaviour: E opens it for 5 seconds",
        source: r#"-- Give this object an Interaction behaviour (Gameplay in its Properties).
-- Pressing E there opens it for a few seconds.
local door = script.object

door:on('use', function(player)
  door:hide()
  player:message('The door is open for 5 seconds')
  wait(5)
  door:show()
end)
"#,
    },
    Example {
        name: "Welcome message",
        about: "Greets every player who joins",
        source: r#"world.on('join', function(player)
  player:message('Welcome, ' .. player.name .. '!', 5)
end)
"#,
    },
    Example {
        name: "Blinking object",
        about: "Shows and hides this object every second",
        source: r#"local light = script.object

every(1, function()
  light:toggle()
end)
"#,
    },
    Example {
        name: "Zone messages",
        about: "Give the object a Trigger zone: tells players when they enter and leave",
        source: r#"-- Give this object a Trigger zone behaviour (Gameplay in its Properties).
script.object:on('enter', function(player)
  player:message('You entered ' .. script.object.name)
end)

script.object:on('leave', function(player)
  player:message('You left ' .. script.object.name)
end)
"#,
    },
    Example {
        name: "Lift",
        about: "Moves this object up and down; players standing on it ride along",
        source: r#"local lift = script.object
local bottom = lift.position

while true do
  lift:moveTo(bottom + vector(0, 0, 4), 3)
  wait(5)
  lift:moveTo(bottom, 3)
  wait(5)
end
"#,
    },
    Example {
        name: "Sliding door",
        about: "Give the object an Interaction behaviour: E slides it aside and back",
        source: r#"-- Give this object an Interaction behaviour (Gameplay in its Properties).
local door = script.object
local closed = door.position
local busy = false

door:on('use', function(player)
  if busy then return end
  busy = true
  door:moveTo(closed + vector(2.5, 0, 0), 1)
  wait(4)
  door:moveTo(closed, 1)
  wait(1)
  busy = false
end)
"#,
    },
    Example {
        name: "Spinner",
        about: "Turns this object round and round",
        source: r#"local spinner = script.object
local heading = spinner.heading

while true do
  heading = heading + 90
  spinner:turnTo(heading, 1)
  wait(1)
end
"#,
    },
    Example {
        name: "Traffic light",
        about: "Turns this object green, yellow and red in turn",
        source: r#"local light = script.object

while true do
  light.color = rgb(0.2, 1, 0.3)
  wait(6)
  light.color = rgb(1, 0.85, 0.1)
  wait(2)
  light.color = rgb(1, 0.15, 0.1)
  wait(6)
end
"#,
    },
    Example {
        name: "Crate dropper",
        about: "Give the object an Interaction behaviour: E drops a copy of it that falls away after 10 seconds",
        source: r#"-- Give this object an Interaction behaviour (Gameplay in its Properties).
local box = script.object

box:on('use', function(player)
  local copy = box:clone(box.position + vector(0, 0, 3))
  copy.color = rgb(math.random(), math.random(), math.random())
  copy:moveTo(box.position + vector(math.random(-4, 4), math.random(-4, 4), 0), 1)
  wait(10)
  if copy.exists then
    copy:destroy()
  end
end)
"#,
    },
    Example {
        name: "Race results",
        about: "Tells everyone each finish time of the world's checkpoint races",
        source: r#"local best = {}

world.on('finish', function(player, race, seconds)
  local text = string.format('%s finished %s in %.1f s', player.name, race, seconds)
  if not best[race] or seconds < best[race] then
    best[race] = seconds
    text = text .. ' - a new best!'
  end
  world.message(text, 6)
end)
"#,
    },
    Example {
        name: "Day and night switch",
        about: "Give the object an Interaction behaviour: E switches day and night",
        source: r#"local night = false

script.object:on('use', function(player)
  night = not night
  world.setHour(night and 22 or 12)
  world.message(player.name .. (night and ' turned on the night' or ' brought back the day'))
end)
"#,
    },
    Example {
        name: "Score counter",
        about: "Counts uses per player and shares the count with other scripts",
        source: r#"-- Give this object an Interaction behaviour. Each E is a point.
local points = {}

script.object:on('use', function(player)
  points[player.id] = (points[player.id] or 0) + 1
  player:hud('points', 'Points: ' .. points[player.id])
  world.emit('scored', player.name, points[player.id])
end)

-- Another script can listen: world.on('scored', function(name, points) ... end)
"#,
    },
];

/// Short help, shown in the editor.
const HELP: &str = "\
script.object        the object this script is on (nil for world scripts)
world.find(name)     an object by name;  world.objects(), world.players()
object.name  .visible  .position  .heading  .color  (set any but name)
object.color = rgb(1, 0.5, 0)  (nil: its own colour again)
object:show()  :hide()  :toggle()  :on(event, function)
object:moveTo(vector(x, y, z), seconds, heading)  object:turnTo(heading, seconds)
object:clone(vector(x, y, z), heading) a copy (not saved)   copy:destroy()
player.name  .id  .position  .driving
player:message(text, seconds)  player:teleport(object or vector(x, y, z), heading)
player:hud(name, text)  world.hud(name, text)   on-screen text (nil removes it)
world.message(text, seconds)   world.setHour(0-23)   world.setWeather('rain')
world.on('enter' | 'leave' | 'use', [object,] function(player, object) end)
world.on('checkpoint', function(player, race, number, total) end)
world.on('finish', function(player, race, seconds) end)
world.on('join' | 'quit', function(player) end)
world.emit(name, ...)  /  world.on(name, function(...) end)   between scripts
wait(seconds)  spawn(function)  delay(seconds, function)  every(seconds, function)
print(...)  shows in Output.   vector(x, y, z) + - * , :length(), :distance(v)
Objects that scripts show, hide or move: the script's own object, objects named
in quotes, and objects ticked \"Scripts can change it\". Moved objects carry
players standing on them; their zones stay where they were placed.";

pub fn script(layout: &Layout, at: ScriptAt) -> Option<&Script> {
    match at {
        ScriptAt::World(i) => layout.settings.scripts.get(i),
        ScriptAt::Object(id, i) => layout.node(id)?.scripts.get(i),
    }
}

/// A command changing the script list of `owner` (an object, or the world
/// for None), as one undo step.
fn change(
    build: &BuildMode,
    owner: Option<NodeId>,
    label: &str,
    edit: impl FnOnce(&mut Vec<Script>),
) -> Option<Command> {
    match owner {
        None => {
            let before = build.doc.layout.settings.clone();
            let mut after = before.clone();
            edit(&mut after.scripts);
            (after != before).then_some(Command::Settings { before, after })
        }
        Some(id) => {
            let before = build.doc.layout.node(id)?.clone();
            let mut after = before.clone();
            edit(&mut after.scripts);
            (after != before).then(|| Command::Modify {
                label: label.into(),
                changes: vec![(before, after)],
            })
        }
    }
}

fn total(build: &BuildMode) -> usize {
    build.doc.layout.all_scripts().len()
}

/// Add an example script to `owner` and open it.
pub fn add(build: &mut BuildMode, owner: Option<NodeId>, example: &Example) {
    flush(build);
    if total(build) >= Script::MAX_SCRIPTS {
        build.error(format!(
            "A world can hold at most {} scripts",
            Script::MAX_SCRIPTS
        ));
        return;
    }
    let existing: Vec<String> = match owner {
        None => build.doc.layout.settings.scripts.iter(),
        Some(id) => match build.doc.layout.node(id) {
            Some(node) => node.scripts.iter(),
            None => return,
        },
    }
    .map(|s| s.name.clone())
    .collect();
    let base = if example.name == "Empty" {
        "Script"
    } else {
        example.name
    };
    let mut name = base.to_string();
    let mut n = 2;
    while existing.contains(&name) {
        name = format!("{base} {n}");
        n += 1;
    }
    let index = existing.len();
    let script = Script::new(name, example.source);
    if let Some(command) = change(build, owner, "Add script", |list| list.push(script)) {
        build.execute(command);
        open(build, ScriptAt::of(owner, index));
    }
}

/// Open a script in the editor window.
pub fn open(build: &mut BuildMode, at: ScriptAt) {
    flush(build);
    let Some(script) = script(&build.doc.layout, at) else {
        return;
    };
    build.ui.script = Some(Editor {
        at,
        draft: script.source.clone(),
        dirty: false,
        typed: None,
        problem: None,
        checked: String::new(),
        help: false,
    });
}

/// Write the text being typed to the world, as one undo step.
pub fn flush(build: &mut BuildMode) {
    let Some(editor) = build.ui.script.as_mut().filter(|e| e.dirty) else {
        return;
    };
    editor.dirty = false;
    editor.typed = None;
    let (at, draft) = (editor.at, editor.draft.clone());
    let name = script(&build.doc.layout, at).map(|s| s.name.clone());
    let label = format!("Edit script {}", name.unwrap_or_default());
    if let Some(command) = change(build, at.owner(), &label, |list| {
        if let Some(script) = list.get_mut(at.index()) {
            script.source = draft;
        }
    }) {
        build.execute(command);
    }
}

fn display_name(layout: &Layout, at: ScriptAt) -> String {
    let name = script(layout, at).map_or("", |s| s.name.as_str());
    match at.owner().and_then(|id| layout.node(id)) {
        Some(node) => format!("{}/{name}", node.name),
        None => name.to_string(),
    }
}

/// The editor window, once a frame.
pub fn window(build: &mut BuildMode, ctx: &egui::Context) {
    let Some(at) = build.ui.script.as_ref().map(|e| e.at) else {
        return;
    };
    let Some(current) = script(&build.doc.layout, at).cloned() else {
        // Deleted, or undone away.
        build.ui.script = None;
        return;
    };
    let full_name = display_name(&build.doc.layout, at);
    let locked = at
        .owner()
        .is_some_and(|id| build.doc.layout.effective_locked(id));
    let mut open = true;
    let mut rename: Option<String> = None;
    let mut enabled = current.enabled;
    let mut insert: Option<&'static str> = None;
    {
        let editor = build.ui.script.as_mut().unwrap();
        if !editor.dirty && editor.draft != current.source {
            // Undo, redo or another edit changed it.
            editor.draft = current.source.clone();
        }
        if editor.checked != editor.draft {
            editor.checked = editor.draft.clone();
            editor.problem = sa_lua::world::check(&full_name, &editor.draft);
        }
    }
    let mut name = current.name.clone();
    egui::Window::new(format!("📜 {full_name}"))
        .id(egui::Id::new("script-editor"))
        .open(&mut open)
        .default_size([620.0, 460.0])
        .default_pos(ctx.content_rect().center())
        .pivot(egui::Align2::CENTER_CENTER)
        .fade_in(false)
        .fade_out(false)
        .resizable(true)
        .collapsible(true)
        .show(ctx, |ui| {
            let editor = build.ui.script.as_mut().unwrap();
            ui.add_enabled_ui(!locked, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Name");
                    let field = ui.add(egui::TextEdit::singleline(&mut name).desired_width(170.0));
                    if field.lost_focus() && name.trim() != current.name && !name.trim().is_empty()
                    {
                        rename = Some(name.trim().chars().take(64).collect());
                    }
                    ui.checkbox(&mut enabled, "Enabled")
                        .on_hover_text("Off: kept in the world but not run");
                    ui.menu_button("+ Example", |ui| {
                        for example in EXAMPLES.iter().filter(|e| e.name != "Empty") {
                            if ui
                                .button(example.name)
                                .on_hover_text(example.about)
                                .clicked()
                            {
                                insert = Some(example.source);
                                ui.close();
                            }
                        }
                    })
                    .response
                    .on_hover_text("Add an example to the end of this script");
                    ui.toggle_value(&mut editor.help, "? Help");
                });
            });
            if editor.help {
                egui::Frame::new()
                    .fill(Color32::from_black_alpha(60))
                    .corner_radius(4.0)
                    .inner_margin(6)
                    .show(ui, |ui| {
                        ui.label(RichText::new(HELP).monospace().size(11.5).color(TEXT));
                    });
            }
            match &editor.problem {
                Some(problem) => {
                    ui.label(RichText::new(format!("⚠ {problem}")).color(ERROR));
                }
                None => {
                    ui.label(
                        RichText::new(
                            "✔ No syntax errors. Runs when you Play (F5) or play the world.",
                        )
                        .color(SUCCESS)
                        .small(),
                    );
                }
            }
            if locked {
                ui.label(
                    RichText::new("The object is locked: unlock it to edit its scripts.")
                        .color(WARNING),
                );
            }
            let error_line = editor
                .problem
                .as_deref()
                .and_then(|p| sa_lua::world::error_line(&full_name, p));
            let mut layouter = |ui: &Ui, text: &dyn egui::TextBuffer, wrap: f32| {
                let mut job = highlight(text.as_str(), error_line);
                job.wrap.max_width = wrap;
                ui.fonts_mut(|f| f.layout_job(job))
            };
            egui::ScrollArea::vertical()
                .id_salt("script-text")
                .auto_shrink(false)
                .show(ui, |ui| {
                    let response = ui.add_enabled(
                        !locked,
                        egui::TextEdit::multiline(&mut editor.draft)
                            .code_editor()
                            .lock_focus(true)
                            .desired_rows(18)
                            .desired_width(f32::INFINITY)
                            .layouter(&mut layouter),
                    );
                    if response.changed() {
                        editor.dirty = true;
                        editor.typed = Some(Instant::now());
                    }
                    if response.lost_focus() && editor.dirty {
                        editor.typed = Some(Instant::now() - PAUSE);
                    }
                });
        });
    if let Some(source) = insert {
        let editor = build.ui.script.as_mut().unwrap();
        if !editor.draft.ends_with('\n') && !editor.draft.is_empty() {
            editor.draft.push('\n');
        }
        editor.draft.push('\n');
        editor.draft.push_str(source);
        editor.dirty = true;
        editor.typed = Some(Instant::now() - PAUSE);
    }
    let paused = build
        .ui
        .script
        .as_ref()
        .and_then(|e| e.typed)
        .is_some_and(|t| t.elapsed() >= PAUSE);
    if paused || !open {
        flush(build);
    }
    if let Some(editor) = build.ui.script.as_ref().filter(|e| e.dirty) {
        let wait = PAUSE.saturating_sub(editor.typed.map_or(PAUSE, |t| t.elapsed()));
        ctx.request_repaint_after(wait);
    }
    if enabled != current.enabled || rename.is_some() {
        let label = if rename.is_some() {
            "Rename script"
        } else if enabled {
            "Enable script"
        } else {
            "Disable script"
        };
        if let Some(command) = change(build, at.owner(), label, |list| {
            if let Some(script) = list.get_mut(at.index()) {
                script.enabled = enabled;
                if let Some(name) = rename {
                    script.name = name;
                }
            }
        }) {
            flush(build);
            build.execute(command);
        }
    }
    if !open {
        build.ui.script = None;
    }
}

/// Rows of an object's scripts under it in the Explorer.
pub fn explorer_rows(build: &mut BuildMode, ui: &mut Ui, node: &Node, depth: usize) {
    for (i, script) in node.scripts.iter().enumerate() {
        let at = ScriptAt::Object(node.id, i);
        let open = build.ui.script.as_ref().is_some_and(|e| e.at == at);
        ui.horizontal(|ui| {
            ui.add_space(22.0 + depth as f32 * 14.0);
            let label = RichText::new(format!("📜 {}", script.name)).color(if script.enabled {
                TEXT
            } else {
                MUTED
            });
            if ui
                .selectable_label(open, label)
                .on_hover_text("A script on this object: click to edit it")
                .clicked()
            {
                build.selection = vec![node.id];
                self::open(build, at);
            }
        });
    }
}

/// The scripts of one owner as rows with open, on/off and delete.
fn list(build: &mut BuildMode, ui: &mut Ui, owner: Option<NodeId>, locked: bool) {
    let scripts: Vec<Script> = match owner {
        None => build.doc.layout.settings.scripts.clone(),
        Some(id) => build
            .doc
            .layout
            .node(id)
            .map(|n| n.scripts.clone())
            .unwrap_or_default(),
    };
    let mut remove = None;
    let mut toggle = None;
    for (i, script) in scripts.iter().enumerate() {
        let at = ScriptAt::of(owner, i);
        ui.horizontal(|ui| {
            let mut on = script.enabled;
            if ui
                .add_enabled(!locked, egui::Checkbox::without_text(&mut on))
                .on_hover_text("Run this script")
                .changed()
            {
                toggle = Some((i, on));
            }
            let open = build.ui.script.as_ref().is_some_and(|e| e.at == at);
            let problem = checked(&script.name, &script.source);
            let mut text = RichText::new(format!("📜 {}", script.name));
            if problem.is_some() {
                text = text.color(ERROR);
            }
            let row = ui.selectable_label(open, text);
            let row = match &problem {
                Some(p) => row.on_hover_text(p),
                None => row.on_hover_text("Edit this script"),
            };
            if row.clicked() {
                self::open(build, at);
            }
            ui.label(
                RichText::new(format!("{} lines", script.source.lines().count()))
                    .small()
                    .color(MUTED),
            );
            if ui
                .add_enabled(!locked, egui::Button::new("🗑").small())
                .on_hover_text("Delete this script")
                .clicked()
            {
                remove = Some(i);
            }
        });
    }
    if let Some((i, on)) = toggle {
        let label = if on {
            "Enable script"
        } else {
            "Disable script"
        };
        if let Some(command) = change(build, owner, label, |list| list[i].enabled = on) {
            flush(build);
            build.execute(command);
        }
    }
    if let Some(i) = remove {
        flush(build);
        if build
            .ui
            .script
            .as_ref()
            .is_some_and(|e| e.at.owner() == owner)
        {
            build.ui.script = None;
        }
        if let Some(command) = change(build, owner, "Delete script", |list| {
            list.remove(i);
        }) {
            build.execute(command);
        }
    }
    ui.add_enabled_ui(!locked, |ui| {
        ui.menu_button("+ Add script", |ui| {
            for example in EXAMPLES {
                if ui
                    .button(example.name)
                    .on_hover_text(example.about)
                    .clicked()
                {
                    add(build, owner, example);
                    ui.close();
                }
            }
        })
        .response
        .on_hover_text("A Lua script that runs while the world is played");
    });
}

/// The Scripts section of an object's Properties.
pub fn object_section(build: &mut BuildMode, ui: &mut Ui, id: NodeId, node: &Node) {
    section(ui, "Scripts");
    let locked = build.doc.layout.effective_locked(id);
    list(build, ui, Some(id), locked);
    let mut scriptable = node.scriptable;
    if ui
        .add_enabled(
            !locked,
            egui::Checkbox::new(&mut scriptable, "Scripts can change it"),
        )
        .on_hover_text("Scripts may show or hide this object while the world is played. Objects with scripts of their own, or named in quotes in a script, can be changed anyway.")
        .changed()
    {
        super::panels::run(
            build,
            sa_editor::ops::modify(&build.doc, "Scripts can change it", &[id], |n| {
                n.scriptable = scriptable
            }),
        );
    }
}

/// The Explorer's Scripts page: the world's own scripts and every object's.
pub fn world_page(build: &mut BuildMode, ui: &mut Ui) {
    section(ui, "World scripts");
    ui.label(
        RichText::new("Scripts for the whole world, not one object. They run when the world is played: in Play (F5), from the launcher and on servers, once per instance.")
            .small()
            .color(MUTED),
    );
    list(build, ui, None, false);
    let objects: Vec<(NodeId, String, usize)> = build
        .doc
        .layout
        .nodes
        .iter()
        .filter(|n| !n.scripts.is_empty())
        .map(|n| (n.id, n.name.clone(), n.scripts.len()))
        .collect();
    section(ui, "Scripts on objects");
    if objects.is_empty() {
        ui.label(
            RichText::new("None yet. Select an object and use + Add script in its Properties.")
                .color(MUTED),
        );
    }
    for (id, name, count) in objects {
        ui.horizontal(|ui| {
            if ui.link(name).on_hover_text("Select the object").clicked() {
                build.selection = vec![id];
            }
            ui.label(
                RichText::new(format!(
                    "{count} script{}",
                    if count == 1 { "" } else { "s" }
                ))
                .color(MUTED),
            );
        });
    }
    ui.add_space(6.0);
    ui.label(
        RichText::new(format!(
            "{} of at most {} scripts. Errors and print() show in Output while playing.",
            total(build),
            Script::MAX_SCRIPTS
        ))
        .small()
        .color(MUTED),
    );
}

/// [`sa_lua::world::check`], remembered per text: lists redraw every frame.
fn checked(name: &str, source: &str) -> Option<String> {
    use std::collections::HashMap;
    use std::hash::{Hash, Hasher};
    thread_local! {
        static CHECKS: std::cell::RefCell<HashMap<u64, Option<String>>> = Default::default();
    }
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    (name, source).hash(&mut hasher);
    let key = hasher.finish();
    CHECKS.with(|checks| {
        let mut checks = checks.borrow_mut();
        if checks.len() > 512 {
            checks.clear();
        }
        checks
            .entry(key)
            .or_insert_with(|| sa_lua::world::check(name, source))
            .clone()
    })
}

/// Lua coloured for the editor: keywords, the script API, strings,
/// numbers and comments; the line with a syntax error is underlined.
fn highlight(text: &str, error_line: Option<usize>) -> LayoutJob {
    const KEYWORDS: &[&str] = &[
        "and", "break", "do", "else", "elseif", "end", "false", "for", "function", "goto", "if",
        "in", "local", "nil", "not", "or", "repeat", "return", "then", "true", "until", "while",
    ];
    const API: &[&str] = &[
        "world", "script", "wait", "spawn", "delay", "every", "print", "vector", "pairs", "ipairs",
        "string", "math", "table", "tostring", "tonumber", "pcall",
    ];
    let font = FontId::monospace(13.0);
    let colour = |c: Color32| TextFormat::simple(font.clone(), c);
    let keyword = colour(Color32::from_rgb(198, 120, 221));
    let api = colour(ACCENT);
    let string = colour(Color32::from_rgb(152, 195, 121));
    let number = colour(Color32::from_rgb(209, 154, 102));
    let comment = colour(MUTED);
    let plain = colour(TEXT);
    let mut job = LayoutJob::default();
    let bytes = text.as_bytes();
    let mut line = 1;
    let mut i = 0;
    let push = |job: &mut LayoutJob, piece: &str, mut format: TextFormat, line: usize| {
        if Some(line) == error_line {
            format.underline = egui::Stroke::new(1.5, ERROR);
        }
        job.append(piece, 0.0, format);
    };
    while i < bytes.len() {
        let c = bytes[i];
        let start = i;
        if c == b'\n' {
            i += 1;
            push(&mut job, "\n", plain.clone(), line);
            line += 1;
            continue;
        }
        let format = if text[i..].starts_with("--") {
            // A comment to the end of the line (block comments too, line by line).
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            comment.clone()
        } else if c == b'"' || c == b'\'' {
            i += 1;
            while i < bytes.len() && bytes[i] != c && bytes[i] != b'\n' {
                // An escape, unless it would swallow the line's end.
                i += if bytes[i] == b'\\' && bytes.get(i + 1) != Some(&b'\n') {
                    2
                } else {
                    1
                };
            }
            // Past a real closing quote only: an unfinished string ends at
            // the line's end, which is counted as a line.
            if i < bytes.len() && bytes[i] == c {
                i += 1;
            }
            string.clone()
        } else if c.is_ascii_digit() {
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'.') {
                i += 1;
            }
            number.clone()
        } else if c.is_ascii_alphabetic() || c == b'_' {
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            let word = &text[start..i];
            if KEYWORDS.contains(&word) {
                keyword.clone()
            } else if API.contains(&word) {
                api.clone()
            } else {
                plain.clone()
            }
        } else {
            // Anything else, one character at a time (keeps UTF-8 whole).
            i += text[i..].chars().next().map_or(1, char::len_utf8);
            plain.clone()
        };
        // An escape may have stepped into a character: finish it.
        while i < bytes.len() && !text.is_char_boundary(i) {
            i += 1;
        }
        let end = i.min(bytes.len());
        let piece = text.get(start..end).unwrap_or("");
        push(&mut job, piece, format, line);
    }
    job
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_example_is_valid_lua() {
        for example in EXAMPLES {
            assert_eq!(
                sa_lua::world::check(example.name, example.source),
                None,
                "{}",
                example.name
            );
        }
    }

    #[test]
    fn an_unfinished_string_does_not_shift_the_error_line() {
        let text = "local a = 'open\nprint(a\nx = = 1\n";
        let job = highlight(text, Some(3));
        assert_eq!(job.text, text);
        let underlined: String = job
            .sections
            .iter()
            .filter(|s| s.format.underline.width > 0.0)
            .map(|s| &text[s.byte_range.start.0..s.byte_range.end.0])
            .collect();
        assert_eq!(underlined, "x = = 1\n");
    }

    #[test]
    fn highlighting_keeps_every_character() {
        let text = "local x = 'a\\'b' -- note ø\nprint(x, 12.5, '\\ø')\n";
        let job = highlight(text, Some(2));
        assert_eq!(job.text, text);
    }
}
