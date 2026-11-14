//! Server-owned entities (vehicles, peds and objects) as they travel to
//! clients. A SARE server replicates them through ordinary script events, so
//! no protocol change is needed: `ENTITIES_EVENT` carries created or changed
//! entities, `ENTITY_DELETE_EVENT` the handles that left the player's scope
//! (deleted, or moved to another routing bucket).
use serde::{Deserialize, Serialize};

pub const ENTITIES_EVENT: &str = "__sare:entities";
pub const ENTITY_DELETE_EVENT: &str = "__sare:entityDelete";
/// Server entity handles start here, far above player IDs (players' ped
/// handles are their player IDs, as in single-ped FiveM servers).
pub const FIRST_HANDLE: u32 = 0x0010_0000;
/// Most server entities one server keeps at a time.
pub const MAX_ENTITIES: usize = 2048;
/// Entities per replication event, so a batch stays under the event payload
/// limit with long model names.
pub const ENTITIES_PER_EVENT: usize = 24;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EntityKind {
    Ped,
    Vehicle,
    Object,
}
impl EntityKind {
    /// FiveM's `GetEntityType`: 1 ped, 2 vehicle, 3 object.
    pub fn type_id(self) -> u8 {
        match self {
            Self::Ped => 1,
            Self::Vehicle => 2,
            Self::Object => 3,
        }
    }
}

/// A model as the script gave it: a name (`'infernus'`) or a number, which is
/// either a FiveM-style hash (`` `infernus` ``, `GetHashKey`) or an original
/// model ID from the player's own data files.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ModelRef {
    Hash(i64),
    Name(String),
}
impl ModelRef {
    pub fn valid(&self) -> bool {
        match self {
            Self::Hash(_) => true,
            Self::Name(name) => {
                !name.is_empty()
                    && name.len() <= 32
                    && name
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
            }
        }
    }
}

/// One entity on the wire. Positions are San Andreas world coordinates and
/// headings degrees, as in scripts.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EntityState {
    #[serde(rename = "h")]
    pub handle: u32,
    #[serde(rename = "k")]
    pub kind: EntityKind,
    #[serde(rename = "m")]
    pub model: ModelRef,
    #[serde(rename = "p")]
    pub position: [f32; 3],
    #[serde(rename = "r")]
    pub heading: f32,
    /// Vehicle paint as sRGB, when a script set one.
    #[serde(rename = "c", default, skip_serializing_if = "Option::is_none")]
    pub color: Option<[u8; 3]>,
}
impl EntityState {
    pub fn valid(&self) -> bool {
        self.handle >= FIRST_HANDLE
            && self.model.valid()
            && self
                .position
                .iter()
                .all(|v| v.is_finite() && v.abs() < 100_000.0)
            && self.heading.is_finite()
    }
}

/// Parse the arguments of an `ENTITIES_EVENT`; invalid entries are skipped.
pub fn parse_entities(payload: &str) -> Vec<EntityState> {
    let Ok((list,)) = serde_json::from_str::<(Vec<serde_json::Value>,)>(payload) else {
        return Vec::new();
    };
    list.into_iter()
        .filter_map(|v| serde_json::from_value::<EntityState>(v).ok())
        .filter(EntityState::valid)
        .collect()
}
/// Parse the arguments of an `ENTITY_DELETE_EVENT`.
pub fn parse_deletes(payload: &str) -> Vec<u32> {
    serde_json::from_str::<(Vec<u32>,)>(payload)
        .map(|(list,)| list)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn entity_batches_round_trip_and_fit_one_event() {
        let entity = EntityState {
            handle: FIRST_HANDLE,
            kind: EntityKind::Vehicle,
            model: ModelRef::Name("a".repeat(32)),
            position: [-99_999.5, 99_999.5, -99_999.5],
            heading: 359.99,
            color: Some([255, 255, 255]),
        };
        let batch = vec![entity.clone(); ENTITIES_PER_EVENT];
        let payload = serde_json::to_string(&(batch,)).unwrap();
        assert!(
            crate::valid_event(ENTITIES_EVENT, &payload),
            "{}",
            payload.len()
        );
        assert_eq!(parse_entities(&payload).len(), ENTITIES_PER_EVENT);
        let hashed = EntityState {
            model: ModelRef::Hash(-1216765807),
            kind: EntityKind::Ped,
            ..entity.clone()
        };
        let payload = serde_json::to_string(&(vec![hashed.clone()],)).unwrap();
        assert_eq!(parse_entities(&payload), [hashed]);
        assert!(crate::valid_event(
            ENTITY_DELETE_EVENT,
            "[[1048576,1048577]]"
        ));
        assert_eq!(
            parse_deletes("[[1048576,1048577]]"),
            [FIRST_HANDLE, FIRST_HANDLE + 1]
        );
    }
    #[test]
    fn invalid_entities_are_skipped() {
        for bad in [
            r#"[[{"h":5,"k":"ped","m":"x","p":[0,0,0],"r":0}]]"#,
            r#"[[{"h":1048576,"k":"tank","m":"x","p":[0,0,0],"r":0}]]"#,
            r#"[[{"h":1048576,"k":"ped","m":"../x","p":[0,0,0],"r":0}]]"#,
            r#"[[{"h":1048576,"k":"ped","m":"x","p":[1e9,0,0],"r":0}]]"#,
            "not json",
        ] {
            assert!(parse_entities(bad).is_empty(), "{bad}");
        }
    }
}
