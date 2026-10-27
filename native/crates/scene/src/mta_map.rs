//! MTA:SA `.map` files: XML lists of `<object>` placements and
//! `<removeWorldObject>` entries. Implemented from the attribute list on the
//! public MTA wiki (Object, RemoveWorldModel, SetElementRotation pages).
//! Spawn points become teleport targets. Other elements (vehicles, markers,
//! pickups) are skipped, and scripts are never run: model replacements are
//! read from them as text, see [`script_models`].
use anyhow::{bail, ensure, Context, Result};
use quick_xml::events::{BytesStart, Event};

pub(crate) const MAX_MAP_ELEMENTS: usize = 20_000;

#[derive(Debug, PartialEq)]
pub(crate) struct MapObject {
    pub model: i32,
    pub position: [f32; 3],
    /// IPL convention: the conjugate of the object's rotation.
    pub rotation: [f32; 4],
    pub interior: i32,
    pub scale: f32,
    /// `collisions="false"`: the object is drawn but has no collision.
    pub solid: bool,
}
#[derive(Debug, PartialEq)]
pub(crate) struct WorldRemoval {
    pub model: i32,
    pub lod_model: Option<i32>,
    pub position: [f32; 3],
    pub radius: f32,
    /// `None` applies to every interior.
    pub interior: Option<i32>,
}
/// What a race pickup does when a car drives through it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PickupKind {
    Nitro,
    Repair,
    /// Swap the car for this original vehicle ID, keeping its speed.
    VehicleChange(i32),
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MapPickup {
    pub kind: PickupKind,
    /// San Andreas world coordinates.
    pub position: [f32; 3],
    /// Milliseconds before it can be taken again; 0 leaves it to the game.
    pub respawn_ms: u32,
}
#[derive(Debug, Default)]
pub(crate) struct MtaMap {
    pub objects: Vec<MapObject>,
    pub removals: Vec<WorldRemoval>,
    pub spawns: Vec<[f32; 3]>,
    pub pickups: Vec<MapPickup>,
    /// Elements SARE does not load yet, such as vehicles or markers.
    pub skipped: usize,
}

pub(crate) fn parse(data: &[u8]) -> Result<MtaMap> {
    let text = std::str::from_utf8(data).context("MTA map is not UTF-8")?;
    let mut reader = quick_xml::Reader::from_str(text);
    let mut map = MtaMap::default();
    let mut depth = 0_usize;
    let mut root = false;
    loop {
        let (element, nested) = match reader.read_event()? {
            Event::Start(element) => (element, true),
            Event::Empty(element) => (element, false),
            Event::End(_) => {
                depth = depth.saturating_sub(1);
                continue;
            }
            Event::Eof => break,
            _ => continue,
        };
        if depth == 0 {
            ensure!(
                element.name().as_ref() == b"map",
                "MTA map root must be <map>"
            );
            root = true;
        } else if depth == 1 {
            match element.name().as_ref() {
                // One malformed or invisible (scale 0) object skips only itself.
                b"object" => match object(&element) {
                    Ok(object) => map.objects.push(object),
                    Err(_) => map.skipped += 1,
                },
                b"spawnpoint" if map.spawns.len() < 64 => map.spawns.push(position(&element)?),
                b"racepickup" if map.pickups.len() < 256 => match pickup(&element) {
                    Ok(pickup) => map.pickups.push(pickup),
                    Err(_) => map.skipped += 1,
                },
                b"removeWorldObject" => map.removals.push(removal(&element)?),
                _ => map.skipped += 1,
            }
            ensure!(
                map.objects.len() + map.removals.len() <= MAX_MAP_ELEMENTS,
                "MTA map exceeds {MAX_MAP_ELEMENTS} objects"
            );
        }
        if nested {
            depth += 1;
            ensure!(depth <= 16, "MTA map nesting too deep");
        }
    }
    ensure!(root, "MTA map has no <map> root");
    Ok(map)
}

