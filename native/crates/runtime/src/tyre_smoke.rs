//! Tyre smoke from burnouts and slides: soft grey puffs that rise, spread
//! and fade behind the driven car. A fixed pool of camera-facing quads in one
//! small dynamic buffer and one draw call.
use glam::Vec3;
use sa_scene::Vertex;

pub const PUFFS: usize = 96;
pub const VERTICES: usize = PUFFS * 6;
/// Seconds a puff lives.
const LIFE: f32 = 1.6;
/// Puffs per second from each rear tyre at full intensity.
const RATE: f32 = 14.0;
/// Width of a puff when it appears and when it fades out, m.
const START_SIZE: f32 = 0.5;
const END_SIZE: f32 = 2.4;
/// Side length of the soft round puff texture, pixels.
pub const TEXTURE_SIZE: u32 = 32;

#[derive(Clone, Copy, Default)]
struct Puff {
    position: Vec3,
    velocity: Vec3,
    age: f32,
    strength: f32,
}

pub struct TyreSmoke {
    puffs: Vec<Puff>,
    next: usize,
    /// Fractional puffs owed from earlier frames.
    owed: f32,
    seed: u32,
    pub vertices: Vec<Vertex>,
    /// Vertices to draw this frame.
    pub count: u32,
}

impl Default for TyreSmoke {
    fn default() -> Self {
        Self {
            puffs: vec![
                Puff {
                    age: LIFE,
                    ..Puff::default()
                };
                PUFFS
            ],
            next: 0,
            owed: 0.0,
            seed: 0x9e37_79b9,
            vertices: vec![
                Vertex {
                    position: [0.0; 3],
                    uv: [0.0; 2],
                    color: [0.0; 4],
                };
                VERTICES
            ],
            count: 0,
        }
    }
}

impl TyreSmoke {
    fn random(&mut self) -> f32 {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 17;
        self.seed ^= self.seed << 5;
        self.seed as f32 / u32::MAX as f32
    }
    /// Smoke the car's rear tyres for `dt` seconds; `intensity` is 0 for
    /// gripping tyres up to 1 for a burnout.
    pub fn emit(&mut self, car: &sa_scene::vehicle::Car, intensity: f32, dt: f32) {
        let intensity = intensity.clamp(0.0, 1.0);
        if intensity <= 0.0 || car.airborne() {
            self.owed = 0.0;
            return;
        }
        let forward = car.forward();
        let side = Vec3::new(forward.z, 0.0, -forward.x);
        let ground = car.position - Vec3::Y * (car.clearance - 0.25);
        self.owed += RATE * 2.0 * intensity * dt;
        while self.owed >= 1.0 {
            self.owed -= 1.0;
            let tyre = if self.next.is_multiple_of(2) { -0.8 } else { 0.8 };
            let jitter = Vec3::new(self.random() - 0.5, 0.0, self.random() - 0.5) * 0.4;
            let drift = Vec3::new(self.random() - 0.5, 0.0, self.random() - 0.5) * 1.2;
            self.puffs[self.next] = Puff {
                position: ground - forward * 1.3 + side * tyre + jitter,
                velocity: car.velocity() * 0.3 + drift + Vec3::Y * (0.6 + self.random() * 0.5),
                age: 0.0,
                strength: intensity,
            };
            self.next = (self.next + 1) % PUFFS;
        }
    }
    /// Age the puffs and build the quads facing the camera.
    pub fn update(&mut self, right: Vec3, up: Vec3, dt: f32) {
        let mut used = 0;
        for puff in &mut self.puffs {
            if puff.age >= LIFE {
                continue;
            }
            puff.age += dt;
            puff.velocity *= (-1.5 * dt).exp();
            puff.position += puff.velocity * dt;
            let t = (puff.age / LIFE).min(1.0);
            let half = (START_SIZE + (END_SIZE - START_SIZE) * t) * 0.5;
            let alpha = 0.55 * puff.strength * (1.0 - t) * (t * 8.0).min(1.0);
            let (r, u) = (right * half, up * half);
            let p = puff.position;
            let corners = [
                (p - r - u, [0.0, 1.0]),
                (p + r - u, [1.0, 1.0]),
                (p + r + u, [1.0, 0.0]),
                (p - r - u, [0.0, 1.0]),
                (p + r + u, [1.0, 0.0]),
                (p - r + u, [0.0, 0.0]),
            ];
            for (vertex, (corner, uv)) in self.vertices[used..used + 6].iter_mut().zip(corners) {
                *vertex = Vertex {
                    position: corner.to_array(),
                    uv,
                    color: [0.86, 0.86, 0.86, alpha],
                };
            }
            used += 6;
        }
        self.count = used as u32;
    }
}

