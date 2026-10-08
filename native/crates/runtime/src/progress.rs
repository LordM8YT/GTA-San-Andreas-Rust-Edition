//! Small offline checkpoints, kept separate from settings and server resources.
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{Read, Write},
    path::Path,
};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Save {
    pub version: u32,
    /// Original SA coordinates: east, north, height at the player's feet.
    pub position: [f32; 3],
    pub yaw: f32,
    pub pitch: f32,
    pub car: String,
    pub ped: String,
    pub clothes: Vec<(String, bool)>,
}
impl Save {
    pub fn center(&self) -> [f32; 2] {
        [self.position[0], self.position[1]]
    }
    pub fn player(
        &self,
        world: &sa_scene::collision::CollisionWorld,
    ) -> Option<sa_scene::collision::Player> {
        let feet = glam::Vec3::new(
            self.position[0] - crate::ORIGIN[0],
            self.position[2],
            crate::ORIGIN[1] - self.position[1],
        );
        world
            .standing_at(feet, feet.y + 0.5)
            .filter(|p| (p.feet.y - feet.y).abs() < 0.75)
    }
    fn validate(&self) -> Result<()> {
        ensure!(self.version == 1, "Unsupported free-roam save version");
        ensure!(
            self.position
                .iter()
                .all(|v| v.is_finite() && v.abs() < 10000.0)
                && self.position[2] > -100.0
                && self.position[2] < 2000.0,
            "Invalid saved position"
        );
        ensure!(
            self.yaw.is_finite()
                && self.yaw.abs() < 100000.0
                && self.pitch.is_finite()
                && self.pitch.abs() <= 1.5,
            "Invalid saved camera"
        );
        let valid_name = |name: &str| {
            !name.trim().is_empty()
                && name.chars().count() <= 128
                && !name.chars().any(char::is_control)
        };
        ensure!(
            valid_name(&self.car)
                && valid_name(&self.ped)
                && self.clothes.len() <= 16
                && self.clothes.iter().all(|(name, _)| valid_name(name)),
            "Invalid saved appearance"
        );
        Ok(())
    }
    pub fn load(path: &Path) -> Result<Option<Self>> {
        let file = match fs::File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        let mut data = Vec::new();
        file.take(16 * 1024 + 1).read_to_end(&mut data)?;
        ensure!(data.len() <= 16 * 1024, "Free-roam save exceeds 16 KiB");
        let save: Self = serde_json::from_slice(&data)?;
        save.validate()?;
        Ok(Some(save))
    }
    pub fn write(&self, path: &Path) -> Result<()> {
        self.validate()?;
        let data = serde_json::to_vec_pretty(self)?;
        ensure!(data.len() <= 16 * 1024, "Free-roam save exceeds 16 KiB");
        let parent = path.parent().context("Save path has no parent")?;
        fs::create_dir_all(parent)?;
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos();
        let staging = parent.join(format!(".progress-{}-{nonce}.tmp", std::process::id()));
        let result = (|| {
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&staging)?;
            file.write_all(&data)?;
            file.sync_all()?;
            drop(file);
            fs::rename(&staging, path)?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(staging);
        }
        result
    }
}

/// Catalog indices can change when packs are added. Ambiguous labels fall back.
pub(super) fn unique_index(names: &[String], selected: &str) -> Option<usize> {
    let mut matches = names
        .iter()
        .enumerate()
        .filter(|(_, name)| name.as_str() == selected);
    let index = matches.next()?.0;
    matches.next().is_none().then_some(index)
}