/// A drop-in MTA resource folder: its `meta.xml` becomes an enabled native
/// manifest listing the `<map src>` files and the model files its client
/// scripts load. `read` returns a resource file; scripts are only read.
pub(crate) fn meta_manifest(
    data: &[u8],
    folder_name: &str,
    read: impl Fn(&str) -> Result<Vec<u8>>,
) -> Result<serde_json::Value> {
    let text = std::str::from_utf8(data).context("MTA meta.xml is not UTF-8")?;
    let mut reader = quick_xml::Reader::from_str(text);
    let mut name = None;
    let mut maps = Vec::new();
    let mut scripts = Vec::new();
    let mut depth = 0_usize;
    loop {
        let (element, nested) = match reader.read_event()? {
            Event::Start(element) => (element, true),
            Event::Empty(element) => (element, false),
            Event::End(_) => {
                depth = depth.saturating_sub(1);
                continue;
            }
            Event::Eof => break,
            _ => continue,
        };
        if depth == 0 {
            ensure!(
                element.name().as_ref() == b"meta",
                "MTA meta.xml root must be <meta>"
            );
        } else if depth == 1 {
            match element.name().as_ref() {
                b"info" => name = attribute(&element, "name")?.filter(|n| !n.is_empty()),
                b"map" => {
                    let src = attribute(&element, "src")?.context("MTA <map> lacks src")?;
                    maps.push(src.replace('\\', "/"));
                    ensure!(maps.len() <= 16, "MTA map limit (16 per resource)");
                }
                b"script" if attribute(&element, "type")?.as_deref() != Some("server") => {
                    if let Some(src) = attribute(&element, "src")? {
                        scripts.push(src.replace('\\', "/"));
                        ensure!(scripts.len() <= 128, "MTA script limit (128 per resource)");
                    }
                }
                _ => {}
            }
        }
        if nested {
            depth += 1;
            ensure!(depth <= 16, "MTA meta.xml nesting too deep");
        }
    }
    ensure!(!maps.is_empty(), "MTA meta.xml lists no <map> files");
    let name: String = name
        .unwrap_or_else(|| folder_name.to_owned())
        .chars()
        .filter(|c| !c.is_control())
        .take(96)
        .collect();
    let mut models = std::collections::BTreeMap::new();
    for script in scripts {
        // A missing or unreadable script only loses its model replacements.
        let Ok(text) = read(&script) else { continue };
        for model in script_models(&String::from_utf8_lossy(&text)) {
            let entry = models.entry(model.id).or_insert_with(ScriptModel::default);
            entry.id = model.id;
            entry.dff = model.dff.or(entry.dff.take());
            entry.txd = model.txd.or(entry.txd.take());
            entry.col = model.col.or(entry.col.take());
        }
    }
    let models: Vec<_> = models
        .into_values()
        .take(256)
        .map(|m| serde_json::json!({ "id": m.id, "dff": m.dff, "txd": m.txd, "col": m.col }))
        .collect();
    Ok(serde_json::json!({
        "enabled": true,
        "name": name,
        "mta_maps": maps,
        "mta_models": models,
    }))
}

/// A model file set that a client script assigns to an ID.
#[derive(Debug, Default, PartialEq)]
pub(crate) struct ScriptModel {
    pub id: i32,
    pub dff: Option<String>,
    pub txd: Option<String>,
    pub col: Option<String>,
}

