use super::{GpuBatch, State};
use glam::{Quat, Vec3};
use sa_net::{Peer, Pose, Session};
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
}
impl State {
    pub(super) fn network_action(&mut self, host: bool) {
        if self.network_session.is_some() {
            self.menu.message = "Disconnect from the current session first.".into();
            return;
        }
        if self.menu.peds.iter().all(|n| n != "Grove Street")
            || self.menu.cars.iter().all(|n| n != "Taxi")
        {
            self.menu.message =
                "Multiplayer requires the original Taxi and Grove Street models.".into();
            return;
        }
        let address = if host {
            &self.menu.host_address
        } else {
            &self.menu.join_address
        };
        let result = if self.menu.relay_mode {
            self.menu
                .relay_address
                .trim()
                .parse()
                .map_err(|_| "Use a relay IP and port, for example 127.0.0.1:7778.".to_string())
                .and_then(|relay| {
                    if host {
                        Session::host_relay(relay, &self.menu.player_name, self.menu.public_session)
                            .map(|(session, publication)| {
                                self.network_publication = Some(publication);
                                session
                            })
                    } else {
                        Session::join_relay(relay, &self.menu.join_code, &self.menu.player_name)
                    }
                    .map_err(|e| e.to_string())
                })
        } else {
            address
                .trim()
                .parse()
                .map_err(|_| {
                    "Use an IP address and port, for example 192.168.1.10:7777.".to_string()
                })
                .and_then(|address| {
                    if host {
                        Session::host(address, &self.menu.player_name)
                    } else {
                        Session::join(address, &self.menu.player_name)
                    }
                    .map_err(|e| e.to_string())
                })
        };
        match result {
            Ok(session) => {
                eprintln!(
                    "Multiplayer {}: {}",
                    if host { "host listening" } else { "joining" },
                    session.address
                );
                self.network_session = Some(session);
                self.network_revision = 0;
                self.menu.message.clear();
                // Keep the session page open until the handshake succeeds.
            }
            Err(error) => self.menu.message = error,
        }
    }
    pub(super) fn disconnect_network(&mut self) {
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
            ..Pose::default()
        }
    }
    fn make_remote(&self, peer: &Peer) -> Option<RemoteActor> {
        let ped_index = self.menu.peds.iter().rposition(|n| n == "Grove Street")?;
        let car_index = self.menu.cars.iter().rposition(|n| n == "Taxi")?;
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
        })
    }
    pub(super) fn update_network(&mut self) {
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
            // Allocate at most one new avatar per frame rather than 19 at once.
            if let Some(peer) = report.peers.iter().find(|p| {
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
        let ped_index = self.menu.peds.iter().rposition(|n| n == "Grove Street");
        let source = ped_index.and_then(|i| {
            if i == self.active_ped {
                self.ped.as_ref()
            } else {
                self.ped_catalog.get(i).and_then(|p| p.as_ref())
            }
        });
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
            } else if let Some((ped, _)) = source {
                let clip = if pose.moving {
                    "walk_player"
                } else {
                    "idle_stance"
                };
                if let Ok(posed) = ped.frame(clip, now.duration_since(self.started).as_secs_f32()) {
                    for (mesh, batch) in posed.iter().zip(&actor.ped) {
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
                position: [1.0, 0.0, 0.0],
                ..Pose::default()
            },
            0.02,
        );
        assert!(pose.yaw > 3.1 && pose.yaw < 3.2);
        assert!(pose.position[0] > 0.0 && pose.position[0] < 1.0);
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
