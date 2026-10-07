use super::{GpuBatch, State};
use glam::{Quat, Vec3};
use sa_net::{Peer, Pose};
use std::time::{Duration, Instant};
use wgpu::util::DeviceExt;

pub(super) struct BrowserRequest {
    address: std::net::SocketAddr,
    receiver: std::sync::mpsc::Receiver<Result<Vec<sa_net::relay::Listing>, String>>,
}

pub(super) struct RemoteActor {
    pub id: u32,
    pub current: Pose,
    pub target: Pose,
    pub visible: bool,
    pub ped: Vec<GpuBatch>,
    pub car: Vec<GpuBatch>,
    clearance: f32,
    pub ped_model: usize,
    pub car_model: usize,
    model_changed: Instant,
}
fn duplicate(device: &wgpu::Device, batches: &[GpuBatch]) -> Vec<GpuBatch> {
    batches
        .iter()
        .map(|b| GpuBatch {
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
fn interpolate(current: &mut Pose, target: Pose, dt: f32) {
    let a = Vec3::from_array(current.position);
    let b = Vec3::from_array(target.position);
    if current.interior != target.interior
        || current.driving != target.driving
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
}
impl State {
    pub(super) fn network_action(&mut self, host: bool) {
        if let Err(error) = self.prepare_network(host) {
            self.menu.message = format!("Could not prepare multiplayer: {error:#}");
        }
    }
    pub(super) fn disconnect_network(&mut self) {
        self.clear_session_resources();
        self.network_publication = None;
        self.network_session = None;
        self.remote_actors.clear();
        self.menu.network_status = "Offline".into();
        self.menu.network_players.clear();
        self.menu.network_active = false;
        self.menu.session_code.clear();
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
                };
            }
        }
        let feet = self
            .player
            .as_ref()
            .filter(|_| self.walking)
            .map(|p| p.feet)
            .unwrap_or(self.position - Vec3::Y * 1.6);
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
            ped: duplicate(&self.device, ped),
            car: duplicate(&self.device, car_batches),
            clearance: car.clearance,
            ped_model: ped_index,
            car_model: car_index,
            model_changed: Instant::now(),
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
        if let Some(report) = session.update(self.local_pose()) {
            let ready = report.connected && report.peers.iter().any(|p| p.id == report.local_id);
            if ready && !self.menu.network_ready && report.local_id != 0 {
                self.spread_network_spawn(&report);
            }
            self.menu.network_ready = ready;
            if !report.connected && report.revision > 0 {
                let status = report.status.clone();
                self.disconnect_network();
                self.menu.message = status;
                self.menu.open(crate::menu::Page::Network);
                return;
            }
            self.menu.network_status = report.status.clone();
            if let Some(publication) = &self.network_publication {
                let published = publication.report();
                if !published.code.is_empty() && self.menu.session_code != published.code {
                    eprintln!("Multiplayer join code: {}", published.code);
                }
                self.menu.session_code = published.code;
                self.menu.network_status = format!("{} | {}", report.status, published.status);
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
        for actor in &mut self.remote_actors {
            interpolate(&mut actor.current, actor.target, dt);
            actor.visible = actor.current.interior == self.interior
                && Vec3::from_array(actor.current.position).distance(self.position) < 300.0;
        }
        if now.duration_since(self.network_pose_last) < Duration::from_millis(33) {
            return;
        }
        self.network_pose_last = now;
        for actor in self.remote_actors.iter().filter(|a| a.visible) {
            let pose = actor.current;
            let feet = Vec3::from_array(pose.position);
            let rotation = Quat::from_rotation_y(pose.yaw + std::f32::consts::PI);
            if pose.driving {
                let rotation = rotation
                    * Quat::from_rotation_x(pose.pitch)
                    * Quat::from_rotation_z(-pose.roll);
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
            } else if let Some((ped, _)) = if actor.ped_model == self.active_ped {
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
