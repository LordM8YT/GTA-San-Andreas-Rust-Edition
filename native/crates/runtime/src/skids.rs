//! Tyre marks left on the road when the player's car slides or brakes hard.
//! A fixed ring of quads: the oldest marks are overwritten by new ones.
use glam::Vec3;
use sa_scene::Vertex;

pub const SEGMENTS: usize = 1024;
pub const VERTICES: usize = SEGMENTS * 6;
/// Shortest stretch of road a new segment covers, in metres.
const STEP: f32 = 0.35;
const WIDTH: f32 = 0.24;

pub struct Skids {
    pub vertices: Vec<Vertex>,
    next: usize,
    /// Last mark point of each rear tyre while it is still sliding.
    last: [Option<Vec3>; 2],
    pub dirty: bool,
}

impl Default for Skids {
    fn default() -> Self {
        Self {
            vertices: vec![Vertex::default(); VERTICES],
            next: 0,
            last: [None; 2],
            dirty: false,
        }
    }
}

/// 0 when gripping, up to 1 for a full slide or a locked-wheel stop.
pub fn intensity(car: &sa_scene::vehicle::Car, throttle: f32, handbrake: bool) -> f32 {
    if car.burning_out() {
        return 0.9;
    }
    if car.airborne() || car.speed.abs() < 3.0 {
        return 0.0;
    }
    let slide = ((car.slip() - 2.0) / 4.0).clamp(0.0, 1.0);
    let braking = throttle * car.speed.signum() < -0.5 && car.speed.abs() > 12.0;
    let locked = (handbrake || braking) as u8 as f32 * 0.8;
    slide.max(locked)
}

impl Skids {
    pub fn update(&mut self, car: &sa_scene::vehicle::Car, intensity: f32) {
        let forward = car.forward();
        let side = Vec3::new(forward.z, 0.0, -forward.x);
        let ground = car.position - Vec3::Y * (car.clearance - 0.03);
        for (tyre, offset) in [-0.8f32, 0.8].into_iter().enumerate() {
            let point = ground - forward * 1.3 + side * offset;
            if intensity <= 0.0 {
                self.last[tyre] = None;
                continue;
            }
            let Some(last) = self.last[tyre] else {
                self.last[tyre] = Some(point);
                continue;
            };
            if last.distance(point) < STEP {
                continue;
            }
            if last.distance(point) > 4.0 {
                // Teleport or respawn; do not draw a streak across the map.
                self.last[tyre] = Some(point);
                continue;
            }
            self.push(last, point, side, intensity);
            self.last[tyre] = Some(point);
        }
    }
    fn push(&mut self, from: Vec3, to: Vec3, side: Vec3, intensity: f32) {
        let half = side * (WIDTH * 0.5);
        let color = [0.06, 0.06, 0.06, 0.25 + 0.5 * intensity];
        let corners = [
            from - half,
            from + half,
            to + half,
            from - half,
            to + half,
            to - half,
        ];
        let start = self.next * 6;
        for (vertex, corner) in self.vertices[start..start + 6].iter_mut().zip(corners) {
            *vertex = Vertex {
                position: corner.to_array(),
                uv: [0.5, 0.5],
                color,
            };
        }
        self.next = (self.next + 1) % SEGMENTS;
        self.dirty = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sa_scene::vehicle::Car;
    #[test]
    fn sliding_lays_marks_and_gripping_stops_them() {
        let mut car = Car::new(Vec3::new(0.0, 1.0, 0.0), 0.5);
        car.speed = 20.0;
        let mut skids = Skids::default();
        for step in 0..10 {
            car.position.z = step as f32 * 0.5;
            skids.update(&car, 1.0);
        }
        assert_eq!(skids.next, 18, "two tyres, nine segments each");
        assert!(skids.vertices[0].color[3] > 0.7);
        assert!((skids.vertices[0].position[1] - 0.53).abs() < 1e-4);
        skids.update(&car, 0.0);
        car.position.z += 0.5;
        skids.update(&car, 1.0);
        assert_eq!(
            skids.next, 18,
            "a new slide starts without joining the old one"
        );
        car.position.z += 50.0;
        skids.update(&car, 1.0);
        assert_eq!(skids.next, 18, "teleports are not drawn");
    }
    #[test]
    fn handbrake_at_speed_marks_but_crawling_does_not() {
        let mut car = Car::new(Vec3::ZERO, 0.5);
        car.speed = 15.0;
        assert!(intensity(&car, 0.0, true) > 0.5);
        assert_eq!(intensity(&car, 0.0, false), 0.0);
        assert!(intensity(&car, -1.0, false) > 0.5);
        car.speed = 1.0;
        assert_eq!(intensity(&car, 0.0, true), 0.0);
    }
}
