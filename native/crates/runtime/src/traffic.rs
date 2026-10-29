//! Ambient city traffic along the game's own car paths (`nodes*.dat`).
//!
//! Traffic cars do not run the full vehicle physics: each one steers along
//! its lane of the road graph towards a point a little ahead, so
//! it rounds corners in smooth arcs, sits on the real road surface, leans
//! into turns and dips under braking. Every driver has their own pace, slows
//! for turns and stops behind anything in front of it. Cars appear out of the
//! player's immediate surroundings and vanish once far behind, so only a
//! handful exist at a time. That keeps the cost low on weak PCs.
use super::{GpuBatch, State};
use crate::car_contacts::Footprint;
use crate::streaming::ORIGIN;
use glam::Vec3;
use sa_scene::roads::RoadGraph;

/// Half the width of a lane, as in SA (5.4 m lanes): the middle of the
/// first right-hand lane of a two-way road is this far off the centre line.
const LANE: f32 = 2.7;
const CRUISE: f32 = 13.0;
/// Tightest turn a traffic car takes, in metres.
const TURN_RADIUS: f32 = 4.5;
const ACCELERATION: f32 = 3.5;
const BRAKING: f32 = 9.0;
/// Cars appear this far from the player...
const SPAWN_NEAR: f32 = 70.0;
const SPAWN_FAR: f32 = 150.0;
/// ...and vanish beyond this.
const DESPAWN: f32 = 200.0;
/// Space kept to whatever is ahead, in metres, between centres at spawn and
/// from the front bumper while driving.
const GAP: f32 = 8.0;
const LOOK_AHEAD: f32 = 3.5;
/// Cars inside this radius of a junction node keep it to themselves.
const JUNCTION: f32 = 7.0;
/// After waiting this long at a junction a driver goes anyway, which breaks
/// rare circles of cars each waiting on the next.
const PATIENCE: f32 = 15.0;
/// How long a car waits after the player knocks into it.
const KNOCK_HOLD: f32 = 2.5;
/// Seconds a burnt-out wreck stays before it is towed away.
const WRECK_SECONDS: f32 = 45.0;
const SPAWN_INTERVAL: f32 = 0.6;
/// One spawned car in this many is a police patrol, where the area has one.
const PATROL_ODDS: u32 = 10;
/// Top speed of a police car chasing someone.
const CHASE_CRUISE: f32 = 26.0;
/// A chasing car leaves the road and heads straight for the suspect inside
/// this distance...
const CLOSE_IN: f32 = 40.0;
/// ...and goes back to the roads once the suspect has got this far away.
const FALL_BACK: f32 = 80.0;
/// A chasing car pulls up this far from the suspect.
const PULL_UP: f32 = 7.0;
/// Police cars come from roads this far from the suspect.
const PURSUIT_NEAR: f32 = 70.0;
const PURSUIT_FAR: f32 = 130.0;
/// A police car left parked off the road is cleared away beyond this.
const PARKED_DESPAWN: f32 = 90.0;
/// Roads this close to another player who runs traffic are theirs to fill.
pub const TAKEN_RADIUS: f32 = 150.0;

/// What a client's traffic must know about the world around it.
pub struct Surroundings<'a, G> {
    /// Where the local player is: traffic lives around them.
    pub player: Vec3,
    /// Other things on the road to stop for (the player, peers, parked cars,
    /// other players' traffic that has right of way).
    pub blocked: &'a [Footprint],
    /// Height of the road surface below a point, no higher than a limit.
    pub ground: G,
    /// Players whose own traffic fills the roads around them, so no new
    /// cars are added there.
    pub taken: &'a [Vec3],
}

#[derive(Clone, Debug)]
pub struct TrafficCar {
    /// Stable while the car exists, so the caller can keep its mesh.
    pub id: u64,
    /// Car catalog index this car is drawn with.
    pub catalog: usize,
    from: u32,
    to: u32,
    /// The link after this one, picked early so the car can turn into it.
    next: u32,
    /// Which lane the driver keeps to, counted from the inside lane; on a
    /// road with fewer lanes the car takes the outermost one there is.
    lane: u8,
    /// No legal way on (the end of a one-way road): retired next update.
    stranded: bool,
    /// Metres travelled along the current link.
    along: f32,
    pub speed: f32,
    /// This driver's own cruising speed.
    cruise: f32,
    /// World position of the car's centre on the road, and its heading.
    pub position: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    pub roll: f32,
    /// Half width, half length and height, from the model once it has a mesh.
    pub size: [f32; 3],
    /// Seconds left standing still after a knock.
    hold: f32,
    /// Seconds spent waiting for other cars.
    waited: f32,
    /// The junction this car has taken its turn at.
    committed: Option<u32>,
    /// Heading towards the point the driver steers for.
    aim_yaw: f32,
    pub damage: sa_scene::vehicle::Damage,
    /// Metres driven, for turning the wheels.
    travel: f32,
    /// Seconds since this car blew up.
    pub burnt_for: f32,
    /// Set for police cars.
    pub police: Option<Patrol>,
}

/// What a police car is up to.
#[derive(Clone, Debug, Default)]
pub struct Patrol {
    /// After a wanted player, with siren and lights on.
    pub chasing: bool,
    /// Left the road to drive straight at the suspect; it keeps to no lane
    /// until it has found its way back to a road node.
    pub off_road: bool,
    /// Officers still sitting in the car, and out of it on foot.
    pub crew: u8,
    pub out: u8,
    /// The road node an off-road car is heading back to.
    rejoin: Option<u32>,
    /// Seconds the car has stood still while chasing.
    pub stopped_for: f32,
    /// Seconds left backing up to get round something in the way.
    reversing: f32,
}

impl TrafficCar {
    /// A car at the start of the link `from` -> `to`, turning into `next` after.
    fn new(id: u64, catalog: usize, [from, to, next]: [u32; 3], lane: u8, cruise: f32) -> Self {
        Self {
            id,
            catalog,
            from,
            to,
            next,
            lane,
            stranded: false,
            along: 0.0,
            speed: cruise * 0.6,
            cruise,
            position: Vec3::ZERO,
            yaw: 0.0,
            pitch: 0.0,
            roll: 0.0,
            size: sa_scene::vehicle::DEFAULT_SIZE,
            hold: 0.0,
            waited: 0.0,
            committed: None,
            aim_yaw: 0.0,
            damage: Default::default(),
            travel: 0.0,
            burnt_for: 0.0,
            police: None,
        }
    }
    pub fn chasing(&self) -> bool {
        self.police.as_ref().is_some_and(|p| p.chasing)
    }
    fn off_road(&self) -> bool {
        self.police.as_ref().is_some_and(|p| p.off_road)
    }
    /// Siren and lights on.
    pub fn siren(&self) -> bool {
        self.chasing() && !self.damage.wrecked
    }
    fn footprint(&self) -> Footprint {
        Footprint::car(self.position, self.yaw, self.size)
    }
    /// The road just in front of the bumper this car must find clear,
    /// pointing where the driver is steering, so a turning car looks into
    /// its turn rather than at whoever waits across the junction.
    fn path_ahead(&self) -> Footprint {
        let look = LOOK_AHEAD + self.speed * 0.6;
        let toward = Vec3::new(self.aim_yaw.sin(), 0.0, self.aim_yaw.cos());
        Footprint {
            center: self.position + self.forward() * self.size[1] + toward * (look / 2.0),
            yaw: self.aim_yaw,
            half: glam::Vec2::new(self.size[0], look / 2.0),
            ..self.footprint()
        }
    }
    fn forward(&self) -> Vec3 {
        Vec3::new(self.yaw.sin(), 0.0, self.yaw.cos())
    }
    /// The player ran into this car: a hard enough hit makes the driver stop
    /// for a moment, as SA drivers do, and dents the side that was hit.
    /// `towards` points from this car to the one that hit it.
    pub fn knock(&mut self, impact: f32, towards: Vec3) {
        if impact > 2.0 {
            self.speed = 0.0;
            self.hold = KNOCK_HOLD;
        }
        self.damage
            .crash(impact, sa_scene::vehicle::to_model(self.yaw, towards));
    }
    /// Wheels, steering and dents as they should be drawn.
    pub fn look(&self) -> sa_scene::vehicle::Look {
        let turn = self.aim_yaw - self.yaw;
        sa_scene::vehicle::Look {
            travel: self.travel,
            steer: turn.sin().atan2(turn.cos()).clamp(-0.6, 0.6),
            dents: self.damage.dents,
            burnt: self.damage.wrecked,
            ..Default::default()
        }
    }
}