impl crate::State {
    fn progress_snapshot(&self) -> Option<Save> {
        if !self.menu.has_played
            || self.interior != 0
            || !self.walking
            || self.network_session.is_some()
            || self.offline_world.is_some()
            || self.resource_job.is_some()
            || self.resource_installing.is_some()
            || self.destination.is_some()
            || self.loading()
        {
            return None;
        }
        let world = self.collision.as_ref()?;
        let player = if self.driving {
            self.car.as_ref()?.0.exit_player(world)?
        } else {
            let current = self.player.as_ref()?;
            if !current.grounded {
                return None;
            }
            world.standing_at(current.feet, current.feet.y + 0.1)?
        };
        Some(Save {
            version: 1,
            position: [
                crate::ORIGIN[0] + player.feet.x,
                crate::ORIGIN[1] - player.feet.z,
                player.feet.y,
            ],
            yaw: self.yaw.rem_euclid(std::f32::consts::TAU),
            pitch: self.pitch,
            car: self.menu.cars.get(self.active_car)?.clone(),
            ped: self.menu.peds.get(self.active_ped)?.clone(),
            clothes: self.menu.clothes.clone(),
        })
    }
    pub(super) fn checkpoint_progress(&mut self) {
        if self
            .progress_retry
            .is_some_and(|retry| std::time::Instant::now() < retry)
        {
            return;
        }
        let Some(path) = &self.progress_path else {
            return;
        };
        let Some(save) = self.progress_snapshot() else {
            return;
        };
        if self.last_progress.as_ref() == Some(&save) {
            return;
        }
        match save.write(path) {
            Ok(()) => {
                self.progress_retry = None;
                self.last_progress = Some(save);
                self.menu.has_checkpoint = true;
            }
            Err(error) => {
                self.progress_retry =
                    Some(std::time::Instant::now() + std::time::Duration::from_secs(5));
                self.menu.message = "Could not save free roam. Check the save folder.".into();
                eprintln!("Free-roam checkpoint failed: {error:#}");
            }
        }
    }
    pub(super) fn restore_progress(&mut self, save: &Save) -> bool {
        let Some(player) = self.collision.as_ref().and_then(|world| save.player(world)) else {
            return false;
        };
        self.position = player.eye();
        self.player = Some(player);
        self.walking = true;
        self.yaw = save.yaw;
        self.pitch = save.pitch;
        self.region = save.center();
        let car_index = unique_index(&self.menu.cars, &save.car);
        let ped_index = unique_index(&self.menu.peds, &save.ped);
        if let Some(index) = car_index {
            self.apply_menu_action(Some(crate::menu::Action::Car(index)));
        }
        self.driving = false;
        if let Some((car, _)) = &mut self.car {
            car.stop();
        }
        if let Some(index) = ped_index {
            self.apply_menu_action(Some(crate::menu::Action::Ped(index)));
            let names: Vec<_> = self
                .menu
                .clothes
                .iter()
                .map(|(name, _)| name.clone())
                .collect();
            for (name, enabled) in &save.clothes {
                if let Some(index) = unique_index(&names, name) {
                    self.apply_menu_action(Some(crate::menu::Action::Clothing(index, *enabled)));
                }
            }
        }
        if car_index.is_none_or(|index| index != self.active_car) || ped_index.is_none() {
            self.menu.message =
                "Position restored. An unavailable saved model uses the current default.".into();
        }
        self.menu.has_played = true;
        self.menu.has_checkpoint = true;
        self.menu.open(crate::menu::Page::Main);
        self.capture(false);
        eprintln!(
            "Restored offline free-roam checkpoint at {:?}",
            save.position
        );
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Save {
        Save {
            version: 1,
            position: [2490.0, -1665.0, 13.2],
            yaw: 1.0,
            pitch: 0.2,
            car: "Taxi".into(),
            ped: "Grove".into(),
            clothes: vec![("Hat".into(), false)],
        }
    }
    #[test]
    fn checkpoint_replaces_atomically_and_invalid_updates_keep_previous_save() {
        let path = std::env::temp_dir().join(format!(
            "sa-save-test-{}-{}.json",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut save = fixture();
        assert!(Save::load(&path).unwrap().is_none());
        save.write(&path).unwrap();
        save.position[0] += 20.0;
        save.write(&path).unwrap();
        assert_eq!(Save::load(&path).unwrap(), Some(save.clone()));
        save.pitch = f32::NAN;
        assert!(save.write(&path).is_err());
        assert_eq!(Save::load(&path).unwrap().unwrap().position[0], 2510.0);
        fs::write(&path, b"{\"version\":99}").unwrap();
        assert!(Save::load(&path).is_err());
        fs::remove_file(path).unwrap();
    }
    #[test]
    fn removed_or_duplicate_catalog_names_never_select_an_unrelated_model() {
        let names = vec!["Custom".into(), "Taxi".into(), "Custom".into()];
        assert_eq!(unique_index(&names, "Taxi"), Some(1));
        assert_eq!(unique_index(&names, "Custom"), None);
        assert_eq!(unique_index(&names, "Removed"), None);
        for position in [[f32::INFINITY, 0.0, 0.0], [0.0, 0.0, -200.0]] {
            let mut save = fixture();
            save.position = position;
            assert!(save.validate().is_err());
        }
    }
    #[test]
    fn resume_checks_current_floor_height_and_real_head_clearance() {
        let world = |height: f32, roof: bool| {
            let mut vertices = Vec::new();
            for y in std::iter::once(height).chain(roof.then_some(height + 1.3)) {
                for (x, z) in [
                    (-15.0, -10.0),
                    (-15.0, 0.0),
                    (-5.0, 0.0),
                    (-15.0, -10.0),
                    (-5.0, 0.0),
                    (-5.0, -10.0),
                ] {
                    vertices.push(sa_scene::Vertex {
                        position: [x, y, z],
                        uv: [0.0; 2],
                        color: [1.0; 4],
                    });
                }
            }
            sa_scene::collision::CollisionWorld::from_batches(&[sa_scene::Batch {
                key: "owned-floor".into(),
                vertices,
                alpha: false,
                animated: false,
                uv_animation: None,
            }])
        };
        let save = fixture();
        let restored = save.player(&world(13.2, false)).unwrap();
        assert_eq!(restored.feet, glam::Vec3::new(-10.0, 13.2, -5.0));
        assert!(save.player(&world(13.4, false)).is_some());
        assert!(save.player(&world(15.2, false)).is_none());
        assert!(save.player(&world(12.0, false)).is_none());
        assert!(save.player(&world(13.2, true)).is_none());
        assert!(save
            .player(&sa_scene::collision::CollisionWorld::from_batches(&[]))
            .is_none());
    }
}
