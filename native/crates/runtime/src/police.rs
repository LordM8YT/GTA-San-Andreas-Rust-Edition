//! Police, as in SA: patrol cars drive among the traffic, crimes they see
//! give the player wanted stars, and with stars police cars come after the
//! player with sirens wailing and lights flashing. Close by, the officers get
//! out and run at the player; one who gets hold of them, on foot or in a car
//! that has stopped, makes the arrest. Busted, the player is taken to the
//! nearest police station and the stars are gone.
//!
//! The cars and officers are the game's own (`copcarla`, `lapd1` and the
//! rest, from the player's install), picked by the city the player is in.
//! Police cars are traffic cars with a patrol attached and officers are
//! pedestrians with a flag, so they are drawn, shot, run over and blown up
//! like everyone else, at next to no extra cost. A chase sends four cars at
//! most.
use super::State;
use crate::car_contacts::Footprint;
use crate::streaming::ORIGIN;
use crate::traffic::Surroundings;
use crate::wanted::{self, Crime, Wanted};
use glam::Vec3;
use sa_scene::collision::Player;

/// A cop this close to a crime sees it.
const WITNESS: f32 = 45.0;
/// A cop this close to a wanted player keeps them in sight.
const SIGHT: f32 = 70.0;
/// Seconds between police cars sent to join a chase.
const DISPATCH_INTERVAL: f32 = 3.0;
/// A chasing car that has pulled up this close lets its officers out.
const GET_OUT: f32 = 25.0;
/// Officers go back to their car once the suspect is this far away.
const BACK_TO_CAR: f32 = 45.0;
const RUN: f32 = 5.5;
const JOG: f32 = 2.2;
/// How close an officer must get to grab someone on foot, and for how long.
const GRAB: f32 = 1.3;
const ARREST_SECONDS: f32 = 0.8;
/// Hauling someone out of a car takes a little longer, and the car must
/// have all but stopped.
const CAR_ARREST_SECONDS: f32 = 1.2;
const CAR_ARREST_SPEED: f32 = 2.0;
/// Sirens heard at once; the nearest cars get them.
const SIRENS: usize = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Area {
    LosSantos,
    SanFierro,
    LasVenturas,
    Countryside,
}

impl Area {
    /// Roughly which city a point (runtime coordinates) lies in.
    fn at(point: Vec3) -> Self {
        let (x, y) = (point.x + ORIGIN[0], ORIGIN[1] - point.z);
        if x > 800.0 && y > 550.0 {
            Self::LasVenturas
        } else if x < -1250.0 && (-1100.0..1700.0).contains(&y) {
            Self::SanFierro
        } else if x > 50.0 && y < -700.0 {
            Self::LosSantos
        } else {
            Self::Countryside
        }
    }
    fn index(self) -> usize {
        self as usize
    }
}

/// Each area's police car and officer, in `Area` order.
const CARS: [&str; 4] = ["copcarla", "copcarsf", "copcarvg", "copcarru"];
const OFFICERS: [&str; 4] = ["lapd1", "sfpd1", "lvpd1", "csher"];

/// Outside the police stations, in SA map coordinates: Los Santos, San
/// Fierro, Las Venturas, then the small-town stations of Dillimore, Fort
/// Carson, El Quebrados and Angel Pine.
const STATIONS: [[f32; 2]; 7] = [
    [1540.0, -1675.0],
    [-1605.0, 720.0],
    [2287.0, 2420.0],
    [627.0, -585.0],
    [-217.0, 985.0],
    [-1390.0, 2645.0],
    [-2161.0, -2390.0],
];

/// The station a player busted at `point` (runtime coordinates) is taken to.
fn nearest_station(point: Vec3) -> [f32; 2] {
    let at = [point.x + ORIGIN[0], ORIGIN[1] - point.z];
    let distance = |s: &[f32; 2]| (s[0] - at[0]).powi(2) + (s[1] - at[1]).powi(2);
    *STATIONS
        .iter()
        .min_by(|a, b| distance(a).total_cmp(&distance(b)))
        .expect("stations")
}

/// An officer out of their car.
struct Officer {
    /// The pedestrian drawing them.
    walker: u64,
    /// The car they came in, to go back to.
    car: Option<u64>,
    body: Player,
    /// Seconds spent holding on to the suspect.
    grip: f32,
}

/// Where a siren comes from: one of our cars, or another player's.
#[derive(Clone, Copy, PartialEq, Eq)]
enum SirenSource {
    Local(u64),
    Remote(u32, u32),
}

