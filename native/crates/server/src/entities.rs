//! Server-owned vehicles, peds and objects (FiveM's server-side
//! `CreateVehicle`, `CreatePed`, `CreateObject`). The server keeps the
//! authoritative list; every player gets the entities in their own routing
//! bucket through `sa_net::entities` events, and loses them again when they
//! are deleted or the player or entity changes bucket. Entities belong to the
//! resource that created them and are deleted when it stops.
use sa_net::entities::{
    EntityKind, EntityState, ModelRef, ENTITIES_PER_EVENT, FIRST_HANDLE, MAX_ENTITIES,
};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug)]
pub struct Entity {
    pub state: EntityState,
    pub bucket: u32,
    pub resource: String,
    /// Bumped on every change, so players get the new state once.
    pub revision: u64,
    /// The player driving it. A driven vehicle is the driver's own synced
    /// car meanwhile, so players do not get it as an entity.
    pub driver: Option<u32>,
}

#[derive(Default)]
pub struct Registry {
    entities: BTreeMap<u32, Entity>,
    next: u32,
    revision: u64,
    /// What each player was last sent: handle -> revision.
    known: BTreeMap<u32, BTreeMap<u32, u64>>,
}

/// Events for one player: entity batches to send, then handles to delete.
#[derive(Debug, Default, PartialEq)]
pub struct Update {
    pub upserts: Vec<Vec<EntityState>>,
    pub deletes: Vec<u32>,
}
impl Update {
    pub fn is_empty(&self) -> bool {
        self.upserts.is_empty() && self.deletes.is_empty()
    }
}

