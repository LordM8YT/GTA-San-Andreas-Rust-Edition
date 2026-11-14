//! How a content-pack character looks: body, skin, hair and clothes. The
//! server owns every player's appearance (scripts set it with the
//! `SetPlayerAppearance` native) and sends it to all players with
//! [`APPEARANCE_EVENT`]; clients only draw what they were sent.
use serde::{Deserialize, Serialize};
use serde_json::Value as Json;

/// Server -> client: `[{ "<player id>": appearance or null, ... }]`. A `null`
/// entry forgets the player (they left or their look was cleared).
pub const APPEARANCE_EVENT: &str = "__sare:appearance";
/// Highest style index accepted for hair, tops, pants and shoes.
pub const MAX_STYLE: u8 = 31;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Appearance {
    /// 0 = male base, 1 = female base.
    pub body: u8,
    /// sRGB colours.
    pub skin: [u8; 3],
    pub hair: u8,
    pub hair_color: [u8; 3],
    pub top: u8,
    pub top_color: [u8; 3],
    pub pants: u8,
    pub pants_color: [u8; 3],
    pub shoes: u8,
    pub shoes_color: [u8; 3],
}
impl Default for Appearance {
    fn default() -> Self {
        Self {
            body: 0,
            skin: [214, 160, 120],
            hair: 1,
            hair_color: [58, 40, 28],
            top: 0,
            top_color: [70, 110, 170],
            pants: 0,
            pants_color: [52, 62, 84],
            shoes: 0,
            shoes_color: [235, 235, 235],
        }
    }
}

fn color(value: Option<&Json>, fallback: [u8; 3]) -> [u8; 3] {
    let Some(Json::Array(parts)) = value else {
        return fallback;
    };
    if parts.len() != 3 {
        return fallback;
    }
    let mut out = fallback;
    for (slot, part) in out.iter_mut().zip(parts) {
        match part.as_f64() {
            Some(v) if v.is_finite() => *slot = v.clamp(0.0, 255.0).round() as u8,
            _ => return fallback,
        }
    }
    out
}
fn style(value: Option<&Json>, fallback: u8, max: u8) -> u8 {
    match value.and_then(Json::as_f64) {
        Some(v) if v.is_finite() => v.clamp(0.0, max as f64) as u8,
        _ => fallback,
    }
}

impl Appearance {
    /// Read a script table or wire value leniently: missing or broken fields
    /// keep their defaults, numbers are clamped, unknown keys are ignored.
    /// `None` if the value is not an object at all.
    pub fn from_json(value: &Json) -> Option<Self> {
        let map = value.as_object()?;
        let d = Self::default();
        let field = |name: &str, alt: &str| map.get(name).or_else(|| map.get(alt));
        Some(Self {
            body: style(field("body", "body"), d.body, 1),
            skin: color(field("skin", "skin"), d.skin),
            hair: style(field("hair", "hair"), d.hair, MAX_STYLE),
            hair_color: color(field("hairColor", "hair_color"), d.hair_color),
            top: style(field("top", "top"), d.top, MAX_STYLE),
            top_color: color(field("topColor", "top_color"), d.top_color),
            pants: style(field("pants", "pants"), d.pants, MAX_STYLE),
            pants_color: color(field("pantsColor", "pants_color"), d.pants_color),
            shoes: style(field("shoes", "shoes"), d.shoes, MAX_STYLE),
            shoes_color: color(field("shoesColor", "shoes_color"), d.shoes_color),
        })
    }
    pub fn to_json(&self) -> Json {
        serde_json::to_value(self).unwrap_or(Json::Null)
    }
}

/// Parse the argument of an [`APPEARANCE_EVENT`]: player id -> look, with
/// `None` for players to forget. Broken entries are skipped.
pub fn parse_event(payload: &str) -> Vec<(u32, Option<Appearance>)> {
    let Ok(Json::Array(args)) = serde_json::from_str::<Json>(payload) else {
        return Vec::new();
    };
    let Some(Json::Object(map)) = args.first() else {
        return Vec::new();
    };
    map.iter()
        .filter_map(|(id, value)| {
            let id = id.parse().ok()?;
            match value {
                Json::Null => Some((id, None)),
                other => Some((id, Some(Appearance::from_json(other)?))),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn script_tables_are_clamped_and_defaults_fill_gaps() {
        let look = Appearance::from_json(&json!({
            "body": 7, "skin": [300, -4, 12.4], "hair": 3, "topColor": [1, 2],
            "pants_color": [9, 8, 7], "shoes": -2, "extra": true
        }))
        .unwrap();
        assert_eq!(look.body, 1);
        assert_eq!(look.skin, [255, 0, 12]);
        assert_eq!(look.hair, 3);
        assert_eq!(look.top_color, Appearance::default().top_color);
        assert_eq!(look.pants_color, [9, 8, 7]);
        assert_eq!(look.shoes, 0);
        assert!(Appearance::from_json(&json!("nope")).is_none());
    }
    #[test]
    fn events_round_trip() {
        let look = Appearance {
            hair: 4,
            ..Default::default()
        };
        let payload = json!([{ "3": look.to_json(), "9": null, "x": {} }]).to_string();
        let mut parsed = parse_event(&payload);
        parsed.sort_by_key(|(id, _)| *id);
        assert_eq!(parsed, vec![(3, Some(look)), (9, None)]);
    }
}
