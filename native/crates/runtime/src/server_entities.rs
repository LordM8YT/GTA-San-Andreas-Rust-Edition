//! Server-owned vehicles and peds from a SARE server (`CreateVehicle`,
//! `CreatePed`). The server only sends the entities in this player's routing
//! bucket; they are drawn through the remote-actor path, like other players'
//! parked cars and avatars. Objects are kept but not drawn yet.
use crate::ORIGIN;
use sa_net::entities::{
    parse_deletes, parse_entities, EntityKind, EntityState, ModelRef, ENTITIES_EVENT,
    ENTITY_DELETE_EVENT, MAX_ENTITIES,
};
use sa_net::{Peer, Pose, VehiclePose};
use std::collections::{BTreeMap, HashMap};

#[derive(Default)]
pub(crate) struct ServerEntities {
    pub entities: BTreeMap<u32, EntityState>,
    /// Bumped on every change, so remote actors are rebuilt once.
    pub revision: u64,
    /// Remote-actor poses for the entities, and the revision they show.
    pub peers: Vec<Peer>,
    pub peers_revision: Option<u64>,
}
impl ServerEntities {
    /// Apply a replication event; false for other events.
    pub fn handle_event(&mut self, name: &str, payload: &str) -> bool {
        match name {
            ENTITIES_EVENT => {
                for entity in parse_entities(payload) {
                    if self.entities.len() < MAX_ENTITIES
                        || self.entities.contains_key(&entity.handle)
                    {
                        self.entities.insert(entity.handle, entity);
                    }
                }
            }
            ENTITY_DELETE_EVENT => {
                for handle in parse_deletes(payload) {
                    self.entities.remove(&handle);
                }
            }
            _ => return false,
        }
        self.revision += 1;
        true
    }
}

/// The catalog index for a server model: an original model file name
/// (`infernus`), a catalog display name, an original model ID from the
/// player's data files (`411`), or a FiveM-style hash of a model name.
pub(crate) fn resolve_model(
    model: &ModelRef,
    names: &[String],
    models: &HashMap<String, String>,
    ids: &HashMap<i32, String>,
) -> Option<usize> {
    let file = |i: usize| models.get(&names[i]).map(String::as_str);
    match model {
        ModelRef::Name(wanted) => (0..names.len()).find(|&i| {
            file(i).is_some_and(|f| f.eq_ignore_ascii_case(wanted))
                || names[i].eq_ignore_ascii_case(wanted)
        }),
        ModelRef::Hash(number) => {
            let by_id = i32::try_from(*number)
                .ok()
                .and_then(|id| ids.get(&id))
                .and_then(|m| (0..names.len()).find(|&i| file(i) == Some(m.as_str())));
            by_id.or_else(|| {
                let hash = *number as i32;
                (0..names.len()).find(|&i| {
                    file(i).is_some_and(|f| sa_lua::cfxlua::joaat(f) == hash)
                        || sa_lua::cfxlua::joaat(&names[i]) == hash
                })
            })
        }
    }
}

/// A remote-actor pose for an entity with resolved catalog indices. World
/// coordinates become runtime coordinates as `__sare:setCoords` does.
pub(crate) fn entity_peer(entity: &EntityState, ped: usize, car: usize) -> Option<Peer> {
    let [x, y, z] = entity.position;
    let position = [x - ORIGIN[0], z, ORIGIN[1] - y];
    let yaw = entity.heading.to_radians();
    let mut pose = Pose {
        position,
        yaw,
        ped_model: u16::try_from(ped).ok()?,
        car_model: u16::try_from(car).ok()?,
        paint: entity.color,
        ..Pose::default()
    };
    match entity.kind {
        EntityKind::Vehicle => {
            pose.vehicle = Some(VehiclePose {
                position,
                yaw,
                ..VehiclePose::default()
            })
        }
        EntityKind::Ped => {}
        EntityKind::Object => return None,
    }
    Some(Peer {
        id: entity.handle,
        name: String::new(),
        pose,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use sa_net::entities::FIRST_HANDLE;
    fn catalog() -> (Vec<String>, HashMap<String, String>, HashMap<i32, String>) {
        let names = vec!["My mod car".to_string(), "Infernus".into(), "Taxi".into()];
        let models = HashMap::from([
            ("Infernus".to_string(), "infernus".to_string()),
            ("Taxi".to_string(), "taxi".to_string()),
        ]);
        let ids = HashMap::from([(411, "infernus".to_string()), (420, "taxi".to_string())]);
        (names, models, ids)
    }
    #[test]
    fn models_resolve_by_file_name_display_name_id_and_hash() {
        let (names, models, ids) = catalog();
        let resolve = |m: ModelRef| resolve_model(&m, &names, &models, &ids);
        assert_eq!(resolve(ModelRef::Name("INFERNUS".into())), Some(1));
        assert_eq!(resolve(ModelRef::Name("my mod car".into())), Some(0));
        assert_eq!(resolve(ModelRef::Hash(420)), Some(2));
        let hash = sa_lua::cfxlua::joaat("infernus");
        assert_eq!(resolve(ModelRef::Hash(i64::from(hash))), Some(1));
        // Unsigned hashes from scripts that use them wrap the same way.
        assert_eq!(resolve(ModelRef::Hash(i64::from(hash as u32))), Some(1));
        assert_eq!(resolve(ModelRef::Name("adder".into())), None);
    }
    #[test]
    fn events_add_update_and_remove_entities_and_poses_use_world_coordinates() {
        let mut list = ServerEntities::default();
        let car = format!(
            r#"[[{{"h":{FIRST_HANDLE},"k":"vehicle","m":"taxi","p":[2510.0,-1680.0,13.5],"r":90}}]]"#
        );
        assert!(list.handle_event(ENTITIES_EVENT, &car));
        assert!(!list.handle_event("chat:addMessage", "[]"));
        let entity = &list.entities[&FIRST_HANDLE];
        let peer = entity_peer(entity, 0, 2).unwrap();
        assert_eq!(peer.pose.position, [10.0, 13.5, 10.0]);
        assert_eq!(peer.pose.vehicle.unwrap().position, [10.0, 13.5, 10.0]);
        assert!((peer.pose.yaw - std::f32::consts::FRAC_PI_2).abs() < 1e-6);
        assert!(!peer.pose.driving && peer.pose.car_model == 2);
        let object = EntityState {
            kind: EntityKind::Object,
            ..entity.clone()
        };
        assert!(entity_peer(&object, 0, 0).is_none());
        let revision = list.revision;
        assert!(list.handle_event(ENTITY_DELETE_EVENT, &format!("[[{FIRST_HANDLE}]]")));
        assert!(list.entities.is_empty() && list.revision > revision);
    }
}
