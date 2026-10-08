use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub enum Renderer {
    #[default]
    Auto,
    Vulkan,
    DirectX12,
}
impl Renderer {
    pub fn name(self) -> &'static str {
        match self {
            Self::Auto => "Automatic",
            Self::Vulkan => "Vulkan",
            Self::DirectX12 => "DirectX 12",
        }
    }
    pub fn backends(self) -> wgpu::Backends {
        match self {
            Self::Auto => wgpu::Backends::VULKAN | wgpu::Backends::DX12 | wgpu::Backends::METAL,
            Self::Vulkan => wgpu::Backends::VULKAN,
            Self::DirectX12 => wgpu::Backends::DX12,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub fov: f32,
    pub sensitivity: f32,
    pub invert_y: bool,
    pub vsync: bool,
    pub fullscreen: bool,
    pub show_hud: bool,
    pub show_minimap: bool,
    pub show_speedometer: bool,
    pub minimap_zoom: f32,
    pub vehicle_handling: f32,
    pub fly_speed: f32,
    pub render_scale: u32,
    pub fxaa: bool,
    pub texture_anisotropy: u16,
    pub sharpness: f32,
    pub bloom: f32,
    pub exposure: f32,
    pub saturation: f32,
    pub vignette: f32,
    pub atmospheric_fog: bool,
    pub fsr1: bool,
    pub renderer: Renderer,
    pub fps_limit: u32,
    pub master_volume: f32,
    pub music_volume: f32,
    pub effects_volume: f32,
    pub auto_mod_downloads: bool,
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
            show_minimap: true,
            show_speedometer: true,
            minimap_zoom: 1.0,
            vehicle_handling: 1.0,
            fly_speed: 12.0,
            render_scale: 100,
            fxaa: true,
            texture_anisotropy: 8,
            sharpness: 0.15,
            bloom: 0.12,
            exposure: 1.0,
            saturation: 1.05,
            vignette: 0.12,
            atmospheric_fog: true,
            fsr1: true,
            renderer: Renderer::Auto,
            fps_limit: 0,
            master_volume: 1.0,
            music_volume: 0.5,
            effects_volume: 1.0,
            auto_mod_downloads: true,
        }
    }
}
impl Settings {
    pub fn sanitize(&mut self) {
        let defaults = Self::default();
        self.render_scale = self.render_scale.clamp(50, 150);
        if ![1, 2, 4, 8, 16].contains(&self.texture_anisotropy) {
            self.texture_anisotropy = defaults.texture_anisotropy;
        }
        if ![0, 30, 60, 90, 120, 144, 165, 240].contains(&self.fps_limit) {
            self.fps_limit = 0;
        }
        for (value, fallback, low, high) in [
            (&mut self.sharpness, defaults.sharpness, 0.0, 1.0),
            (&mut self.bloom, defaults.bloom, 0.0, 0.6),
            (&mut self.exposure, defaults.exposure, 0.5, 1.8),
            (&mut self.saturation, defaults.saturation, 0.0, 1.5),
            (&mut self.vignette, defaults.vignette, 0.0, 0.5),
            (&mut self.master_volume, defaults.master_volume, 0.0, 1.0),
            (&mut self.music_volume, defaults.music_volume, 0.0, 1.0),
            (&mut self.effects_volume, defaults.effects_volume, 0.0, 1.0),
        ] {
            *value = if value.is_finite() {
                value.clamp(low, high)
            } else {
                fallback
            };
        }
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
        self.minimap_zoom = if self.minimap_zoom.is_finite() {
            self.minimap_zoom.clamp(0.5, 2.5)
        } else {
            defaults.minimap_zoom
        };
        self.vehicle_handling = if self.vehicle_handling.is_finite() {
            self.vehicle_handling.clamp(0.5, 1.5)
        } else {
            defaults.vehicle_handling
        };
        self.fly_speed = if self.fly_speed.is_finite() {
            self.fly_speed.clamp(6.0, 60.0)
        } else {
            defaults.fly_speed
        };
    }
    fn path() -> PathBuf {
        if cfg!(target_os = "windows") {
            std::env::var_os("LOCALAPPDATA")
                .map(PathBuf::from)
                .unwrap_or_else(std::env::temp_dir)
                .join("SAFreeroam/settings.json")
        } else {
            std::env::var_os("XDG_CONFIG_HOME")
                .map(PathBuf::from)
                .or_else(|| {
                    std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config"))
                })
                .unwrap_or_else(std::env::temp_dir)
                .join("sa-freeroam/settings.json")
        }
    }
    pub fn render_size(&self, width: u32, height: u32) -> [u32; 2] {
        let scale = self.render_scale.clamp(50, 150) as u64;
        [width, height].map(|value| ((value as u64 * scale / 100).clamp(1, 8192)) as u32)
    }
    pub fn apply_preset(&mut self, index: usize) {
        self.render_scale = [67, 83, 100, 150][index.min(3)];
        self.fxaa = true;
        self.sharpness = [0.3, 0.2, 0.15, 0.05][index.min(3)];
        self.bloom = [0.0, 0.08, 0.12, 0.18][index.min(3)];
        self.exposure = 1.0;
        self.saturation = 1.05;
        self.vignette = 0.12;
        self.atmospheric_fog = true;
        self.texture_anisotropy = [1, 4, 8, 16][index.min(3)];
    }
    pub fn preset(&self) -> &'static str {
        for (index, name) in ["Performance", "Balanced", "Quality", "Ultra"]
            .into_iter()
            .enumerate()
        {
            let mut candidate = self.clone();
            candidate.apply_preset(index);
            if candidate == *self {
                return name;
            }
        }
        "Custom"
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
        assert_eq!(
            settings.texture_anisotropy, 8,
            "older settings should gain the default filter"
        );
        settings.texture_anisotropy = 99;
        settings.sanitize();
        assert_eq!(settings.texture_anisotropy, 8);
        settings.minimap_zoom = f32::NAN;
        settings.sanitize();
        assert_eq!(settings.minimap_zoom, 1.0);
        settings.minimap_zoom = 99.0;
        settings.sanitize();
        assert_eq!(settings.minimap_zoom, 2.5);
        settings.fov = f32::NAN;
        settings.sanitize();
        assert_eq!(settings.fov, 70.0);
    }
    #[test]
    fn graphics_profiles_preserve_controls_and_bound_render_targets() {
        let mut settings = Settings {
            sensitivity: 2.0,
            fullscreen: true,
            ..Default::default()
        };
        settings.apply_preset(0);
        assert_eq!(settings.preset(), "Performance");
        assert_eq!(settings.texture_anisotropy, 1);
        assert_eq!(settings.render_size(1920, 1080), [1286, 723]);
        assert_eq!(settings.sensitivity, 2.0);
        assert!(settings.fullscreen);
        settings.bloom = f32::NAN;
        settings.render_scale = u32::MAX;
        settings.sanitize();
        assert_eq!(settings.render_scale, 150);
        assert_eq!(settings.bloom, 0.12);
        assert_eq!(settings.render_size(u32::MAX, 0), [8192, 1]);
        let restored: Settings =
            serde_json::from_str(&serde_json::to_string(&settings).unwrap()).unwrap();
        assert_eq!(settings, restored);
    }
}
