//! Player collision over original COL geometry and visual fallback meshes.
use crate::Batch;
use glam::Vec3;
use std::collections::{HashMap, HashSet};

const CELL: f32 = 8.0;
const RADIUS: f32 = 0.32;
const HEIGHT: f32 = 1.75;
const STEP_HEIGHT: f32 = 0.38;

#[derive(Clone, Copy)]
struct Triangle {
    a: Vec3,
    b: Vec3,
    c: Vec3,
    normal: Vec3,
}

pub struct CollisionWorld {
    triangles: Vec<Triangle>,
    cells: HashMap<(i32, i32), Vec<usize>>,
}
impl CollisionWorld {
    pub fn from_batches(batches: &[Batch]) -> Self {
        let mut triangles = Vec::new();
        let mut cells: HashMap<(i32, i32), Vec<usize>> = HashMap::new();
        for batch in batches {
            if batch.alpha {
                continue;
            }
            for v in batch.vertices.as_chunks::<3>().0.iter() {
                let (a, b, c) = (
                    Vec3::from_array(v[0].position),
                    Vec3::from_array(v[1].position),
                    Vec3::from_array(v[2].position),
                );
                let cross = (b - a).cross(c - a);
                if cross.length_squared() < 1e-8 {
                    continue;
                }
                let normal = cross.normalize();
                let min = a.min(b).min(c);
                let max = a.max(b).max(c);
                let (x0, x1, z0, z1) = (
                    (min.x / CELL).floor() as i32,
                    (max.x / CELL).floor() as i32,
                    (min.z / CELL).floor() as i32,
                    (max.z / CELL).floor() as i32,
                );
                if (x1 - x0 + 1) as i64 * (z1 - z0 + 1) as i64 > 4096 {
                    continue;
                }
                let index = triangles.len();
                triangles.push(Triangle { a, b, c, normal });
                for x in x0..=x1 {
                    for z in z0..=z1 {
                        cells.entry((x, z)).or_default().push(index);
                    }
                }
            }
        }
        Self { triangles, cells }
    }
    pub fn triangle_count(&self) -> usize {
        self.triangles.len()
    }
    /// Pull a chase camera in front of geometry, including terrain and roofs.
    /// Rim rays give the camera clearance rather than testing one thin ray.
    pub fn clip_camera(&self, target: Vec3, desired: Vec3) -> Vec3 {
        let segment = desired - target;
        let length = segment.length();
        if length < 0.001 {
            return desired;
        }
        let steps = (length / (CELL * 0.5)).ceil() as usize;
        let mut candidates = HashSet::new();
        for step in 0..=steps {
            candidates.extend(self.nearby(target + segment * (step as f32 / steps as f32), 0.3));
        }
        let right = segment.cross(Vec3::Y).normalize_or_zero() * 0.2;
        let up = right.cross(segment.normalize()).normalize_or_zero() * 0.2;
        let mut fraction = 1.0f32;
        for offset in [Vec3::ZERO, right, -right, up, -up] {
            let origin = target + offset;
            for &index in &candidates {
                let t = self.triangles[index];
                let ab = t.b - t.a;
                let ac = t.c - t.a;
                let p = segment.cross(ac);
                let determinant = ab.dot(p);
                if determinant.abs() < 1e-7 {
                    continue;
                }
                let inverse = determinant.recip();
                let relative = origin - t.a;
                let u = relative.dot(p) * inverse;
                let q = relative.cross(ab);
                let v = segment.dot(q) * inverse;
                let hit = ac.dot(q) * inverse;
                if u >= 0.0 && v >= 0.0 && u + v <= 1.0 && (0.0..=1.0).contains(&hit) {
                    fraction = fraction.min((hit - 0.25 / length).max(0.0));
                }
            }
        }
        target + segment * fraction
    }
    /// A grounded player placement with full body and head clearance.
    pub fn standing_at(&self, position: Vec3, max_ground: f32) -> Option<Player> {
        let ground = self.ground_below(position, max_ground)?;
        if ground < max_ground - 3.0 {
            return None;
        }
        let feet = Vec3::new(position.x, ground, position.z);
        if self.resolve_walls(feet).distance_squared(feet) > 0.0001
            || self
                .ceiling_above(feet, ground + 0.01)
                .is_some_and(|y| y < ground + HEIGHT)
        {
            return None;
        }
        Some(Player {
            feet,
            vertical_speed: 0.0,
            grounded: true,
        })
    }
    fn nearby(&self, pos: Vec3, r: f32) -> Vec<usize> {
        let (x0, x1, z0, z1) = (
            ((pos.x - r) / CELL).floor() as i32,
            ((pos.x + r) / CELL).floor() as i32,
            ((pos.z - r) / CELL).floor() as i32,
            ((pos.z + r) / CELL).floor() as i32,
        );
        let mut seen = HashSet::new();
        for x in x0..=x1 {
            for z in z0..=z1 {
                if let Some(indices) = self.cells.get(&(x, z)) {
                    seen.extend(indices);
                }
            }
        }
        seen.into_iter().collect()
    }
    pub fn ground_below(&self, pos: Vec3, max_y: f32) -> Option<f32> {
        let mut best = None;
        for i in self.nearby(pos, RADIUS) {
            let t = self.triangles[i];
            if t.normal.y.abs() < 0.55 {
                continue;
            }
            // Barycentric test on the XZ projection, independent of winding.
            let x1 = t.b.x - t.a.x;
            let z1 = t.b.z - t.a.z;
            let x2 = t.c.x - t.a.x;
            let z2 = t.c.z - t.a.z;
            let det = x1 * z2 - x2 * z1;
            if det.abs() < 1e-6 {
                continue;
            }
            let x = pos.x - t.a.x;
            let z = pos.z - t.a.z;
            let u = (x * z2 - x2 * z) / det;
            let v = (x1 * z - x * z1) / det;
            if u < -0.001 || v < -0.001 || u + v > 1.001 {
                continue;
            }
            let y = t.a.y + u * (t.b.y - t.a.y) + v * (t.c.y - t.a.y);
            if y <= max_y + 0.001 && best.is_none_or(|old| y > old) {
                best = Some(y);
            }
        }
        best
    }
    /// Lowest surface crossed by the player's head during upward movement.
    /// Samples include the body rim so a partial overhang can block a jump.
    pub fn ceiling_above(&self, pos: Vec3, min_y: f32) -> Option<f32> {
        let mut best = None;
        let candidates = self.nearby(pos, RADIUS);
        for offset in [
            Vec3::ZERO,
            Vec3::X * RADIUS,
            -Vec3::X * RADIUS,
            Vec3::Z * RADIUS,
            -Vec3::Z * RADIUS,
        ] {
            let sample = pos + offset;
            for &i in &candidates {
                let t = self.triangles[i];
                if t.normal.y.abs() < 0.1 {
                    continue;
                }
                let ab = t.b - t.a;
                let ac = t.c - t.a;
                let det = ab.x * ac.z - ac.x * ab.z;
                if det.abs() < 1e-6 {
                    continue;
                }
                let p = sample - t.a;
                let u = (p.x * ac.z - ac.x * p.z) / det;
                let v = (ab.x * p.z - p.x * ab.z) / det;
                if u < -0.001 || v < -0.001 || u + v > 1.001 {
                    continue;
                }
                let y = t.a.y + u * ab.y + v * ac.y;
                if y >= min_y - 0.001 && best.is_none_or(|old| y < old) {
                    best = Some(y);
                }
            }
        }
        best
    }
    fn closest(p: Vec3, t: Triangle) -> Vec3 {
        let ab = t.b - t.a;
        let ac = t.c - t.a;
        let ap = p - t.a;
        let d1 = ab.dot(ap);
        let d2 = ac.dot(ap);
        if d1 <= 0.0 && d2 <= 0.0 {
            return t.a;
        }
        let bp = p - t.b;
        let d3 = ab.dot(bp);
        let d4 = ac.dot(bp);
        if d3 >= 0.0 && d4 <= d3 {
            return t.b;
        }
        let vc = d1 * d4 - d3 * d2;
        if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
            return t.a + ab * (d1 / (d1 - d3));
        }
        let cp = p - t.c;
        let d5 = ab.dot(cp);
        let d6 = ac.dot(cp);
        if d6 >= 0.0 && d5 <= d6 {
            return t.c;
        }
        let vb = d5 * d2 - d1 * d6;
        if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
            return t.a + ac * (d2 / (d2 - d6));
        }
        let va = d3 * d6 - d5 * d4;
        if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
            return t.b + (t.c - t.b) * ((d4 - d3) / ((d4 - d3) + (d5 - d6)));
        }
        let denom = 1.0 / (va + vb + vc);
        t.a + ab * (vb * denom) + ac * (vc * denom)
    }
    pub fn resolve_walls(&self, mut feet: Vec3) -> Vec3 {
        feet = self.resolve_body(feet, RADIUS, HEIGHT);
        feet
    }
    pub fn resolve_body(&self, mut feet: Vec3, radius: f32, height: f32) -> Vec3 {
        for _ in 0..3 {
            let mut correction = Vec3::ZERO;
            for i in self.nearby(feet, radius + 0.5) {
                let t = self.triangles[i];
                if t.normal.y.abs() > 0.7 || t.a.y.max(t.b.y).max(t.c.y) <= feet.y + 0.01 {
                    continue;
                }
                for height in [0.25, height * 0.5, height - 0.25] {
                    let sample = feet + Vec3::Y * height;
                    let nearest = Self::closest(sample, t);
                    let horizontal = Vec3::new(sample.x - nearest.x, 0.0, sample.z - nearest.z);
                    let d = horizontal.length();
                    if d < radius && ((sample.y - nearest.y).abs() < 0.4) {
                        let direction = if d > 1e-4 {
                            horizontal / d
                        } else {
                            Vec3::new(t.normal.x, 0.0, t.normal.z).normalize_or_zero()
                        };
                        let push = direction * (radius - d + 0.005);
                        if push.length_squared() > correction.length_squared() {
                            correction = push;
                        }
                    }
                }
            }
            if correction.length_squared() < 1e-8 {
                break;
            }
            feet += correction;
        }
        feet
    }
}

