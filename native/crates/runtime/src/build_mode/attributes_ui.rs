//! The Properties panel's Attributes and Tags: the creator's own values and
//! labels on an object or group, read by scripts (`object:get`,
//! `world.tagged`).
use super::panels::{run, section};
use super::BuildMode;
use crate::ui_theme::MUTED;
use egui::{RichText, Ui};
use sa_editor::ops;
use sa_scene::world_layout::{Attribute, Node, NodeId};

/// What is being typed for a new attribute or tag.
#[derive(Default)]
pub struct Draft {
    name: String,
    kind: Kind,
    tag: String,
}

#[derive(Clone, Copy, Default, PartialEq)]
enum Kind {
    #[default]
    Number,
    Text,
    Bool,
    Vector,
}
impl Kind {
    const ALL: [Self; 4] = [Self::Number, Self::Text, Self::Bool, Self::Vector];
    fn label(self) -> &'static str {
        match self {
            Self::Number => "Number",
            Self::Text => "Text",
            Self::Bool => "True/false",
            Self::Vector => "Vector",
        }
    }
    fn empty(self) -> Attribute {
        match self {
            Self::Number => Attribute::Number(0.0),
            Self::Text => Attribute::Text(String::new()),
            Self::Bool => Attribute::Bool(false),
            Self::Vector => Attribute::Vector([0.0; 3]),
        }
    }
}

pub fn section_ui(build: &mut BuildMode, ui: &mut Ui, id: NodeId, node: &Node, locked: bool) {
    section(ui, "Attributes");
    ui.add_enabled_ui(!locked, |ui| attributes(build, ui, id, node));
    section(ui, "Tags");
    ui.add_enabled_ui(!locked, |ui| tags(build, ui, id, node));
}

fn attributes(build: &mut BuildMode, ui: &mut Ui, id: NodeId, node: &Node) {
    let mut remove = None;
    for (name, value) in &node.attributes {
        ui.horizontal(|ui| {
            ui.label(name)
                .on_hover_text(format!("object:get('{name}') in scripts"));
            let mut edited = value.clone();
            let changed = match &mut edited {
                Attribute::Number(n) => ui.add(egui::DragValue::new(n).speed(0.1)).changed(),
                Attribute::Text(t) => ui
                    .add(
                        egui::TextEdit::singleline(t)
                            .desired_width(120.0)
                            .char_limit(Attribute::MAX_TEXT),
                    )
                    .changed(),
                Attribute::Bool(b) => ui.checkbox(b, "").changed(),
                Attribute::Vector(v) => {
                    let mut changed = false;
                    for n in v.iter_mut() {
                        changed |= ui
                            .add(egui::DragValue::new(n).speed(0.05).max_decimals(3))
                            .changed();
                    }
                    changed
                }
                Attribute::Other(value) => {
                    ui.label(RichText::new(value.to_string()).small().color(MUTED))
                        .on_hover_text("Made by a newer SARE: kept as it is");
                    false
                }
            };
            if changed && edited.is_valid() {
                let name = name.clone();
                build.preview("Edit attribute", &[id], |n| {
                    n.attributes.insert(name.clone(), edited.clone());
                });
            }
            if ui
                .small_button("🗑")
                .on_hover_text("Remove the attribute")
                .clicked()
            {
                remove = Some(name.clone());
            }
        });
    }
    if let Some(name) = remove {
        run(
            build,
            ops::modify(
                &build.doc,
                &format!("Remove attribute {name}"),
                &[id],
                |n| {
                    n.attributes.remove(&name);
                },
            ),
        );
    }
    // Values from the groups it is in, which scripts see too.
    let inherited: Vec<(String, String)> = build
        .doc
        .layout
        .ancestry(id)
        .skip(1)
        .flat_map(|group| {
            group
                .attributes
                .keys()
                .filter(|k| !node.attributes.contains_key(*k))
                .map(|k| (k.clone(), group.name.clone()))
                .collect::<Vec<_>>()
        })
        .collect();
    for (name, group) in inherited {
        ui.label(
            RichText::new(format!("{name} (from {group})"))
                .small()
                .color(MUTED),
        );
    }
    // A new one.
    let draft = &mut build.ui.attributes;
    let mut add = false;
    ui.horizontal(|ui| {
        let response = ui.add(
            egui::TextEdit::singleline(&mut draft.name)
                .hint_text("New attribute")
                .desired_width(110.0)
                .char_limit(Attribute::MAX_NAME),
        );
        egui::ComboBox::from_id_salt(("attribute kind", id))
            .selected_text(draft.kind.label())
            .width(86.0)
            .show_ui(ui, |ui| {
                for kind in Kind::ALL {
                    ui.selectable_value(&mut draft.kind, kind, kind.label());
                }
            });
        add = ui.small_button("Add").clicked()
            || (response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)));
    });
    if add {
        let name = draft.name.trim().to_owned();
        let kind = draft.kind;
        if !Attribute::valid_name(&name) {
            build.warn("Give the attribute a name (up to 64 characters)");
        } else if node.attributes.contains_key(&name) {
            build.warn(format!("{} already has an attribute {name}", node.name));
        } else if node.attributes.len() >= Attribute::MAX_PER_NODE {
            build.warn(format!(
                "An object has at most {} attributes",
                Attribute::MAX_PER_NODE
            ));
        } else {
            build.ui.attributes.name.clear();
            run(
                build,
                ops::modify(&build.doc, &format!("Add attribute {name}"), &[id], |n| {
                    n.attributes.insert(name.clone(), kind.empty());
                }),
            );
        }
    }
    if node.attributes.is_empty() {
        ui.label(
            RichText::new("Your own values (a price, a team, a speed). Scripts read them with object:get('Name').")
                .small()
                .color(MUTED),
        );
    }
}