#[derive(Default)]
pub(super) struct Police {
    pub wanted: Wanted,
    officers: Vec<Officer>,
    since_dispatch: f32,
    /// Catalog indices of each area's car and officer, and the catalog
    /// sizes they were looked up for.
    car_models: [Option<usize>; 4],
    officer_models: [Option<usize>; 4],
    models_for: (usize, usize),
    siren_sound: Option<sa_audio::SoundEffect>,
    sirens: Vec<(SirenSource, sa_audio::LoopEmitter)>,
}

/// What the police know about the player this frame.
struct Suspect {
    at: Vec3,
    /// The car they sit in, if any: its body and speed.
    car: Option<(Footprint, f32)>,
}

impl State {
    /// A crime by the player at `at`. It counts in full when a cop is close
    /// enough to see it.
    pub(super) fn report_crime(&mut self, crime: Crime, at: Vec3) {
        if !self.menu.settings.police {
            return;
        }
        let witnessed = self.cop_near(at, WITNESS);
        self.police.wanted.commit(crime, witnessed);
    }

    /// Whether a police car or an officer is within `radius` of `at`.
    fn cop_near(&self, at: Vec3, radius: f32) -> bool {
        self.traffic
            .cars
            .iter()
            .filter(|car| car.police.is_some() && !car.damage.wrecked)
            .any(|car| car.position.distance(at) < radius)
            || self
                .crowd
                .walkers
                .iter()
                .filter(|w| w.cop && w.dead_for.is_none())
                .any(|w| w.feet.distance(at) < radius)
    }

    fn suspect(&self) -> Suspect {
        match (&self.car, &self.player) {
            (Some((car, _)), _) if self.driving => {
                let ground = car.position - Vec3::Y * car.clearance;
                Suspect {
                    at: ground,
                    car: Some((Footprint::car(ground, car.yaw, car.size), car.speed.abs())),
                }
            }
            (_, Some(player)) if self.walking => Suspect {
                at: player.feet,
                car: None,
            },
            _ => Suspect {
                at: self.position - Vec3::Y * 1.6,
                car: None,
            },
        }
    }

    fn refresh_police_models(&mut self) {
        let find =
            |names: &[String], models: &std::collections::HashMap<String, String>, wanted: &str| {
                names
                    .iter()
                    .position(|name| models.get(name).is_some_and(|m| m == wanted))
            };
        let police = &mut self.police;
        for area in 0..4 {
            police.car_models[area] = find(&self.menu.cars, &self.original_car_models, CARS[area]);
            police.officer_models[area] =
                find(&self.menu.peds, &self.original_ped_models, OFFICERS[area]);
        }
        police.models_for = (self.menu.cars.len(), self.menu.peds.len());
    }

    /// The area's own police car, or any police car this install has.
    fn police_car_model(&self, area: Area) -> Option<usize> {
        let models = &self.police.car_models;
        models[area.index()].or_else(|| models.iter().flatten().next().copied())
    }

    fn officer_model(&self, area: Area) -> Option<usize> {
        let models = &self.police.officer_models;
        models[area.index()].or_else(|| models.iter().flatten().next().copied())
    }

    /// Turn the police off: no stars, no chase, no officers, no sirens.
    fn stand_police_down(&mut self) {
        self.police.wanted.clear();
        self.traffic.chase = None;
        self.traffic.patrol_models.clear();
        self.crowd.injuries.clear();
        self.recall_officers();
        self.police.sirens.clear();
        self.menu.wanted = (0, false);
    }

    /// Every officer on foot gets back in their car.
    fn recall_officers(&mut self) {
        for officer in std::mem::take(&mut self.police.officers) {
            self.crowd.walkers.retain(|w| w.id != officer.walker);
            if let Some(police) = officer.car.and_then(|id| {
                self.traffic
                    .cars
                    .iter_mut()
                    .find(|car| car.id == id)
                    .and_then(|car| car.police.as_mut())
            }) {
                police.out = police.out.saturating_sub(1);
                police.crew += 1;
            }
        }
    }

