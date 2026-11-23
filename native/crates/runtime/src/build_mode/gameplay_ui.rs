//! The Inspector's Gameplay section: give an object a behaviour (trigger
//! zone, interaction, checkpoint, spawn point) and edit its actions.
//!
//! Field edits go through `BuildMode::preview`, so typing a message or
//! dragging a zone size is one undo step; adding and removing behaviours
//! and actions are commands of their own. The data and its rules live in
//! `sa_scene::world_logic`.
use super::panels::{axis_row, run, section, ERROR, WARNING};
use super::BuildMode;
use crate::ui_theme::{MUTED, TEXT};
use egui::{RichText, Ui};
use sa_editor::ops;
use sa_scene::world_layout::{Node, NodeId};
use sa_scene::world_logic::{self, Action, Component, Kept, Weather, MAX_ZONE};

/// Behaviours offered by "Add behaviour", as the built-in preset they copy.
const ADD: [(&str, &str, &str); 4] = [
    (
        "Trigger zone",
        "marker-trigger",
        "Runs actions when a player walks in or out",
    ),
    (
        "Interaction (E)",
        "marker-interact",
        "Shows a prompt; runs actions when the player presses E",
    ),
    (
        "Checkpoint",
        "marker-checkpoint",
        "One stop of a timed course, in order",
    ),
    (
        "Spawn point",
        "marker-spawn",
        "Players start here, facing the object's +Y",
    ),
];

pub fn inspector(build: &mut BuildMode, ui: &mut Ui, id: NodeId, node: &Node) {
    section(ui, "Gameplay");
    let locked = build.doc.layout.effective_locked(id);
    match &node.gameplay {
        None => {
            ui.label(
                RichText::new("No behaviour. Add one to make this object do something.")
                    .color(MUTED),
            );
            ui.add_enabled_ui(!locked, |ui| {
                ui.horizontal_wrapped(|ui| {
                    for (label, preset, hint) in ADD {
                        if ui
                            .button(format!("+ {label}"))
                            .on_hover_text(hint)
                            .clicked()
                        {
                            let order = world_logic::next_checkpoint(&build.doc.layout);
                            let component = world_logic::preset(preset, order);
                            run(
                                build,
                                ops::modify(&build.doc, "Add gameplay", &[id], |n| {
                                    n.gameplay = component.clone().map(Kept::Known);
                                }),
                            );
                        }
                    }
                });
            });
        }
        Some(Kept::Unknown(value)) => {
            ui.label(
                RichText::new(
                    "This behaviour was made by a newer SARE. It is kept as it is but does nothing here.",
                )
                .color(WARNING),
            );
            ui.label(
                RichText::new(value.to_string())
                    .monospace()
                    .small()
                    .color(MUTED),
            );
            remove_button(build, ui, id, locked);
        }
        Some(Kept::Known(component)) => {
            ui.horizontal(|ui| {
                ui.label(RichText::new(component.label()).strong().color(TEXT));
                remove_button(build, ui, id, locked);
            });
            let mut edited = component.clone();
            let targets = targets(build, id);
            ui.add_enabled_ui(!locked, |ui| fields(ui, &mut edited, &targets, id));
            if edited != *component {
                build.preview("Edit gameplay", &[id], |n| {
                    n.gameplay = Some(Kept::Known(edited.clone()));
                });
            }
        }
    }
    for problem in world_logic::check(&build.doc.layout)
        .into_iter()
        .filter(|p| p.node == id)
    {
        ui.label(RichText::new(format!("⚠ {}", problem.text)).color(ERROR));
    }
}

fn remove_button(build: &mut BuildMode, ui: &mut Ui, id: NodeId, locked: bool) {
    if ui
        .add_enabled(!locked, egui::Button::new("Remove").small())
        .on_hover_text("Take the behaviour off this object")
        .clicked()
    {
        run(
            build,
            ops::modify(&build.doc, "Remove gameplay", &[id], |n| n.gameplay = None),
        );
    }
}

/// Every object an action can point at, by name.
fn targets(build: &BuildMode, id: NodeId) -> Vec<(NodeId, String)> {
    build
        .doc
        .layout
        .nodes
        .iter()
        .filter(|n| !n.is_group() && n.id != id)
        .map(|n| (n.id, n.name.clone()))
        .collect()
}

