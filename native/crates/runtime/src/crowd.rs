//! Pedestrians walking the game's own footpaths (the ped nodes in
//! `nodes*.dat`): pavements, crossings and parks.
//!
//! Like traffic, walkers are kinematic. Each one strolls along the path
//! graph at its own pace, picks a new branch at every junction, stops for
//! anything right in front of it and disappears once far away. Their count
//! is capped by the same kind of setting as traffic, so weak PCs can keep it
//! low.
use super::{GpuBatch, State};
use crate::streaming::ORIGIN;
use glam::{Quat, Vec3};
use sa_scene::roads::RoadGraph;

const SPAWN_NEAR: f32 = 35.0;
const SPAWN_FAR: f32 = 90.0;
const DESPAWN: f32 = 120.0;
const SPAWN_INTERVAL: f32 = 0.4;
/// Sideways offset so people walking in opposite directions pass each other.
const KEEP_RIGHT: f32 = 0.6;
/// How close something must be in front before a walker waits.
const PERSONAL_SPACE: f32 = 1.4;

#[derive(Clone, Debug)]
pub struct Walker {
    pub id: u64,
    /// Ped catalog index this walker is drawn with.
    pub catalog: usize,
    from: u32,
    to: u32,
    along: f32,
    /// Walking pace in m/s; zero while waiting.
    pub speed: f32,
    pace: f32,
    pub feet: Vec3,
    pub yaw: f32,
    pub health: f32,
    /// Seconds since this walker died, if it has.
    pub dead_for: Option<f32>,
    /// Hit but still standing; the renderer plays a flinch and clears it.
    pub flinch: bool,
    /// Seconds left running away after gunfire nearby.
    pub panic: f32,
    /// How fast a body knocked down by a car or a blast is sliding.
    pub thrown: Vec3,
    /// Seconds before a car can hurt this walker again.
    bumped: f32,
    /// A police officer on foot. Officers are moved by the police, not
    /// along the footpaths, and do not count against the pedestrian limit.
    pub cop: bool,
}

/// Someone the player hurt, for the police to hear about.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Injury {
    pub cop: bool,
    pub killed: bool,
    pub at: Vec3,
}

/// Running pace while fleeing.
const FLEE: f32 = 4.5;
/// How long a body stays before it is cleared away.
const BODY_SECONDS: f32 = 20.0;
/// People this close to gunfire run.
const HEARING: f32 = 30.0;
/// A car slower than this in m/s only nudges people.
const RUN_OVER_SPEED: f32 = 3.0;
/// Health a walker loses per m/s of the car that hits them: about
/// 70 km/h kills.
const RUN_OVER_DAMAGE: f32 = 5.0;
/// Rough body size for hits: a vertical capsule.
const BODY_HEIGHT: f32 = 1.8;
const BODY_RADIUS: f32 = 0.4;

#[derive(Default)]
pub struct Crowd {
    pub walkers: Vec<Walker>,
    seed: u32,
    since_spawn: f32,
    next_id: u64,
    /// Hits since the police last looked.
    pub injuries: Vec<Injury>,
}

fn world(paths: &RoadGraph, node: u32) -> Vec3 {
    let [x, y, z] = paths.position(node);
    Vec3::new(x - ORIGIN[0], z, ORIGIN[1] - y)
}

fn next_random(seed: &mut u32) -> u32 {
    *seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
    *seed >> 8
}

