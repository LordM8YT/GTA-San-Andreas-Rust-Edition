//! Content-pack characters and vehicles in the game: the player's own
//! customisable character (walking and seated at the wheel), other players'
//! characters with the look the server sent, pack vehicle paint, and
//! getting in and out of server vehicles. See sa_scene::character,
//! sa_scene::pack_vehicle and the server's vehicles.rs.
use super::{GpuBatch, State, ORIGIN};
use glam::{Quat, Vec3};
use sa_net::appearance::Appearance;
use sa_scene::character::{Animator, CharacterModel, Motion};
use sa_scene::content_pack::VehicleSpec;
use sa_scene::Scene;
use serde_json::Value as Json;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Must match the server (crates/server/src/vehicles.rs).
pub const ENTER: &str = "__sare:enterVehicle";
pub const EXIT: &str = "__sare:exitVehicle";
pub const GRANTED: &str = "__sare:vehicleGranted";
pub const REFUSED: &str = "__sare:vehicleRefused";
pub const REVOKED: &str = "__sare:vehicleRevoked";
/// How close a server vehicle must be for F to ask for it.
const REACH: f32 = 4.5;

/// What a content pack adds besides its world.
#[derive(Default)]
pub struct PackAssets {
    pub characters: Option<CharacterModel>,
    pub vehicles: Vec<(VehicleSpec, Scene)>,
}
impl PackAssets {
    pub fn load(folder: &Path) -> Self {
        let Ok(manifest) = sa_scene::content_pack::Manifest::read(folder) else {
            return Self::default();
        };
        let characters = match sa_scene::content_pack::load_characters(folder, &manifest) {
            Ok(model) => model,
            Err(error) => {
                eprintln!("Content pack characters unavailable: {error:#}");
                None
            }
        };
        Self {
            characters,
            vehicles: sa_scene::content_pack::load_vehicles(folder, &manifest),
        }
    }
}

/// Runtime state for pack characters and server vehicles.
#[derive(Default)]
pub struct PackActors {
    pub characters: Option<Arc<CharacterModel>>,
    /// The local player's character mesh (one animated batch).
    pub batches: Vec<GpuBatch>,
    pub animator: Animator,
    scratch: Vec<f32>,
    /// Looks the server sent, by player id.
    pub looks: HashMap<u32, Appearance>,
    /// Driver seat per car menu name (model space).
    pub seats: HashMap<String, Vec3>,
    /// The server vehicle the player drives, by entity handle.
    pub server_vehicle: Option<u32>,
    /// Paint of the player's car when it is not the model's own.
    pub paint: Option<[u8; 3]>,
    pub rendered_paint: Option<[u8; 3]>,
    /// The personal car is put away (after leaving a server vehicle).
    pub car_stowed: bool,
    pub asked: Option<Instant>,
}

/// Write a posed character into `out`, turned by `rotation` and moved to
/// `offset`.
pub fn pose_character(
    model: &CharacterModel,
    animator: &Animator,
    look: &Appearance,
    rotation: Quat,
    offset: Vec3,
    out: &mut Vec<f32>,
) {
    model.pose_into(&animator.plays(model), look, out);
    for v in out.as_chunks_mut::<9>().0.iter_mut() {
        let p = rotation * Vec3::new(v[0], v[1], v[2]) + offset;
        v[..3].copy_from_slice(&p.to_array());
    }
}

/// Recolour a posed car batch if it is a pack vehicle's paint.
pub fn repaint(batch: &GpuBatch, raw: &mut [f32], paint: Option<[u8; 3]>) {
    if let (Some(paint), Some(default)) = (
        paint,
        sa_scene::pack_vehicle::paint_default(&batch.texture_key),
    ) {
        sa_scene::pack_vehicle::repaint(raw, default, paint);
    }
}

fn rgb(value: Option<&Json>) -> Option<[u8; 3]> {
    let parts = value?.as_array()?;
    if parts.len() != 3 {
        return None;
    }
    let mut out = [0u8; 3];
    for (slot, part) in out.iter_mut().zip(parts) {
        *slot = part.as_f64()?.clamp(0.0, 255.0) as u8;
    }
    Some(out)
}

