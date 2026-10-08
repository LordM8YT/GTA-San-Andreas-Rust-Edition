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
pub struct Save {
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
    pub fn validate(&self) -> Result<()> {
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

/// Schedule at most one checkpoint per interval, even after a long pause.
pub fn autosave_due(next: &mut std::time::Instant, now: std::time::Instant) -> bool {
    if now < *next {
        return false;
    }
    *next = now + std::time::Duration::from_secs(60);
    true
}
#[cfg(test)]
mod tests {
    #[test]
    fn periodic_checkpoint_does_not_save_early_or_burst_after_a_pause() {
        use std::time::{Duration, Instant};
        let start = Instant::now();
        let mut next = start + Duration::from_secs(60);
        assert!(!super::autosave_due(
            &mut next,
            start + Duration::from_secs(59)
        ));
        let due = start + Duration::from_secs(60);
        assert!(super::autosave_due(&mut next, due));
        assert!(!super::autosave_due(&mut next, due));
        let late = start + Duration::from_secs(600);
        assert!(super::autosave_due(&mut next, late));
        assert!(!super::autosave_due(
            &mut next,
            late + Duration::from_secs(1)
        ));
    }
}