#[derive(Default)]
pub struct Traffic {
    pub cars: Vec<TrafficCar>,
    seed: u32,
    since_spawn: f32,
    next_id: u64,
    /// Where police cars are chasing someone, if they are.
    pub chase: Option<Vec3>,
    /// Catalog indices of the police cars that patrol the area; empty for
    /// no patrols.
    pub patrol_models: Vec<usize>,
}

/// World position (X east, Y up, Z south) of a road node.
fn world(roads: &RoadGraph, node: u32) -> Vec3 {
    let [x, y, z] = roads.position(node);
    Vec3::new(x - ORIGIN[0], z, ORIGIN[1] - y)
}

impl Traffic {
    fn random(&mut self) -> u32 {
        self.seed = self
            .seed
            .wrapping_mul(1_664_525)
            .wrapping_add(1_013_904_223);
        self.seed >> 8
    }

    /// Move every car, retire distant ones and add new ones up to `limit`.
    /// `models` are the catalog indices traffic may use.
    /// Returns the index in `cars` of a newly added car, if any, so the caller
    /// can give it a mesh.
    pub fn update<G: Fn(Vec3, f32) -> Option<f32>>(
        &mut self,
        roads: &RoadGraph,
        around: &Surroundings<G>,
        models: &[usize],
        limit: usize,
        dt: f32,
    ) -> Option<usize> {
        let (player, blocked, ground) = (around.player, around.blocked, &around.ground);
        let chase = self.chase;
        for car in &mut self.cars {
            if let Some(police) = &mut car.police {
                if police.chasing != chase.is_some() {
                    police.chasing = chase.is_some();
                    car.cruise = if police.chasing { CHASE_CRUISE } else { CRUISE };
                }
            }
        }
        // Police cars chasing someone, or parked where a chase ended, do not
        // count against the traffic limit.
        let mut kept = 0;
        self.cars.retain(|car| {
            let distance = car.position.distance(player);
            let parked = car.off_road() && !car.chasing();
            distance < DESPAWN
                && car.burnt_for < WRECK_SECONDS
                && !car.stranded
                && !(parked && distance > PARKED_DESPAWN)
                && (car.chasing() || car.off_road() || {
                    kept += 1;
                    kept <= limit
                })
        });
        let bodies: Vec<Footprint> = self.cars.iter().map(TrafficCar::footprint).collect();
        let paths: Vec<Footprint> = self.cars.iter().map(TrafficCar::path_ahead).collect();
        let junctions: Vec<Junction> = self.cars.iter().map(|c| junction(c, roads)).collect();
        let cars = &self.cars;
        // Bodies in the way are hard stops; junction turns are courtesy.
        let body = |me: usize, other: usize| {
            other != me && paths[me].push_out_of(&bodies[other]).is_some()
        };
        // A police car on a chase gives way to nobody at junctions.
        let turn = |me: usize, other: usize| {
            other != me
                && !cars[me].chasing()
                && cars[me].waited < PATIENCE
                && gives_way(me, other, cars, &junctions)
        };
        let obstructed: Vec<bool> = (0..cars.len())
            .map(|me| {
                blocked.iter().any(|b| paths[me].push_out_of(b).is_some())
                    || (0..cars.len()).any(|other| {
                        // Nose to nose: the older car goes first.
                        (body(me, other) && !(body(other, me) && cars[me].id < cars[other].id))
                            // Never wait for a car that is stuck behind us,
                            // and break ties between two polite drivers.
                            || (turn(me, other)
                                && !(turn(other, me) && cars[me].id < cars[other].id))
                    })
            })
            .collect();
        for ((car, obstructed), junction) in self.cars.iter_mut().zip(obstructed).zip(&junctions) {
            // Through the junction (or no longer heading for it).
            if car
                .committed
                .is_some_and(|node| junction.node != Some(node))
            {
                car.committed = None;
            }
            if !obstructed && junction.approaching {
                car.committed = junction.node;
            }
            car.hold = (car.hold - dt).max(0.0);
            // Patience only comes back once the car is really under way, so
            // it cannot flicker between waiting and going.
            car.waited = if car.speed > 1.0 {
                0.0
            } else {
                car.waited + dt
            };
            if car.damage.wrecked {
                car.speed = 0.0;
                car.burnt_for += dt;
                continue;
            }
            if car.off_road() {
                // Driven by `pursue`, or parked where the chase ended.
                if !car.chasing() {
                    car.speed = (car.speed - BRAKING * dt).max(0.0);
                }
                continue;
            }
            // A driver whose car is on fire pulls up.
            let obstructed = obstructed || car.hold > 0.0 || car.damage.burning();
            let toward = chase.filter(|_| car.chasing());
            step(car, roads, ground, obstructed, toward, dt, &mut self.seed);
            car.travel += car.speed * dt;
        }
        self.since_spawn += dt;
        let ambient = self
            .cars
            .iter()
            .filter(|car| !car.chasing() && !car.off_road())
            .count();
        if ambient >= limit || models.is_empty() || self.since_spawn < SPAWN_INTERVAL {
            return None;
        }
        self.since_spawn = 0.0;
        let at = [player.x + ORIGIN[0], ORIGIN[1] - player.z];
        let mut candidates = roads.nodes_between(at, SPAWN_NEAR, SPAWN_FAR);
        candidates.retain(|&node| {
            let here = world(roads, node);
            around
                .taken
                .iter()
                .all(|other| flat_length(here, *other) > TAKEN_RADIUS)
        });
        if candidates.is_empty() {
            return None;
        }
        let from = candidates[self.random() as usize % candidates.len()];
        // Only ever down a one-way road in its own direction.
        let exits: Vec<u32> = roads.exits(from).collect();
        if exits.is_empty() {
            return None;
        }
        let to = exits[self.random() as usize % exits.len()];
        let lane = (self.random() % roads.lanes(from, to).max(1) as u32) as u8;
        let patrol = !self.patrol_models.is_empty() && self.random().is_multiple_of(PATROL_ODDS);
        let catalog = if patrol {
            let pick = self.random() as usize;
            self.patrol_models[pick % self.patrol_models.len()]
        } else {
            models[self.random() as usize % models.len()]
        };
        let next = pick_next(roads, from, to, &mut self.seed)?;
        let cruise = CRUISE * (0.8 + (self.random() % 1000) as f32 / 1000.0 * 0.35);
        self.next_id += 1;
        let mut car = TrafficCar::new(self.next_id, catalog, [from, to, next], lane, cruise);
        car.police = patrol.then(|| Patrol {
            crew: 1,
            ..Patrol::default()
        });
        car.position = world(roads, from);
        place(&mut car, roads);
        settle(&mut car, roads, ground, 0.0, 0.0);
        // Never drop a car on top of another one, or on someone.
        if self
            .cars
            .iter()
            .any(|other| other.position.distance(car.position) < GAP)
            || blocked
                .iter()
                .any(|b| car.footprint().push_out_of(b).is_some())
        {
            return None;
        }
        self.cars.push(car);
        Some(self.cars.len() - 1)
    }