impl State {
    pub(super) fn local_look(&self) -> Appearance {
        self.pack
            .looks
            .get(&self.script_local_id)
            .copied()
            .unwrap_or_default()
    }
    /// Driver seat of the active car, if it is a pack vehicle.
    pub(super) fn active_seat(&self) -> Option<Vec3> {
        self.menu
            .cars
            .get(self.active_car)
            .and_then(|name| self.pack.seats.get(name))
            .copied()
    }
    pub(super) fn seat_of(&self, car_index: usize) -> Option<Vec3> {
        self.menu
            .cars
            .get(car_index)
            .and_then(|name| self.pack.seats.get(name))
            .copied()
    }
    /// Pose the local character walking at `feet`.
    pub(super) fn pose_local_character(&mut self, feet: Vec3, speed: f32, airborne: bool, dt: f32) {
        let Some(model) = self.pack.characters.clone() else {
            return;
        };
        if self.pack.batches.is_empty() {
            return;
        }
        let look = self.local_look();
        self.pack.animator.update(
            Motion {
                speed,
                airborne,
                seated: false,
            },
            dt,
        );
        let mut raw = std::mem::take(&mut self.pack.scratch);
        pose_character(
            &model,
            &self.pack.animator,
            &look,
            Quat::from_rotation_y(self.ped_yaw),
            feet,
            &mut raw,
        );
        self.queue
            .write_buffer(&self.pack.batches[0].buffer, 0, bytemuck::cast_slice(&raw));
        self.pack.scratch = raw;
    }
    /// Pose the local character in the driver seat of the player's car.
    pub(super) fn pose_seated_character(&mut self, dt: f32) {
        let (Some(model), Some(seat)) = (self.pack.characters.clone(), self.active_seat()) else {
            return;
        };
        let Some((car, _)) = &self.car else {
            return;
        };
        if self.pack.batches.is_empty() {
            return;
        }
        let rotation = car.model_orientation();
        let offset = car.position + rotation * seat;
        let look = self.local_look();
        self.pack.animator.update(
            Motion {
                seated: true,
                ..Default::default()
            },
            dt,
        );
        let mut raw = std::mem::take(&mut self.pack.scratch);
        pose_character(
            &model,
            &self.pack.animator,
            &look,
            rotation * Quat::from_rotation_y(std::f32::consts::PI),
            offset,
            &mut raw,
        );
        self.queue
            .write_buffer(&self.pack.batches[0].buffer, 0, bytemuck::cast_slice(&raw));
        self.pack.scratch = raw;
    }
    /// Is the local character drawn this frame?
    pub(super) fn character_drawn(&self) -> bool {
        self.ped.is_none()
            && !self.pack.batches.is_empty()
            && self.third_person
            && self.ped_visible
            && ((self.walking && !self.driving) || (self.driving && self.active_seat().is_some()))
    }

    /// F near a server vehicle: ask the server for it. True if asked.
    pub(super) fn request_server_vehicle(&mut self) -> bool {
        if self.driving || self.network_session.is_none() {
            return false;
        }
        if self
            .pack
            .asked
            .is_some_and(|at| at.elapsed() < Duration::from_millis(800))
        {
            return true;
        }
        let feet = self.player.as_ref().map_or(self.position, |p| p.feet);
        let own = self
            .car
            .as_ref()
            .filter(|_| !self.pack.car_stowed)
            .map_or(f32::MAX, |(car, _)| car.position.distance(feet));
        let nearest = self
            .remote_actors
            .iter()
            .filter(|a| a.entity == Some(sa_net::entities::EntityKind::Vehicle))
            .filter_map(|a| {
                let car = a.current.vehicle?;
                Some((a.id, Vec3::from_array(car.position).distance(feet)))
            })
            .filter(|(_, d)| *d < REACH && *d < own)
            .min_by(|a, b| a.1.total_cmp(&b.1));
        let Some((handle, _)) = nearest else {
            return false;
        };
        if let Some(session) = &self.network_session {
            let _ = session.trigger(
                None,
                ENTER,
                &Json::from(vec![Json::from(handle)]).to_string(),
            );
        }
        self.pack.asked = Some(Instant::now());
        true
    }