impl Crowd {
    /// Move every walker, retire distant ones and add one up to `limit`.
    /// `blocked` are things to wait for (the player, cars). Returns the
    /// index in `walkers` of a newly added walker so the caller can give it
    /// a mesh.
    pub fn update(
        &mut self,
        paths: &RoadGraph,
        player: Vec3,
        blocked: &[Vec3],
        models: &[usize],
        limit: usize,
        dt: f32,
    ) -> Option<usize> {
        let mut kept = 0;
        self.walkers.retain(|w| {
            w.feet.distance(player) < DESPAWN
                && w.dead_for.is_none_or(|t| t < BODY_SECONDS)
                && (w.cop || {
                    kept += 1;
                    kept <= limit
                })
        });
        let positions: Vec<Vec3> = self.walkers.iter().map(|w| w.feet).collect();
        for (index, walker) in self.walkers.iter_mut().enumerate() {
            walker.bumped = (walker.bumped - dt).max(0.0);
            if let Some(dead) = &mut walker.dead_for {
                *dead += dt;
                walker.speed = 0.0;
                // A body thrown by a car or a blast slides to a stop.
                walker.feet += walker.thrown * dt;
                walker.thrown *= (-3.0 * dt).exp();
                continue;
            }
            if walker.cop {
                continue;
            }
            walker.panic = (walker.panic - dt).max(0.0);
            let forward = Vec3::new(walker.yaw.sin(), 0.0, walker.yaw.cos());
            let ahead = |point: Vec3| {
                let offset = point - walker.feet;
                let distance = offset.dot(forward);
                distance > 0.1
                    && distance < PERSONAL_SPACE
                    && (offset - forward * distance).length() < 0.6
                    && offset.y.abs() < 2.0
            };
            let waiting = blocked.iter().copied().any(ahead)
                || positions
                    .iter()
                    .enumerate()
                    .any(|(other, &p)| other != index && ahead(p));
            walker.speed = if walker.panic > 0.0 {
                FLEE
            } else if waiting {
                0.0
            } else {
                walker.pace
            };
            walker.along += walker.speed * dt;
            let mut guard = 0;
            while walker.along >= length(paths, walker.from, walker.to) && guard < 8 {
                guard += 1;
                walker.along -= length(paths, walker.from, walker.to);
                let options: Vec<u32> = paths
                    .neighbours(walker.to)
                    .iter()
                    .copied()
                    .filter(|&n| n != walker.from)
                    .collect();
                let next = if options.is_empty() {
                    walker.from
                } else {
                    options[next_random(&mut self.seed) as usize % options.len()]
                };
                walker.from = walker.to;
                walker.to = next;
            }
            place(walker, paths);
        }
        self.since_spawn += dt;
        let civilians = self.walkers.iter().filter(|w| !w.cop).count();
        if civilians >= limit || models.is_empty() || self.since_spawn < SPAWN_INTERVAL {
            return None;
        }
        self.since_spawn = 0.0;
        let at = [player.x + ORIGIN[0], ORIGIN[1] - player.z];
        let candidates = paths.nodes_between(at, SPAWN_NEAR, SPAWN_FAR);
        if candidates.is_empty() {
            return None;
        }
        let from = candidates[next_random(&mut self.seed) as usize % candidates.len()];
        let next = paths.neighbours(from);
        if next.is_empty() {
            return None;
        }
        let to = next[next_random(&mut self.seed) as usize % next.len()];
        let catalog = models[next_random(&mut self.seed) as usize % models.len()];
        // 1.1 to 1.6 m/s: most people amble, a few are in a hurry.
        let pace = 1.1 + (next_random(&mut self.seed) % 100) as f32 * 0.005;
        self.next_id += 1;
        let mut walker = Walker {
            id: self.next_id,
            catalog,
            from,
            to,
            along: 0.0,
            speed: pace,
            pace,
            feet: Vec3::ZERO,
            yaw: 0.0,
            health: 100.0,
            dead_for: None,
            flinch: false,
            panic: 0.0,
            thrown: Vec3::ZERO,
            bumped: 0.0,
            cop: false,
        };
        place(&mut walker, paths);
        if self
            .walkers
            .iter()
            .any(|other| other.feet.distance(walker.feet) < PERSONAL_SPACE)
        {
            return None;
        }
        self.walkers.push(walker);
        Some(self.walkers.len() - 1)
    }

    /// A police officer on foot at `feet`, drawn with ped catalog index
    /// `catalog`. Returns their index in `walkers`.
    pub fn add_cop(&mut self, catalog: usize, feet: Vec3, yaw: f32) -> usize {
        self.next_id += 1;
        self.walkers.push(Walker {
            id: self.next_id,
            catalog,
            from: 0,
            to: 0,
            along: 0.0,
            speed: 0.0,
            pace: 0.0,
            feet,
            yaw,
            health: 100.0,
            dead_for: None,
            flinch: false,
            panic: 0.0,
            thrown: Vec3::ZERO,
            bumped: 0.0,
            cop: true,
        });
        self.walkers.len() - 1
    }

