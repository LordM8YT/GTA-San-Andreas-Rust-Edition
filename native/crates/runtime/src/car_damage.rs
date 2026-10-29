//! Cars that move and break as in SA: wheels roll and steer, doors swing
//! open for whoever climbs in or out, crashes dent the panels that hit and
//! cost health, bullets hurt, a car below a quarter health catches fire and
//! blows up, and an explosion hurts everyone and every car around it.
//!
//! Every car is already re-posed on the CPU when it moves, so turning a few
//! wheels and doors on the way costs next to nothing, even on weak PCs.
use super::car_contacts::Footprint;
use super::{GpuBatch, State};
use glam::{Affine3A, Quat, Vec3};
use sa_scene::vehicle::Look;

/// How far an explosion reaches, in metres.
const BLAST_RADIUS: f32 = 9.0;
/// Harm at the centre of an explosion to people and to other cars; it
/// falls off to nothing at the edge.
const BLAST_PEOPLE: f32 = 250.0;
const BLAST_CARS: f32 = 600.0;
/// Seconds an explosion's flash lasts.
const FLASH_SECONDS: f32 = 1.2;
/// Seconds a wreck keeps burning after it blew up.
const WRECK_FIRE_SECONDS: f32 = 10.0;
/// Fire and flash sprites drawn at most; the furthest are left out.
const FIRE_GLOWS: usize = 40;
pub const FIRE_VERTICES: usize = FIRE_GLOWS * 6;
const CORNERS: [[f32; 2]; 6] = [
    [-1.0, -1.0],
    [1.0, -1.0],
    [1.0, 1.0],
    [-1.0, -1.0],
    [1.0, 1.0],
    [-1.0, 1.0],
];

/// The vertices of one car batch placed in the world: its door, wheel or
/// panel moved for `look`, then the whole car turned and moved into place.
pub fn pose_car(batch: &GpuBatch, look: &Look, rotation: Quat, offset: Vec3) -> Vec<f32> {
    let mut raw = batch.base.clone();
    let local = match batch.piece {
        Some(piece) => piece.transform(look),
        None => Some(Affine3A::IDENTITY),
    };
    let Some(local) = local else {
        // The other look of this part is drawn; fold this one into a point.
        for v in raw.as_chunks_mut::<9>().0.iter_mut() {
            v[..3].copy_from_slice(&offset.to_array());
        }
        return raw;
    };
    for v in raw.as_chunks_mut::<9>().0.iter_mut() {
        let point = rotation * local.transform_point3(Vec3::new(v[0], v[1], v[2])) + offset;
        v[..3].copy_from_slice(&point.to_array());
        if look.burnt {
            // A burnt-out shell is charred black.
            for channel in &mut v[5..8] {
                *channel *= 0.15;
            }
        }
    }
    raw
}

/// A door swinging open and shut while someone climbs in or out.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DoorSwing {
    /// 0 front left, 1 front right.
    pub door: usize,
    pub elapsed: f32,
    /// Seconds to swing fully open.
    pub opening: f32,
    /// When it starts to close, and when it is shut again.
    pub closing_at: f32,
    pub total: f32,
}

impl DoorSwing {
    /// Climbing in: open during the first clip, shut during the last.
    pub fn entering(door: usize, open: f32, climb: f32, shut: f32) -> Self {
        Self {
            door,
            elapsed: 0.0,
            opening: open,
            closing_at: open + climb,
            total: open + climb + shut,
        }
    }
    /// Climbing out: the door swings open at once, then shuts behind.
    pub fn leaving(door: usize, climb: f32, shut: f32) -> Self {
        Self {
            door,
            elapsed: 0.0,
            opening: 0.3,
            closing_at: climb,
            total: climb + shut,
        }
    }
    /// How far open the door is, 0 shut to 1 wide.
    pub fn open(&self) -> f32 {
        let opened = (self.elapsed / self.opening.max(0.05)).clamp(0.0, 1.0);
        let shut = ((self.elapsed - self.closing_at) / (self.total - self.closing_at).max(0.05))
            .clamp(0.0, 1.0);
        let amount = opened.min(1.0 - shut);
        // Ease in and out like a door on a hinge.
        amount * amount * (3.0 - 2.0 * amount)
    }
}

/// Which car a shot or a blast hit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CarRef {
    Own,
    /// A traffic car, by its id.
    Traffic(u64),
}