    pub(super) fn update_police(&mut self, dt: f32) {
        if self.police.models_for != (self.menu.cars.len(), self.menu.peds.len()) {
            self.refresh_police_models();
        }
        if !self.menu.settings.police {
            if self.police.wanted.level() > 0 || !self.police.officers.is_empty() {
                self.stand_police_down();
            }
            self.traffic.patrol_models.clear();
            self.crowd.injuries.clear();
            return;
        }
        // Everyone the player hurt since last frame.
        for injury in std::mem::take(&mut self.crowd.injuries) {
            let crime = match (injury.cop, injury.killed) {
                (true, true) => Crime::KillCop,
                (true, false) => Crime::HitCop,
                (false, true) => Crime::KillPerson,
                (false, false) => Crime::HitPerson,
            };
            self.report_crime(crime, injury.at);
        }
        let suspect = self.suspect();
        let area = Area::at(suspect.at);
        self.traffic.patrol_models = self.police_car_model(area).into_iter().collect();
        let seen = self.cop_near(suspect.at, SIGHT);
        self.police.wanted.update(seen, dt);
        let level = self.police.wanted.level();
        self.traffic.chase = (level > 0).then_some(suspect.at);
        let blocked: Vec<Footprint> = match suspect.car {
            Some((body, _)) => vec![body],
            None => vec![Footprint::person(suspect.at)],
        };
        self.police.since_dispatch += dt;
        let chasing = self
            .traffic
            .cars
            .iter()
            .filter(|car| car.chasing() && !car.damage.wrecked)
            .count();
        let mut dispatched = None;
        if let (Some(roads), Some(model)) = (self.roads.as_ref(), self.police_car_model(area)) {
            if chasing < wanted::pursuit_cars(level)
                && self.police.since_dispatch >= DISPATCH_INTERVAL
                && self.interior == 0
            {
                self.police.since_dispatch = 0.0;
                let collision = self.collision.as_ref();
                dispatched = self.traffic.add_pursuer(
                    roads,
                    &Surroundings {
                        player: suspect.at,
                        blocked: &blocked,
                        ground: |at, max| collision?.ground_below(at, max),
                        taken: &[],
                    },
                    model,
                    wanted::crew(level),
                );
            }
            let collision = self.collision.as_ref();
            self.traffic.pursue(
                roads,
                suspect.at,
                &blocked,
                &|at: Vec3, max: f32| collision?.ground_below(at, max),
                &|from: Vec3, to: Vec3| {
                    collision.is_some_and(|world| world.clip_camera(from, to).distance(to) > 0.3)
                },
                dt,
            );
        }
        if let Some(index) = dispatched {
            self.mesh_traffic_car(index);
        }
        if level > 0 {
            self.let_officers_out(&suspect, area);
        }
        if self.move_officers(&suspect, level, dt) {
            self.bust(suspect.at);
        }
        self.menu.wanted = (self.police.wanted.level(), self.police.wanted.flashing());
        self.update_sirens(dt);
    }

    /// Officers climb out of chasing cars that have pulled up near the suspect.
    fn let_officers_out(&mut self, suspect: &Suspect, area: Area) {
        let Some(model) = self.officer_model(area) else {
            return;
        };
        let mut leaving = Vec::new();
        for car in &mut self.traffic.cars {
            let Some(police) = car.police.as_mut() else {
                continue;
            };
            let close = Vec3::new(
                car.position.x - suspect.at.x,
                0.0,
                car.position.z - suspect.at.z,
            )
            .length()
                < GET_OUT;
            if !police.chasing
                || !police.off_road
                || police.crew == 0
                || police.stopped_for < 0.4
                || car.damage.wrecked
                || !close
            {
                continue;
            }
            let forward = Vec3::new(car.yaw.sin(), 0.0, car.yaw.cos());
            let left = Vec3::new(forward.z, 0.0, -forward.x);
            for seat in 0..police.crew {
                // Driver's side first, then the passenger's.
                let side = if seat == 0 { -1.0 } else { 1.0 };
                let feet = car.position + left * side * (car.size[0] + 0.6) + forward * 0.4;
                leaving.push((car.id, feet, car.yaw));
            }
            police.out += police.crew;
            police.crew = 0;
        }
        for (car, feet, yaw) in leaving {
            let feet = self
                .collision
                .as_ref()
                .and_then(|world| world.ground_below(feet + Vec3::Y * 1.5, feet.y + 1.5))
                .filter(|y| (y - feet.y).abs() < 2.0)
                .map_or(feet, |y| Vec3::new(feet.x, y, feet.z));
            let index = self.crowd.add_cop(model, feet, yaw);
            let walker = self.crowd.walkers[index].id;
            if self.mesh_walker(index) {
                self.police.officers.push(Officer {
                    walker,
                    car: Some(car),
                    body: Player {
                        feet,
                        vertical_speed: 0.0,
                        grounded: true,
                    },
                    grip: 0.0,
                });
            } else if let Some(police) = self
                .traffic
                .cars
                .iter_mut()
                .find(|c| c.id == car)
                .and_then(|c| c.police.as_mut())
            {
                police.out -= 1;
                police.crew += 1;
            }
        }
    }