fn tags(build: &mut BuildMode, ui: &mut Ui, id: NodeId, node: &Node) {
    let mut remove = None;
    ui.horizontal_wrapped(|ui| {
        for tag in &node.tags {
            if ui
                .small_button(format!("{tag}  🗑"))
                .on_hover_text("Remove the tag")
                .clicked()
            {
                remove = Some(tag.clone());
            }
        }
    });
    if let Some(tag) = remove {
        run(
            build,
            ops::modify(&build.doc, &format!("Remove tag {tag}"), &[id], |n| {
                n.tags.retain(|t| *t != tag);
            }),
        );
    }
    let inherited: Vec<String> = build
        .doc
        .layout
        .ancestry(id)
        .skip(1)
        .flat_map(|g| {
            g.tags
                .iter()
                .map(|t| format!("{t} (from {})", g.name))
                .collect::<Vec<_>>()
        })
        .collect();
    if !inherited.is_empty() {
        ui.label(RichText::new(inherited.join(", ")).small().color(MUTED));
    }
    let mut add = false;
    ui.horizontal(|ui| {
        let response = ui.add(
            egui::TextEdit::singleline(&mut build.ui.attributes.tag)
                .hint_text("New tag")
                .desired_width(110.0)
                .char_limit(Attribute::MAX_NAME),
        );
        add = ui.small_button("Add").clicked()
            || (response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)));
    });
    if add {
        let tag = build.ui.attributes.tag.trim().to_owned();
        if !Attribute::valid_name(&tag) {
            build.warn("A tag is a word or a few, up to 64 characters");
        } else if node.tags.contains(&tag) {
            build.ui.attributes.tag.clear();
        } else if node.tags.len() >= Attribute::MAX_TAGS {
            build.warn(format!(
                "An object has at most {} tags",
                Attribute::MAX_TAGS
            ));
        } else {
            build.ui.attributes.tag.clear();
            run(
                build,
                ops::modify(&build.doc, &format!("Add tag {tag}"), &[id], |n| {
                    n.tags.push(tag.clone());
                }),
            );
        }
    }
    if node.tags.is_empty()
        && build
            .doc
            .layout
            .ancestry(id)
            .skip(1)
            .all(|g| g.tags.is_empty())
    {
        let hint = if node.is_group() {
            "Labels for everything inside (Lamp, Coin). Scripts find them with world.tagged('Lamp')."
        } else {
            "Labels that sort objects into sets (Lamp, Coin). Scripts find them with world.tagged('Lamp')."
        };
        ui.label(RichText::new(hint).small().color(MUTED));
    }
}
