//! Where the world comes from. A content source turns a streaming request
//! into a `Scene`; the engine does not need to know whether the geometry came
//! from the player's own San Andreas installation or from SARE's own content.
//! This is the first seam of the standalone plan in docs/STANDALONE.md.
use crate::{Scene, WorldLoader};
use anyhow::Result;
use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ContentSource {
    /// The player's own San Andreas installation (the default today).
    Original(PathBuf),
    /// SARE Testbed: original generated geometry, no game files needed.
    Testbed,
}
impl ContentSource {
    /// `--content testbed`, or the original installation in `game_dir`.
    pub fn from_args(args: &[String], game_dir: Option<PathBuf>) -> Result<Self> {
        match args.windows(2).find(|w| w[0] == "--content").map(|w| w[1].as_str()) {
            Some("testbed") => Ok(Self::Testbed),
            Some("original") | None => game_dir.map(Self::Original).ok_or_else(|| {
                anyhow::anyhow!(
                    "No San Andreas installation found. Pass --game-dir <folder>, or use --content testbed."
                )
            }),
            Some(other) => anyhow::bail!("Unknown content source {other}; use original or testbed"),
        }
    }
    pub fn needs_original_game(&self) -> bool {
        matches!(self, Self::Original(_))
    }
    pub fn open(&self) -> Result<Box<dyn WorldSource>> {
        Ok(match self {
            Self::Original(game) => Box::new(WorldLoader::open(game)?),
            Self::Testbed => Box::new(Testbed),
        })
    }
}

/// Streams world geometry around a point. `center` is in world coordinates,
/// `origin` the runtime origin the scene is made relative to.
pub trait WorldSource {
    fn name(&self) -> &str;
    fn load(&mut self, center: [f32; 2], origin: [f32; 2], radius: f32) -> Result<Scene>;
    /// A safe place to start, in world coordinates (x, y, height).
    fn spawn(&self) -> Option<[f32; 3]>;
}
impl WorldSource for WorldLoader {
    fn name(&self) -> &str {
        "San Andreas installation"
    }
    fn load(&mut self, center: [f32; 2], origin: [f32; 2], radius: f32) -> Result<Scene> {
        WorldLoader::load(self, center, origin, radius)
    }
    fn spawn(&self) -> Option<[f32; 3]> {
        None
    }
}

/// The built-in testbed, centred on world (0, 0).
pub struct Testbed;
impl WorldSource for Testbed {
    fn name(&self) -> &str {
        "SARE Testbed"
    }
    fn load(&mut self, _center: [f32; 2], origin: [f32; 2], _radius: f32) -> Result<Scene> {
        Ok(crate::testbed::build([0.0, 0.0], origin))
    }
    fn spawn(&self) -> Option<[f32; 3]> {
        Some(crate::testbed::spawn([0.0, 0.0]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }
    #[test]
    fn testbed_needs_no_game_files_and_original_needs_a_folder() {
        let source = ContentSource::from_args(&args(&["--content", "testbed"]), None).unwrap();
        assert!(!source.needs_original_game());
        let mut world = source.open().unwrap();
        assert_eq!(world.name(), "SARE Testbed");
        let scene = world.load([0.0, 0.0], [2500.0, -1670.0], 300.0).unwrap();
        assert!(scene.collision.is_some() && world.spawn().is_some());
        assert!(ContentSource::from_args(&[], None).is_err());
        assert!(ContentSource::from_args(&args(&["--content", "gta5"]), None).is_err());
        let original = ContentSource::from_args(&[], Some(PathBuf::from("/games/sa"))).unwrap();
        assert!(original.needs_original_game());
    }
}
