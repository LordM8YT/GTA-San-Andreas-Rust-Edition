//! Script-driven UI state, drawn by the game with its own egui widgets. Scripts
//! never supply markup or code for the UI: only text, numbers and choices, so
//! no web view or downloaded UI code runs on players' machines.
use serde_json::Value as Json;

const MAX_TEXT: usize = 256;
const MAX_OPTIONS: usize = 32;
const MAX_ROWS: usize = 16;
const MAX_NOTIFICATIONS: usize = 6;

/// Plain text, without control characters, bounded.
pub fn text(value: &Json) -> String {
    let raw = match value {
        Json::String(s) => s.clone(),
        Json::Null => String::new(),
        other => other.to_string(),
    };
    raw.chars()
        .filter(|c| !c.is_control() || *c == '\n')
        .take(MAX_TEXT)
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Inform,
    Success,
    Warning,
    Error,
}
impl Kind {
    fn parse(value: &Json) -> Self {
        match value.as_str() {
            Some("success") => Self::Success,
            Some("warning") => Self::Warning,
            Some("error") => Self::Error,
            _ => Self::Inform,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Notification {
    pub title: String,
    pub description: String,
    pub kind: Kind,
    pub expires_ms: i64,
}

/// Where a choice goes when the player picks it.
#[derive(Clone, Debug, PartialEq)]
pub enum Owner {
    /// A client resource; it receives the index or values back.
    Resource(String),
    /// The server; menus run their option's event, dialogs reply by event.
    Server,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ContextOption {
    pub title: String,
    pub description: String,
    pub disabled: bool,
    /// Server menus only: `event` (client) or `serverEvent` with `args`.
    pub event: Option<String>,
    pub server_event: Option<String>,
    pub args: Json,
}
#[derive(Clone, Debug, PartialEq)]
pub struct Context {
    pub id: String,
    pub title: String,
    pub options: Vec<ContextOption>,
    pub owner: Owner,
}

#[derive(Clone, Debug, PartialEq)]
pub enum RowKind {
    Input,
    Number,
    Checkbox,
    Select(Vec<(String, String)>),
}
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub kind: RowKind,
    pub label: String,
    pub required: bool,
    /// Current value: string, number or boolean.
    pub value: Json,
}
#[derive(Clone, Debug, PartialEq)]
pub struct Dialog {
    pub heading: String,
    pub rows: Vec<Row>,
    pub owner: Owner,
    /// Reply token for the owner.
    pub token: i64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Progress {
    pub label: String,
    pub started_ms: i64,
    pub duration_ms: i64,
    pub owner: Owner,
    pub token: i64,
}
impl Progress {
    pub fn fraction(&self, now_ms: i64) -> f32 {
        ((now_ms - self.started_ms) as f32 / self.duration_ms.max(1) as f32).clamp(0.0, 1.0)
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Ui {
    pub notifications: Vec<Notification>,
    pub context: Option<Context>,
    pub dialog: Option<Dialog>,
    pub progress: Option<Progress>,
    /// Text shown at the side of the screen, like ox_lib's text UI.
    pub text: Option<String>,
}
impl Ui {
    /// Whether the player needs the mouse for this UI.
    pub fn wants_cursor(&self) -> bool {
        self.context.is_some() || self.dialog.is_some()
    }
    pub fn notify(&mut self, data: &Json, now_ms: i64) {
        let data = match data {
            Json::String(_) => serde_json::json!({ "description": data }),
            other => other.clone(),
        };
        let duration = data["duration"].as_i64().unwrap_or(3000).clamp(500, 30_000);
        self.notifications.push(Notification {
            title: text(&data["title"]),
            description: text(&data["description"]),
            kind: Kind::parse(&data["type"]),
            expires_ms: now_ms + duration,
        });
        let excess = self.notifications.len().saturating_sub(MAX_NOTIFICATIONS);
        self.notifications.drain(..excess);
    }
    pub fn expire(&mut self, now_ms: i64) {
        self.notifications.retain(|n| n.expires_ms > now_ms);
    }
    pub fn parse_context(data: &Json, owner: Owner) -> Option<Context> {
        let options = data["options"]
            .as_array()?
            .iter()
            .take(MAX_OPTIONS)
            .map(|o| ContextOption {
                title: text(&o["title"]),
                description: text(&o["description"]),
                disabled: o["disabled"].as_bool().unwrap_or(false),
                event: o["event"].as_str().map(str::to_string),
                server_event: o["serverEvent"].as_str().map(str::to_string),
                args: o["args"].clone(),
            })
            .collect();
        Some(Context {
            id: text(&data["id"]),
            title: text(&data["title"]),
            options,
            owner,
        })
    }
    pub fn parse_dialog(heading: &Json, rows: &Json, owner: Owner, token: i64) -> Option<Dialog> {
        let rows = rows
            .as_array()?
            .iter()
            .take(MAX_ROWS)
            .map(|r| {
                let r = match r {
                    Json::String(_) => serde_json::json!({ "type": "input", "label": r }),
                    other => other.clone(),
                };
                let kind = match r["type"].as_str() {
                    Some("number") => RowKind::Number,
                    Some("checkbox") => RowKind::Checkbox,
                    Some("select") => RowKind::Select(
                        r["options"]
                            .as_array()
                            .map(|list| {
                                list.iter()
                                    .take(MAX_OPTIONS)
                                    .map(|o| {
                                        let value = text(&o["value"]);
                                        let label = o.get("label").map_or(value.clone(), text);
                                        (value, label)
                                    })
                                    .collect()
                            })
                            .unwrap_or_default(),
                    ),
                    _ => RowKind::Input,
                };
                let value = match (&kind, &r["default"]) {
                    (RowKind::Checkbox, v) => Json::Bool(v.as_bool().unwrap_or(false)),
                    (RowKind::Number, v) => v.as_f64().map_or(Json::Null, Json::from),
                    (_, Json::Null) => Json::String(String::new()),
                    (_, v) => Json::String(text(v)),
                };
                Row {
                    kind,
                    label: text(&r["label"]),
                    required: r["required"].as_bool().unwrap_or(false),
                    value,
                }
            })
            .collect();
        Some(Dialog {
            heading: text(heading),
            rows,
            owner,
            token,
        })
    }
    /// Values to return, or `None` while a required row is empty.
    pub fn dialog_values(dialog: &Dialog) -> Option<Vec<Json>> {
        dialog
            .rows
            .iter()
            .map(|row| {
                let empty = match &row.value {
                    Json::Null => true,
                    Json::String(s) => s.trim().is_empty(),
                    _ => false,
                };
                (!row.required || !empty).then(|| row.value.clone())
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn notifications_are_sanitized_bounded_and_expire() {
        let mut ui = Ui::default();
        for i in 0..10 {
            ui.notify(
                &json!({ "title": format!("t{i}\u{7}"), "type": "error", "duration": 1000 }),
                0,
            );
        }
        assert_eq!(ui.notifications.len(), MAX_NOTIFICATIONS);
        assert_eq!(ui.notifications[0].title, "t4");
        assert_eq!(ui.notifications[0].kind, Kind::Error);
        ui.notify(&json!("plain"), 0);
        assert_eq!(ui.notifications.last().unwrap().description, "plain");
        ui.expire(5000);
        assert!(ui.notifications.is_empty());
    }

    #[test]
    fn dialogs_parse_rows_and_require_values() {
        let dialog = Ui::parse_dialog(
            &json!("Bank"),
            &json!([
                { "type": "number", "label": "Amount", "required": true },
                { "type": "select", "label": "Account", "options": [{ "value": "cash" }, { "value": "bank", "label": "Bank" }], "default": "bank" },
                { "type": "checkbox", "label": "Receipt", "default": true },
                "Note"
            ]),
            Owner::Server,
            7,
        )
        .unwrap();
        assert_eq!(dialog.rows.len(), 4);
        assert_eq!(
            dialog.rows[1].kind,
            RowKind::Select(vec![
                ("cash".into(), "cash".into()),
                ("bank".into(), "Bank".into())
            ])
        );
        assert_eq!(Ui::dialog_values(&dialog), None);
        let mut filled = dialog.clone();
        filled.rows[0].value = json!(250.0);
        assert_eq!(
            Ui::dialog_values(&filled),
            Some(vec![json!(250.0), json!("bank"), json!(true), json!("")])
        );
    }
}