fn fields(ui: &mut Ui, component: &mut Component, targets: &[(NodeId, String)], id: NodeId) {
    if let Some(size) = component.size_mut() {
        let mut value = *size;
        if axis_row(ui, "Zone size", &mut value, 0.05, " m") {
            *size = value.map(|v| v.clamp(0.1, MAX_ZONE));
        }
        ui.label(
            RichText::new("Width, depth and height in metres; turns with the object, not scaled.")
                .small()
                .color(MUTED),
        );
    }
    match component {
        Component::Spawn(_) => {
            ui.label(
                RichText::new(
                    "Players start here, facing the arrow (+Y). The first spawn point in the World Outliner is used instead of the world's spawn.",
                )
                .color(MUTED),
            );
        }
        Component::Trigger(trigger) => {
            ui.checkbox(&mut trigger.once, "Only the first time")
                .on_hover_text("Runs once per play session");
            actions(
                ui,
                "When a player enters",
                &mut trigger.on_enter,
                targets,
                (id, 0),
            );
            actions(
                ui,
                "When a player leaves",
                &mut trigger.on_leave,
                targets,
                (id, 1),
            );
        }
        Component::Interact(interact) => {
            ui.horizontal(|ui| {
                ui.label("Prompt");
                ui.add(
                    egui::TextEdit::singleline(&mut interact.prompt)
                        .hint_text("Use")
                        .desired_width(190.0),
                );
            });
            actions(
                ui,
                "When the player presses E",
                &mut interact.actions,
                targets,
                (id, 0),
            );
        }
        Component::Checkpoint(checkpoint) => {
            ui.horizontal(|ui| {
                ui.label("Course");
                ui.add(egui::TextEdit::singleline(&mut checkpoint.race).desired_width(120.0));
                ui.label("Number");
                ui.add(egui::DragValue::new(&mut checkpoint.order).range(1..=999));
            });
            ui.label(
                RichText::new(
                    "The lowest number starts the clock; the others must follow in order; the highest finishes.",
                )
                .small()
                .color(MUTED),
            );
        }
    }
}

fn actions(
    ui: &mut Ui,
    title: &str,
    list: &mut Vec<Kept<Action>>,
    targets: &[(NodeId, String)],
    salt: (NodeId, usize),
) {
    ui.add_space(2.0);
    ui.label(RichText::new(title).strong());
    let mut remove = None;
    let mut raise = None;
    for (index, action) in list.iter_mut().enumerate() {
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.horizontal(|ui| {
                let label = action.known().map_or("Unknown action", Action::label);
                ui.label(RichText::new(label).color(TEXT));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("✕").on_hover_text("Remove").clicked() {
                        remove = Some(index);
                    }
                    if index > 0 && ui.small_button("↑").on_hover_text("Run earlier").clicked() {
                        raise = Some(index);
                    }
                });
            });
            let id = ui.id().with((salt, index));
            match action {
                Kept::Known(Action::Message { text, seconds }) => {
                    ui.add(egui::TextEdit::singleline(text).desired_width(f32::INFINITY));
                    ui.add(
                        egui::DragValue::new(seconds)
                            .range(0.5..=60.0)
                            .speed(0.1)
                            .suffix(" s on screen"),
                    );
                }
                Kept::Known(Action::Teleport { target }) => {
                    pick(ui, id, target, targets, false);
                }
                Kept::Known(
                    Action::Show { node } | Action::Hide { node } | Action::Toggle { node },
                ) => {
                    pick(ui, id, node, targets, true);
                }
                Kept::Known(Action::SetTime { hour }) => {
                    ui.add(egui::Slider::new(hour, 0..=23).text("hour"));
                }
                Kept::Known(Action::SetWeather { weather }) => {
                    let current = Weather::parse(weather);
                    let shown = current.map_or_else(|| weather.clone(), |w| w.label().to_owned());
                    egui::ComboBox::from_id_salt(id)
                        .selected_text(shown)
                        .show_ui(ui, |ui| {
                            for option in Weather::ALL {
                                if ui
                                    .selectable_label(current == Some(option), option.label())
                                    .clicked()
                                {
                                    *weather = option.as_str().into();
                                }
                            }
                        });
                }
                Kept::Unknown(value) => {
                    ui.label(
                        RichText::new(format!("Needs a newer SARE: {value}"))
                            .small()
                            .color(WARNING),
                    );
                }
            }
        });
    }
    if let Some(index) = remove {
        list.remove(index);
    }
    if let Some(index) = raise {
        list.swap(index - 1, index);
    }
    ui.menu_button("+ Add action", |ui| {
        for template in Action::templates() {
            if ui.button(template.label()).clicked() {
                list.push(Kept::Known(template));
                ui.close();
            }
        }
    });
}

/// Choose the object an action points at. `own`: None means this object.
fn pick(
    ui: &mut Ui,
    id: egui::Id,
    value: &mut Option<NodeId>,
    targets: &[(NodeId, String)],
    own: bool,
) {
    let name = |target: NodeId| {
        targets
            .iter()
            .find(|(id, _)| *id == target)
            .map_or_else(|| format!("missing object {target}"), |(_, n)| n.clone())
    };
    let empty = if own {
        "This object"
    } else {
        "Choose a destination"
    };
    let shown = value.map_or_else(|| empty.to_owned(), name);
    ui.horizontal(|ui| {
        ui.label(if own { "Object" } else { "To" });
        egui::ComboBox::from_id_salt(id)
            .selected_text(shown)
            .width(190.0)
            .show_ui(ui, |ui| {
                if own {
                    ui.selectable_value(value, None, "This object");
                }
                for (target, label) in targets {
                    ui.selectable_value(value, Some(*target), label);
                }
            });
    });
    if !own && value.is_none() {
        ui.label(
            RichText::new("Tip: place a Destination point and pick it here.")
                .small()
                .color(MUTED),
        );
    }
}
