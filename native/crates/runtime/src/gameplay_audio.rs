use super::State;
use glam::Vec3;
use sa_audio::{
    gameplay::{EngineKind, GameplaySounds},
    EngineEmitter,
};
use std::collections::HashMap;

#[derive(Default)]
pub(super) struct GameplayAudio {
    pub sounds: Option<GameplaySounds>,
    engines: HashMap<u32, (EngineKind, EngineEmitter)>,
    steps: StepDistance,
    step_index: usize,
    pub engine_starts: u64,
    pub footsteps: u64,
}

#[derive(Default)]
struct StepDistance {
    previous: Option<Vec3>,
    distance: f32,
}
impl StepDistance {
    fn step(&mut self, feet: Option<Vec3>, dt: f32) -> bool {
        let previous = self.previous;
        self.previous = feet;
        let Some((previous, feet)) = previous.zip(feet) else {
            self.distance = 0.0;
            return false;
        };
        let delta = feet - previous;
        let distance = Vec3::new(delta.x, 0.0, delta.z).length();
        if dt <= 0.0 || dt > 0.25 || distance > 3.0 || delta.y.abs() > 1.0 {
            self.distance = 0.0;
            return false;
        }
        let stride = if distance / dt > 5.0 { 2.4 } else { 1.4 };
        self.distance += distance;
        if self.distance >= stride {
            self.distance %= stride;
            true
        } else {
            false
        }
    }
}

fn engine_kind(name: Option<&String>) -> EngineKind {
    if name.is_some_and(|name| name.eq_ignore_ascii_case("Infernus")) {
        EngineKind::Sports
    } else {
        EngineKind::Sedan
    }
}

impl State {
    pub(super) fn update_gameplay_audio(&mut self, dt: f32) {
        let Some(audio) = &mut self.audio else { return };
        let sounds = &mut self.gameplay_audio;
        let Some(bank) = &sounds.sounds else { return };
        // Menus/preparation mute gameplay voices while menu cues keep working.
        let active = dt > 0.0 && self.animation.is_none() && self.resource_installing.is_none();
        let mut cars = Vec::with_capacity(21);
        if active && self.driving {
            if let Some((car, _)) = &self.car {
                let throttle = (f32::from(self.keys.contains(&winit::keyboard::KeyCode::KeyW))
                    - f32::from(self.keys.contains(&winit::keyboard::KeyCode::KeyS))
                    + self.gamepad.throttle)
                    .clamp(-1.0, 1.0);
                cars.push((
                    u32::MAX,
                    engine_kind(self.menu.cars.get(self.active_car)),
                    car.position,
                    car.speed,
                    throttle,
                ));
            }
        }
        if active {
            for actor in &self.remote_actors {
                if !actor.current.driving {
                    continue;
                }
                let Some(car) = actor.current.vehicle else {
                    continue;
                };
                let position = Vec3::from_array(car.position);
                if car.interior == self.interior
                    && position.distance_squared(self.position) < 65.0 * 65.0
                {
                    cars.push((
                        actor.id,
                        engine_kind(self.menu.cars.get(actor.car_model)),
                        position,
                        car.speed,
                        0.0,
                    ));
                }
            }
        }
        cars.sort_by(|a, b| {
            a.2.distance_squared(self.position)
                .total_cmp(&b.2.distance_squared(self.position))
        });
        cars.truncate(8);
        sounds
            .engines
            .retain(|id, _| cars.iter().any(|car| car.0 == *id));
        for (id, kind, position, speed, throttle) in cars {
            if sounds.engines.get(&id).is_some_and(|voice| voice.0 != kind) {
                sounds.engines.remove(&id);
            }
            if let std::collections::hash_map::Entry::Vacant(entry) = sounds.engines.entry(id) {
                match audio.engine_emitter(bank.engine(kind), position.to_array()) {
                    Ok(voice) => {
                        entry.insert((kind, voice));
                        sounds.engine_starts += 1;
                    }
                    Err(error) => {
                        eprintln!("Engine audio unavailable: {error:#}");
                    }
                }
            }
            if let Some((_, voice)) = sounds.engines.get_mut(&id) {
                let _ = voice.update(position.to_array(), speed, throttle);
            }
        }
        let feet = self
            .player
            .as_ref()
            .filter(|player| {
                active
                    && self.walking
                    && !self.driving
                    && self.passenger.is_none()
                    && player.grounded
            })
            .map(|player| player.feet);
        if sounds.steps.step(feet, dt) {
            if let Some(feet) = feet {
                match audio.play_spatial(bank.footstep(sounds.step_index), feet.to_array()) {
                    Ok(()) => {
                        sounds.footsteps += 1;
                    }
                    Err(error) => {
                        eprintln!("Footstep audio unavailable: {error:#}");
                    }
                }
                sounds.step_index = sounds.step_index.wrapping_add(1);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn footsteps_use_ground_distance_and_ignore_teleports_air_and_pauses() {
        let mut steps = StepDistance::default();
        assert!(!steps.step(Some(Vec3::ZERO), 0.1));
        assert!(!steps.step(Some(Vec3::X * 0.5), 0.1));
        assert!(!steps.step(Some(Vec3::X), 0.1));
        assert!(steps.step(Some(Vec3::X * 1.5), 0.1));
        for _ in 0..10 {
            assert!(!steps.step(Some(Vec3::X * 1.5), 0.1));
        }
        assert!(!steps.step(Some(Vec3::X * 100.0), 0.1));
        assert!(!steps.step(None, 0.1));
        assert!(!steps.step(Some(Vec3::ZERO), 0.1));
        assert!(!steps.step(Some(Vec3::X), 0.0));
        assert!(!steps.step(Some(Vec3::X * 1.5), 0.1));
    }
}