    /// Officers on foot run at the suspect, or back to their car once the
    /// suspect is out of reach or no longer wanted. Returns true when one of
    /// them makes the arrest.
    fn move_officers(&mut self, suspect: &Suspect, level: u8, dt: f32) -> bool {
        let mut arrested = false;
        let mut officers = std::mem::take(&mut self.police.officers);
        officers.retain_mut(|officer| {
            let car = officer.car.and_then(|id| {
                self.traffic
                    .cars
                    .iter()
                    .position(|car| car.id == id && !car.damage.wrecked)
            });
            let Some(index) = self
                .crowd
                .walkers
                .iter()
                .position(|w| w.id == officer.walker && w.dead_for.is_none())
            else {
                // Down, or cleared away: their car stops waiting for them.
                if let Some(police) = car.and_then(|i| self.traffic.cars[i].police.as_mut()) {
                    police.out = police.out.saturating_sub(1);
                }
                return false;
            };
            let flat = |a: Vec3, b: Vec3| Vec3::new(b.x - a.x, 0.0, b.z - a.z);
            let feet = officer.body.feet;
            let to_suspect = flat(feet, suspect.at);
            let chase = level > 0 && (to_suspect.length() < BACK_TO_CAR || car.is_none());
            let (toward, speed) = if chase {
                let distance = to_suspect.length();
                let reach = match suspect.car {
                    Some((body, _)) => body.half.y,
                    None => GRAB * 0.8,
                };
                let speed = if distance > 5.0 {
                    RUN
                } else if distance > reach {
                    JOG
                } else {
                    0.0
                };
                (to_suspect, speed)
            } else if let Some(car) = car {
                let to_car = flat(feet, self.traffic.cars[car].position);
                if to_car.length() < 3.0 {
                    // Back in the car.
                    if let Some(police) = self.traffic.cars[car].police.as_mut() {
                        police.out = police.out.saturating_sub(1);
                        police.crew += 1;
                    }
                    self.crowd.walkers.remove(index);
                    return false;
                }
                (to_car, RUN * 0.8)
            } else {
                (to_suspect, 0.0)
            };
            let direction = toward.normalize_or_zero();
            match &self.collision {
                Some(world) => officer.body.step(world, direction * speed, false, dt),
                None => officer.body.feet += direction * speed * dt,
            }
            let walker = &mut self.crowd.walkers[index];
            let moved = flat(walker.feet, officer.body.feet).length();
            walker.feet = officer.body.feet;
            walker.speed = if dt > 0.0 { (moved / dt).min(RUN) } else { 0.0 };
            if direction != Vec3::ZERO {
                walker.yaw = direction.x.atan2(direction.z);
            }
            // Hold on to the suspect: on foot when close, in a car when it
            // has all but stopped and the officer is at its side.
            let distance = to_suspect.length();
            // Nobody grabs a player hovering overhead in free-fly.
            let level_with = (suspect.at.y - officer.body.feet.y).abs() < 2.5;
            let (holding, needed) = match suspect.car {
                Some((body, speed)) => (
                    speed < CAR_ARREST_SPEED && distance < body.half.y + 0.6,
                    CAR_ARREST_SECONDS,
                ),
                None => (distance < GRAB, ARREST_SECONDS),
            };
            officer.grip = if chase && holding && level_with {
                officer.grip + dt
            } else {
                (officer.grip - dt).max(0.0)
            };
            arrested |= officer.grip >= needed;
            true
        });
        self.police.officers = officers;
        arrested
    }

    /// Busted: the stars are gone and the player wakes up outside the
    /// nearest police station. Their car stays where it was.
    fn bust(&mut self, at: Vec3) {
        self.police.wanted.clear();
        self.traffic.chase = None;
        self.recall_officers();
        if self.driving {
            self.bail_out();
        }
        // On foot at the station, even if there was no room to climb out.
        self.driving = false;
        self.walking = true;
        self.leave_passenger();
        self.interior_destination = None;
        self.destination = Some(nearest_station(at));
        self.menu.busted = true;
        self.menu.wanted = (0, false);
        self.chat
            .log("Busted. The police took you to the nearest station.".to_owned());
    }