/// Model replacements in MTA client Lua, found by reading the text, never by
/// running it. Two common forms are recognised: tables whose fields name a
/// `.dff`/`.txd`/`.col` file and a model ID, and `engineLoadDFF/TXD/COL`
/// results passed to `engineReplaceModel`, `engineImportTXD` or
/// `engineReplaceCOL` with a literal ID. Anything computed at run time is missed.
pub(crate) fn script_models(source: &str) -> Vec<ScriptModel> {
    let text = strip_lua_comments(source);
    let mut found = Vec::new();
    // Form 1: { dffFile = "a.dff", txdFile = "a.txd", modelID = 1234 }
    let mut stack = Vec::new();
    for (index, ch) in text.char_indices() {
        match ch {
            '{' => stack.push(index),
            '}' => {
                if let Some(start) = stack.pop() {
                    let block = &text[start + 1..index];
                    if !block.contains('{') {
                        found.extend(table_model(block));
                    }
                }
            }
            _ => {}
        }
    }
    // Form 2: local d = engineLoadDFF("a.dff") ... engineReplaceModel(d, 1234)
    let loads = [
        ("engineLoadDFF", "engineReplaceModel"),
        ("engineLoadTXD", "engineImportTXD"),
        ("engineLoadCOL", "engineReplaceCOL"),
    ];
    for (kind, (load, apply)) in loads.iter().enumerate() {
        let mut variables = std::collections::HashMap::new();
        for (at, _) in text.match_indices(load) {
            let Some(path) = call_args(&text[at + load.len()..])
                .first()
                .and_then(|a| string_literal(a))
            else {
                continue;
            };
            let before = text[..at].trim_end();
            if let Some(before) = before.strip_suffix('=') {
                let name: String = before
                    .trim_end()
                    .chars()
                    .rev()
                    .take_while(|c| c.is_alphanumeric() || *c == '_')
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .collect();
                if !name.is_empty() {
                    variables.insert(name, path);
                }
            }
        }
        for (at, _) in text.match_indices(apply) {
            let args = call_args(&text[at + apply.len()..]);
            let (Some(first), Some(id)) = (args.first(), args.get(1).and_then(|a| a.parse().ok()))
            else {
                continue;
            };
            let path = first
                .strip_prefix(load)
                .and_then(|inner| call_args(inner).first().and_then(|a| string_literal(a)))
                .or_else(|| variables.get(*first).cloned());
            let Some(path) = path else { continue };
            let mut model = ScriptModel {
                id,
                ..Default::default()
            };
            *[&mut model.dff, &mut model.txd, &mut model.col][kind] = Some(path);
            found.push(model);
        }
    }
    found
        .into_iter()
        .filter(|m| {
            m.id >= 0
                && [&m.dff, &m.txd, &m.col]
                    .iter()
                    .all(|p| p.as_deref().is_none_or(local_path))
        })
        .collect()
}
fn table_model(block: &str) -> Option<ScriptModel> {
    let mut model = ScriptModel::default();
    let mut id = None;
    for field in block.split([',', ';', '\n']) {
        let Some((key, value)) = field.split_once('=') else {
            continue;
        };
        let key = key
            .trim()
            .trim_matches(['[', ']', '"', '\''])
            .to_ascii_lowercase();
        let value = value.trim();
        if let Some(path) = string_literal(value) {
            let lower = path.to_ascii_lowercase();
            if lower.ends_with(".dff") {
                model.dff = Some(path);
            } else if lower.ends_with(".txd") {
                model.txd = Some(path);
            } else if lower.ends_with(".col") {
                model.col = Some(path);
            }
        } else if ["modelid", "model", "id", "objectid"].contains(&key.as_str()) {
            id = value.parse().ok();
        }
    }
    model.id = id?;
    (model.dff.is_some() || model.txd.is_some()).then_some(model)
}
/// Arguments of the call whose `(` starts `rest`, split at top-level commas.
fn call_args(rest: &str) -> Vec<&str> {
    let Some(rest) = rest.trim_start().strip_prefix('(') else {
        return Vec::new();
    };
    let (mut depth, mut start, mut args, mut quote) = (0, 0, Vec::new(), None);
    for (index, ch) in rest.char_indices() {
        match (quote, ch) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'') => quote = Some(ch),
            (None, '(' | '{') => depth += 1,
            (None, ')') if depth == 0 => {
                args.push(rest[start..index].trim());
                return args;
            }
            (None, ')' | '}') => depth -= 1,
            (None, ',') if depth == 0 => {
                args.push(rest[start..index].trim());
                start = index + 1;
            }
            _ => {}
        }
        if index > 4096 {
            break;
        }
    }
    Vec::new()
}
fn string_literal(value: &str) -> Option<String> {
    let value = value.trim();
    let quote = value.chars().next().filter(|c| *c == '"' || *c == '\'')?;
    let inner = value[1..].strip_suffix(quote)?;
    (!inner.contains(quote)).then(|| inner.replace('\\', "/"))
}
/// Paths inside this resource only; `:other/file` and parent paths are skipped.
fn local_path(path: &str) -> bool {
    !path.is_empty() && !path.starts_with([':', '/']) && !path.split('/').any(|part| part == "..")
}
fn strip_lua_comments(source: &str) -> String {
    let mut out = String::with_capacity(source.len());
    let mut rest = source;
    while let Some(at) = rest.find("--") {
        out.push_str(&rest[..at]);
        let after = &rest[at + 2..];
        let level = after
            .strip_prefix('[')
            .map(|r| r.chars().take_while(|c| *c == '=').count())
            .filter(|n| after[1 + n..].starts_with('['));
        rest = match level {
            Some(n) => {
                let close = format!("]{}]", "=".repeat(n));
                after
                    .find(&close)
                    .map_or("", |end| &after[end + close.len()..])
            }
            None => after.find('\n').map_or("", |end| &after[end..]),
        };
    }
    out.push_str(rest);
    out
}

