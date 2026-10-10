//! Script UI drawn with the game's own egui widgets: notifications, text UI,
//! progress bar, context menu and input dialog. Scripts only supply text and
//! choices (see `sa_lua::ui`), never markup or code.
use egui::{Color32, RichText};
use sa_lua::{
    client::Host,
    ui::{Kind, RowKind, Ui},
};
use serde_json::Value as Json;

pub enum Action {
    Select(usize),
    CloseContext,
    /// Confirmed values, or `None` when cancelled.
    Submit(Option<Vec<Json>>),
}

pub fn apply(host: &mut Host, actions: Vec<Action>) {
    for action in actions {
        match action {
            Action::Select(index) => host.select_context(index),
            Action::CloseContext => host.close_context(),
            Action::Submit(values) => host.submit_dialog(values),
        }
    }
}

fn panel() -> egui::Frame {
    egui::Frame::new()
        .fill(Color32::from_black_alpha(200))
        .inner_margin(egui::Margin::same(10))
        .corner_radius(6)
}
fn accent(kind: Kind) -> Color32 {
    match kind {
        Kind::Inform => Color32::from_rgb(110, 170, 255),
        Kind::Success => Color32::from_rgb(110, 210, 120),
        Kind::Warning => Color32::from_rgb(240, 190, 80),
        Kind::Error => Color32::from_rgb(235, 90, 80),
    }
}

/// Number rows are edited as text and returned as numbers.
fn number(value: &Json) -> Json {
    match value {
        Json::String(text) => match text.trim().parse::<f64>() {
            Ok(n) if n.fract() == 0.0 && n.abs() < 9e15 => Json::from(n as i64),
            Ok(n) if n.is_finite() => Json::from(n),
            _ => Json::Null,
        },
        other => other.clone(),
    }
}

