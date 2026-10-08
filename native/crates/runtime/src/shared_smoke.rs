//! Same-machine GPU integration choreography; never used during normal play.
use crate::{menu, State};
use glam::Vec3;
use std::{collections::HashSet, path::Path, time::Instant};
use winit::keyboard::KeyCode;

pub struct SharedSmoke {
    stage: u8,
    phase: Instant,
    started: Instant,
    vehicle: Option<u32>,
    regions: HashSet<(i32, i32)>,
}
impl Default for SharedSmoke {
    fn default() -> Self {
        Self {
            stage: 0,
            phase: Instant::now(),
            started: Instant::now(),
            vehicle: None,
            regions: HashSet::new(),
        }
    }
}
impl SharedSmoke {
    fn advance(&mut self, stage: u8, state: &State) {
        self.stage = stage;
        self.phase = Instant::now();
        eprintln!(
            "Shared car GPU smoke {} stage {stage}; vehicle {:?}; region {:?}",
            state.menu.player_name, self.vehicle, state.region
        );
    }
    fn capture(state: &mut State, directory: Option<&Path>, name: &str) {
        if let Some(directory) = directory {
            state.capture_next = Some(directory.join(name));
        }
    }
    fn approach(state: &mut State, id: u32) -> bool {
        let Some(pose) = state
            .remote_actors
            .iter()
            .find(|a| a.id == id)
            .and_then(|a| a.target.vehicle)
        else {
            return false;
        };
        let Some(world) = &state.collision else {
            return false;
        };
        let mut car = sa_scene::vehicle::Car::new(Vec3::from_array(pose.position), 0.0);
        car.yaw = pose.yaw;
        let Some(player) = car.exit_player(world) else {
            return false;
        };
        state.position = player.eye();
        state.player = Some(player);
        state.walking = true;
        true
    }
    pub fn tick(&mut self, state: &mut State, rendered: bool, directory: Option<&Path>) -> bool {
        assert!(
            self.started.elapsed().as_secs() < 100,
            "shared car smoke timed out at stage {}: {} / {}",
            self.stage,
            state.menu.network_status,
            state.menu.ride_status
        );
        if !rendered {
            return false;
        }
        self.regions.insert((
            state.region[0].round() as i32,
            state.region[1].round() as i32,
        ));
        state.keys.clear();
        let host = state.menu.player_name == "HostTest";
        let elapsed = self.phase.elapsed().as_secs_f32();
        if self.stage == 0 {
            if state.menu.network_ready && state.menu.network_players.len() == 2 {
                if host {
                    // Known outdoor road, original collision support, westbound.
                    let player = sa_scene::collision::Player::spawn(
                        state.collision.as_ref().unwrap(),
                        Vec3::new(0.0, 40.0, 60.0),
                    );
                    state.position = player.eye();
                    state.player = Some(player);
                    state.walking = true;
                    state.yaw = -std::f32::consts::FRAC_PI_2;
                    let index = state
                        .menu
                        .cars
                        .iter()
                        .position(|n| n == "Infernus")
                        .unwrap();
                    state.apply_menu_action(Some(menu::Action::Car(index)));
                }
                self.advance(1, state);
            }
            return false;
        }
        if host {
            match self.stage {
                1 => {
                    if let Some(grant) = state.driver_grant {
                        self.vehicle = Some(grant.vehicle);
                        if elapsed < 4.0 {
                            state.keys.insert(KeyCode::KeyW);
                        } else {
                            state.car.as_mut().unwrap().0.stop();
                            state.toggle_car();
                            if !state.driving {
                                self.advance(2, state);
                            }
                        }
                    }
                }
                2 => {
                    if self.vehicle.is_some_and(|id| {
                        state
                            .remote_actors
                            .iter()
                            .any(|a| a.id == id && a.target.driving)
                    }) && Self::approach(state, self.vehicle.unwrap())
                    {
                        assert!(state.try_passenger(true));
                        self.advance(3, state);
                    }
                }
                3 => {
                    if let Some(seat) = state.passenger {
                        assert_eq!(
                            Some(seat.owner),
                            self.vehicle,
                            "host boarded a different vehicle"
                        );
                        Self::capture(state, directory, "shared-passenger.png");
                        self.advance(4, state);
                    }
                }
                4 => {
                    if state.menu.network_players.len() == 1 {
                        assert!(
                            state.passenger.is_some(),
                            "driver disconnect removed passenger reservation"
                        );
                        let car = state
                            .remote_actors
                            .iter()
                            .find(|a| Some(a.id) == self.vehicle)
                            .unwrap();
                        assert!(
                            !car.target.driving && car.target.vehicle.unwrap().speed == 0.0,
                            "disconnected driver's vehicle did not park"
                        );
                        assert!(
                            self.regions.len() >= 2,
                            "passenger did not stream into another region"
                        );
                        state.try_leave_passenger();
                        self.advance(5, state);
                    }
                }
                5 => {
                    if state.passenger.is_none()
                        && state.ride_request.is_none()
                        && Self::approach(state, self.vehicle.unwrap())
                    {
                        state.toggle_car();
                        self.advance(6, state);
                    }
                }
                6 => {
                    if let Some(grant) = state.driver_grant {
                        assert_eq!(
                            Some(grant.vehicle),
                            self.vehicle,
                            "parked car ID changed after disconnect"
                        );
                        if state.menu.network_players.len() == 2 {
                            Self::capture(state, directory, "shared-rejoined.png");
                            self.advance(7, state);
                        }
                    }
                }
                7 => {
                    if elapsed > 2.0 && state.capture_next.is_none() {
                        state.disconnect_network();
                        self.advance(8, state);
                        Self::capture(state, directory, "offline-restored.png");
                    }
                }
                8 => {
                    if state.capture_next.is_none() {
                        return true;
                    }
                }
                _ => unreachable!(),
            }
        } else {
            match self.stage {
                1 => {
                    if self.vehicle.is_none() {
                        self.vehicle = state
                            .remote_actors
                            .iter()
                            .find(|a| {
                                a.id >= sa_net::vehicles::FIRST_VEHICLE_ID
                                    && a.target.vehicle.is_some()
                            })
                            .map(|a| a.id);
                    }
                    if let Some(id) = self.vehicle {
                        if state
                            .remote_actors
                            .iter()
                            .any(|a| a.id == id && !a.target.driving)
                            && Self::approach(state, id)
                        {
                            state.toggle_car();
                            self.advance(2, state);
                        }
                    }
                }
                2 => {
                    if let Some(grant) = state.driver_grant {
                        assert_eq!(
                            Some(grant.vehicle),
                            self.vehicle,
                            "guest took a different car"
                        );
                        if state
                            .remote_actors
                            .iter()
                            .any(|a| a.target.ride.is_some_and(|s| Some(s.owner) == self.vehicle))
                        {
                            Self::capture(state, directory, "shared-driver.png");
                            self.advance(3, state);
                        }
                    }
                }
                3 => {
                    state.keys.insert(KeyCode::KeyW);
                    if elapsed > 20.0 {
                        assert!(
                            self.regions.len() >= 2,
                            "driver did not stream into another region; car at {:?}",
                            state.car.as_ref().map(|c| c.0.position)
                        );
                        eprintln!(
                            "Shared drive streamed {} regions; car {:?}",
                            self.regions.len(),
                            state.car.as_ref().map(|c| c.0.position)
                        );
                        eprintln!(
                            "Shared drive final sample: {}",
                            state.metrics.text(
                                state.batches.len(),
                                state.loading(),
                                true,
                                state.offline_world.is_some()
                            )
                        );
                        state.disconnect_network();
                        self.advance(4, state);
                        Self::capture(state, directory, "shared-disconnected-offline.png");
                    }
                }
                4 => {
                    if elapsed > 3.0 && state.capture_next.is_none() {
                        assert!(state.network_session.is_none() && state.offline_world.is_none());
                        state.auto_enter = true;
                        state.network_action(false);
                        self.advance(5, state);
                    }
                }
                5 => {
                    if state.menu.network_ready
                        && state
                            .remote_actors
                            .iter()
                            .any(|a| Some(a.id) == self.vehicle && a.target.driving)
                    {
                        assert!(
                            state.driver_grant.is_none() && state.passenger.is_none(),
                            "rejoin inherited a former seat"
                        );
                        Self::capture(state, directory, "shared-rejoined.png");
                        self.advance(7, state);
                    }
                }
                7 => {
                    if elapsed > 2.0 && state.capture_next.is_none() {
                        state.disconnect_network();
                        self.advance(8, state);
                        Self::capture(state, directory, "offline-restored.png");
                    }
                }
                8 => {
                    if state.capture_next.is_none() {
                        return true;
                    }
                }
                _ => unreachable!(),
            }
        }
        false
    }
}
