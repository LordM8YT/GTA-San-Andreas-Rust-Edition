//! FiveM-inspired MLO interior definitions and deterministic portal visibility.
//!
//! The data model mirrors the useful runtime concepts from Cfx/FiveM while
//! remaining independent of GTA V binary YTYP/YMAP serialization. Legacy SA
//! DFF/TXD/COL resources can describe the same rooms and portals in resource.json.
use anyhow::{ensure, Result};
use serde::Deserialize;
use std::collections::{HashSet, VecDeque};

const MAX_ROOMS: usize = 128;
const MAX_PORTALS: usize = 512;
const MAX_ENTITIES: usize = 2048;
const MAX_ENTITY_SETS: usize = 64;
const MAX_TIME_CYCLE_MODIFIERS: usize = 64;

#[derive(Clone, Debug, Deserialize)]
pub struct MloDefinition {
    pub name: String,
    #[serde(default)]
    pub entities: Vec<MloEntity>,
    #[serde(default)]
    pub rooms: Vec<MloRoom>,
    #[serde(default)]
    pub portals: Vec<MloPortal>,
    #[serde(default)]
    pub entity_sets: Vec<MloEntitySet>,
    #[serde(default)]
    pub timecycle_modifiers: Vec<MloTimecycleModifier>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct MloEntity {
    pub model_id: i32,
    #[serde(default)]
    pub position: [f32; 3],
    #[serde(default = "identity")]
    pub rotation: [f32; 4],
    /// None means the entity belongs to the exterior shell.
    #[serde(default)]
    pub room: Option<usize>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct MloRoom {
    pub name: String,
    pub min: [f32; 3],
    pub max: [f32; 3],
    #[serde(default)]
    pub flags: u32,
    #[serde(default)]
    pub blend: f32,
    #[serde(default)]
    pub timecycle: Option<String>,
    #[serde(default)]
    pub exterior_visibility_depth: u8,
    #[serde(default)]
    pub entities: Vec<usize>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct MloPortal {
    pub room_from: usize,
    pub room_to: usize,
    /// Four or more local-space polygon corners, in winding order.
    pub corners: Vec<[f32; 3]>,
    #[serde(default)]
    pub flags: u32,
    #[serde(default = "default_opacity")]
    pub opacity: f32,
    #[serde(default)]
    pub audio_occlusion: f32,
    #[serde(default = "default_true")]
    pub open: bool,
    #[serde(default)]
    pub entities: Vec<usize>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct MloEntitySet {
    pub name: String,
    #[serde(default)]
    pub entities: Vec<usize>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct MloTimecycleModifier {
    pub name: String,
    pub sphere: [f32; 4],
    #[serde(default)]
    pub percentage: f32,
    #[serde(default)]
    pub range: f32,
    #[serde(default)]
    pub start_hour: u8,
    #[serde(default = "default_end_hour")]
    pub end_hour: u8,
}

fn identity() -> [f32; 4] {
    [0.0, 0.0, 0.0, 1.0]
}
fn default_opacity() -> f32 {
    1.0
}
fn default_true() -> bool {
    true
}
fn default_end_hour() -> u8 {
    24
}

impl MloDefinition {
    /// Reject malformed or excessively large metadata before it reaches the renderer.
    pub fn validate(&self, model_ids: &HashSet<i32>) -> Result<()> {
        ensure!(
            !self.name.trim().is_empty() && self.name.len() <= 128,
            "MLO name must contain 1-128 characters"
        );
        ensure!(
            self.rooms.len() <= MAX_ROOMS,
            "MLO room limit ({MAX_ROOMS})"
        );
        ensure!(
            self.portals.len() <= MAX_PORTALS,
            "MLO portal limit ({MAX_PORTALS})"
        );
        ensure!(
            self.entities.len() <= MAX_ENTITIES,
            "MLO entity limit ({MAX_ENTITIES})"
        );
        ensure!(
            self.entity_sets.len() <= MAX_ENTITY_SETS,
            "MLO entity-set limit ({MAX_ENTITY_SETS})"
        );
        ensure!(
            self.timecycle_modifiers.len() <= MAX_TIME_CYCLE_MODIFIERS,
            "MLO timecycle modifier limit ({MAX_TIME_CYCLE_MODIFIERS})"
        );

        for (index, entity) in self.entities.iter().enumerate() {
            ensure!(
                model_ids.contains(&entity.model_id),
                "MLO entity {index} references unknown model {}",
                entity.model_id
            );
            ensure!(
                entity
                    .position
                    .iter()
                    .chain(entity.rotation.iter())
                    .all(|v| v.is_finite()),
                "MLO entity {index} has a non-finite transform"
            );
            let norm = entity.rotation.iter().map(|v| v * v).sum::<f32>();
            ensure!(
                (0.98..=1.02).contains(&norm),
                "MLO entity {index} has an invalid rotation"
            );
            ensure!(
                entity.room.is_none_or(|room| room < self.rooms.len()),
                "MLO entity {index} references an unknown room"
            );
        }

        for (index, room) in self.rooms.iter().enumerate() {
            ensure!(
                !room.name.trim().is_empty() && room.name.len() <= 128,
                "MLO room {index} needs a 1-128 character name"
            );
            ensure!(
                room.min
                    .iter()
                    .chain(room.max.iter())
                    .all(|v| v.is_finite()),
                "MLO room {index} has non-finite bounds"
            );
            ensure!(
                (0..3).all(|axis| room.min[axis] < room.max[axis]),
                "MLO room {index} has inverted or empty bounds"
            );
            ensure!(
                room.blend.is_finite() && (0.0..=100.0).contains(&room.blend),
                "MLO room {index} blend must be between 0 and 100"
            );
            ensure!(
                room.entities
                    .iter()
                    .all(|&entity| entity < self.entities.len()),
                "MLO room {index} references an unknown entity"
            );
        }

        for (index, portal) in self.portals.iter().enumerate() {
            ensure!(
                portal.room_from < self.rooms.len()
                    && portal.room_to < self.rooms.len()
                    && portal.room_from != portal.room_to,
                "MLO portal {index} has invalid room endpoints"
            );
            ensure!(
                portal.corners.len() >= 3 && portal.corners.len() <= 32,
                "MLO portal {index} must have 3-32 polygon corners"
            );
            ensure!(
                portal.corners.iter().flatten().all(|v| v.is_finite()),
                "MLO portal {index} has non-finite corners"
            );
            ensure!(
                portal.opacity.is_finite() && (0.0..=1.0).contains(&portal.opacity),
                "MLO portal {index} opacity must be between 0 and 1"
            );
            ensure!(
                portal.audio_occlusion.is_finite()
                    && (0.0..=1.0).contains(&portal.audio_occlusion),
                "MLO portal {index} audio occlusion must be between 0 and 1"
            );
            ensure!(
                portal
                    .entities
                    .iter()
                    .all(|&entity| entity < self.entities.len()),
                "MLO portal {index} references an unknown entity"
            );
        }

        let mut set_names = HashSet::new();
        for set in &self.entity_sets {
            ensure!(
                !set.name.trim().is_empty()
                    && set.name.len() <= 128
                    && set_names.insert(set.name.to_ascii_lowercase()),
                "MLO entity-set names must be unique and 1-128 characters"
            );
            ensure!(
                set.entities
                    .iter()
                    .all(|&entity| entity < self.entities.len()),
                "MLO entity set {} references an unknown entity",
                set.name
            );
        }

        for (index, modifier) in self.timecycle_modifiers.iter().enumerate() {
            ensure!(
                !modifier.name.trim().is_empty() && modifier.name.len() <= 128,
                "MLO timecycle modifier {index} needs a name"
            );
            ensure!(
                modifier.sphere.iter().all(|v| v.is_finite()) && modifier.sphere[3] >= 0.0,
                "MLO timecycle modifier {index} has an invalid sphere"
            );
            ensure!(
                modifier.percentage.is_finite()
                    && (0.0..=100.0).contains(&modifier.percentage)
                    && modifier.range.is_finite()
                    && modifier.range >= 0.0
                    && modifier.start_hour <= 23
                    && modifier.end_hour <= 24,
                "MLO timecycle modifier {index} has invalid parameters"
            );
        }
        Ok(())
    }

    /// Find the most specific room containing a local-space point.
    pub fn room_at(&self, point: [f32; 3]) -> Option<usize> {
        self.rooms.iter().position(|room| {
            (0..3).all(|axis| point[axis] >= room.min[axis] && point[axis] <= room.max[axis])
        })
    }

    /// Rooms visible through the currently open portal graph. Closed portals
    /// stop traversal; exterior visibility is handled by the caller.
    pub fn visible_rooms(&self, start: usize) -> Vec<usize> {
        if start >= self.rooms.len() {
            return Vec::new();
        }
        let mut seen = vec![false; self.rooms.len()];
        let mut queue = VecDeque::from([start]);
        seen[start] = true;
        while let Some(room) = queue.pop_front() {
            for portal in self.portals.iter().filter(|portal| portal.open) {
                let next = if portal.room_from == room {
                    Some(portal.room_to)
                } else if portal.room_to == room {
                    Some(portal.room_from)
                } else {
                    None
                };
                if let Some(next) = next {
                    if !seen[next] {
                        seen[next] = true;
                        queue.push_back(next);
                    }
                }
            }
        }
        seen.into_iter()
            .enumerate()
            .filter_map(|(index, visible)| visible.then_some(index))
            .collect()
    }

    pub fn visible_entity_indices(&self, visible_rooms: &[usize]) -> Vec<usize> {
        let visible: HashSet<_> = visible_rooms.iter().copied().collect();
        self.entities
            .iter()
            .enumerate()
            .filter_map(|(index, entity)| {
                entity
                    .room
                    .is_none_or(|room| visible.contains(&room))
                    .then_some(index)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> MloDefinition {
        MloDefinition {
            name: "police_station".into(),
            entities: vec![
                MloEntity {
                    model_id: 18697,
                    position: [0.0; 3],
                    rotation: identity(),
                    room: Some(0),
                },
                MloEntity {
                    model_id: 18698,
                    position: [1.0, 0.0, 0.0],
                    rotation: identity(),
                    room: Some(1),
                },
                MloEntity {
                    model_id: 18699,
                    position: [0.0; 3],
                    rotation: identity(),
                    room: None,
                },
            ],
            rooms: vec![
                MloRoom {
                    name: "lobby".into(),
                    min: [-5.0; 3],
                    max: [5.0; 3],
                    flags: 0,
                    blend: 0.0,
                    timecycle: None,
                    exterior_visibility_depth: 0,
                    entities: vec![0],
                },
                MloRoom {
                    name: "office".into(),
                    min: [5.0, -5.0, -5.0],
                    max: [15.0, 5.0, 5.0],
                    flags: 0,
                    blend: 0.0,
                    timecycle: None,
                    exterior_visibility_depth: 0,
                    entities: vec![1],
                },
                MloRoom {
                    name: "cells".into(),
                    min: [15.0, -5.0, -5.0],
                    max: [25.0, 5.0, 5.0],
                    flags: 0,
                    blend: 0.0,
                    timecycle: None,
                    exterior_visibility_depth: 0,
                    entities: vec![],
                },
            ],
            portals: vec![
                MloPortal {
                    room_from: 0,
                    room_to: 1,
                    corners: vec![
                        [5.0, -1.0, -1.0],
                        [5.0, 1.0, -1.0],
                        [5.0, 1.0, 1.0],
                        [5.0, -1.0, 1.0],
                    ],
                    flags: 0,
                    opacity: 1.0,
                    audio_occlusion: 0.0,
                    open: true,
                    entities: vec![],
                },
                MloPortal {
                    room_from: 1,
                    room_to: 2,
                    corners: vec![
                        [15.0, -1.0, -1.0],
                        [15.0, 1.0, -1.0],
                        [15.0, 1.0, 1.0],
                        [15.0, -1.0, 1.0],
                    ],
                    flags: 0,
                    opacity: 1.0,
                    audio_occlusion: 0.0,
                    open: true,
                    entities: vec![],
                },
            ],
            entity_sets: vec![MloEntitySet {
                name: "night_shift".into(),
                entities: vec![1],
            }],
            timecycle_modifiers: vec![],
        }
    }

    #[test]
    fn validates_mlo_and_resolves_rooms_and_portal_visibility() {
        let mlo = sample();
        let ids = HashSet::from([18697, 18698, 18699]);
        mlo.validate(&ids).unwrap();
        assert_eq!(mlo.room_at([0.0; 3]), Some(0));
        assert_eq!(mlo.room_at([10.0, 0.0, 0.0]), Some(1));
        assert_eq!(mlo.room_at([40.0; 3]), None);
        assert_eq!(mlo.visible_rooms(0), vec![0, 1, 2]);
        assert_eq!(mlo.visible_entity_indices(&[0]), vec![0, 2]);
    }

    #[test]
    fn closed_portal_blocks_visibility_and_invalid_references_are_rejected() {
        let mut mlo = sample();
        mlo.portals[1].open = false;
        assert_eq!(mlo.visible_rooms(0), vec![0, 1]);
        assert_eq!(mlo.visible_rooms(99), Vec::<usize>::new());
        let ids = HashSet::from([18697, 18698, 18699]);
        mlo.entities[0].room = Some(99);
        assert!(mlo.validate(&ids).is_err());
    }

    #[test]
    fn rejects_unknown_models_bad_bounds_and_invalid_portal_endpoints() {
        let ids = HashSet::from([18697, 18698, 18699]);
        let mut mlo = sample();
        mlo.entities[0].model_id = 99999;
        assert!(mlo.validate(&ids).is_err());
        let mut mlo = sample();
        mlo.rooms[0].max[0] = mlo.rooms[0].min[0];
        assert!(mlo.validate(&ids).is_err());
        let mut mlo = sample();
        mlo.portals[0].room_to = 77;
        assert!(mlo.validate(&ids).is_err());
    }
}