    /// Sirens on the nearest police cars that have them on, ours or other
    /// players'.
    fn update_sirens(&mut self, dt: f32) {
        let Some(audio) = &mut self.audio else {
            return;
        };
        let mut sources: Vec<(SirenSource, Vec3)> = self
            .traffic
            .cars
            .iter()
            .filter(|car| car.siren())
            .map(|car| (SirenSource::Local(car.id), car.position))
            .chain(
                self.remote_traffic
                    .iter()
                    .filter(|car| car.siren())
                    .map(|car| (SirenSource::Remote(car.owner, car.id), car.position)),
            )
            .collect();
        if dt <= 0.0 || self.animation.is_some() {
            sources.clear();
        }
        let listener = self.position;
        sources.sort_by(|a, b| {
            a.1.distance_squared(listener)
                .total_cmp(&b.1.distance_squared(listener))
        });
        sources.truncate(SIRENS);
        let police = &mut self.police;
        police
            .sirens
            .retain(|(source, _)| sources.iter().any(|(s, _)| s == source));
        let sound = police
            .siren_sound
            .get_or_insert_with(sa_audio::SoundEffect::siren);
        for (source, at) in sources {
            let position = (at + Vec3::Y * 1.5).to_array();
            match police.sirens.iter_mut().find(|(s, _)| *s == source) {
                Some((_, emitter)) => {
                    if let Err(error) = emitter.update(position) {
                        eprintln!("Siren unavailable: {error:#}");
                    }
                }
                None => match audio.loop_emitter(sound, position) {
                    Ok(emitter) => police.sirens.push((source, emitter)),
                    Err(error) => eprintln!("Siren unavailable: {error:#}"),
                },
            }
        }
    }

    /// The flashing red and blue lights on the roofs of police cars with
    /// their sirens on: where, how big and what colour.
    pub(super) fn light_bars(&self, time: f32) -> Vec<(Vec3, f32, [f32; 4])> {
        let cars = self
            .traffic
            .cars
            .iter()
            .filter(|car| car.siren())
            .map(|car| (car.id as u32, car.position, car.yaw, car.size))
            .chain(
                self.remote_traffic
                    .iter()
                    .filter(|car| car.siren())
                    .map(|car| (car.id, car.position, car.yaw, car.size)),
            );
        let mut lights = Vec::new();
        for (id, position, yaw, size) in cars {
            let forward = Vec3::new(yaw.sin(), 0.0, yaw.cos());
            let left = Vec3::new(forward.z, 0.0, -forward.x);
            let roof = position + Vec3::Y * (size[2] + 0.1) - forward * 0.2;
            // Left and right take turns, each car on its own beat.
            let red_on = (time * 3.0 + (id % 7) as f32 * 0.3).fract() < 0.5;
            let bright = |on: bool| if on { 1.0 } else { 0.25 };
            lights.push((
                roof + left * 0.45,
                0.9 * bright(red_on),
                [2.2 * bright(red_on), 0.15, 0.1, 0.5],
            ));
            lights.push((
                roof - left * 0.45,
                0.9 * bright(!red_on),
                [0.1, 0.35, 2.4 * bright(!red_on), 0.5],
            ));
        }
        lights
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn runtime(x: f32, y: f32) -> Vec3 {
        Vec3::new(x - ORIGIN[0], 0.0, ORIGIN[1] - y)
    }

    #[test]
    fn each_city_has_its_own_police() {
        assert_eq!(Area::at(runtime(2495.0, -1687.0)), Area::LosSantos);
        assert_eq!(Area::at(runtime(-2000.0, 300.0)), Area::SanFierro);
        assert_eq!(Area::at(runtime(2000.0, 1500.0)), Area::LasVenturas);
        assert_eq!(Area::at(runtime(200.0, -500.0)), Area::Countryside);
        assert_eq!(Area::at(runtime(-2161.0, -2390.0)), Area::Countryside);
        assert_eq!(CARS[Area::SanFierro.index()], "copcarsf");
        assert_eq!(OFFICERS[Area::LasVenturas.index()], "lvpd1");
    }

    #[test]
    fn busted_players_go_to_the_nearest_station() {
        assert_eq!(nearest_station(runtime(2495.0, -1687.0)), STATIONS[0]);
        assert_eq!(nearest_station(runtime(-2000.0, 300.0)), STATIONS[1]);
        assert_eq!(nearest_station(runtime(700.0, -500.0)), STATIONS[3]);
    }
}