/// Where flames come out of a car: the engine under the bonnet.
fn engine(position: Vec3, yaw: f32, size: [f32; 3]) -> Vec3 {
    let forward = Vec3::new(yaw.sin(), 0.0, yaw.cos());
    position + forward * size[1] * 0.65 + Vec3::Y * size[2] * 0.6
}

impl State {
    /// The look of the player's car this frame, with the door anyone is
    /// climbing through.
    pub(super) fn own_car_look(&self) -> Option<Look> {
        let (car, _) = self.car.as_ref()?;
        let mut look = car.look();
        if let Some(swing) = &self.door_swing {
            look.doors[swing.door.min(3)] = swing.open();
        }
        Some(look)
    }

    /// The nearest car a shot from `origin` along unit `direction` hits
    /// within `range`, and how far along the ray.
    pub(super) fn first_car_hit(
        &self,
        origin: Vec3,
        direction: Vec3,
        range: f32,
    ) -> Option<(CarRef, f32)> {
        let own = self.car.as_ref().filter(|_| !self.driving).map(|(car, _)| {
            (
                CarRef::Own,
                Footprint::car(car.position - Vec3::Y * car.clearance, car.yaw, car.size),
            )
        });
        let traffic = self.traffic.cars.iter().map(|car| {
            (
                CarRef::Traffic(car.id),
                Footprint::car(car.position, car.yaw, car.size),
            )
        });
        own.into_iter()
            .chain(traffic)
            .filter_map(|(which, body)| {
                body.ray_hit(origin, direction, range)
                    .map(|distance| (which, distance))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
    }

    /// Bullets or a blast take `amount` off a car's health.
    pub(super) fn hurt_car(&mut self, which: CarRef, amount: f32) {
        match which {
            CarRef::Own => {
                if let Some((car, _)) = &mut self.car {
                    car.damage.hurt(amount);
                }
            }
            CarRef::Traffic(id) => {
                if let Some(car) = self.traffic.cars.iter_mut().find(|car| car.id == id) {
                    car.damage.hurt(amount);
                    // A shot-at driver stops, like a knock.
                    car.knock(3.0, Vec3::ZERO);
                    let crime = if car.police.is_some() {
                        crate::wanted::Crime::HitPoliceCar
                    } else {
                        crate::wanted::Crime::DamageCar
                    };
                    let at = car.position;
                    self.report_crime(crime, at);
                }
            }
        }
    }

    /// Burn down cars on fire, blow up the ones with no health left and
    /// move doors along.
    pub(super) fn update_car_damage(&mut self, dt: f32) {
        if let Some(swing) = &mut self.door_swing {
            swing.elapsed += dt;
            if swing.elapsed >= swing.total {
                self.door_swing = None;
            }
        }
        let mut blasts = Vec::new();
        if let Some((car, _)) = &mut self.car {
            if car.damage.update(dt) {
                car.blast();
                blasts.push((car.position, CarRef::Own));
            }
        }
        let mut crimes = Vec::new();
        for car in &mut self.traffic.cars {
            if car.damage.update(dt) {
                blasts.push((car.position, CarRef::Traffic(car.id)));
                crimes.push((
                    if car.police.is_some() {
                        crate::wanted::Crime::BlowUpPolice
                    } else {
                        crate::wanted::Crime::BlowUpCar
                    },
                    car.position,
                ));
            }
        }
        for (at, source) in blasts {
            self.explode(at, source);
        }
        for (crime, at) in crimes {
            self.report_crime(crime, at);
        }
        for flash in &mut self.blast_flashes {
            flash.1 += dt;
        }
        self.blast_flashes.retain(|flash| flash.1 < FLASH_SECONDS);
        self.wreck_fire_seconds += dt;
    }

    /// A car blew up at `at`: people and cars around it are hurt, which can
    /// set off the next car, and whoever sat in it is thrown out.
    fn explode(&mut self, at: Vec3, source: CarRef) {
        self.blast_flashes.push((at, 0.0));
        self.crowd.blast(at, BLAST_RADIUS, BLAST_PEOPLE);
        let harm = |position: Vec3| {
            let distance = position.distance(at);
            (distance < BLAST_RADIUS).then(|| BLAST_CARS * (1.0 - distance / BLAST_RADIUS))
        };
        for car in &mut self.traffic.cars {
            if source == CarRef::Traffic(car.id) {
                continue;
            }
            if let Some(amount) = harm(car.position) {
                car.damage.hurt(amount);
            }
        }
        if source == CarRef::Own {
            if self.driving {
                self.bail_out();
            }
        } else if let Some((car, _)) = &mut self.car {
            if let Some(amount) = harm(car.position) {
                car.damage.hurt(amount);
                car.blast();
            }
        }
        if let (Some(audio), Some(sounds)) = (&mut self.audio, &self.gameplay_audio.sounds) {
            // The heaviest gunshot in the bank stands in for the blast.
            if let Some(sound) = sounds.gunshot("shotgun") {
                if let Err(error) = audio.play_spatial(sound, at.to_array()) {
                    eprintln!("Explosion sound unavailable: {error:#}");
                }
            }
        }
    }

    /// Thrown out of a car that blew up, or hauled out by the police.
    pub(super) fn bail_out(&mut self) {
        let (Some(world), Some((car, _))) = (&self.collision, &mut self.car) else {
            return;
        };
        car.stop();
        if let Some(player) = car.exit_player(world) {
            self.position = player.eye();
            self.ped_yaw = car.yaw;
            self.ped_animator.reset();
            self.player = Some(player);
            self.driving = false;
            self.walking = true;
            self.entering_car = None;
            self.door_swing = None;
        }
    }

    /// Flames on burning cars and the flash of fresh explosions, as
    /// always-lit coronas. Returns the vertex count written to `out`.
    pub(super) fn fire_glows(&self, out: &mut [sa_scene::Vertex; FIRE_VERTICES]) -> u32 {
        let time = self.wreck_fire_seconds;
        let mut glows: Vec<(Vec3, f32, [f32; 4])> = Vec::new();
        let mut flame = |at: Vec3, seed: f32, strength: f32| {
            let flicker = 0.75
                + 0.25
                    * ((time * 13.0 + seed).sin() * 0.6 + (time * 23.0 + seed * 1.7).sin() * 0.4);
            glows.push((
                at + Vec3::Y * 0.1 * flicker,
                1.6 * flicker * strength,
                [1.0, 0.45, 0.1, 0.5],
            ));
        };
        if let Some((car, _)) = &self.car {
            let at = engine(car.position - Vec3::Y * car.clearance, car.yaw, car.size);
            if car.damage.burning() {
                flame(at, 0.0, 1.0);
            }
        }
        for car in &self.traffic.cars {
            let at = engine(car.position, car.yaw, car.size);
            if car.damage.burning() {
                flame(at, car.id as f32, 1.0);
            } else if car.damage.wrecked && car.burnt_for < WRECK_FIRE_SECONDS {
                flame(at, car.id as f32, 1.0 - car.burnt_for / WRECK_FIRE_SECONDS);
            }
        }
        glows.extend(self.light_bars(time));
        for (at, age) in &self.blast_flashes {
            let fade = 1.0 - age / FLASH_SECONDS;
            glows.push((
                *at + Vec3::Y,
                7.0 * (0.4 + 0.6 * fade),
                [2.4 * fade, 1.5 * fade, 0.5 * fade, 0.5],
            ));
        }
        glows.sort_by(|a, b| {
            a.0.distance_squared(self.position)
                .total_cmp(&b.0.distance_squared(self.position))
        });
        let mut count = 0;
        for (at, size, color) in glows.into_iter().take(FIRE_GLOWS) {
            for [x, y] in CORNERS {
                out[count] = sa_scene::Vertex {
                    position: at.to_array(),
                    uv: [x * size, y * size],
                    color,
                };
                count += 1;
            }
        }
        count as u32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_door_opens_while_climbing_in_and_shuts_after() {
        let mut swing = DoorSwing::entering(0, 0.5, 1.0, 0.5);
        assert_eq!(swing.open(), 0.0);
        swing.elapsed = 0.25;
        assert!((0.3..0.7).contains(&swing.open()), "{}", swing.open());
        swing.elapsed = 1.0;
        assert_eq!(swing.open(), 1.0, "held open while climbing in");
        swing.elapsed = 2.0;
        assert_eq!(swing.open(), 0.0, "shut at the end");
    }

    #[test]
    fn a_door_is_open_at_once_when_climbing_out() {
        let mut swing = DoorSwing::leaving(1, 1.0, 0.6);
        swing.elapsed = 0.4;
        assert_eq!(swing.open(), 1.0);
        swing.elapsed = 1.3;
        assert!(swing.open() > 0.0 && swing.open() < 1.0);
        swing.elapsed = 1.6;
        assert_eq!(swing.open(), 0.0);
    }
}