    /// The nearest living walker a ray hits within `range`, and how far along
    /// the ray the hit is.
    pub fn first_hit(&self, origin: Vec3, direction: Vec3, range: f32) -> Option<(usize, f32)> {
        self.walkers
            .iter()
            .enumerate()
            .filter(|(_, w)| w.dead_for.is_none())
            .filter_map(|(index, w)| {
                ray_capsule(origin, direction, w.feet, BODY_HEIGHT, BODY_RADIUS)
                    .filter(|&t| t <= range)
                    .map(|t| (index, t))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
    }

    /// Apply a hit. Returns true when it killed.
    pub fn damage(&mut self, index: usize, amount: f32) -> bool {
        let Some(walker) = self.walkers.get_mut(index) else {
            return false;
        };
        if walker.dead_for.is_some() {
            return false;
        }
        walker.health -= amount;
        walker.panic = 10.0;
        let killed = walker.health <= 0.0;
        if killed {
            walker.dead_for = Some(0.0);
            walker.speed = 0.0;
        } else {
            walker.flinch = true;
        }
        self.injuries.push(Injury {
            cop: walker.cop,
            killed,
            at: walker.feet,
        });
        killed
    }

    /// A car at `speed` m/s ran into this walker, pushing along `away`.
    /// A slow nudge does nothing; faster it hurts, and from about 70 km/h
    /// it kills and throws the body. Returns true when it killed.
    pub fn run_over(&mut self, index: usize, speed: f32, away: Vec3) -> bool {
        let Some(walker) = self.walkers.get_mut(index) else {
            return false;
        };
        if speed < RUN_OVER_SPEED || walker.bumped > 0.0 || walker.dead_for.is_some() {
            return false;
        }
        walker.bumped = 1.0;
        let killed = self.damage(index, speed * RUN_OVER_DAMAGE);
        if killed {
            self.walkers[index].thrown =
                Vec3::new(away.x, 0.0, away.z).normalize_or_zero() * speed * 0.6;
        }
        killed
    }
    /// An explosion at `at`: everyone near is hurt, the closest are killed
    /// and thrown, and everyone further out runs.
    pub fn blast(&mut self, at: Vec3, radius: f32, damage: f32) {
        self.scare(at);
        for index in 0..self.walkers.len() {
            let offset = self.walkers[index].feet - at;
            let distance = offset.length();
            if distance < radius && self.damage(index, damage * (1.0 - distance / radius)) {
                self.walkers[index].thrown =
                    Vec3::new(offset.x, 0.0, offset.z).normalize_or_zero() * (radius - distance);
            }
        }
    }
    /// Everyone within earshot of a shot runs for a while; officers hold
    /// their ground.
    pub fn scare(&mut self, at: Vec3) {
        for walker in &mut self.walkers {
            if !walker.cop && walker.dead_for.is_none() && walker.feet.distance(at) < HEARING {
                walker.panic = walker.panic.max(8.0);
            }
        }
    }
}

/// Distance along a ray (unit `direction`) to a vertical capsule standing on
/// `feet`, if it hits.
fn ray_capsule(origin: Vec3, direction: Vec3, feet: Vec3, height: f32, radius: f32) -> Option<f32> {
    let a = feet + Vec3::Y * radius;
    let b = feet + Vec3::Y * (height - radius);
    let axis = b - a;
    // Closest points between the ray and the capsule's axis segment.
    let w = origin - a;
    let (dd, da, aa) = (
        direction.dot(direction),
        direction.dot(axis),
        axis.dot(axis),
    );
    let (dw, aw) = (direction.dot(w), axis.dot(w));
    let denominator = dd * aa - da * da;
    let mut s = if denominator.abs() > 1e-6 {
        ((da * aw - aa * dw) / denominator).max(0.0)
    } else {
        0.0
    };
    let mut u = ((da * s + aw) / aa).clamp(0.0, 1.0);
    s = ((u * da - dw) / dd).max(0.0);
    u = ((da * s + aw) / aa).clamp(0.0, 1.0);
    let closest = origin + direction * s;
    let distance = closest.distance(a + axis * u);
    (distance <= radius).then(|| (s - (radius * radius - distance * distance).sqrt()).max(0.0))
}

fn length(paths: &RoadGraph, from: u32, to: u32) -> f32 {
    (world(paths, to) - world(paths, from)).length().max(0.1)
}

fn place(walker: &mut Walker, paths: &RoadGraph) {
    let (a, b) = (world(paths, walker.from), world(paths, walker.to));
    let direction = (b - a).normalize_or_zero();
    let flat = Vec3::new(direction.x, 0.0, direction.z).normalize_or_zero();
    let right = Vec3::new(-flat.z, 0.0, flat.x);
    walker.feet = a + direction * walker.along + right * KEEP_RIGHT;
    if flat != Vec3::ZERO {
        walker.yaw = flat.x.atan2(flat.z);
    }
}

/// A walker's own copy of its mesh, posed on the CPU every frame.
pub(super) struct WalkerMesh {
    id: u64,
    pub batches: Vec<GpuBatch>,
    animator: crate::animation::Animator,
}

impl State {
    fn refresh_crowd_models(&mut self) {
        let civilian: std::collections::HashSet<String> =
            sa_scene::ped_list::original_peds(&self.resource_game_dir)
                .into_iter()
                .filter(|ped| matches!(ped.kind.as_str(), "CIVMALE" | "CIVFEMALE"))
                .map(|ped| ped.model)
                .collect();
        self.crowd_models = (0..self.menu.peds.len())
            .filter(|&index| {
                self.menu
                    .peds
                    .get(index)
                    .and_then(|name| self.original_ped_models.get(name))
                    .is_some_and(|model| civilian.contains(model))
            })
            .collect();
        self.crowd_models_for = self.menu.peds.len();
    }

    /// Give the walker just added at `index` its own copy of its model, or
    /// drop them when the model cannot be loaded. Returns whether they stay.
    pub(super) fn mesh_walker(&mut self, index: usize) -> bool {
        let (id, catalog) = (
            self.crowd.walkers[index].id,
            self.crowd.walkers[index].catalog,
        );
        self.ensure_ped_loaded(catalog);
        let source = if catalog == self.active_ped {
            self.ped.as_ref()
        } else {
            self.ped_catalog.get(catalog).and_then(|p| p.as_ref())
        };
        match source {
            Some((_, batches)) => {
                self.crowd_meshes.push(WalkerMesh {
                    id,
                    batches: crate::multiplayer::duplicate(&self.device, batches),
                    animator: crate::animation::Animator::civilian((id % 17) as f32 * 0.13),
                });
                true
            }
            None => {
                self.crowd.walkers.remove(index);
                false
            }
        }
    }

    pub(super) fn update_crowd(&mut self, dt: f32) {
        if self.crowd_models_for != self.menu.peds.len() {
            self.refresh_crowd_models();
        }
        if self.ped_paths.is_none() && !self.ped_paths_tried {
            self.ped_paths_tried = true;
            match RoadGraph::load_peds(&self.resource_game_dir) {
                Ok(paths) => self.ped_paths = Some(paths),
                Err(error) => eprintln!("Pedestrian paths unavailable: {error:#}"),
            }
        }
        let Some(paths) = self.ped_paths.as_ref() else {
            return;
        };
        let limit = if self.interior == 0 {
            self.menu.settings.pedestrians as usize
        } else {
            0
        };
        let player = match (&self.car, &self.player) {
            (Some((car, _)), _) if self.driving => car.position,
            (_, Some(player)) if self.walking => player.feet,
            _ => self.position,
        };
        let mut blocked = vec![player];
        blocked.extend(self.traffic.cars.iter().map(|car| car.position));
        let added = self
            .crowd
            .update(paths, player, &blocked, &self.crowd_models, limit, dt);
        let walkers = &self.crowd.walkers;
        self.crowd_meshes
            .retain(|mesh| walkers.iter().any(|w| w.id == mesh.id));
        if let Some(index) = added {
            self.mesh_walker(index);
        }
        for mesh in &mut self.crowd_meshes {
            let Some(walker) = self.crowd.walkers.iter_mut().find(|w| w.id == mesh.id) else {
                continue;
            };
            let source = if walker.catalog == self.active_ped {
                self.ped.as_ref()
            } else {
                self.ped_catalog
                    .get(walker.catalog)
                    .and_then(|p| p.as_ref())
            };
            let Some((ped, _)) = source else { continue };
            if std::mem::take(&mut walker.flinch) {
                mesh.animator
                    .overlay(&["hit_front", "hit_l", "hit_r"], |name| {
                        ped.clip_duration(name)
                    });
            }
            mesh.animator.update(
                crate::animation::Input {
                    speed: walker.speed,
                    grounded: true,
                    dead: walker.dead_for.is_some(),
                    ..Default::default()
                },
                dt,
                |name| ped.clip_duration(name),
            );
            let dying = mesh.animator.motion() == crate::animation::Motion::Dead;
            let posed = ped.blend_with_clothing(&mesh.animator.plays(), ped.clothing_mask());
            let Ok(posed) = posed else { continue };
            let mut rotation = Quat::from_rotation_y(walker.yaw + std::f32::consts::PI);
            let mut feet = walker.feet;
            if let Some(dead) = walker.dead_for.filter(|_| !dying) {
                // Without a death clip, topple backwards over half a second and lie on the ground.
                let fall = (dead / 0.5).min(1.0) * std::f32::consts::FRAC_PI_2;
                rotation *= Quat::from_rotation_x(fall);
                feet += Vec3::Y * 0.15 * (fall / std::f32::consts::FRAC_PI_2);
            }
            for (part, batch) in posed.iter().zip(&mesh.batches) {
                if part.vertices.len() * 9 != batch.base.len() {
                    continue;
                }
                let mut raw = Vec::with_capacity(part.vertices.len() * 9);
                for vertex in &part.vertices {
                    raw.extend((rotation * Vec3::from_array(vertex.position) + feet).to_array());
                    raw.extend(vertex.uv);
                    raw.extend(vertex.color);
                }
                self.queue
                    .write_buffer(&batch.buffer, 0, bytemuck::cast_slice(&raw));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A straight pavement running north with a junction halfway.
    fn pavement() -> RoadGraph {
        let o = ORIGIN;
        let mut nodes: Vec<[f32; 3]> = (0..30)
            .map(|i| [o[0] + 5.0, o[1] + i as f32 * 5.0, 10.0])
            .collect();
        nodes.push([o[0] + 20.0, o[1] + 75.0, 10.0]);
        let mut links: Vec<Vec<u32>> = (0..30u32)
            .map(|i| {
                let mut l = Vec::new();
                if i > 0 {
                    l.push(i - 1);
                }
                if i < 29 {
                    l.push(i + 1);
                }
                l
            })
            .collect();
        links[15].push(30);
        links.push(vec![15]);
        RoadGraph::from_parts(nodes, links)
    }

    #[test]
    fn walkers_appear_away_from_the_player_and_walk() {
        let paths = pavement();
        let mut crowd = Crowd::default();
        assert_eq!(crowd.update(&paths, Vec3::ZERO, &[], &[7], 4, 1.0), Some(0));
        let start = crowd.walkers[0].feet;
        assert!(start.distance(Vec3::ZERO) >= SPAWN_NEAR - 2.0);
        assert_eq!(crowd.walkers[0].catalog, 7);
        for _ in 0..20 {
            crowd.update(&paths, Vec3::ZERO, &[], &[7], 4, 0.1);
        }
        let walked = crowd.walkers[0].feet.distance(start);
        assert!((1.5..4.0).contains(&walked), "{walked}");
        assert!((start.y - 10.0).abs() < 1e-3, "feet stay on the path");
    }

    #[test]
    fn walkers_wait_for_someone_in_front() {
        let paths = pavement();
        let mut crowd = Crowd::default();
        crowd.update(&paths, Vec3::ZERO, &[], &[1], 1, 1.0);
        let w = crowd.walkers[0].clone();
        let forward = Vec3::new(w.yaw.sin(), 0.0, w.yaw.cos());
        let blocker = w.feet + forward;
        crowd.update(&paths, Vec3::ZERO, &[blocker], &[1], 1, 0.1);
        assert_eq!(crowd.walkers[0].speed, 0.0);
        assert!(crowd.walkers[0].feet.distance(w.feet) < 1e-3);
    }

    #[test]
    fn shots_hit_the_nearest_walker_and_kill_after_enough_damage() {
        let paths = pavement();
        let mut crowd = Crowd::default();
        crowd.update(&paths, Vec3::ZERO, &[], &[1], 1, 1.0);
        let target = crowd.walkers[0].feet;
        let origin = target + Vec3::new(-10.0, 1.5, 0.0);
        let aim = (target + Vec3::Y * 1.2 - origin).normalize();
        let (index, distance) = crowd.first_hit(origin, aim, 50.0).unwrap();
        assert_eq!(index, 0);
        assert!((9.0..10.0).contains(&distance), "{distance}");
        assert!(crowd.first_hit(origin, aim, 5.0).is_none(), "out of range");
        assert!(crowd.first_hit(origin, -aim, 50.0).is_none(), "behind");
        assert!(!crowd.damage(0, 60.0));
        assert!(crowd.walkers[0].panic > 0.0);
        assert!(crowd.damage(0, 60.0));
        assert!(
            crowd.first_hit(origin, aim, 50.0).is_none(),
            "bodies are not hit again"
        );
        let body = crowd.walkers[0].feet;
        crowd.update(&paths, Vec3::ZERO, &[], &[1], 1, 0.5);
        assert_eq!(crowd.walkers[0].feet, body, "the dead do not walk");
    }

    #[test]
    fn a_fast_car_kills_and_throws_but_a_slow_one_only_nudges() {
        let paths = pavement();
        let mut crowd = Crowd::default();
        crowd.update(&paths, Vec3::ZERO, &[], &[1], 1, 1.0);
        assert!(!crowd.run_over(0, 2.0, Vec3::X));
        assert_eq!(crowd.walkers[0].health, 100.0, "a nudge does no harm");
        assert!(!crowd.run_over(0, 8.0, Vec3::X), "hurt, not killed");
        assert_eq!(crowd.walkers[0].health, 60.0);
        assert!(!crowd.run_over(0, 30.0, Vec3::X), "not hit twice at once");
        crowd.update(&paths, Vec3::ZERO, &[], &[1], 1, 1.1);
        assert!(crowd.run_over(0, 25.0, Vec3::X));
        let body = crowd.walkers[0].feet;
        crowd.update(&paths, Vec3::ZERO, &[], &[1], 1, 0.5);
        let slid = crowd.walkers[0].feet - body;
        assert!(
            slid.x > 1.0 && slid.z.abs() < 1e-4,
            "thrown along the hit: {slid}"
        );
    }

    #[test]
    fn a_blast_kills_close_by_and_scares_further_out() {
        let paths = pavement();
        let mut crowd = Crowd::default();
        crowd.update(&paths, Vec3::ZERO, &[], &[1], 1, 1.0);
        let at = crowd.walkers[0].feet;
        crowd.blast(at + Vec3::X * 20.0, 9.0, 250.0);
        assert!(crowd.walkers[0].dead_for.is_none());
        assert!(crowd.walkers[0].panic > 0.0);
        crowd.blast(at + Vec3::X * 2.0, 9.0, 250.0);
        assert!(crowd.walkers[0].dead_for.is_some());
        assert!(
            crowd.walkers[0].thrown.x < 0.0,
            "thrown away from the blast"
        );
    }

    #[test]
    fn gunfire_makes_people_run() {
        let paths = pavement();
        let mut crowd = Crowd::default();
        crowd.update(&paths, Vec3::ZERO, &[], &[1], 1, 1.0);
        let at = crowd.walkers[0].feet;
        crowd.scare(at + Vec3::X * 5.0);
        crowd.update(&paths, Vec3::ZERO, &[], &[1], 1, 0.1);
        assert_eq!(crowd.walkers[0].speed, FLEE);
    }

    #[test]
    fn officers_stand_where_the_police_put_them_and_are_not_counted() {
        let paths = pavement();
        let mut crowd = Crowd::default();
        let at = Vec3::new(3.0, 10.0, -3.0);
        let index = crowd.add_cop(4, at, 0.0);
        for _ in 0..50 {
            crowd.update(&paths, Vec3::ZERO, &[], &[1], 0, 0.1);
        }
        assert_eq!(crowd.walkers.len(), 1, "kept with pedestrians turned off");
        assert_eq!(
            crowd.walkers[index].feet, at,
            "the footpaths do not move them"
        );
        crowd.scare(at);
        assert_eq!(crowd.walkers[index].panic, 0.0);
        assert!(!crowd.damage(index, 30.0));
        assert!(crowd.damage(index, 80.0));
        assert_eq!(
            crowd.injuries,
            [
                Injury {
                    cop: true,
                    killed: false,
                    at
                },
                Injury {
                    cop: true,
                    killed: true,
                    at
                },
            ]
        );
    }

    #[test]
    fn limit_and_distance_retire_walkers() {
        let paths = pavement();
        let mut crowd = Crowd::default();
        for _ in 0..100 {
            crowd.update(&paths, Vec3::ZERO, &[], &[1, 2], 6, 0.1);
        }
        assert!(!crowd.walkers.is_empty() && crowd.walkers.len() <= 6);
        crowd.update(&paths, Vec3::ZERO, &[], &[1], 2, 0.1);
        assert!(crowd.walkers.len() <= 2);
        crowd.update(&paths, Vec3::splat(5000.0), &[], &[], 6, 0.1);
        assert!(crowd.walkers.is_empty());
    }
}
