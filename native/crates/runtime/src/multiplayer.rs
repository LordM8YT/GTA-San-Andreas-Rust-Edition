use super::{GpuBatch, State};
use glam::{Quat, Vec3};
use sa_net::{Peer, Pose, VehiclePose};
use std::time::{Duration, Instant};
use wgpu::util::DeviceExt;

pub(super) struct BrowserRequest {
    address: std::net::SocketAddr,
    receiver: std::sync::mpsc::Receiver<Result<Vec<sa_net::relay::Listing>, String>>,
}

#[cfg(test)]
mod parked_tests {
    use super::*;
    #[test]
    fn parked_mesh_updates_skip_small_jitter_but_keep_half_turns_and_motion() {
        let car = VehiclePose::default();
        assert!(car_changed(None, car));
        assert!(!car_changed(Some(car), car));
        assert!(!car_changed(
            Some(car),
            VehiclePose {
                position: [0.0001, 0.0, 0.0],
                ..car
            }
        ));
        assert!(car_changed(
            Some(car),
            VehiclePose {
                position: [0.01, 0.0, 0.0],
                ..car
            }
        ));
        assert!(car_changed(
            Some(car),
            VehiclePose {
                yaw: std::f32::consts::PI,
                ..car
            }
        ));
        assert!(!car_changed(
            Some(car),
            VehiclePose {
                yaw: std::f32::consts::TAU,
                ..car
            }
        ));
    }
    #[test]
    fn walking_and_teleporting_owner_do_not_move_their_parked_car() {
        let car = VehiclePose {
            position: [10.0, 3.0, 20.0],
            yaw: 1.0,
            ..VehiclePose::default()
        };
        let mut current = Pose {
            vehicle: Some(car),
            ..Pose::default()
        };
        let walking = Pose {
            position: [3.0, 0.0, 0.0],
            moving: true,
            vehicle: Some(car),
            ..Pose::default()
        };
        interpolate(&mut current, walking, 0.05);
        assert!(current.position[0] > 0.0 && current.position[0] < 3.0);
        assert_eq!(current.vehicle, Some(car));
        let distant = Pose {
            position: [350.0, 0.0, 0.0],
            ..walking
        };
        interpolate(&mut current, distant, 0.05);
        assert_eq!(current.position, distant.position);
        assert_eq!(current.vehicle, Some(car));
    }
    #[test]
    fn exiting_keeps_car_pose_and_large_vehicle_spawn_changes_are_immediate() {
        let car = VehiclePose {
            position: [0.0, 3.0, 0.0],
            ..VehiclePose::default()
        };
        let mut current = Pose {
            position: car.position,
            driving: true,
            vehicle: Some(car),
            ..Pose::default()
        };
        let walking = Pose {
            position: [2.0, 3.0, 0.0],
            vehicle: Some(car),
            ..Pose::default()
        };
        interpolate(&mut current, walking, 0.05);
        assert_eq!(current, walking);
        let moved = VehiclePose {
            position: [100.0, 3.0, 0.0],
            ..car
        };
        interpolate(
            &mut current,
            Pose {
                vehicle: Some(moved),
                ..walking
            },
            0.05,
        );
        assert_eq!(current.vehicle, Some(moved));
        interpolate(
            &mut current,
            Pose {
                vehicle: None,
                ..walking
            },
            0.05,
        );
        assert_eq!(current.vehicle, None);
    }
}