/// How hard the car's tyres should smoke: full in a burnout, and rising
/// with the sideways slide of a drift.
pub fn intensity(car: &sa_scene::vehicle::Car) -> f32 {
    if car.burning_out() {
        1.0
    } else {
        ((car.slip() - 4.0) / 6.0).clamp(0.0, 0.8)
    }
}

/// A soft round white puff, transparent at the edge, as RGBA8.
pub fn texture() -> Vec<u8> {
    let size = TEXTURE_SIZE as usize;
    let mut pixels = Vec::with_capacity(size * size * 4);
    for y in 0..size {
        for x in 0..size {
            let dx = (x as f32 + 0.5) / size as f32 * 2.0 - 1.0;
            let dy = (y as f32 + 0.5) / size as f32 * 2.0 - 1.0;
            let falloff = (1.0 - (dx * dx + dy * dy).sqrt()).clamp(0.0, 1.0);
            pixels.extend_from_slice(&[255, 255, 255, (falloff * falloff * 255.0) as u8]);
        }
    }
    pixels
}

#[cfg(test)]
mod tests {
    use super::*;
    use sa_scene::{collision::CollisionWorld, vehicle::Car};

    #[test]
    fn gripping_tyres_make_no_smoke_and_puffs_fade_away() {
        let world = CollisionWorld::from_batches(&[]);
        let mut car = Car::new(Vec3::Y * 50.0, 0.6);
        car.step(&world, 0.0, 0.0, false, 1.0, 1.0 / 60.0);
        let mut smoke = TyreSmoke::default();
        smoke.emit(&car, intensity(&car), 0.5);
        smoke.update(Vec3::X, Vec3::Y, 0.1);
        assert_eq!(smoke.count, 0);
        // A car on the ground at full intensity puffs from both rear tyres.
        let mut grounded = Car::new(Vec3::Y * 0.6, 0.6);
        let floor = sa_scene::Batch {
            key: "floor".into(),
            alpha: false,
            animated: false,
            uv_animation: None,
            vertices: [
                [-50.0, 0.0, -50.0],
                [50.0, 0.0, -50.0],
                [50.0, 0.0, 50.0],
                [-50.0, 0.0, -50.0],
                [50.0, 0.0, 50.0],
                [-50.0, 0.0, 50.0],
            ]
            .map(|p| Vertex {
                position: p,
                uv: [0.0; 2],
                color: [1.0; 4],
            })
            .to_vec(),
        };
        let world = CollisionWorld::from_batches(&[floor]);
        grounded.step(&world, 0.0, 0.0, false, 1.0, 1.0 / 60.0);
        smoke.emit(&grounded, 1.0, 0.5);
        smoke.update(Vec3::X, Vec3::Y, 0.1);
        assert_eq!(smoke.count, 14 * 6);
        for _ in 0..40 {
            smoke.update(Vec3::X, Vec3::Y, 0.05);
        }
        assert_eq!(smoke.count, 0, "puffs are gone after their life");
    }

    #[test]
    fn the_puff_texture_is_solid_in_the_middle_and_clear_at_the_edge() {
        let pixels = texture();
        let size = TEXTURE_SIZE as usize;
        let alpha = |x: usize, y: usize| pixels[(y * size + x) * 4 + 3];
        assert!(alpha(size / 2, size / 2) > 200);
        assert_eq!(alpha(0, 0), 0);
    }
}
