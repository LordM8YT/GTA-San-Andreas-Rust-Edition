//! MTA:SA `.map` files: XML lists of `<object>` placements and
//! `<removeWorldObject>` entries. Implemented from the attribute list on the
//! public MTA wiki (Object, RemoveWorldModel, SetElementRotation pages).
//! Other elements (vehicles, spawnpoints, markers, scripts) are skipped.
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
#[derive(Debug, Default)]
pub(crate) struct MtaMap {
    pub objects: Vec<MapObject>,
    pub removals: Vec<WorldRemoval>,
    /// Elements SARE does not load yet, such as vehicles or markers.
    pub skipped: usize,
    /// Objects whose `scale` differs from 1; they are placed unscaled.
    pub unscaled: usize,
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
                b"object" => {
                    let object = object(&element)?;
                    if attribute(&element, "scale")?
                        .map(|s| s.parse::<f32>())
                        .transpose()?
                        .is_some_and(|scale| (scale - 1.0).abs() > 1e-3)
                    {
                        map.unscaled += 1;
                    }
                    map.objects.push(object);
                }
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
    Ok(MapObject {
        model,
        position: position(element)?,
        rotation: ipl_rotation(degrees),
        interior,
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
    <object id="crate" model="1271" posX="1" posY="2" posZ="3" scale="2" />
    <removeWorldObject id="r1" radius="5.5" interior="0" model="17699" lodModel="17979" posX="10" posY="20" posZ="30" rotX="0" rotY="0" rotZ="0"></removeWorldObject>
    <vehicle id="v" model="411" posX="0" posY="0" posZ="0" />
    <spawnpoint id="s" posX="0" posY="0" posZ="0" />
</map>"#;

    #[test]
    fn reads_objects_and_removals() {
        let map = parse(SAMPLE.as_bytes()).unwrap();
        assert_eq!(map.objects.len(), 2);
        assert_eq!(map.objects[0].model, 1634);
        assert_eq!(map.objects[0].position, [2495.5, -1668.25, 13.1]);
        assert_eq!(map.objects[1].rotation, [0.0, 0.0, 0.0, 1.0]);
        assert_eq!(map.unscaled, 1);
        assert_eq!(map.skipped, 2);
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
    fn rejects_non_map_xml() {
        assert!(parse(b"<meta><map src=\"a.map\"/></meta>").is_err());
        assert!(parse(b"<map><object posX=\"1\" posY=\"2\" posZ=\"3\"/></map>").is_err());
    }
}