pub struct Player {
    pub feet: Vec3,
    pub vertical_speed: f32,
    pub grounded: bool,
}
impl Player {
    /// Interior portals have no exterior floor behind them. Keep horizontal
    /// movement on supported room geometry until actual portal travel exists.
    pub fn step_in_room(&mut self, world: &CollisionWorld, horizontal: Vec3, jump: bool, dt: f32) {
        let previous = self.feet;
        self.step(world, horizontal, jump, dt);
        if world
            .ground_below(self.feet, previous.y + STEP_HEIGHT)
            .is_none_or(|floor| floor < previous.y - 3.0)
        {
            self.feet.x = previous.x;
            self.feet.z = previous.z;
            if self.vertical_speed <= 0.0 {
                if let Some(floor) = world.ground_below(self.feet, previous.y + STEP_HEIGHT) {
                    if self.feet.y <= floor + STEP_HEIGHT {
                        self.feet.y = floor;
                        self.vertical_speed = 0.0;
                        self.grounded = true;
                    }
                }
            }
        }
    }
    pub fn step_in_water(
        &mut self,
        world: &CollisionWorld,
        water: &crate::water::WaterMap,
        origin: [f32; 2],
        horizontal: Vec3,
        jump: bool,
        dt: f32,
    ) {
        if dt <= 0.0 {
            return;
        }
        let level = water.level_at([origin[0] + self.feet.x, origin[1] - self.feet.z]);
        let swimming = level.is_some_and(|y| self.feet.y <= y - 1.1) && !self.grounded;
        if swimming && jump {
            self.vertical_speed = 6.5;
        }
        self.step(
            world,
            horizontal * if swimming { 0.55 } else { 1.0 },
            jump,
            dt,
        );
        if let Some(level) = water.level_at([origin[0] + self.feet.x, origin[1] - self.feet.z]) {
            if self.feet.y < level - 1.2
                && world
                    .ground_below(self.feet, level)
                    .is_none_or(|ground| ground < level - 1.2)
            {
                self.feet.y = level - 1.2;
                self.vertical_speed = 0.0;
                self.grounded = false;
            }
        }
    }
    pub fn spawn(world: &CollisionWorld, eye: Vec3) -> Self {
        let ground = world.ground_below(eye, eye.y - 0.1).unwrap_or(eye.y - 1.6);
        Self {
            feet: Vec3::new(eye.x, ground, eye.z),
            vertical_speed: 0.0,
            grounded: true,
        }
    }
    pub fn eye(&self) -> Vec3 {
        self.feet + Vec3::Y * 1.6
    }
    pub fn step(&mut self, world: &CollisionWorld, horizontal: Vec3, jump: bool, dt: f32) {
        let dt = dt.clamp(0.0, 0.05);
        if dt == 0.0 {
            return;
        }
        if jump && self.grounded {
            self.vertical_speed = 5.2;
            self.grounded = false;
        }
        let old_y = self.feet.y;
        let movement = Vec3::new(horizontal.x, 0.0, horizontal.z) * dt;
        let substeps = if movement.length_squared() < 1e-10 {
            0
        } else {
            ((movement.length() / 0.1).ceil() as usize).clamp(1, 8)
        };
        for _ in 0..substeps {
            let mut candidate = self.feet + movement / substeps as f32;
            // Climb short ledges before wall resolution, keeping the whole body
            // clear of the ceiling. Airborne movement does not auto-step.
            if self.grounded && self.vertical_speed <= 0.0 {
                let leading_edge = candidate + movement.normalize_or_zero() * (RADIUS + 0.02);
                let ground = world
                    .ground_below(candidate, old_y + STEP_HEIGHT)
                    .into_iter()
                    .chain(world.ground_below(leading_edge, old_y + STEP_HEIGHT))
                    .reduce(f32::max);
                if let Some(ground) = ground {
                    if ground > candidate.y {
                        candidate.y = ground;
                    }
                }
            }
            candidate = world.resolve_walls(candidate);
            if world
                .ceiling_above(candidate, candidate.y + 0.01)
                .is_none_or(|roof| roof >= candidate.y + HEIGHT - 0.001)
            {
                self.feet = candidate;
            }
        }
        self.vertical_speed -= 17.0 * dt;
        self.feet.y += self.vertical_speed * dt;
        if self.vertical_speed > 0.0 {
            if let Some(roof) = world.ceiling_above(self.feet, old_y + HEIGHT) {
                if self.feet.y + HEIGHT >= roof {
                    self.feet.y = roof - HEIGHT;
                    self.vertical_speed = 0.0;
                }
            }
        }
        let ceiling = if self.vertical_speed <= 0.0 {
            old_y + STEP_HEIGHT
        } else {
            self.feet.y
        };
        if let Some(y) = world.ground_below(self.feet, ceiling) {
            if self.vertical_speed <= 0.0 && self.feet.y <= y + STEP_HEIGHT {
                self.feet.y = y;
                self.vertical_speed = 0.0;
                self.grounded = true;
            } else {
                self.grounded = false;
            }
        } else {
            self.grounded = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Vertex;
    fn batch(points: &[[f32; 3]]) -> Batch {
        Batch {
            key: "test".into(),
            alpha: false,
            animated: false,
            vertices: points
                .iter()
                .map(|p| Vertex {
                    position: *p,
                    ..Default::default()
                })
                .collect(),
        }
    }
    #[test]
    fn floor_and_wall() {
        let world = CollisionWorld::from_batches(&[
            batch(&[
                [-5.0, 0.0, -5.0],
                [5.0, 0.0, -5.0],
                [5.0, 0.0, 5.0],
                [-5.0, 0.0, -5.0],
                [5.0, 0.0, 5.0],
                [-5.0, 0.0, 5.0],
            ]),
            batch(&[
                [1.0, 0.0, -2.0],
                [1.0, 2.0, -2.0],
                [1.0, 2.0, 2.0],
                [1.0, 0.0, -2.0],
                [1.0, 2.0, 2.0],
                [1.0, 0.0, 2.0],
            ]),
        ]);
        assert_eq!(world.ground_below(Vec3::ZERO, 1.0), Some(0.0));
        let mut player = Player::spawn(&world, Vec3::new(0.0, 1.6, 0.0));
        for _ in 0..60 {
            player.step(&world, Vec3::X * 4.0, false, 1.0 / 60.0);
        }
        assert!(player.feet.x < 0.75);
        assert!(player.grounded);
        let mut fast = Player::spawn(&world, Vec3::new(0.0, 1.6, 0.0));
        fast.step(&world, Vec3::X * 20.0, false, 0.05);
        assert!(fast.feet.x < 0.75);
        player.step(&world, Vec3::ZERO, true, 1.0 / 60.0);
        assert!(player.feet.y > 0.0);
    }
    #[test]
    fn interior_open_portal_keeps_player_on_floor_and_allows_jumping() {
        let world = CollisionWorld::from_batches(&[batch(&[
            [-2.0, 1000.0, -2.0],
            [2.0, 1000.0, -2.0],
            [2.0, 1000.0, 2.0],
            [-2.0, 1000.0, -2.0],
            [2.0, 1000.0, 2.0],
            [-2.0, 1000.0, 2.0],
        ])]);
        let mut player = Player::spawn(&world, Vec3::new(0.0, 1001.6, 0.0));
        let mut jumped = false;
        for frame in 0..240 {
            player.step_in_room(&world, Vec3::X * 9.0, frame == 50, 1.0 / 60.0);
            assert!(player.feet.x <= 2.0 && player.feet.y >= 1000.0);
            jumped |= player.feet.y > 1000.1;
        }
        assert!(jumped && player.grounded);
        assert!(player.feet.x > 1.8);
    }
    #[test]
    fn jumping_under_a_low_ceiling_stops_the_head_and_lands() {
        let floor = batch(&[
            [-5.0, 0.0, -5.0],
            [5.0, 0.0, -5.0],
            [5.0, 0.0, 5.0],
            [-5.0, 0.0, -5.0],
            [5.0, 0.0, 5.0],
            [-5.0, 0.0, 5.0],
        ]);
        let roof = batch(&[
            [-5.0, 2.0, -5.0],
            [5.0, 2.0, -5.0],
            [5.0, 2.0, 5.0],
            [-5.0, 2.0, -5.0],
            [5.0, 2.0, 5.0],
            [-5.0, 2.0, 5.0],
        ]);
        let world = CollisionWorld::from_batches(&[floor, roof]);
        let mut player = Player::spawn(&world, Vec3::new(0.0, 1.6, 0.0));
        player.step(&world, Vec3::ZERO, true, 0.05);
        let mut peak = player.feet.y;
        for _ in 0..120 {
            player.step(&world, Vec3::ZERO, false, 1.0 / 60.0);
            peak = peak.max(player.feet.y);
            assert!(player.feet.y + HEIGHT <= 2.001);
        }
        assert!(peak > 0.0);
        assert!(player.grounded);
        assert!(player.feet.y.abs() < 0.001);
        player.step(&world, Vec3::ZERO, true, 0.0);
        assert!(player.grounded);
        assert_eq!(player.vertical_speed, 0.0);
    }
    fn floor_patch(x0: f32, x1: f32, y: f32) -> Batch {
        batch(&[
            [x0, y, -5.0],
            [x1, y, -5.0],
            [x1, y, 5.0],
            [x0, y, -5.0],
            [x1, y, 5.0],
            [x0, y, 5.0],
        ])
    }
    #[test]
    fn chase_camera_stays_in_front_of_walls_roofs_and_terrain() {
        let wall = batch(&[
            [10.0, 0.0, -5.0],
            [10.0, 5.0, -5.0],
            [10.0, 5.0, 5.0],
            [10.0, 0.0, -5.0],
            [10.0, 5.0, 5.0],
            [10.0, 0.0, 5.0],
        ]);
        let world = CollisionWorld::from_batches(&[
            floor_patch(-20.0, 20.0, 0.0),
            wall,
            floor_patch(-20.0, 20.0, 4.0),
        ]);
        let target = Vec3::new(0.0, 2.0, 0.0);
        let camera = world.clip_camera(target, Vec3::new(18.0, 2.0, 0.0));
        assert!(camera.x > 9.0 && camera.x < 9.8);
        let reverse = world.clip_camera(Vec3::new(18.0, 2.0, 0.0), target);
        assert!(reverse.x > 10.2 && reverse.x < 11.0);
        assert!(world.clip_camera(target, Vec3::new(0.0, -3.0, 0.0)).y > 0.2);
        assert!(world.clip_camera(target, Vec3::new(0.0, 7.0, 0.0)).y < 3.8);
        assert_eq!(world.clip_camera(target, target), target);
        let clear = Vec3::new(0.0, 2.0, 3.0);
        assert_eq!(world.clip_camera(target, clear), clear);
    }
    #[test]
    fn vehicle_exit_uses_clear_side_and_rejects_missing_floor_or_low_roof() {
        let wall = batch(&[
            [1.0, 0.0, -5.0],
            [1.0, 4.0, -5.0],
            [1.0, 4.0, 5.0],
            [1.0, 0.0, -5.0],
            [1.0, 4.0, 5.0],
            [1.0, 0.0, 5.0],
        ]);
        let world = CollisionWorld::from_batches(&[floor_patch(-5.0, 5.0, 0.0), wall]);
        let car = crate::vehicle::Car::new(Vec3::Y * 0.6, 0.6);
        let player = car.exit_player(&world).unwrap();
        assert!(player.feet.x < -2.0 && player.grounded);
        assert_eq!(player.feet.y, 0.0);
        let empty = CollisionWorld::from_batches(&[]);
        assert!(car.exit_player(&empty).is_none());
        let roof = CollisionWorld::from_batches(&[
            floor_patch(-5.0, 5.0, 0.0),
            floor_patch(-5.0, 5.0, 1.5),
        ]);
        assert!(car.exit_player(&roof).is_none());
        assert!(world.standing_at(Vec3::new(1.0, 0.0, 0.0), 0.6).is_none());
    }
    #[test]
    fn low_overhang_blocks_entry_but_full_height_doorway_allows_it() {
        for (height, should_pass) in [(1.5, false), (2.2, true)] {
            let world = CollisionWorld::from_batches(&[
                floor_patch(-5.0, 5.0, 0.0),
                floor_patch(1.0, 5.0, height),
            ]);
            let mut player = Player::spawn(&world, Vec3::new(0.0, 1.6, 0.0));
            for _ in 0..60 {
                player.step(&world, Vec3::X * 4.0, false, 1.0 / 60.0);
            }
            assert!(player.grounded);
            if should_pass {
                assert!(player.feet.x > 3.0);
            } else {
                assert!(player.feet.x + RADIUS < 1.001);
            }
        }
    }
    #[test]
    fn small_step_is_walkable_but_step_under_low_roof_is_blocked() {
        for (height, low_roof) in [(0.3, false), (0.3, true), (0.6, false)] {
            let mut geometry = vec![
                floor_patch(-5.0, 1.0, 0.0),
                floor_patch(1.0, 5.0, height),
                batch(&[
                    [1.0, 0.0, -5.0],
                    [1.0, height, -5.0],
                    [1.0, height, 5.0],
                    [1.0, 0.0, -5.0],
                    [1.0, height, 5.0],
                    [1.0, 0.0, 5.0],
                ]),
            ];
            if low_roof {
                geometry.push(floor_patch(1.0, 5.0, 1.9));
            }
            let world = CollisionWorld::from_batches(&geometry);
            let mut player = Player::spawn(&world, Vec3::new(0.0, 1.6, 0.0));
            for _ in 0..60 {
                player.step(&world, Vec3::X * 4.0, false, 1.0 / 60.0);
            }
            assert!(player.grounded);
            if low_roof || height > STEP_HEIGHT {
                assert!(player.feet.x < 1.0);
                assert!(player.feet.y.abs() < 0.001);
            } else {
                assert!(player.feet.x > 3.0, "step stopped at {:?}", player.feet);
                assert!((player.feet.y - 0.3).abs() < 0.001);
            }
        }
    }
}