    /// The server let the player drive one of its vehicles.
    pub(super) fn take_server_vehicle(&mut self, args: &[Json]) {
        use crate::server_entities::resolve_model;
        let number = |i: usize| args.get(i).and_then(Json::as_f64).map(|v| v as f32);
        let (Some(handle), Some(x), Some(y), Some(z), Some(heading)) = (
            args.first().and_then(Json::as_u64).map(|h| h as u32),
            number(2),
            number(3),
            number(4),
            number(5),
        ) else {
            return;
        };
        let model = match args.get(1) {
            Some(Json::String(name)) => sa_net::entities::ModelRef::Name(name.clone()),
            Some(value) => sa_net::entities::ModelRef::Hash(value.as_i64().unwrap_or(0)),
            None => return,
        };
        self.pack.asked = None;
        let Some(index) = resolve_model(
            &model,
            &self.menu.cars,
            &self.original_car_models,
            &self.car_ids,
        ) else {
            self.menu.message = format!("Vehicle model {model:?} is not available");
            self.release_server_vehicle();
            return;
        };
        self.ensure_car_loaded(index);
        if index != self.active_car {
            let Some(next) = self.car_catalog.get_mut(index).and_then(Option::take) else {
                self.release_server_vehicle();
                return;
            };
            self.car_catalog[self.active_car] = self.car.replace(next);
            self.active_car = index;
        }
        let Some((car, _)) = &mut self.car else {
            return;
        };
        car.stop();
        let ground = Vec3::new(x - ORIGIN[0], z, ORIGIN[1] - y);
        car.position = ground + Vec3::Y * car.clearance;
        car.yaw = heading.to_radians();
        car.pitch = 0.0;
        car.roll = 0.0;
        car.damage = Default::default();
        self.car_render_pose = None;
        self.leave_passenger();
        self.pack.server_vehicle = Some(handle);
        self.pack.paint = rgb(args.get(6));
        self.pack.car_stowed = false;
        self.driving = true;
        self.walking = true;
        self.network_car_spawned = true;
        self.keys.clear();
    }
    /// Tell the server the player left its vehicle and put the car away.
    pub(super) fn release_server_vehicle(&mut self) {
        let Some(handle) = self.pack.server_vehicle.take() else {
            return;
        };
        if let Some(session) = &self.network_session {
            let _ = session.trigger(
                None,
                EXIT,
                &Json::from(vec![Json::from(handle)]).to_string(),
            );
        }
        self.stow_car();
    }
    /// Hide the personal car: it is the server's vehicle again.
    pub(super) fn stow_car(&mut self) {
        self.pack.paint = None;
        self.pack.car_stowed = true;
        self.network_car_spawned = false;
        if let Some((car, _)) = &mut self.car {
            car.stop();
            car.position = Vec3::new(0.0, -5000.0, 0.0);
        }
        self.car_render_pose = None;
    }
    /// The server took its vehicle back (deleted, or `TaskLeaveVehicle`).
    pub(super) fn server_vehicle_revoked(&mut self, handle: u32) {
        if self.pack.server_vehicle != Some(handle) {
            return;
        }
        if self.driving {
            if let (Some(world), Some((car, _))) = (&self.collision, &mut self.car) {
                if let Some(player) = car.exit_player(world) {
                    self.position = player.eye();
                    self.ped_yaw = car.yaw;
                    self.player = Some(player);
                }
            }
            self.driving = false;
            self.walking = true;
        }
        self.pack.server_vehicle = None;
        self.stow_car();
    }
    pub(super) fn handle_pack_event(&mut self, name: &str, args: &[Json], payload: &str) -> bool {
        match name {
            sa_net::appearance::APPEARANCE_EVENT => {
                for (id, look) in sa_net::appearance::parse_event(payload) {
                    match look {
                        Some(look) => {
                            self.pack.looks.insert(id, look);
                        }
                        None => {
                            self.pack.looks.remove(&id);
                        }
                    }
                }
            }
            GRANTED => self.take_server_vehicle(args),
            REFUSED => {
                self.pack.asked = None;
                if let Some(reason) = args.get(1).and_then(Json::as_str) {
                    let reason: String = reason.chars().take(120).collect();
                    self.chat.add_message(
                        "Vehicle",
                        &reason,
                        egui::Color32::from_rgb(255, 200, 120),
                    );
                }
            }
            REVOKED => {
                if let Some(handle) = args.first().and_then(Json::as_u64) {
                    self.server_vehicle_revoked(handle as u32);
                }
            }
            _ => return false,
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn colours_parse_from_events() {
        assert_eq!(
            rgb(Some(&serde_json::json!([1, 300, 2.5]))),
            Some([1, 255, 2])
        );
        assert_eq!(rgb(Some(&serde_json::json!([1, 2]))), None);
        assert_eq!(rgb(Some(&Json::Null)), None);
    }
}