pub fn draw(ctx: &egui::Context, host: &Host, scale: f32) -> Vec<Action> {
    let now = host.now_ms();
    let mut actions = Vec::new();
    let mut ui_state = host.ui_mut();
    let ui_state: &mut Ui = &mut ui_state;
    let font = |size: f32| egui::FontId::proportional(size * scale);

    if !ui_state.notifications.is_empty() {
        egui::Area::new("script-notifications".into())
            .anchor(egui::Align2::RIGHT_TOP, [-18.0 * scale, 60.0 * scale])
            .interactable(false)
            .show(ctx, |ui| {
                ui.set_max_width(320.0 * scale);
                for note in &ui_state.notifications {
                    panel().show(ui, |ui| {
                        ui.set_min_width(260.0 * scale);
                        if !note.title.is_empty() {
                            ui.label(
                                RichText::new(&note.title)
                                    .font(font(16.0))
                                    .strong()
                                    .color(accent(note.kind)),
                            );
                        }
                        if !note.description.is_empty() {
                            ui.label(
                                RichText::new(&note.description)
                                    .font(font(14.0))
                                    .color(Color32::WHITE),
                            );
                        }
                    });
                    ui.add_space(4.0 * scale);
                }
            });
    }

    if let Some(text) = &ui_state.text {
        egui::Area::new("script-text".into())
            .anchor(egui::Align2::LEFT_CENTER, [18.0 * scale, 0.0])
            .interactable(false)
            .show(ctx, |ui| {
                panel().show(ui, |ui| {
                    ui.label(RichText::new(text).font(font(15.0)).color(Color32::WHITE));
                });
            });
    }

    if let Some(progress) = &ui_state.progress {
        egui::Area::new("script-progress".into())
            .anchor(egui::Align2::CENTER_BOTTOM, [0.0, -90.0 * scale])
            .interactable(false)
            .show(ctx, |ui| {
                panel().show(ui, |ui| {
                    ui.set_width(320.0 * scale);
                    ui.label(
                        RichText::new(&progress.label)
                            .font(font(15.0))
                            .color(Color32::WHITE),
                    );
                    ui.add(
                        egui::ProgressBar::new(progress.fraction(now)).desired_width(320.0 * scale),
                    );
                    ui.label(
                        RichText::new("X to cancel")
                            .font(font(12.0))
                            .color(Color32::GRAY),
                    );
                });
            });
    }

    if let Some(context) = &ui_state.context {
        egui::Area::new("script-context".into())
            .anchor(egui::Align2::RIGHT_CENTER, [-40.0 * scale, 0.0])
            .show(ctx, |ui| {
                panel().show(ui, |ui| {
                    ui.set_width(300.0 * scale);
                    ui.label(
                        RichText::new(&context.title)
                            .font(font(18.0))
                            .strong()
                            .color(Color32::WHITE),
                    );
                    ui.add_space(6.0 * scale);
                    for (index, option) in context.options.iter().enumerate() {
                        let mut text: egui::WidgetText = RichText::new(&option.title)
                            .font(font(15.0))
                            .strong()
                            .into();
                        if !option.description.is_empty() {
                            let mut job = egui::text::LayoutJob::default();
                            RichText::new(format!("{}\n", option.title))
                                .font(font(15.0))
                                .strong()
                                .color(Color32::WHITE)
                                .append_to(
                                    &mut job,
                                    ui.style(),
                                    egui::FontSelection::Default,
                                    egui::Align::LEFT,
                                );
                            RichText::new(&option.description)
                                .font(font(12.5))
                                .color(Color32::LIGHT_GRAY)
                                .append_to(
                                    &mut job,
                                    ui.style(),
                                    egui::FontSelection::Default,
                                    egui::Align::LEFT,
                                );
                            text = job.into();
                        }
                        let button =
                            egui::Button::new(text).min_size(egui::vec2(300.0 * scale, 0.0));
                        if ui.add_enabled(!option.disabled, button).clicked() {
                            actions.push(Action::Select(index));
                        }
                    }
                    ui.add_space(6.0 * scale);
                    if ui
                        .button(RichText::new("Close (Esc)").font(font(13.0)))
                        .clicked()
                    {
                        actions.push(Action::CloseContext);
                    }
                });
            });
    }

    if let Some(dialog) = &mut ui_state.dialog {
        // Numbers are edited as text; parse_dialog stores them as numbers.
        for row in &mut dialog.rows {
            if row.kind == RowKind::Number && !row.value.is_string() {
                row.value = Json::String(
                    row.value
                        .as_f64()
                        .map(|n| n.to_string())
                        .unwrap_or_default(),
                );
            }
        }
        egui::Area::new("script-dialog".into())
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                panel().show(ui, |ui| {
                    ui.set_width(360.0 * scale);
                    ui.label(
                        RichText::new(&dialog.heading)
                            .font(font(18.0))
                            .strong()
                            .color(Color32::WHITE),
                    );
                    ui.add_space(6.0 * scale);
                    for (index, row) in dialog.rows.iter_mut().enumerate() {
                        let label = if row.required {
                            format!("{} *", row.label)
                        } else {
                            row.label.clone()
                        };
                        ui.label(RichText::new(label).font(font(14.0)).color(Color32::WHITE));
                        match &row.kind {
                            RowKind::Input | RowKind::Number => {
                                let mut text = row.value.as_str().unwrap_or_default().to_string();
                                let edit = egui::TextEdit::singleline(&mut text)
                                    .font(font(14.0))
                                    .char_limit(256)
                                    .desired_width(340.0 * scale);
                                if ui.add(edit).changed() {
                                    row.value = Json::String(text);
                                }
                            }
                            RowKind::Checkbox => {
                                let mut on = row.value.as_bool().unwrap_or(false);
                                if ui.checkbox(&mut on, "").changed() {
                                    row.value = Json::Bool(on);
                                }
                            }
                            RowKind::Select(options) => {
                                let current = row.value.as_str().unwrap_or_default().to_string();
                                let shown = options
                                    .iter()
                                    .find(|(value, _)| *value == current)
                                    .map_or("Choose...", |(_, label)| label.as_str());
                                egui::ComboBox::from_id_salt(("script-dialog-select", index))
                                    .selected_text(shown)
                                    .width(340.0 * scale)
                                    .show_ui(ui, |ui| {
                                        for (value, label) in options {
                                            if ui
                                                .selectable_label(*value == current, label)
                                                .clicked()
                                            {
                                                row.value = Json::String(value.clone());
                                            }
                                        }
                                    });
                            }
                        }
                        ui.add_space(4.0 * scale);
                    }
                    let mut values = dialog.clone();
                    for row in &mut values.rows {
                        if row.kind == RowKind::Number {
                            row.value = number(&row.value);
                        }
                    }
                    let values = Ui::dialog_values(&values);
                    ui.horizontal(|ui| {
                        if ui
                            .add_enabled(
                                values.is_some(),
                                egui::Button::new(RichText::new("Confirm").font(font(14.0))),
                            )
                            .clicked()
                        {
                            actions.push(Action::Submit(values.clone()));
                        }
                        if ui
                            .button(RichText::new("Cancel").font(font(14.0)))
                            .clicked()
                        {
                            actions.push(Action::Submit(None));
                        }
                    });
                });
            });
    }
    actions
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn number_rows_parse_to_numbers() {
        assert_eq!(number(&Json::from("250")), Json::from(250));
        assert_eq!(number(&Json::from(" 2.5 ")), Json::from(2.5));
        assert_eq!(number(&Json::from("abc")), Json::Null);
        assert_eq!(number(&Json::from("")), Json::Null);
    }
}