pub(super) struct RemoteActor {
    pub id: u32,
    pub current: Pose,
    pub target: Pose,
    pub visible: bool,
    pub car_visible: bool,
    pub ped_visible: bool,
    pub ped: Vec<GpuBatch>,
    pub car: Vec<GpuBatch>,
    clearance: f32,
    pub ped_model: usize,
    pub car_model: usize,
    model_changed: Instant,
    car_render_pose: Option<VehiclePose>,
}
fn duplicate(device: &wgpu::Device, batches: &[GpuBatch]) -> Vec<GpuBatch> {
    batches
        .iter()
        .map(|b| GpuBatch {
            bounds: None,
            texture_key: b.texture_key.clone(),
            buffer: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("remote player mesh"),
                contents: bytemuck::cast_slice(&b.base),
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            }),
            count: b.count,
            texture: b.texture.clone(),
            alpha: b.alpha,
            animated: b.animated,
            uv_animation: b.uv_animation.clone(),
            base: b.base.clone(),
        })
        .collect()
}
fn blend_angle(a: f32, b: f32, t: f32) -> f32 {
    let delta = (b - a).sin().atan2((b - a).cos());
    a + delta * t
}
pub(super) fn car_changed(previous: Option<VehiclePose>, current: VehiclePose) -> bool {
    previous.is_none_or(|previous| {
        previous.interior != current.interior
            || Vec3::from_array(previous.position)
                .distance_squared(Vec3::from_array(current.position))
                > 0.000_001
            || {
                let delta = previous.yaw - current.yaw;
                delta.sin().atan2(delta.cos()).abs() > 0.0001
            }
            || (previous.pitch - current.pitch).abs() > 0.0001
            || (previous.roll - current.roll).abs() > 0.0001
    })
}
fn interpolate(current: &mut Pose, target: Pose, dt: f32) {
    let a = Vec3::from_array(current.position);
    let b = Vec3::from_array(target.position);
    if current.interior != target.interior
        || current.driving != target.driving
        || current.ride != target.ride
        || a.distance(b) > 30.0
    {
        *current = target;
        return;
    }
    let t = 1.0 - (-16.0 * dt).exp();
    current.position = a.lerp(b, t).to_array();
    current.yaw = blend_angle(current.yaw, target.yaw, t);
    current.pitch += (target.pitch - current.pitch) * t;
    current.roll += (target.roll - current.roll) * t;
    current.speed = target.speed;
    current.moving = target.moving;
    current.ped_model = target.ped_model;
    current.car_model = target.car_model;
    current.clothes = target.clothes;
    current.ride = target.ride;
    current.ride_reply = target.ride_reply;
    match (&mut current.vehicle, target.vehicle) {
        (Some(car), Some(target))
            if car.interior == target.interior
                && Vec3::from_array(car.position).distance(Vec3::from_array(target.position))
                    <= 30.0 =>
        {
            car.position = Vec3::from_array(car.position)
                .lerp(Vec3::from_array(target.position), t)
                .to_array();
            car.yaw = blend_angle(car.yaw, target.yaw, t);
            car.pitch += (target.pitch - car.pitch) * t;
            car.roll += (target.roll - car.roll) * t;
            car.speed = target.speed;
        }
        (_, vehicle) => current.vehicle = vehicle,
    }
}
impl State {
    fn request_vehicle(&mut self, action: sa_net::VehicleAction) {
        self.vehicle_sequence = self.vehicle_sequence.wrapping_add(1).max(1);
        self.vehicle_request = Some(sa_net::VehicleRequest {
            sequence: self.vehicle_sequence,
            action,
        });
        self.keys.clear();
    }
    pub(super) fn release_driver(&mut self) {
        if self.driver_grant.is_some() || self.vehicle_request.is_some() {
            self.request_vehicle(sa_net::VehicleAction::Leave);
            self.driver_grant = None;
            self.driving = false;
            self.walking = true;
        }
    }
    pub(super) fn spawn_shared_car(&mut self) {
        if !self.menu.network_ready {
            return;
        }
        if let Some((car, _)) = &self.car {
            let pose = VehiclePose {
                position: (car.position - Vec3::Y * car.clearance).to_array(),
                yaw: car.yaw,
                pitch: car.pitch,
                roll: car.roll,
                speed: 0.0,
                interior: self.interior,
            };
            self.request_vehicle(sa_net::VehicleAction::Spawn {
                model: self.active_car as u16,
                pose,
            });
            self.driving = false;
            self.walking = true;
            self.menu.ride_status = "Requesting vehicle spawn...".into();
        }
    }
    pub(super) fn toggle_shared_car(&mut self) -> bool {
        if self.passenger.is_some() {
            self.try_leave_passenger();
            return true;
        }
        if self.vehicle_request.is_some() {
            return true;
        }
        if self.driver_grant.is_some() && self.driving {
            if let (Some(world), Some((car, _))) = (&self.collision, &mut self.car) {
                if let Some(player) = car.exit_player(world) {
                    car.stop();
                    self.position = player.eye();
                    self.player = Some(player);
                    self.release_driver();
                } else {
                    self.menu.ride_status =
                        "No clear supported exit. Wait for an open road.".into();
                }
            }
            return true;
        }
        if !self.menu.network_ready {
            return true;
        }
        let feet = self
            .player
            .as_ref()
            .map_or(self.position - Vec3::Y * 1.6, |p| p.feet);
        let nearest = self
            .remote_actors
            .iter()
            .filter(|a| a.id >= sa_net::vehicles::FIRST_VEHICLE_ID)
            .filter_map(|a| {
                let car = a.target.vehicle?;
                let d = feet.distance(Vec3::from_array(car.position));
                (car.interior == self.interior && d < 6.0).then_some((a.id, d, a.target.driving))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1));
        if let Some((vehicle, _, occupied)) = nearest {
            if occupied {
                self.request_ride(Some(vehicle));
            } else {
                self.request_vehicle(sa_net::VehicleAction::Enter { vehicle });
            }
            self.menu.ride_status = "Requesting a vehicle seat...".into();
        } else {
            self.menu.ride_status =
                "Move closer to a shared vehicle, or use F7 to spawn one.".into();
        }
        true
    }
    fn adopt_driver(&mut self, grant: sa_net::DriverGrant, car: sa_net::VehicleRecord) {
        let index = usize::from(car.model);
        if index >= self.car_catalog.len() {
            self.release_driver();
            self.menu.ride_status =
                "The server vehicle model is not in the prepared catalog.".into();
            return;
        }
        if index != self.active_car {
            if let Some(next) = self.car_catalog[index].take() {
                self.car_catalog[self.active_car] = self.car.replace(next);
                self.active_car = index;
            }
        }
        if let Some((vehicle, _)) = &mut self.car {
            let mut next = sa_scene::vehicle::Car::new(
                Vec3::from_array(car.pose.position) + Vec3::Y * vehicle.clearance,
                vehicle.clearance,
            )
            .with_handling(vehicle.handling);
            next.yaw = car.pose.yaw;
            next.pitch = car.pose.pitch;
            next.roll = car.pose.roll;
            next.speed = car.pose.speed;
            *vehicle = next;
        }
        let changed = self.driver_grant != Some(grant);
        self.driver_grant = Some(grant);
        self.driving = true;
        self.walking = true;
        self.car_render_pose = None;
        if changed {
            self.keys.clear();
        }
    }
    fn request_ride(&mut self, owner: Option<u32>) {
        self.ride_sequence = self.ride_sequence.wrapping_add(1).max(1);
        self.ride_request = Some(sa_net::RideRequest {
            sequence: self.ride_sequence,
            owner,
        });
        self.keys.clear();
    }
    pub(super) fn try_passenger(&mut self, explicit: bool) -> bool {
        if self.network_session.is_none() || !self.menu.network_ready || self.driving {
            return false;
        }
        if self.passenger.is_some() {
            self.try_leave_passenger();
            return true;
        }
        let feet = self
            .player
            .as_ref()
            .map_or(self.position - Vec3::Y * 1.6, |p| p.feet);
        let nearest = self
            .remote_actors
            .iter()
            .filter(|actor| actor.id >= sa_net::vehicles::FIRST_VEHICLE_ID)
            .filter_map(|actor| {
                let car = actor.target.vehicle?;
                if car.interior != self.interior {
                    return None;
                }
                let distance = feet.distance(Vec3::from_array(car.position));
                (distance < 6.0).then_some((actor.id, distance))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1));
        if let Some((owner, distance)) = nearest {
            let own_distance = self
                .car
                .as_ref()
                .map_or(f32::INFINITY, |(car, _)| feet.distance(car.position));
            if explicit || self.network_session.is_some() || distance < own_distance {
                self.request_ride(Some(owner));
                self.menu.ride_status = "Requesting a passenger seat...".into();
                return true;
            }
        }
        if explicit {
            self.menu.ride_status = "Move within 6 metres of another player's car.".into();
        }
        false
    }
    fn exit_passenger_position(&mut self) {
        if let Some(car) = self.passenger_car {
            if let Some(world) = &self.collision {
                let mut vehicle = sa_scene::vehicle::Car::new(Vec3::from_array(car.position), 0.0);
                vehicle.yaw = car.yaw;
                if let Some(player) = vehicle.exit_player(world) {
                    self.position = player.eye();
                    self.player = Some(player);
                } else {
                    let player = sa_scene::collision::Player::spawn(
                        world,
                        Vec3::from_array(car.position) + Vec3::Y * 3.0,
                    );
                    self.position = player.eye();
                    self.player = Some(player);
                }
            }
        }
        self.passenger = None;
        self.passenger_car = None;
        self.driving = false;
        self.walking = true;
        self.keys.clear();
    }
    pub(super) fn leave_passenger(&mut self) {
        if self.passenger.is_some() || self.ride_request.is_some_and(|r| r.owner.is_some()) {
            self.request_ride(None);
            if self.passenger.is_some() {
                self.exit_passenger_position();
            }
        }
    }
    pub(super) fn try_leave_passenger(&mut self) {
        if let Some(car) = self.passenger_car {
            if let Some(world) = &self.collision {
                let mut vehicle = sa_scene::vehicle::Car::new(Vec3::from_array(car.position), 0.0);
                vehicle.yaw = car.yaw;
                if vehicle.exit_player(world).is_none() {
                    self.menu.ride_status =
                        "No clear supported exit. Wait for an open road.".into();
                    return;
                }
            }
        }
        self.leave_passenger();
    }
    pub(super) fn network_action(&mut self, host: bool) {
        if let Err(error) = self.prepare_network(host) {
            self.fail_network(format!("Could not prepare multiplayer: {error:#}"));
        }
    }
    pub(super) fn fail_network(&mut self, reason: String) {
        eprintln!("Multiplayer ended: {reason}");
        self.disconnect_network();
        self.menu.message = sa_client::diagnostics::explain(&reason);
        self.menu.open(crate::menu::Page::Network);
    }
    pub(super) fn disconnect_network(&mut self) {
        self.auto_enter = false;
        self.leave_passenger();
        self.ride_request = None;
        self.ride_reply = None;
        self.ride_sequence = 0;
        self.driver_grant = None;
        self.vehicle_request = None;
        self.vehicle_reply = None;
        self.vehicle_sequence = 0;
        self.motion_sequence = 0;
        self.clear_session_resources();
        self.network_publication = None;
        self.network_session = None;
        self.remote_actors.clear();
        self.menu.network_status = "Offline".into();
        self.menu.ride_status.clear();
        self.menu.network_players.clear();
        self.menu.network_active = false;
        self.menu.session_code.clear();
        self.network_car_spawned = false;
    }
    pub(super) fn browse_network(&mut self) {
        if self.network_browser.is_some() {
            return;
        }
        let Ok(address) = self.menu.relay_address.trim().parse() else {
            self.menu.browser_status = "Enter the relay's IP and port first.".into();
            return;
        };
        let (sender, receiver) = std::sync::mpsc::channel();
        match std::thread::Builder::new()
            .name("server-browser".into())
            .spawn(move || {
                let result = sa_net::relay::browse(address).map_err(|e| e.to_string());
                let _ = sender.send(result);
            }) {
            Ok(_) => {
                self.network_browser = Some(BrowserRequest { address, receiver });
                self.menu.server_list.clear();
                self.menu.browser_status = "Refreshing sessions...".into();
            }
            Err(e) => self.menu.browser_status = e.to_string(),
        }
    }
    fn local_pose(&self) -> Pose {
        let vehicle = self
            .car
            .as_ref()
            .filter(|_| self.driving)
            .map(|(car, _)| VehiclePose {
                position: (car.position - Vec3::Y * car.clearance).to_array(),
                yaw: car.yaw,
                pitch: car.pitch,
                roll: car.roll,
                speed: car.speed,
                interior: 0,
            });
        if self.driving {
            if let Some((car, _)) = &self.car {
                return Pose {
                    position: (car.position - Vec3::Y * car.clearance).to_array(),
                    yaw: car.yaw,
                    pitch: car.pitch,
                    roll: car.roll,
                    speed: car.speed,
                    driving: true,
                    moving: car.speed.abs() > 0.1,
                    interior: self.interior,
                    car_model: self.active_car as u16,
                    ped_model: self.active_ped as u16,
                    clothes: self
                        .ped
                        .as_ref()
                        .map(|(ped, _)| ped.clothing_mask())
                        .unwrap_or(0),
                    vehicle,
                    ride_request: self.ride_request,
                    driver: self.driver_grant,
                    vehicle_request: self.vehicle_request,
                    vehicle_motion: self.driver_grant.zip(vehicle).map(|(g, pose)| {
                        sa_net::VehicleMotion {
                            vehicle: g.vehicle,
                            epoch: g.epoch,
                            sequence: self.motion_sequence,
                            pose,
                        }
                    }),
                    ..Pose::default()
                };
            }
        }
        let feet = if let Some((seat, car)) = self.passenger.zip(self.passenger_car) {
            Vec3::from_array(seat.feet(car))
        } else {
            self.player
                .as_ref()
                .filter(|_| self.walking)
                .map(|p| p.feet)
                .unwrap_or(self.position - Vec3::Y * 1.6)
        };
        let feet = match self.vehicle_request.map(|r| r.action) {
            Some(sa_net::VehicleAction::Spawn { pose, .. }) => Vec3::from_array(pose.position),
            _ => feet,
        };
        Pose {
            position: feet.to_array(),
            yaw: if self.walking { self.ped_yaw } else { self.yaw },
            moving: self.walking && self.ped_clip != "idle_stance",
            interior: self.interior,
            car_model: self.active_car as u16,
            ped_model: self.active_ped as u16,
            clothes: self
                .ped
                .as_ref()
                .map(|(ped, _)| ped.clothing_mask())
                .unwrap_or(0),
            vehicle,
            ride_request: self.ride_request,
            vehicle_request: self.vehicle_request,
            ..Pose::default()
        }
    }
    fn make_remote(&self, peer: &Peer) -> Option<RemoteActor> {
        let (ped_index, car_index) = self.appearance_indices(peer.pose)?;
        let (_, ped) = if ped_index == self.active_ped {
            self.ped.as_ref()
        } else {
            self.ped_catalog.get(ped_index)?.as_ref()
        }?;
        let (car, car_batches) = if car_index == self.active_car {
            self.car.as_ref()
        } else {
            self.car_catalog.get(car_index)?.as_ref()
        }?;
        Some(RemoteActor {
            id: peer.id,
            current: peer.pose,
            target: peer.pose,
            visible: false,
            car_visible: false,
            ped_visible: false,
            ped: if peer.id < sa_net::vehicles::FIRST_VEHICLE_ID {
                duplicate(&self.device, ped)
            } else {
                Vec::new()
            },
            car: if peer.id >= sa_net::vehicles::FIRST_VEHICLE_ID {
                duplicate(&self.device, car_batches)
            } else {
                Vec::new()
            },
            clearance: car.clearance,
            ped_model: ped_index,
            car_model: car_index,
            model_changed: Instant::now(),
            car_render_pose: None,
        })
    }
    fn appearance_indices(&self, pose: Pose) -> Option<(usize, usize)> {
        let ped = usize::from(pose.ped_model);
        let car = usize::from(pose.car_model);
        Some((
            if ped < self.ped_catalog.len() {
                ped
            } else {
                self.menu.peds.iter().rposition(|n| n == "Grove Street")?
            },
            if car < self.car_catalog.len() {
                car
            } else {
                self.menu.cars.iter().rposition(|n| n == "Taxi")?
            },
        ))
    }
    pub(super) fn update_network(&mut self) {
        self.update_session_resources();
        if let Some(request) = &self.network_browser {
            match request.receiver.try_recv() {
                Ok(result) => {
                    if self.menu.relay_mode
                        && self
                            .menu
                            .relay_address
                            .trim()
                            .parse::<std::net::SocketAddr>()
                            .ok()
                            == Some(request.address)
                    {
                        match result {
                            Ok(servers) => {
                                self.menu.browser_status =
                                    format!("{} public sessions", servers.len());
                                self.menu.server_list = servers;
                            }
                            Err(e) => {
                                self.menu.browser_status = format!("Server list unavailable: {e}")
                            }
                        }
                    } else {
                        self.menu.server_list.clear();
                        self.menu.browser_status =
                            "Relay changed. Refresh the server browser.".into();
                    }
                    self.network_browser = None;
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.menu.browser_status = "Server list request ended.".into();
                    self.network_browser = None;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
            }
        }
        let now = Instant::now();
        let dt = now
            .duration_since(self.network_last)
            .as_secs_f32()
            .min(0.25);
        self.network_last = now;
        let Some(session) = &self.network_session else {
            return;
        };
        self.motion_sequence = self.motion_sequence.wrapping_add(1).max(1);
        if let Some(mut report) = session.update(self.local_pose()) {
            if let Some(peer) = report.peers.iter().find(|p| p.id == report.local_id) {
                if let Some(reply) = peer.pose.vehicle_reply {
                    if self.vehicle_reply != Some(reply) {
                        self.menu.ride_status = reply.result.label().into();
                        eprintln!(
                            "Shared vehicle: {} ({})",
                            reply.result.label(),
                            reply.sequence
                        );
                        self.vehicle_reply = Some(reply);
                    }
                    if self
                        .vehicle_request
                        .is_some_and(|r| r.sequence == reply.sequence)
                    {
                        self.vehicle_request = None;
                    }
                }
                let grant = peer.pose.driver.filter(|_| self.vehicle_request.is_none());
                if self.vehicle_request.is_none() {
                    if let Some(grant) = grant {
                        if let Some(car) = report.vehicles.iter().find(|c| c.id == grant.vehicle) {
                            // Normal snapshots trail local physics by the RTT.
                            // Correct only a large divergence (e.g. rejected
                            // movement), without fighting ordinary interpolation.
                            let tolerance = 10.0
                                + car.pose.speed.abs()
                                    * report.round_trip.map_or(0.0, |r| r.as_secs_f32())
                                    * 2.0;
                            let divergent = self.car.as_ref().is_some_and(|(local, _)| {
                                (local.position - Vec3::Y * local.clearance)
                                    .distance(Vec3::from_array(car.pose.position))
                                    > tolerance
                            });
                            if self.driver_grant != Some(grant) || !self.driving || divergent {
                                if divergent && self.driver_grant == Some(grant) && self.driving {
                                    eprintln!("Correcting shared vehicle to its last accepted server pose");
                                }
                                self.adopt_driver(grant, *car);
                            }
                        }
                    } else if self.driver_grant.take().is_some() {
                        self.driving = false;
                        self.walking = true;
                    }
                }
                if let Some(reply) = peer.pose.ride_reply {
                    if self.ride_reply != Some(reply) {
                        self.menu.ride_status = reply.result.label().into();
                        eprintln!("Passenger: {} ({})", reply.result.label(), reply.sequence);
                        self.ride_reply = Some(reply);
                    }
                    if self
                        .ride_request
                        .is_some_and(|request| request.sequence == reply.sequence)
                    {
                        self.ride_request = None;
                    }
                }
                // A pending leave must not be undone by an older snapshot.
                let grant = peer
                    .pose
                    .ride
                    .filter(|_| !self.ride_request.is_some_and(|r| r.owner.is_none()));
                if self.passenger.is_some() && grant.is_none() {
                    self.exit_passenger_position();
                }
                self.passenger = grant;
                if grant.is_some() {
                    self.passenger_car = grant.and_then(|seat| {
                        report
                            .vehicles
                            .iter()
                            .find(|car| car.id == seat.owner)
                            .map(|car| car.pose)
                    });
                    self.driving = false;
                    self.walking = false;
                }
            }
            let ready = report.connected && report.peers.iter().any(|p| p.id == report.local_id);
            if ready && !self.menu.network_ready && report.local_id != 0 {
                self.spread_network_spawn(&report);
            }
            self.menu.network_ready = ready;
            if ready && self.auto_enter {
                self.auto_enter = false;
                self.apply_menu_action(Some(crate::menu::Action::Play));
                eprintln!("Launcher join entered prepared session");
            }
            if !report.connected && report.revision > 0 {
                let status = report.status.clone();
                self.fail_network(status);
                return;
            }
            self.menu.network_status = match report.round_trip {
                Some(rtt) => format!(
                    "{} | RTT {:.0} ms",
                    report.status,
                    rtt.as_secs_f64() * 1000.
                ),
                None => report.status.clone(),
            };
            if let Some(publication) = &self.network_publication {
                let published = publication.report();
                if !published.code.is_empty() && self.menu.session_code != published.code {
                    eprintln!("Multiplayer join code: {}", published.code);
                }
                self.menu.session_code = published.code;
                self.menu.network_status =
                    format!("{} | {}", self.menu.network_status, published.status);
            }
            self.menu.network_active = true;
            self.menu.network_players = report
                .peers
                .iter()
                .map(|p| {
                    format!(
                        "{}{}",
                        p.name,
                        if p.id == report.local_id {
                            " (you)"
                        } else {
                            ""
                        }
                    )
                })
                .collect();
            for car in &report.vehicles {
                if car.driver == Some(report.local_id) {
                    continue;
                }
                report.peers.push(Peer {
                    id: car.id,
                    name: String::new(),
                    pose: Pose {
                        position: car.pose.position,
                        yaw: car.pose.yaw,
                        pitch: car.pose.pitch,
                        roll: car.pose.roll,
                        speed: car.pose.speed,
                        driving: car.driver.is_some(),
                        interior: car.pose.interior,
                        car_model: car.model,
                        vehicle: Some(car.pose),
                        ..Pose::default()
                    },
                });
            }
            if report.revision != self.network_revision {
                self.network_revision = report.revision;
                self.remote_actors.retain(|actor| {
                    report
                        .peers
                        .iter()
                        .any(|p| p.id == actor.id && p.id != report.local_id)
                });
                for actor in &mut self.remote_actors {
                    if let Some(peer) = report.peers.iter().find(|p| p.id == actor.id) {
                        actor.target = peer.pose;
                    }
                }
            }
            // Bound model churn as well as initial allocation: one avatar per
            // frame, and at most one model replacement per peer per second.
            let changed = self.remote_actors.iter().position(|actor| {
                now.duration_since(actor.model_changed) >= Duration::from_secs(1)
                    && self
                        .appearance_indices(actor.target)
                        .is_some_and(|(ped, car)| ped != actor.ped_model || car != actor.car_model)
            });
            if let Some(index) = changed {
                let old = &self.remote_actors[index];
                if let Some(peer) = report.peers.iter().find(|p| p.id == old.id) {
                    if let Some(mut actor) = self.make_remote(peer) {
                        actor.current = old.current;
                        self.remote_actors[index] = actor;
                        self.network_pose_last = now - Duration::from_secs(1);
                    }
                }
            } else if let Some(peer) = report.peers.iter().find(|p| {
                p.id != report.local_id && !self.remote_actors.iter().any(|a| a.id == p.id)
            }) {
                if let Some(actor) = self.make_remote(peer) {
                    self.remote_actors.push(actor);
                    self.network_pose_last = now - Duration::from_secs(1);
                }
            }
        }
        let mut visibility_changed = false;
        for actor in &mut self.remote_actors {
            interpolate(&mut actor.current, actor.target, dt);
            let previous_visibility = (actor.ped_visible, actor.car_visible);
            actor.ped_visible = actor.id < sa_net::vehicles::FIRST_VEHICLE_ID
                && !actor.current.driving
                && actor.current.ride.is_none()
                && actor.current.interior == self.interior
                && Vec3::from_array(actor.current.position).distance(self.position) < 300.0;
            actor.car_visible = actor.id >= sa_net::vehicles::FIRST_VEHICLE_ID
                && actor.current.vehicle.is_some_and(|car| {
                    car.interior == self.interior
                        && Vec3::from_array(car.position).distance(self.position) < 300.0
                });
            actor.visible = actor.ped_visible || actor.car_visible;
            visibility_changed |= previous_visibility != (actor.ped_visible, actor.car_visible);
        }
        if visibility_changed {
            self.network_pose_last = now - Duration::from_secs(1);
        }
        if let Some(seat) = self.passenger {
            if let Some(car) = self
                .remote_actors
                .iter()
                .find(|a| a.id == seat.owner)
                .and_then(|a| a.current.vehicle)
            {
                self.passenger_car = Some(car);
                let center = Vec3::from_array(car.position) + Vec3::Y;
                let forward = Vec3::new(car.yaw.sin(), 0.0, car.yaw.cos());
                let desired = center - forward * 7.0 + Vec3::Y * 2.5;
                self.position = self
                    .collision
                    .as_ref()
                    .map_or(desired, |world| world.clip_camera(center, desired));
            }
        }
        if now.duration_since(self.network_pose_last) < Duration::from_millis(33) {
            return;
        }
        self.network_pose_last = now;
        for actor in self.remote_actors.iter_mut().filter(|a| a.visible) {
            let pose = actor.current;
            let feet = Vec3::from_array(pose.position);
            let rotation = Quat::from_rotation_y(pose.yaw + std::f32::consts::PI);
            if let Some(car) = pose
                .vehicle
                .filter(|car| actor.car_visible && car_changed(actor.car_render_pose, *car))
            {
                let feet = Vec3::from_array(car.position);
                let rotation = sa_scene::vehicle::model_rotation(car.yaw, car.pitch, car.roll);
                for batch in &actor.car {
                    let mut raw = batch.base.clone();
                    for v in raw.as_chunks_mut::<9>().0.iter_mut() {
                        let p = rotation * Vec3::new(v[0], v[1], v[2])
                            + feet
                            + Vec3::Y * actor.clearance;
                        v[..3].copy_from_slice(&p.to_array());
                    }
                    self.queue
                        .write_buffer(&batch.buffer, 0, bytemuck::cast_slice(&raw));
                }
                actor.car_render_pose = Some(car);
            }
            if let Some((ped, _)) = if !actor.ped_visible {
                None
            } else if actor.ped_model == self.active_ped {
                self.ped.as_ref()
            } else {
                self.ped_catalog
                    .get(actor.ped_model)
                    .and_then(|p| p.as_ref())
            } {
                let clip = if pose.moving {
                    "walk_player"
                } else {
                    "idle_stance"
                };
                if let Ok(posed) = ped.frame_with_clothing(
                    clip,
                    now.duration_since(self.started).as_secs_f32(),
                    pose.clothes,
                ) {
                    for (mesh, batch) in posed.iter().zip(&actor.ped) {
                        if mesh.vertices.len() * 9 != batch.base.len() {
                            continue;
                        }
                        let mut raw = Vec::with_capacity(mesh.vertices.len() * 9);
                        for vertex in &mesh.vertices {
                            raw.extend(
                                (rotation * Vec3::from_array(vertex.position) + feet).to_array(),
                            );
                            raw.extend(vertex.uv);
                            raw.extend(vertex.color);
                        }
                        self.queue
                            .write_buffer(&batch.buffer, 0, bytemuck::cast_slice(&raw));
                    }
                }
            }
        }
    }
    fn spread_network_spawn(&mut self, report: &sa_net::Report) {
        let (Some(world), Some(player)) = (&self.collision, &self.player) else {
            return;
        };
        let base = player.feet;
        for attempt in 0..24 {
            let angle = (report.local_id as f32 + attempt as f32) * 2.399_963;
            let radius = 4.0 + (attempt / 8) as f32 * 2.0;
            let point = base + Vec3::new(angle.cos() * radius, 0.0, angle.sin() * radius);
            if report.peers.iter().any(|p| {
                p.id != report.local_id && Vec3::from_array(p.pose.position).distance(point) < 3.5
            }) {
                continue;
            }
            if let Some(player) = world.standing_at(point, base.y + 1.5) {
                self.position = player.eye();
                self.player = Some(player);
                self.place_car();
                self.driving = false;
                self.network_car_spawned = false;
                return;
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn interpolation_takes_short_yaw_arc_and_snaps_teleports_or_vehicle_changes() {
        let mut pose = Pose {
            yaw: 3.1,
            ..Pose::default()
        };
        interpolate(
            &mut pose,
            Pose {
                yaw: -3.1,
                car_model: 4,
                ped_model: 3,
                clothes: 2,
                position: [1.0, 0.0, 0.0],
                ..Pose::default()
            },
            0.02,
        );
        assert!(pose.yaw > 3.1 && pose.yaw < 3.2);
        assert!(pose.position[0] > 0.0 && pose.position[0] < 1.0);
        assert_eq!((pose.car_model, pose.ped_model, pose.clothes), (4, 3, 2));
        let teleport = Pose {
            position: [1000.0, 0.0, 0.0],
            ..Pose::default()
        };
        interpolate(&mut pose, teleport, 0.01);
        assert_eq!(pose, teleport);
        let driving = Pose {
            driving: true,
            ..teleport
        };
        interpolate(&mut pose, driving, 0.01);
        assert_eq!(pose, driving);
    }
}