fn attribute(element: &BytesStart, name: &str) -> Result<Option<String>> {
    match element.try_get_attribute(name)? {
        Some(value) => Ok(Some(
            value
                .normalized_value(quick_xml::XmlVersion::Implicit1_0)?
                .trim()
                .to_owned(),
        )),
        None => Ok(None),
    }
}
fn number(element: &BytesStart, name: &str, default: Option<f32>) -> Result<f32> {
    let value = match attribute(element, name)? {
        Some(text) => text
            .parse::<f32>()
            .with_context(|| format!("MTA map attribute {name}=\"{text}\""))?,
        None => default.with_context(|| format!("MTA map element lacks {name}"))?,
    };
    ensure!(value.is_finite(), "MTA map attribute {name} is not finite");
    Ok(value)
}
fn integer(element: &BytesStart, name: &str) -> Result<Option<i32>> {
    attribute(element, name)?
        .map(|text| {
            text.parse::<i32>()
                .with_context(|| format!("MTA map attribute {name}=\"{text}\""))
        })
        .transpose()
}
fn position(element: &BytesStart) -> Result<[f32; 3]> {
    Ok([
        number(element, "posX", None)?,
        number(element, "posY", None)?,
        number(element, "posZ", None)?,
    ])
}
fn object(element: &BytesStart) -> Result<MapObject> {
    let model = integer(element, "model")?.context("MTA map object lacks model")?;
    ensure!(model >= 0, "MTA map object has negative model {model}");
    let degrees = [
        number(element, "rotX", Some(0.0))?,
        number(element, "rotY", Some(0.0))?,
        number(element, "rotZ", Some(0.0))?,
    ];
    let interior = integer(element, "interior")?.unwrap_or(0);
    ensure!(
        (0..=255).contains(&interior),
        "MTA map object interior {interior} out of range"
    );
    let scale = number(element, "scale", Some(1.0))?;
    ensure!(
        (0.01..=100.0).contains(&scale),
        "MTA map object scale {scale} out of range"
    );
    Ok(MapObject {
        model,
        position: position(element)?,
        rotation: ipl_rotation(degrees),
        interior,
        scale,
        solid: attribute(element, "collisions")?.is_none_or(|v| v != "false"),
    })
}
fn pickup(element: &BytesStart) -> Result<MapPickup> {
    let kind = match attribute(element, "type")?.as_deref() {
        Some("nitro") => PickupKind::Nitro,
        Some("repair") => PickupKind::Repair,
        Some("vehiclechange") => PickupKind::VehicleChange(
            integer(element, "vehicle")?.context("vehiclechange pickup lacks vehicle")?,
        ),
        other => bail!("unknown race pickup type {other:?}"),
    };
    let respawn = number(element, "respawn", Some(0.0))?;
    Ok(MapPickup {
        kind,
        position: position(element)?,
        respawn_ms: respawn.clamp(0.0, 3_600_000.0) as u32,
    })
}
fn removal(element: &BytesStart) -> Result<WorldRemoval> {
    let model = integer(element, "model")?.context("removeWorldObject lacks model")?;
    let radius = number(element, "radius", None)?;
    if !(0.0..=10_000.0).contains(&radius) {
        bail!("removeWorldObject radius {radius} out of range");
    }
    Ok(WorldRemoval {
        model,
        lod_model: integer(element, "lodModel")?.filter(|id| *id > 0),
        position: position(element)?,
        radius,
        interior: integer(element, "interior")?.filter(|id| *id >= 0),
    })
}
/// MTA object rotations are degrees applied Z, then X, then Y about the
/// rotated axes. IPL rows store the inverse rotation, so conjugate.
fn ipl_rotation(degrees: [f32; 3]) -> [f32; 4] {
    let [x, y, z] = degrees.map(f32::to_radians);
    let q = glam::Quat::from_euler(glam::EulerRot::ZXY, z, x, y);
    [-q.x, -q.y, -q.z, q.w]
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"<map edf:definitions="editor_main">
    <object id="ramp (1)" doublesided="false" model="1634" interior="0" dimension="0" posX="2495.5" posY="-1668.25" posZ="13.1" rotX="0" rotY="0" rotZ="90"></object>
    <object id="crate" model="1271" posX="1" posY="2" posZ="3" scale="2" collisions="false" />
    <removeWorldObject id="r1" radius="5.5" interior="0" model="17699" lodModel="17979" posX="10" posY="20" posZ="30" rotX="0" rotY="0" rotZ="0"></removeWorldObject>
    <vehicle id="v" model="411" posX="0" posY="0" posZ="0" />
    <spawnpoint id="s" posX="0" posY="0" posZ="0" />
    <racepickup id="p1" type="nitro" vehicle="522" respawn="0" posX="1" posY="2" posZ="3" />
    <racepickup id="p2" type="vehiclechange" vehicle="411" respawn="5000" posX="4" posY="5" posZ="6" />
    <racepickup id="p3" type="teleport" posX="0" posY="0" posZ="0" />
</map>"#;

    #[test]
    fn reads_objects_and_removals() {
        let map = parse(SAMPLE.as_bytes()).unwrap();
        assert_eq!(map.objects.len(), 2);
        assert_eq!(map.objects[0].model, 1634);
        assert_eq!(map.objects[0].position, [2495.5, -1668.25, 13.1]);
        assert_eq!(map.objects[1].rotation, [0.0, 0.0, 0.0, 1.0]);
        assert_eq!(map.objects[1].scale, 2.0);
        assert!(map.objects[0].solid);
        assert!(!map.objects[1].solid);
        assert_eq!(map.spawns, vec![[0.0, 0.0, 0.0]]);
        assert_eq!(map.skipped, 2);
        assert_eq!(
            map.pickups,
            vec![
                MapPickup {
                    kind: PickupKind::Nitro,
                    position: [1.0, 2.0, 3.0],
                    respawn_ms: 0
                },
                MapPickup {
                    kind: PickupKind::VehicleChange(411),
                    position: [4.0, 5.0, 6.0],
                    respawn_ms: 5000
                },
            ]
        );
        assert_eq!(
            map.removals,
            vec![WorldRemoval {
                model: 17699,
                lod_model: Some(17979),
                position: [10.0, 20.0, 30.0],
                radius: 5.5,
                interior: Some(0),
            }]
        );
    }

    #[test]
    fn yaw_matches_ipl_convention() {
        // A 90 degree yaw turns the model's +X axis to world +Y. SARE's
        // `rotate` applies the conjugate of the stored IPL quaternion.
        let q = ipl_rotation([0.0, 0.0, 90.0]);
        let world = crate::rotate([1.0, 0.0, 0.0], q);
        assert!((world[0]).abs() < 1e-5 && (world[1] - 1.0).abs() < 1e-5);
    }

    #[test]
    fn meta_xml_lists_maps() {
        let meta = br#"<meta>
    <info author="someone" type="map" name="Stunt park" version="1.0" />
    <map src="stunt.map" dimension="0"></map>
    <script src="client.lua" type="client" />
    <file src="models/ramp.dff" />
</meta>"#;
        let read = |path: &str| -> Result<Vec<u8>> {
            ensure!(path == "client.lua");
            Ok(
                br#"local d = engineLoadDFF("models/ramp.dff") engineReplaceModel(d, 1634)"#
                    .to_vec(),
            )
        };
        assert_eq!(
            meta_manifest(meta, "stunt", read).unwrap(),
            serde_json::json!({"enabled": true, "name": "Stunt park", "mta_maps": ["stunt.map"],
                "mta_models": [{"id": 1634, "dff": "models/ramp.dff", "txd": null, "col": null}]})
        );
        assert!(meta_manifest(b"<meta><script src=\"a.lua\"/></meta>", "x", read).is_err());
    }

    #[test]
    fn script_models_reads_tables_and_calls_without_running_them() {
        let lua = r#"
local modelsToReplace = {
    --[[{ dffFile = "dff/.dff", modelID = 1 },--]]
    { colFile = "road.col", txdFile = "road.txd", dffFile = "road.dff", modelID = 18450, alphaTransparency = true },
    { dffFile = "dff/tag.dff", modelID = 1524 }, -- { dffFile = "x.dff", modelID = 2 }
}
local btxd = engineLoadTXD("b.txd")
engineImportTXD(btxd, 6959)
local bdff = engineLoadDFF("b.dff", 0)
engineReplaceModel(bdff, 6959)
engineReplaceCOL(engineLoadCOL("b.col"), 6959)
engineReplaceModel(engineLoadDFF(":other/x.dff"), 7)
engineReplaceModel(bdff, someVariable)
"#;
        let model = |id, dff: Option<&str>, txd: Option<&str>, col: Option<&str>| ScriptModel {
            id,
            dff: dff.map(Into::into),
            txd: txd.map(Into::into),
            col: col.map(Into::into),
        };
        assert_eq!(
            script_models(lua),
            vec![
                model(18450, Some("road.dff"), Some("road.txd"), Some("road.col")),
                model(1524, Some("dff/tag.dff"), None, None),
                model(6959, Some("b.dff"), None, None),
                model(6959, None, Some("b.txd"), None),
                model(6959, None, None, Some("b.col")),
            ]
        );
    }

    #[test]
    fn rejects_non_map_xml() {
        assert!(parse(b"<meta><map src=\"a.map\"/></meta>").is_err());
        let map = parse(br#"<map><object posX="1" posY="2" posZ="3"/><object model="1" posX="0" posY="0" posZ="0" scale="0"/></map>"#).unwrap();
        assert!(map.objects.is_empty() && map.skipped == 2);
    }
}