    /// Send a police car with `crew` officers after the suspect at
    /// `around.player`, from a road a little way off, heading their way.
    /// Returns its index in `cars`, so the caller can give it a mesh.
    pub fn add_pursuer<G: Fn(Vec3, f32) -> Option<f32>>(
        &mut self,
        roads: &RoadGraph,
        around: &Surroundings<G>,
        catalog: usize,
        crew: u8,
    ) -> Option<usize> {
        let target = around.player;
        let at = [target.x + ORIGIN[0], ORIGIN[1] - target.z];
        let candidates = roads.nodes_between(at, PURSUIT_NEAR, PURSUIT_FAR);
        if candidates.is_empty() {
            return None;
        }
        let from = candidates[self.random() as usize % candidates.len()];
        let to = roads.exits(from).min_by(|&a, &b| {
            flat_length(world(roads, a), target).total_cmp(&flat_length(world(roads, b), target))
        })?;
        let next = pick_toward(roads, from, to, target)?;
        self.next_id += 1;
        let mut car = TrafficCar::new(self.next_id, catalog, [from, to, next], 0, CHASE_CRUISE);
        car.police = Some(Patrol {
            chasing: true,
            crew,
            ..Patrol::default()
        });
        place(&mut car, roads);
        settle(&mut car, roads, &around.ground, 0.0, 0.0);
        if self
            .cars
            .iter()
            .any(|other| other.position.distance(car.position) < GAP)
            || around
                .blocked
                .iter()
                .any(|b| car.footprint().push_out_of(b).is_some())
        {
            return None;
        }
        self.cars.push(car);
        Some(self.cars.len() - 1)
    }

    /// Drive the police cars that have left the road straight at the
    /// suspect at `target`, round anything in `blocked` (other cars, the
    /// suspect's car) and short of walls, which `walls` reports between two
    /// points. They pull up close by, back up when stuck and head back to
    /// the roads when the suspect gets away.
    pub fn pursue<G, W>(
        &mut self,
        roads: &RoadGraph,
        target: Vec3,
        blocked: &[Footprint],
        ground: &G,
        walls: &W,
        dt: f32,
    ) where
        G: Fn(Vec3, f32) -> Option<f32>,
        W: Fn(Vec3, Vec3) -> bool,
    {
        let bodies: Vec<Footprint> = self.cars.iter().map(TrafficCar::footprint).collect();
        for index in 0..self.cars.len() {
            let car = &mut self.cars[index];
            if !car.chasing() || car.damage.wrecked {
                continue;
            }
            let Some(mut police) = car.police.take() else {
                continue;
            };
            let others = bodies
                .iter()
                .enumerate()
                .filter(|(other, _)| *other != index)
                .map(|(_, body)| body)
                .chain(blocked);
            pursue_car(car, &mut police, roads, target, others, ground, walls, dt);
            car.police = Some(police);
        }
    }
}

/// The fastest a chasing car `distance` from the suspect may go and still
/// pull up in time, braking gently.
fn stopping_speed(distance: f32) -> f32 {
    (2.0 * BRAKING * 0.7 * (distance - PULL_UP).max(0.0))
        .sqrt()
        .min(CHASE_CRUISE)
}

/// One frame of a police car's direct pursuit; see `Traffic::pursue`.
#[allow(clippy::too_many_arguments)]
fn pursue_car<'a>(
    car: &mut TrafficCar,
    police: &mut Patrol,
    roads: &RoadGraph,
    target: Vec3,
    mut others: impl Iterator<Item = &'a Footprint>,
    ground: &impl Fn(Vec3, f32) -> Option<f32>,
    walls: &impl Fn(Vec3, Vec3) -> bool,
    dt: f32,
) {
    let distance = flat_length(car.position, target);
    if !police.off_road {
        if distance >= CLOSE_IN {
            return;
        }
        police.off_road = true;
    }
    if distance < CLOSE_IN {
        police.rejoin = None;
    } else if distance > FALL_BACK && police.rejoin.is_none() {
        let at = [car.position.x + ORIGIN[0], ORIGIN[1] - car.position.z];
        police.rejoin = roads
            .nodes_between(at, 0.0, 60.0)
            .into_iter()
            .filter(|&node| roads.exits(node).next().is_some())
            .min_by(|&a, &b| {
                flat_length(world(roads, a), car.position)
                    .total_cmp(&flat_length(world(roads, b), car.position))
            });
    }
    let aim = police.rejoin.map_or(target, |node| world(roads, node));
    if let Some(node) = police
        .rejoin
        .filter(|_| flat_length(car.position, aim) < 5.0)
    {
        // Back on the road: carry on along it towards the suspect.
        let exits = roads.exits(node).min_by(|&a, &b| {
            flat_length(world(roads, a), target).total_cmp(&flat_length(world(roads, b), target))
        });
        if let Some(to) = exits {
            car.from = node;
            car.to = to;
            car.next = pick_toward(roads, node, to, target).unwrap_or(node);
            car.lane = 0;
            car.along = progress(car.position, lane(roads, node, to, 0)).max(0.0);
            car.committed = None;
            car.speed = car.speed.max(0.0);
            police.reversing = 0.0;
            police.off_road = false;
            police.rejoin = None;
            return;
        }
    }
    let toward = aim - car.position;
    let wanted = toward.x.atan2(toward.z);
    let turn = (wanted - car.yaw).sin().atan2((wanted - car.yaw).cos());
    car.aim_yaw = wanted;
    let mut desired = if police.rejoin.is_some() {
        CHASE_CRUISE * 0.5
    } else {
        stopping_speed(distance)
    };
    if turn.abs() > 1.2 {
        desired = desired.min(8.0);
    }
    // Wait for officers who got out on foot.
    if police.out > 0 {
        desired = 0.0;
    }
    let ahead = car.path_ahead();
    let eye = car.position + Vec3::Y * 0.8;
    let stopping = car.speed.max(0.0).powi(2) / (2.0 * BRAKING);
    let reach = car.size[1] + 1.5 + stopping;
    let stuck = others.any(|body| ahead.push_out_of(body).is_some())
        || walls(eye, eye + car.forward() * reach);
    if police.reversing > 0.0 {
        police.reversing -= dt;
        desired = -4.0;
    } else if stuck {
        desired = 0.0;
        // Wedged against something with the suspect still out of reach:
        // back off and come round it.
        if police.stopped_for > 1.5 && distance > PULL_UP + 5.0 && police.out == 0 {
            police.reversing = 1.2;
        }
    }
    let old_speed = car.speed;
    car.speed = if car.speed < desired {
        (car.speed + ACCELERATION * 1.5 * dt).min(desired)
    } else {
        (car.speed - BRAKING * dt).max(desired)
    };
    let rate = car.speed.abs() / TURN_RADIUS * dt;
    let previous_yaw = car.yaw;
    car.yaw += turn.clamp(-rate, rate);
    car.position += car.forward() * car.speed * dt;
    car.travel += car.speed * dt;
    let (on_ground, roll) = surface(ground, car.position, car.position.y, car.yaw, car.size);
    if let Some((y, pitch)) = on_ground {
        car.position.y = y;
        let accel = if dt > 0.0 {
            (car.speed - old_speed) / dt
        } else {
            0.0
        };
        car.pitch = pitch + (accel * 0.004).clamp(-0.04, 0.04);
    }
    let yaw_rate = if dt > 0.0 {
        (car.yaw - previous_yaw) / dt
    } else {
        0.0
    };
    car.roll = roll + (yaw_rate * car.speed * 0.006).clamp(-0.06, 0.06);
    police.stopped_for = if car.speed.abs() < 0.5 {
        police.stopped_for + dt
    } else {
        0.0
    };
}

/// Where a car stands relative to the junction at the end of its link.
#[derive(Clone, Copy)]
struct Junction {
    node: Option<u32>,
    at: Vec3,
    /// Metres left to the node.
    distance: f32,
    /// Close enough that it must decide whether to go.
    approaching: bool,
}

