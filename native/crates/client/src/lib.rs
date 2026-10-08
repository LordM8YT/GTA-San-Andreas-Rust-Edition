//! Shared client configuration, launch contract and private diagnostics.
pub mod diagnostics;
pub mod install;
pub mod launch;
pub mod progress;
pub mod settings;
use anyhow::{ensure, Result};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};
pub const ORIGIN: [f32; 2] = [2500.0, -1670.0];
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const BUILD_ID: &str = env!("SARE_BUILD_ID");
pub fn config_dir() -> PathBuf {
    settings::Settings::path().parent().unwrap().to_path_buf()
}
pub fn cache_dir() -> PathBuf {
    if std::env::var_os("SARE_CONFIG_DIR").is_some() {
        return config_dir().join("server-cache");
    }
    if cfg!(windows) {
        config_dir().join("server-cache")
    } else {
        std::env::var_os("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|p| PathBuf::from(p).join(".cache")))
            .unwrap_or_else(std::env::temp_dir)
            .join("sa-freeroam/server-cache")
    }
}
pub fn atomic_json(path: &Path, value: &impl Serialize) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(value)?;
    ensure!(bytes.len() <= 64 * 1024, "Configuration exceeds 64 KiB");
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Missing configuration folder"))?;
    fs::create_dir_all(parent)?;
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_nanos();
    let temporary = parent.join(format!(".client-{}-{nonce}.tmp", std::process::id()));
    let result = (|| {
        let mut f = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)?;
        f.write_all(&bytes)?;
        f.sync_all()?;
        drop(f);
        fs::rename(&temporary, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}
pub fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T> {
    let mut data = Vec::new();
    fs::File::open(path)?
        .take(64 * 1024 + 1)
        .read_to_end(&mut data)?;
    ensure!(data.len() <= 64 * 1024, "Configuration exceeds 64 KiB");
    Ok(serde_json::from_slice(&data)?)
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct LauncherConfig {
    pub disable_auto_updates: bool,
    /// Optional user-owned local artwork; never included in resource downloads.
    pub hero_image: Option<PathBuf>,
    pub last_session: Option<Favorite>,
    pub game_dir: PathBuf,
    pub player: String,
    pub relay: String,
    pub favorites: Vec<Favorite>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Favorite {
    pub name: String,
    pub address: String,
    pub relay: bool,
    #[serde(default)]
    pub code: String,
}
impl LauncherConfig {
    pub fn load() -> Self {
        let mut config: Self = read_json(&config_dir().join("launcher.json")).unwrap_or_default();
        config.favorites.truncate(32);
        config
    }
    pub fn save(&self) -> Result<()> {
        ensure!(self.favorites.len() <= 32, "Favorite limit is 32");
        atomic_json(&config_dir().join("launcher.json"), self)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn atomic_config_replacement_and_rejected_update_keep_valid_data() {
        let root = std::env::temp_dir().join(format!(
            "sa-client-atomic-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let path = root.join("settings.json");
        super::atomic_json(&path, &serde_json::json!({"value":1})).unwrap();
        super::atomic_json(&path, &serde_json::json!({"value":2})).unwrap();
        assert!(super::atomic_json(&path, &"x".repeat(65537)).is_err());
        let value: serde_json::Value = super::read_json(&path).unwrap();
        assert_eq!(value["value"], 2);
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 1);
        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir(root).unwrap();
    }
}