impl Registry {
    pub fn create(
        &mut self,
        resource: &str,
        kind: EntityKind,
        model: ModelRef,
        position: [f32; 3],
        heading: f32,
    ) -> Result<u32, &'static str> {
        if !model.valid() {
            return Err("invalid model name");
        }
        if !position
            .iter()
            .all(|v| v.is_finite() && v.abs() < 100_000.0)
            || !heading.is_finite()
        {
            return Err("invalid position or heading");
        }
        if self.entities.len() >= MAX_ENTITIES {
            return Err("too many server entities");
        }
        // Handles are never reused while the server runs, so a stale
        // handle cannot reach a newer entity.
        let handle = FIRST_HANDLE
            .checked_add(self.next)
            .ok_or("entity handles exhausted")?;
        self.next += 1;
        self.revision += 1;
        self.entities.insert(
            handle,
            Entity {
                state: EntityState {
                    handle,
                    kind,
                    model,
                    position,
                    heading: heading.rem_euclid(360.0),
                    color: None,
                },
                bucket: 0,
                resource: resource.to_string(),
                revision: self.revision,
                driver: None,
            },
        );
        Ok(handle)
    }
    pub fn get(&self, handle: u32) -> Option<&Entity> {
        self.entities.get(&handle)
    }
    fn change(&mut self, handle: u32, edit: impl FnOnce(&mut Entity)) -> bool {
        let Some(entity) = self.entities.get_mut(&handle) else {
            return false;
        };
        edit(entity);
        self.revision += 1;
        entity.revision = self.revision;
        true
    }
    pub fn set_position(&mut self, handle: u32, position: [f32; 3]) -> bool {
        position
            .iter()
            .all(|v| v.is_finite() && v.abs() < 100_000.0)
            && self.change(handle, |e| e.state.position = position)
    }
    pub fn set_heading(&mut self, handle: u32, heading: f32) -> bool {
        heading.is_finite() && self.change(handle, |e| e.state.heading = heading.rem_euclid(360.0))
    }
    pub fn set_bucket(&mut self, handle: u32, bucket: u32) -> bool {
        self.change(handle, |e| e.bucket = bucket)
    }
    pub fn set_color(&mut self, handle: u32, color: Option<[u8; 3]>) -> bool {
        self.change(handle, |e| e.state.color = color)
    }
    pub fn set_driver(&mut self, handle: u32, driver: Option<u32>) -> bool {
        self.change(handle, |e| e.driver = driver)
    }
    /// Move a driven vehicle along with its driver. Players do not get
    /// driven vehicles, so this does not count as a change for them.
    pub fn follow(&mut self, handle: u32, position: [f32; 3], heading: f32) {
        if let Some(e) = self.entities.get_mut(&handle) {
            if position
                .iter()
                .all(|v| v.is_finite() && v.abs() < 100_000.0)
                && heading.is_finite()
            {
                e.state.position = position;
                e.state.heading = heading.rem_euclid(360.0);
            }
        }
    }
    pub fn delete(&mut self, handle: u32) -> bool {
        self.entities.remove(&handle).is_some()
    }
    /// Delete every entity `resource` created; returns their handles.
    pub fn delete_owned_by(&mut self, resource: &str) -> Vec<u32> {
        let handles: Vec<u32> = self
            .entities
            .iter()
            .filter(|(_, e)| e.resource == resource)
            .map(|(h, _)| *h)
            .collect();
        for handle in &handles {
            self.entities.remove(handle);
        }
        handles
    }
    /// Handles of one kind, in creation order (`GetAllVehicles` ...).
    pub fn all(&self, kind: EntityKind) -> Vec<u32> {
        self.entities
            .values()
            .filter(|e| e.state.kind == kind)
            .map(|e| e.state.handle)
            .collect()
    }
    /// What `player`, now in `bucket`, still needs. Call `confirm` once the
    /// events are queued; until then the same update is produced again.
    pub fn pending(&self, player: u32, bucket: u32) -> Update {
        let empty = BTreeMap::new();
        let known = self.known.get(&player).unwrap_or(&empty);
        let changed: Vec<EntityState> = self
            .entities
            .values()
            .filter(|e| {
                e.bucket == bucket
                    && e.driver.is_none()
                    && known.get(&e.state.handle) != Some(&e.revision)
            })
            .map(|e| e.state.clone())
            .collect();
        let deletes = known
            .keys()
            .filter(|h| {
                self.entities
                    .get(h)
                    .is_none_or(|e| e.bucket != bucket || e.driver.is_some())
            })
            .copied()
            .collect();
        Update {
            upserts: changed
                .chunks(ENTITIES_PER_EVENT)
                .map(<[EntityState]>::to_vec)
                .collect(),
            deletes,
        }
    }
    /// Record what was sent: the upsert batches in `sent_batches` (a prefix
    /// of the update) and, if `deletes_sent`, its deletions.
    pub fn confirm(
        &mut self,
        player: u32,
        update: &Update,
        sent_batches: usize,
        deletes_sent: bool,
    ) {
        let known = self.known.entry(player).or_default();
        for state in update.upserts.iter().take(sent_batches).flatten() {
            if let Some(entity) = self.entities.get(&state.handle) {
                known.insert(state.handle, entity.revision);
            }
        }
        if deletes_sent {
            for handle in &update.deletes {
                known.remove(handle);
            }
        }
    }
    /// Forget players who left, so a reused player ID starts from scratch.
    pub fn retain_players(&mut self, players: &BTreeSet<u32>) {
        self.known.retain(|id, _| players.contains(id));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sync(r: &mut Registry, player: u32, bucket: u32) -> Update {
        let update = r.pending(player, bucket);
        r.confirm(player, &update, update.upserts.len(), true);
        update
    }
    fn handles(update: &Update) -> Vec<u32> {
        update.upserts.iter().flatten().map(|s| s.handle).collect()
    }
    #[test]
    fn driven_vehicles_leave_every_player_and_come_back_where_they_were_left() {
        let mut r = Registry::default();
        let car = r
            .create(
                "a",
                EntityKind::Vehicle,
                ModelRef::Name("sedan".into()),
                [1.0, 2.0, 3.0],
                0.0,
            )
            .unwrap();
        assert_eq!(handles(&sync(&mut r, 1, 0)), [car]);
        assert!(r.set_driver(car, Some(1)));
        assert_eq!(sync(&mut r, 1, 0).deletes, [car]);
        assert!(sync(&mut r, 2, 0).is_empty());
        r.follow(car, [40.0, 50.0, 3.0], 450.0);
        assert!(sync(&mut r, 1, 0).is_empty());
        assert!(r.set_driver(car, None));
        let back = sync(&mut r, 2, 0);
        assert_eq!(back.upserts[0][0].position, [40.0, 50.0, 3.0]);
        assert_eq!(back.upserts[0][0].heading, 90.0);
        assert!(r.set_color(car, Some([1, 2, 3])));
        assert_eq!(sync(&mut r, 2, 0).upserts[0][0].color, Some([1, 2, 3]));
    }
    #[test]
    fn players_get_entities_of_their_bucket_once_and_lose_them_on_change() {
        let mut r = Registry::default();
        let car = r
            .create(
                "a",
                EntityKind::Vehicle,
                ModelRef::Name("infernus".into()),
                [1.0, 2.0, 3.0],
                -90.0,
            )
            .unwrap();
        let ped = r
            .create("b", EntityKind::Ped, ModelRef::Hash(7), [0.0; 3], 0.0)
            .unwrap();
        assert!(car >= FIRST_HANDLE);
        assert_eq!(r.get(car).unwrap().state.heading, 270.0);
        assert_eq!(handles(&sync(&mut r, 1, 0)), [car, ped]);
        assert!(sync(&mut r, 1, 0).is_empty());
        // A change is sent again; the other player in bucket 5 sees nothing.
        assert!(r.set_position(car, [5.0, 5.0, 5.0]));
        assert_eq!(handles(&sync(&mut r, 1, 0)), [car]);
        assert!(sync(&mut r, 2, 5).is_empty());
        // Moving the car to bucket 5 swaps who has it.
        r.set_bucket(car, 5);
        assert_eq!(sync(&mut r, 1, 0).deletes, [car]);
        assert_eq!(handles(&sync(&mut r, 2, 5)), [car]);
        // The player follows into bucket 5: the ped leaves, the car arrives.
        let update = sync(&mut r, 1, 5);
        assert_eq!((handles(&update), update.deletes), (vec![car], vec![ped]));
        // Stopping resource "a" deletes its car for everyone who had it.
        assert_eq!(r.delete_owned_by("a"), [car]);
        assert_eq!(sync(&mut r, 1, 5).deletes, [car]);
        assert_eq!(sync(&mut r, 2, 5).deletes, [car]);
        assert_eq!(r.all(EntityKind::Ped), [ped]);
        // Unsent batches are produced again; departed players are forgotten.
        let fresh = r.pending(3, 0);
        r.confirm(3, &fresh, 0, true);
        assert_eq!(r.pending(3, 0), fresh);
        r.retain_players(&BTreeSet::from([1]));
        assert!(!r.known.contains_key(&2));
    }
    #[test]
    fn limits_and_invalid_input_are_refused_and_batches_are_bounded() {
        let mut r = Registry::default();
        let bad = ModelRef::Name("../x".into());
        assert!(r
            .create("a", EntityKind::Object, bad, [0.0; 3], 0.0)
            .is_err());
        let model = ModelRef::Name("prop".into());
        assert!(r
            .create(
                "a",
                EntityKind::Object,
                model.clone(),
                [f32::NAN, 0.0, 0.0],
                0.0
            )
            .is_err());
        for _ in 0..MAX_ENTITIES {
            r.create("a", EntityKind::Object, model.clone(), [0.0; 3], 0.0)
                .unwrap();
        }
        assert!(r
            .create("a", EntityKind::Object, model, [0.0; 3], 0.0)
            .is_err());
        let update = r.pending(1, 0);
        assert_eq!(
            update.upserts.len(),
            MAX_ENTITIES.div_ceil(ENTITIES_PER_EVENT)
        );
        assert!(update.upserts.iter().all(|b| b.len() <= ENTITIES_PER_EVENT));
        assert!(!r.set_position(FIRST_HANDLE, [f32::INFINITY, 0.0, 0.0]));
        assert!(!r.delete(1));
    }
}