fn junction(car: &TrafficCar, roads: &RoadGraph) -> Junction {
    let stopping = car.speed * car.speed / (2.0 * BRAKING);
    let reach = JUNCTION + stopping + 2.0;
    // Look past bends so a car starts braking in time even when the
    // junction is a link or two away.
    let (mut previous, mut node) = (car.from, car.to);
    let mut distance = length_of(roads, car.from, car.to) - car.along;
    while !cross_traffic(roads, previous, node) && distance < reach {
        let mut ways = roads.exits(node).filter(|&n| n != previous);
        let (Some(next), None) = (ways.next(), ways.next()) else {
            break;
        };
        distance += length_of(roads, node, next);
        (previous, node) = (node, next);
    }
    let found = cross_traffic(roads, previous, node);
    Junction {
        node: found.then_some(node),
        at: world(roads, node),
        distance,
        approaching: found && distance < reach,
    }
}

/// Whether a car coming from `from` meets cross traffic at `node`: other
/// roads lead in and the car has a choice of ways on (a crossing or a
/// T-junction), or two other roads lead in. A plain bend, a motorway exit
/// and a slip road joining a motorway are not crossings; there cars only
/// keep their distance to the car ahead.
fn cross_traffic(roads: &RoadGraph, from: u32, node: u32) -> bool {
    let others = roads.neighbours(node).iter().filter(|&&n| n != from);
    let inbound = others
        .clone()
        .filter(|&&n| roads.lanes(n, node) > 0)
        .count();
    let outbound = others.filter(|&&n| roads.lanes(node, n) > 0).count();
    inbound >= 2 || (inbound == 1 && outbound >= 2)
}

/// Junctions take one car at a time, like a four-way stop: a car waits while
/// another has taken the junction or is still turning out of it, or while
/// another car has waited longer. A car takes the junction once it
/// is close and nothing stops it, and keeps it until it is through.
fn gives_way(me: usize, other: usize, cars: &[TrafficCar], junctions: &[Junction]) -> bool {
    let mine = junctions[me];
    let Some(node) = mine.node.filter(|_| mine.approaching) else {
        return false;
    };
    let inside = |car: &TrafficCar| {
        let offset = car.position - mine.at;
        Vec3::new(offset.x, 0.0, offset.z).length() < JUNCTION
    };
    if cars[me].committed == Some(node) {
        return false;
    }
    let (car, theirs) = (&cars[other], junctions[other]);
    let bound_here = theirs.node == Some(node);
    // Already going, or just turned out of it and still in the way.
    car.committed == Some(node)
        || (car.from == node && inside(car))
        || (bound_here && theirs.approaching && {
            // First come, first served; among cars arriving together the
            // nearest goes first.
            let (their_wait, my_wait) = (car.waited, cars[me].waited);
            if (their_wait - my_wait).abs() > 0.25 {
                their_wait > my_wait
            } else {
                (theirs.distance, car.id) < (mine.distance, cars[me].id)
            }
        })
}

/// The link to take after `from` -> `to`, never the wrong way down a
/// one-way road. None when there is no legal way on.
fn pick_next(roads: &RoadGraph, from: u32, to: u32, seed: &mut u32) -> Option<u32> {
    let options: Vec<u32> = roads.exits(to).filter(|&n| n != from).collect();
    if options.is_empty() {
        // Dead end: turn around, where the road runs both ways.
        (roads.lanes(to, from) > 0).then_some(from)
    } else {
        *seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        Some(options[(*seed >> 8) as usize % options.len()])
    }
}

/// The way on after `from` -> `to` that leads closest to `target`, for a
/// police car hunting someone down. Never the wrong way down a one-way road,
/// and back the way it came only at a dead end.
fn pick_toward(roads: &RoadGraph, from: u32, to: u32, target: Vec3) -> Option<u32> {
    roads
        .exits(to)
        .filter(|&n| n != from)
        .min_by(|&a, &b| {
            flat_length(world(roads, a), target).total_cmp(&flat_length(world(roads, b), target))
        })
        .or_else(|| (roads.lanes(to, from) > 0).then_some(from))
}

/// Start and end of lane `wanted` of a link. Two-way roads keep their lanes
/// right of the centre line; a one-way road (a motorway carriageway) is
/// centred on its path line with its lanes side by side, as in SA.
fn lane(roads: &RoadGraph, from: u32, to: u32, wanted: u8) -> (Vec3, Vec3) {
    let (a, b) = (world(roads, from), world(roads, to));
    let flat = Vec3::new(b.x - a.x, 0.0, b.z - a.z).normalize_or_zero();
    let lanes = roads.lanes(from, to).max(1);
    let index = wanted.min(lanes - 1) as f32;
    let offset = if roads.lanes(to, from) == 0 {
        (index - (lanes - 1) as f32 / 2.0) * 2.0 * LANE
    } else {
        (index * 2.0 + 1.0) * LANE
    };
    let right = Vec3::new(-flat.z, 0.0, flat.x) * offset;
    (a + right, b + right)
}

/// Where the lane lines of two consecutive links cross, kept near the node
/// they share.
fn corner(current: (Vec3, Vec3), following: (Vec3, Vec3)) -> Vec3 {
    let d1 = current.1 - current.0;
    let d2 = following.1 - following.0;
    let cross = d1.x * d2.z - d1.z * d2.x;
    let midpoint = (current.1 + following.0) / 2.0;
    if cross.abs() < 0.05 * d1.length() * d2.length() {
        return midpoint;
    }
    let gap = following.0 - current.0;
    let t = (gap.x * d2.z - gap.z * d2.x) / cross;
    let point = current.0 + d1 * t;
    if point.distance(midpoint) > 2.0 * LANE + 2.0 {
        midpoint
    } else {
        Vec3::new(point.x, midpoint.y, point.z)
    }
}

/// Metres along a lane segment that `point` has reached.
fn progress(point: Vec3, (start, end): (Vec3, Vec3)) -> f32 {
    let flat = |v: Vec3| Vec3::new(v.x, 0.0, v.z);
    flat(point - start).dot(flat(end - start).normalize_or_zero())
}

fn flat_length(a: Vec3, b: Vec3) -> f32 {
    Vec3::new(b.x - a.x, 0.0, b.z - a.z).length().max(0.1)
}

