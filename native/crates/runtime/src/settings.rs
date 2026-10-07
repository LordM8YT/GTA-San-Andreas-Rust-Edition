use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub fov: f32,
    pub sensitivity: f32,
    pub invert_y: bool,
    pub vsync: bool,
    pub fullscreen: bool,
    pub show_hud: bool,
    pub fly_speed: f32,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            fov: 70.0,
            sensitivity: 1.0,
            invert_y: false,
            vsync: true,
            fullscreen: false,
            show_hud: true,
            fly_speed: 12.0,
        }
    }
}
impl Settings {
    pub fn sanitize(&mut self) {
        let defaults = Self::default();
        self.fov = if self.fov.is_finite() {
            self.fov.clamp(55.0, 110.0)
        } else {
            defaults.fov
        };
        self.sensitivity = if self.sensitivity.is_finite() {
            self.sensitivity.clamp(0.2, 3.0)
        } else {
            defaults.sensitivity
        };
        self.fly_speed = if self.fly_speed.is_finite() {
            self.fly_speed.clamp(6.0, 60.0)
        } else {
            defaults.fly_speed
        };
    }
    fn path() -> PathBuf {
        std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir)
            .join("SAFreeroam/settings.json")
    }
    pub fn load() -> Self {
        let mut settings = std::fs::read(Self::path())
            .ok()
            .and_then(|data| serde_json::from_slice::<Self>(&data).ok())
            .unwrap_or_default();
        settings.sanitize();
        settings
    }
    pub fn save(&self) -> anyhow::Result<()> {
        let path = Self::path();
        std::fs::create_dir_all(path.parent().unwrap())?;
        std::fs::write(path, serde_json::to_vec_pretty(self)?)?;
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn old_and_invalid_settings_have_safe_defaults() {
        let mut settings: Settings =
            serde_json::from_str(r#"{"fov":999,"sensitivity":-2}"#).unwrap();
        settings.sanitize();
        assert_eq!(settings.fov, 110.0);
        assert_eq!(settings.sensitivity, 0.2);
        assert!(settings.vsync);
        settings.fov = f32::NAN;
        settings.sanitize();
        assert_eq!(settings.fov, 70.0);
    }
}