/// Advance one car: steer towards a point ahead on its lane, move along its
/// heading, and move on to the next link once past the end of this one.
/// A car hunting someone at `hunt` turns their way at every junction.
fn step(
    car: &mut TrafficCar,
    roads: &RoadGraph,
    ground: &impl Fn(Vec3, f32) -> Option<f32>,
    obstructed: bool,
    hunt: Option<Vec3>,
    dt: f32,
    seed: &mut u32,
) {
    let current = lane(roads, car.from, car.to, car.lane);
    let length = flat_length(current.0, current.1);
    let following = lane(roads, car.to, car.next, car.lane);
    let here = (current.1 - current.0).normalize_or_zero();
    let there = (following.1 - following.0).normalize_or_zero();
    let u_turn = car.next == car.from;
    // Lanes of two links meet at a corner short of the node on a right turn
    // and past it on a left turn; turning towards that point keeps the car
    // in its lane through the bend.
    let corner = if u_turn {
        current.1
    } else {
        corner(current, following)
    };
    let left = progress(corner, current) - car.along;
    // Ease off before a sharp corner.
    let turn = 1.0 - here.dot(there).max(0.0);
    let near_end = (left / 25.0).clamp(0.0, 1.0);
    let mut target = if obstructed {
        0.0
    } else {
        car.cruise * (1.0 - turn.min(1.0) * 0.75 * (1.0 - near_end))
    };
    if let Some(suspect) = hunt {
        target = target.min(stopping_speed(flat_length(car.position, suspect)));
    }
    let old_speed = car.speed;
    car.speed = if car.speed < target {
        (car.speed + ACCELERATION * dt).min(target)
    } else {
        (car.speed - BRAKING * dt).max(target)
    };
    // Pure pursuit: aim at a point a little way down the lane, round the
    // corner into the next link when close, and turn no tighter than a real
    // car could.
    let reach = 3.0 + car.speed * 0.45;
    let aim = if left > reach || u_turn {
        current.0 + here * (car.along + reach)
    } else {
        corner + there * (reach - left.max(0.0))
    };
    let toward = aim - car.position;
    let previous_yaw = car.yaw;
    if Vec3::new(toward.x, 0.0, toward.z).length_squared() > 0.01 {
        let wanted = toward.x.atan2(toward.z);
        car.aim_yaw = wanted;
        let delta = (wanted - car.yaw).sin().atan2((wanted - car.yaw).cos());
        // A police car can come back to the road still backing up.
        let rate = car.speed.abs() / TURN_RADIUS * dt;
        car.yaw += delta.clamp(-rate, rate);
    }
    car.position += car.forward() * car.speed * dt;
    car.along = progress(car.position, current);
    // Past the end of this lane, or already round the corner into the next.
    let rounded = !u_turn && left < reach && progress(car.position, (corner, following.1)) >= 0.0;
    if car.along >= length || rounded {
        let u_turn = car.next == car.from;
        car.from = car.to;
        car.to = car.next;
        let next = match hunt {
            Some(target) => pick_toward(roads, car.from, car.to, target),
            None => pick_next(roads, car.from, car.to, seed),
        };
        car.next = next.unwrap_or_else(|| {
            car.stranded = true;
            car.from
        });
        car.along = progress(car.position, lane(roads, car.from, car.to, car.lane));
        if u_turn {
            // No room to swing round: drop into the other lane.
            car.along = car.along.max(0.0);
            place(car, roads);
        }
    }
    // A car knocked or squeezed far off its lane rejoins it.
    let (start, end) = lane(roads, car.from, car.to, car.lane);
    let on_lane = start + (end - start).normalize_or_zero() * car.along.max(0.0);
    if Vec3::new(car.position.x - on_lane.x, 0.0, car.position.z - on_lane.z).length() > 12.0 {
        place(car, roads);
    }
    let accel = if dt > 0.0 {
        (car.speed - old_speed) / dt
    } else {
        0.0
    };
    let yaw_rate = if dt > 0.0 {
        (car.yaw - previous_yaw)
            .sin()
            .atan2((car.yaw - previous_yaw).cos())
            / dt
    } else {
        0.0
    };
    settle(car, roads, ground, accel, yaw_rate * car.speed);
}

/// Sit the car on the real road surface under its wheels, with a little body
/// dive under braking and lean in turns. Falls back to the path height where
/// there is no collision (far away, or not streamed yet).
fn settle(
    car: &mut TrafficCar,
    roads: &RoadGraph,
    ground: &impl Fn(Vec3, f32) -> Option<f32>,
    accel: f32,
    lateral: f32,
) {
    let (a, b) = (world(roads, car.from), world(roads, car.to));
    let t = (car.along / flat_length(a, b)).clamp(0.0, 1.0);
    let path = a.y + (b.y - a.y) * t;
    let (on_ground, roll) = surface(ground, car.position, path, car.yaw, car.size);
    let (ground_y, pitch) = on_ground.unwrap_or((path, (b.y - a.y).atan2(flat_length(a, b))));
    car.position.y = ground_y;
    car.pitch = pitch + (accel * 0.004).clamp(-0.04, 0.04);
    car.roll = roll + (lateral * 0.006).clamp(-0.06, 0.06);
}

/// Height and pitch of a car body on the road under its axles, and its roll
/// from the wheels on either side. `near` is roughly where the road is: only
/// ground close to it counts, never a bridge overhead.
pub(super) fn surface(
    ground: &impl Fn(Vec3, f32) -> Option<f32>,
    position: Vec3,
    near: f32,
    yaw: f32,
    size: [f32; 3],
) -> (Option<(f32, f32)>, f32) {
    let forward = Vec3::new(yaw.sin(), 0.0, yaw.cos());
    let side = Vec3::new(forward.z, 0.0, -forward.x);
    let axle = size[1] * 0.65;
    let track = size[0] * 0.8;
    let height = |offset: Vec3| {
        let at = Vec3::new(position.x, near, position.z) + offset;
        ground(at, near + 2.0).filter(|y| *y > near - 4.0)
    };
    let (front, rear) = (height(forward * axle), height(-forward * axle));
    let (left, right) = (height(-side * track), height(side * track));
    let body = match (front, rear) {
        (Some(f), Some(r)) => Some(((f + r) / 2.0, (f - r).atan2(axle * 2.0))),
        _ => None,
    };
    let roll = match (left, right) {
        (Some(l), Some(r)) => (r - l).atan2(track * 2.0),
        _ => 0.0,
    };
    (body, roll)
}

fn length_of(roads: &RoadGraph, from: u32, to: u32) -> f32 {
    flat_length(world(roads, from), world(roads, to))
}

/// Put a car on its lane at its distance along the link, facing down it.
fn place(car: &mut TrafficCar, roads: &RoadGraph) {
    let (start, end) = lane(roads, car.from, car.to, car.lane);
    let flat = Vec3::new(end.x - start.x, 0.0, end.z - start.z).normalize_or_zero();
    car.position = start + flat * car.along;
    if flat != Vec3::ZERO {
        car.yaw = flat.x.atan2(flat.z);
        car.aim_yaw = car.yaw;
    }
}

/// A traffic car's own copy of its mesh, moved on the CPU like remote players.
pub(super) struct TrafficMesh {
    id: u64,
    pub batches: Vec<GpuBatch>,
    clearance: f32,
}

impl State {
    /// Catalog indices of the civilian cars from the player's vehicles.ide.
    fn refresh_traffic_models(&mut self) {
        let civilian: std::collections::HashSet<String> =
            sa_scene::vehicle_list::original_cars(&self.resource_game_dir)
                .into_iter()
                .filter(|car| car.is_civilian())
                .map(|car| car.model)
                .collect();
        self.traffic_models = (0..self.menu.cars.len())
            .filter(|&index| {
                self.menu
                    .cars
                    .get(index)
                    .and_then(|name| self.original_car_models.get(name))
                    .is_some_and(|model| civilian.contains(model))
            })
            .collect();
        self.traffic_models_for = self.menu.cars.len();
    }

    pub(super) fn update_traffic(&mut self, dt: f32) {
        if self.traffic_models_for != self.menu.cars.len() {
            self.refresh_traffic_models();
        }
        let Some(roads) = self.roads.as_ref() else {
            self.traffic.cars.clear();
            self.traffic_meshes.clear();
            return;
        };
        let limit = if self.interior == 0 {
            self.menu.settings.traffic_cars as usize
        } else {
            0
        };
        let player = match (&self.car, &self.player) {
            (Some((car, _)), _) if self.driving => car.position,
            (_, Some(player)) if self.walking => player.feet,
            _ => self.position,
        };
        // The player's car counts whether it is driven or parked.
        let mut blocked: Vec<Footprint> = self
            .car
            .as_ref()
            .map(|(car, _)| {
                Footprint::car(car.position - Vec3::Y * car.clearance, car.yaw, car.size)
            })
            .into_iter()
            .collect();
        if let Some(player) = self
            .player
            .as_ref()
            .filter(|_| self.walking && !self.driving)
        {
            blocked.push(Footprint::person(player.feet));
        }
        for actor in &self.remote_actors {
            if let Some(car) = actor.current.vehicle.filter(|_| actor.car_visible) {
                blocked.push(Footprint::car(
                    Vec3::from_array(car.position),
                    car.yaw,
                    actor.car_size,
                ));
            }
            if actor.visible && !actor.current.driving {
                blocked.push(Footprint::person(Vec3::from_array(actor.current.position)));
            }
        }
        // Stop for people crossing too.
        blocked.extend(self.crowd.walkers.iter().map(|w| Footprint::person(w.feet)));
        blocked.extend(self.traffic_with_right_of_way());
        let taken = self.traffic_owners_before_us();
        let added = self.traffic.update(
            roads,
            &Surroundings {
                player,
                blocked: &blocked,
                ground: |at, max| self.collision.as_ref()?.ground_below(at, max),
                taken: &taken,
            },
            &self.traffic_models,
            limit,
            dt,
        );
        let cars = &self.traffic.cars;
        self.traffic_meshes
            .retain(|mesh| cars.iter().any(|car| car.id == mesh.id));
        if let Some(index) = added {
            self.mesh_traffic_car(index);
        }
    }

    /// Give the car just added at `index` its own copy of its model, or
    /// drop it when the model cannot be loaded.
    pub(super) fn mesh_traffic_car(&mut self, index: usize) {
        let (id, catalog) = (
            self.traffic.cars[index].id,
            self.traffic.cars[index].catalog,
        );
        self.ensure_car_loaded(catalog);
        let source = if catalog == self.active_car {
            self.car.as_ref()
        } else {
            self.car_catalog.get(catalog).and_then(|c| c.as_ref())
        };
        match source {
            Some((car, batches)) => {
                self.traffic.cars[index].size = car.size;
                self.traffic_meshes.push(TrafficMesh {
                    id,
                    batches: crate::multiplayer::duplicate(&self.device, batches),
                    clearance: car.clearance,
                })
            }
            None => {
                self.traffic.cars.remove(index);
            }
        }
    }

    /// Move every traffic car's mesh to where the car is now.
    pub(super) fn pose_traffic(&mut self) {
        for mesh in &self.traffic_meshes {
            let Some(car) = self.traffic.cars.iter().find(|c| c.id == mesh.id) else {
                continue;
            };
            let rotation = sa_scene::vehicle::model_rotation(car.yaw, car.pitch, car.roll);
            let offset = car.position + Vec3::Y * mesh.clearance;
            let look = car.look();
            for batch in &mesh.batches {
                let raw = crate::car_damage::pose_car(batch, &look, rotation, offset);
                self.queue
                    .write_buffer(&batch.buffer, 0, bytemuck::cast_slice(&raw));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A straight road north from the origin with a dead end at each side.
    fn road() -> RoadGraph {
        let o = ORIGIN;
        let nodes = (0..40)
            .map(|i| [o[0], o[1] + i as f32 * 10.0, 10.0])
            .collect::<Vec<_>>();
        let links = (0..40u32)
            .map(|i| {
                let mut l = Vec::new();
                if i > 0 {
                    l.push(i - 1);
                }
                if i < 39 {
                    l.push(i + 1);
                }
                l
            })
            .collect();
        RoadGraph::from_parts(nodes, links)
    }

    fn fill(traffic: &mut Traffic, roads: &RoadGraph, player: Vec3) {
        for _ in 0..200 {
            traffic.update(
                roads,
                &Surroundings {
                    player,
                    blocked: &[],
                    ground: |_, _| None,
                    taken: &[],
                },
                &[3, 4],
                4,
                0.1,
            );
        }
    }

    #[test]
    fn cars_spawn_away_from_the_player_and_keep_right() {
        let roads = road();
        let mut traffic = Traffic::default();
        let player = Vec3::ZERO;
        let added = traffic.update(
            &roads,
            &Surroundings {
                player,
                blocked: &[],
                ground: |_, _| None,
                taken: &[],
            },
            &[3],
            4,
            1.0,
        );
        assert_eq!(added, Some(0));
        assert_eq!(traffic.cars[0].catalog, 3);
        let car = &traffic.cars[0];
        assert!(car.position.distance(player) >= SPAWN_NEAR - LANE - 1.0);
        // Heading north (-Z) keeps right, which is east (+X); south keeps west.
        let forward = Vec3::new(car.yaw.sin(), 0.0, car.yaw.cos());
        let right = Vec3::new(-forward.z, 0.0, forward.x);
        assert!((car.position.x - right.x * LANE).abs() < 1e-3);
    }

    /// A motorway: two one-way carriageways of two lanes, 30 m apart, the
    /// western one (nodes 0..40) running north and the eastern one south.
    fn motorway() -> RoadGraph {
        let o = ORIGIN;
        let mut nodes = Vec::new();
        let mut links = Vec::new();
        for side in 0..2u32 {
            for i in 0..40u32 {
                nodes.push([
                    o[0] - 15.0 + side as f32 * 30.0,
                    o[1] - 200.0 + i as f32 * 10.0,
                    10.0,
                ]);
                let mut l = Vec::new();
                if i > 0 {
                    l.push(side * 40 + i - 1);
                }
                if i < 39 {
                    l.push(side * 40 + i + 1);
                }
                links.push(l);
            }
        }
        let mut roads = RoadGraph::from_parts(nodes, links);
        for i in 0..39u32 {
            roads.set_lanes(i, i + 1, 2);
            roads.set_lanes(i + 1, i, 0);
            roads.set_lanes(40 + i + 1, 40 + i, 2);
            roads.set_lanes(40 + i, 40 + i + 1, 0);
        }
        roads
    }

    #[test]
    fn motorway_traffic_only_drives_its_own_carriageway_way() {
        let roads = motorway();
        let mut traffic = Traffic::default();
        let mut seen = 0;
        for _ in 0..600 {
            traffic.update(
                &roads,
                &Surroundings {
                    player: Vec3::ZERO,
                    blocked: &[],
                    ground: |_, _| None,
                    taken: &[],
                },
                &[3],
                6,
                0.1,
            );
            for car in &traffic.cars {
                assert!(
                    roads.lanes(car.from, car.to) > 0,
                    "wrong way down a carriageway"
                );
                let west = car.from < 40;
                // North is -Z: the western carriageway heads that way.
                let heading = if west { -1.0 } else { 1.0 };
                assert!(car.forward().z * heading > 0.9, "car facing the wrong way");
                // Both lanes sit on the carriageway, either side of its line.
                let line = if west { -15.0 } else { 15.0 };
                let off = (car.position.x - line).abs();
                assert!((off - LANE).abs() < 0.5, "car off its lane: {off}");
                seen += 1;
            }
        }
        assert!(seen > 0);
        assert!(traffic.cars.iter().any(|c| c.speed > 5.0));
    }

    #[test]
    fn a_slip_road_joining_is_no_crossing_but_a_road_junction_is() {
        // Motorway 0 -> 1 -> 2 with a slip road 3 -> 1 joining it.
        let mut roads = RoadGraph::from_parts(
            vec![
                [0.0, 0.0, 0.0],
                [0.0, 50.0, 0.0],
                [0.0, 100.0, 0.0],
                [20.0, 0.0, 0.0],
            ],
            vec![vec![1], vec![0, 2, 3], vec![1], vec![1]],
        );
        for (from, to) in [(1, 0), (2, 1), (1, 3)] {
            roads.set_lanes(from, to, 0);
        }
        assert!(!cross_traffic(&roads, 0, 1));
        assert!(!cross_traffic(&roads, 3, 1));
        // The same shape with two-way roads is a T-junction.
        let roads = RoadGraph::from_parts(
            vec![
                [0.0, 0.0, 0.0],
                [0.0, 50.0, 0.0],
                [0.0, 100.0, 0.0],
                [20.0, 0.0, 0.0],
            ],
            vec![vec![1], vec![0, 2, 3], vec![1], vec![1]],
        );
        assert!(cross_traffic(&roads, 0, 1));
    }

    #[test]
    fn traffic_stays_within_the_limit_and_drives() {
        let roads = road();
        let mut traffic = Traffic::default();
        fill(&mut traffic, &roads, Vec3::ZERO);
        assert!(!traffic.cars.is_empty() && traffic.cars.len() <= 4);
        assert!(traffic.cars.iter().any(|c| c.speed > 5.0));
        assert!(traffic.cars.iter().all(|c| [3, 4].contains(&c.catalog)));
        // Lowering the limit retires cars at once.
        traffic.update(
            &roads,
            &Surroundings {
                player: Vec3::ZERO,
                blocked: &[],
                ground: |_, _| None,
                taken: &[],
            },
            &[3],
            1,
            0.1,
        );
        assert!(traffic.cars.len() <= 1);
    }

    #[test]
    fn a_car_stops_for_the_player_in_its_lane() {
        let roads = road();
        let mut traffic = Traffic::default();
        traffic.update(
            &roads,
            &Surroundings {
                player: Vec3::ZERO,
                blocked: &[],
                ground: |_, _| None,
                taken: &[],
            },
            &[3],
            1,
            1.0,
        );
        let car = traffic.cars[0].clone();
        let forward = Vec3::new(car.yaw.sin(), 0.0, car.yaw.cos());
        let blocker = car.position + forward * 6.0;
        for _ in 0..30 {
            traffic.update(
                &roads,
                &Surroundings {
                    player: Vec3::ZERO,
                    blocked: &[Footprint::person(blocker)],
                    ground: |_, _| None,
                    taken: &[],
                },
                &[3],
                1,
                0.1,
            );
        }
        assert!(traffic.cars[0].speed < 0.1, "{}", traffic.cars[0].speed);
        assert!(traffic.cars[0].position.distance(blocker) > 1.0);
    }

    /// Two roads crossing at node 0: one along X, one along Z.
    fn crossing() -> RoadGraph {
        let o = ORIGIN;
        let mut nodes = vec![[o[0], o[1], 10.0]];
        let mut links: Vec<Vec<u32>> = vec![vec![]];
        for (dx, dy) in [(1.0, 0.0), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0)] {
            let mut previous = 0u32;
            for i in 1..=8 {
                let id = nodes.len() as u32;
                nodes.push([
                    o[0] + dx * i as f32 * 10.0,
                    o[1] + dy * i as f32 * 10.0,
                    10.0,
                ]);
                links.push(vec![previous]);
                links[previous as usize].push(id);
                previous = id;
            }
        }
        RoadGraph::from_parts(nodes, links)
    }

    #[test]
    fn a_car_stops_behind_a_parked_car_in_its_lane() {
        let roads = road();
        let mut traffic = Traffic::default();
        traffic.update(
            &roads,
            &Surroundings {
                player: Vec3::ZERO,
                blocked: &[],
                ground: |_, _| None,
                taken: &[],
            },
            &[3],
            1,
            1.0,
        );
        let car = traffic.cars[0].clone();
        let parked = Footprint::car(
            car.position + car.forward() * 9.0,
            car.yaw + 0.3,
            sa_scene::vehicle::DEFAULT_SIZE,
        );
        for _ in 0..40 {
            traffic.update(
                &roads,
                &Surroundings {
                    player: Vec3::ZERO,
                    blocked: &[parked],
                    ground: |_, _| None,
                    taken: &[],
                },
                &[3],
                1,
                0.1,
            );
        }
        assert!(traffic.cars[0].speed < 0.1);
        assert_eq!(traffic.cars[0].footprint().push_out_of(&parked), None);
    }

    #[test]
    fn traffic_never_drives_through_traffic_at_a_crossing() {
        let roads = crossing();
        for seed in 0..10 {
            let mut traffic = Traffic {
                seed,
                ..Traffic::default()
            };
            let mut moved = 0.0;
            for _ in 0..3000 {
                let before: Vec<Vec3> = traffic.cars.iter().map(|c| c.position).collect();
                traffic.update(
                    &roads,
                    &Surroundings {
                        player: Vec3::ZERO,
                        blocked: &[],
                        ground: |_, _| None,
                        taken: &[],
                    },
                    &[3],
                    10,
                    0.05,
                );
                for (a, car) in traffic.cars.iter().enumerate() {
                    for other in &traffic.cars[a + 1..] {
                        let depth = car
                            .footprint()
                            .push_out_of(&other.footprint())
                            .map_or(0.0, |p| p.length());
                        assert!(depth < 0.3, "overlap {depth}: {car:?} {other:?}");
                    }
                }
                moved += traffic
                    .cars
                    .iter()
                    .zip(&before)
                    .map(|(c, b)| c.position.distance(*b))
                    .sum::<f32>();
            }
            // Nobody deadlocked: traffic kept flowing.
            assert!(moved > 6000.0, "seed {seed}: {moved}");
        }
    }

    #[test]
    fn cars_sit_on_the_road_surface_not_the_path_line() {
        let roads = road();
        let mut traffic = Traffic::default();
        // The path nodes sit at 10 m but the road rises gently northwards.
        let surface = |at: Vec3| 10.3 - at.z * 0.008;
        let ground = |at: Vec3, max: f32| Some(surface(at)).filter(|y| *y <= max);
        for _ in 0..100 {
            traffic.update(
                &roads,
                &Surroundings {
                    player: Vec3::ZERO,
                    blocked: &[],
                    ground,
                    taken: &[],
                },
                &[3],
                4,
                0.1,
            );
        }
        assert!(!traffic.cars.is_empty());
        for car in &traffic.cars {
            assert!(
                (car.position.y - surface(car.position)).abs() < 0.05,
                "{car:?}"
            );
            // Heading north (-Z) is uphill: nose up. South is nose down.
            let uphill = car.forward().z < 0.0;
            assert_eq!(car.pitch > 0.0, uphill, "{car:?}");
        }
        // A bridge high overhead is ignored.
        let bridge = |_: Vec3, max: f32| Some(25.0).filter(|y| *y <= max);
        traffic.update(
            &roads,
            &Surroundings {
                player: Vec3::ZERO,
                blocked: &[],
                ground: bridge,
                taken: &[],
            },
            &[3],
            4,
            0.1,
        );
        assert!(traffic.cars.iter().all(|c| c.position.y < 12.0));
    }

    #[test]
    fn cars_round_corners_smoothly_instead_of_snapping() {
        let roads = crossing();
        let mut traffic = Traffic {
            seed: 3,
            ..Traffic::default()
        };
        let mut turned = 0.0_f32;
        let mut previous: Vec<(u64, f32)> = Vec::new();
        for _ in 0..2000 {
            traffic.update(
                &roads,
                &Surroundings {
                    player: Vec3::ZERO,
                    blocked: &[],
                    ground: |_, _| None,
                    taken: &[],
                },
                &[3],
                4,
                0.05,
            );
            for car in &traffic.cars {
                if let Some((_, yaw)) = previous.iter().find(|(id, _)| *id == car.id) {
                    let step = (car.yaw - yaw).sin().atan2((car.yaw - yaw).cos()).abs();
                    if step > 3.0 {
                        continue; // Dead-end U-turn: drops into the other lane.
                    }
                    // Never more than a real turning circle allows in one frame.
                    assert!(
                        step <= car.speed / TURN_RADIUS * 0.05 + 1e-4,
                        "{step} {car:?}"
                    );
                    turned += step;
                }
            }
            previous = traffic.cars.iter().map(|c| (c.id, c.yaw)).collect();
        }
        assert!(turned > 3.0, "cars took corners: {turned}");
    }

    #[test]
    fn a_wreck_stands_still_and_is_towed_away_later() {
        let roads = road();
        let mut traffic = Traffic::default();
        fill(&mut traffic, &roads, Vec3::ZERO);
        let id = traffic.cars[0].id;
        traffic.cars[0].damage.hurt(5000.0);
        assert!(traffic.cars[0].damage.update(0.1), "blows up");
        let at = traffic.cars[0].position;
        let around = Surroundings {
            player: Vec3::ZERO,
            blocked: &[],
            ground: |_, _| None,
            taken: &[],
        };
        for _ in 0..20 {
            traffic.update(&roads, &around, &[3], 4, 0.1);
        }
        let wreck = traffic.cars.iter().find(|c| c.id == id).unwrap();
        assert_eq!(wreck.position, at);
        assert!(wreck.look().burnt);
        for _ in 0..500 {
            traffic.update(&roads, &around, &[3], 4, 0.1);
        }
        assert!(traffic.cars.iter().all(|c| c.id != id), "towed away");
    }

    #[test]
    fn a_hard_knock_makes_the_driver_wait() {
        let roads = road();
        let mut traffic = Traffic::default();
        fill(&mut traffic, &roads, Vec3::ZERO);
        traffic.cars[0].knock(8.0, Vec3::Z);
        for _ in 0..10 {
            traffic.update(
                &roads,
                &Surroundings {
                    player: Vec3::ZERO,
                    blocked: &[],
                    ground: |_, _| None,
                    taken: &[],
                },
                &[3],
                4,
                0.1,
            );
        }
        assert_eq!(traffic.cars[0].speed, 0.0);
        for _ in 0..40 {
            traffic.update(
                &roads,
                &Surroundings {
                    player: Vec3::ZERO,
                    blocked: &[],
                    ground: |_, _| None,
                    taken: &[],
                },
                &[3],
                4,
                0.1,
            );
        }
        assert!(traffic.cars[0].hold == 0.0);
    }

    #[test]
    fn no_cars_are_added_where_another_player_runs_traffic() {
        let roads = road();
        let mut traffic = Traffic::default();
        let other = Vec3::new(0.0, 10.0, -260.0);
        for _ in 0..200 {
            traffic.update(
                &roads,
                &Surroundings {
                    player: Vec3::ZERO,
                    blocked: &[],
                    ground: |_, _| None,
                    taken: &[other],
                },
                &[3],
                8,
                0.1,
            );
        }
        // Roads nearer to us than to the other player still get cars.
        assert!(!traffic.cars.is_empty());
        let mut fresh = Traffic::default();
        fresh.update(
            &roads,
            &Surroundings {
                player: Vec3::ZERO,
                blocked: &[],
                ground: |_, _| None,
                taken: &[Vec3::ZERO],
            },
            &[3],
            8,
            1.0,
        );
        assert!(fresh.cars.is_empty());
    }

    fn open_road(
        player: Vec3,
        blocked: &[Footprint],
    ) -> Surroundings<'_, impl Fn(Vec3, f32) -> Option<f32>> {
        Surroundings {
            player,
            blocked,
            ground: |_, _| None,
            taken: &[],
        }
    }

    #[test]
    fn a_pursuer_comes_down_the_road_and_pulls_up_by_the_suspect() {
        let roads = road();
        let mut traffic = Traffic::default();
        // The suspect stands beside the road, near its southern end.
        let suspect = Vec3::new(8.0, 10.0, -20.0);
        let index = traffic
            .add_pursuer(&roads, &open_road(suspect, &[]), 5, 2)
            .expect("a road far enough away");
        let car = &traffic.cars[index];
        assert!(car.chasing() && car.siren());
        assert_eq!(car.police.as_ref().unwrap().crew, 2);
        assert!(car.position.distance(suspect) > PURSUIT_NEAR - LANE - 1.0);
        traffic.chase = Some(suspect);
        let person = [Footprint::person(suspect)];
        let mut closest = f32::MAX;
        for _ in 0..600 {
            traffic.update(&roads, &open_road(suspect, &person), &[3], 0, 0.05);
            traffic.pursue(&roads, suspect, &person, &|_, _| None, &|_, _| false, 0.05);
            closest = closest.min(flat_length(traffic.cars[0].position, suspect));
        }
        let car = &traffic.cars[0];
        assert!(car.off_road(), "left the road for the last stretch");
        assert!(car.speed.abs() < 0.5, "pulled up: {}", car.speed);
        assert!(
            (PULL_UP - 2.0..PULL_UP + 4.0).contains(&closest),
            "stopped short of the suspect: {closest}"
        );
        assert!(car.police.as_ref().unwrap().stopped_for > 1.0);
    }

    #[test]
    fn a_wall_stops_a_pursuer_and_it_backs_off() {
        let roads = road();
        let mut traffic = Traffic::default();
        let far = Vec3::new(8.0, 10.0, -20.0);
        traffic.add_pursuer(&roads, &open_road(far, &[]), 5, 1);
        let start = traffic.cars[0].position;
        let suspect = start + Vec3::X * 20.0;
        traffic.chase = Some(suspect);
        traffic.cars[0].police.as_mut().unwrap().off_road = true;
        traffic.cars[0].speed = 0.0;
        let mut backed = false;
        for _ in 0..100 {
            traffic.update(&roads, &open_road(suspect, &[]), &[3], 0, 0.05);
            traffic.pursue(&roads, suspect, &[], &|_, _| None, &|_, _| true, 0.05);
            backed |= traffic.cars[0].speed < -0.5;
        }
        assert!(backed, "reversed out of the way");
        assert!(
            traffic.cars[0].position.distance(start) < 6.0,
            "never drove through"
        );
    }

    #[test]
    fn a_car_backing_up_as_it_rejoins_the_road_drives_on() {
        let roads = road();
        let mut traffic = Traffic::default();
        let suspect = Vec3::new(8.0, 10.0, -20.0);
        traffic.add_pursuer(&roads, &open_road(suspect, &[]), 5, 1);
        traffic.chase = Some(suspect);
        traffic.cars[0].speed = -4.0;
        // On its lane with a negative speed used to panic in the turn clamp.
        for _ in 0..60 {
            traffic.update(&roads, &open_road(suspect, &[]), &[3], 0, 0.05);
        }
        assert!(traffic.cars[0].speed > 0.0, "{:?}", traffic.cars[0]);
    }

    #[test]
    fn chasing_cars_do_not_count_against_traffic_and_park_when_it_ends() {
        let roads = road();
        let mut traffic = Traffic::default();
        let far = Vec3::new(8.0, 10.0, -20.0);
        traffic.add_pursuer(&roads, &open_road(far, &[]), 5, 1);
        let suspect = traffic.cars[0].position + Vec3::X * 20.0;
        traffic.chase = Some(suspect);
        traffic.cars[0].police.as_mut().unwrap().off_road = true;
        traffic.update(&roads, &open_road(suspect, &[]), &[3], 0, 0.1);
        assert_eq!(traffic.cars.len(), 1, "kept with traffic turned off");
        traffic.chase = None;
        for _ in 0..40 {
            traffic.update(&roads, &open_road(suspect, &[]), &[3], 0, 0.1);
        }
        let car = &traffic.cars[0];
        assert!(
            !car.siren() && car.speed == 0.0,
            "parked where the chase ended"
        );
        // Cleared away once the player has left it behind.
        traffic.update(
            &roads,
            &open_road(Vec3::new(0.0, 10.0, -300.0), &[]),
            &[3],
            0,
            0.1,
        );
        assert!(traffic.cars.is_empty());
    }

    #[test]
    fn patrols_mix_into_traffic_and_join_a_chase() {
        let roads = road();
        let mut traffic = Traffic {
            patrol_models: vec![9],
            ..Traffic::default()
        };
        for _ in 0..2000 {
            traffic.update(&roads, &open_road(Vec3::ZERO, &[]), &[3], 8, 0.1);
            if traffic.cars.iter().any(|car| car.police.is_some()) {
                break;
            }
        }
        let patrol = traffic
            .cars
            .iter()
            .find(|car| car.police.is_some())
            .expect("a patrol car");
        assert_eq!(patrol.catalog, 9);
        assert!(!patrol.siren(), "a patrol is quiet");
        traffic.chase = Some(Vec3::ZERO);
        traffic.update(&roads, &open_road(Vec3::ZERO, &[]), &[3], 8, 0.1);
        assert!(traffic
            .cars
            .iter()
            .filter(|car| car.police.is_some())
            .all(|car| car.siren() && car.cruise == CHASE_CRUISE));
    }

    #[test]
    fn far_cars_are_retired() {
        let roads = road();
        let mut traffic = Traffic::default();
        fill(&mut traffic, &roads, Vec3::ZERO);
        traffic.update(
            &roads,
            &Surroundings {
                player: Vec3::new(5000.0, 0.0, 5000.0),
                blocked: &[],
                ground: |_, _| None,
                taken: &[],
            },
            &[],
            4,
            0.1,
        );
        assert!(traffic.cars.is_empty());
    }
}
