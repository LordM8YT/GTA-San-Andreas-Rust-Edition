//! Where the world comes from. A content source turns a streaming request
//! into a `Scene`; the engine does not need to know whether the geometry came
//! from the player's own San Andreas installation or from SARE's own content.
//! This is the first seam of the standalone plan in docs/STANDALONE.md.
use crate::{mlo::MloDefinition, InteriorPortal, Scene, WorldLoader};
use anyhow::Result;
use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ContentSource {
    /// The player's own San Andreas installation (the default today).
    Original(PathBuf),
    /// SARE Testbed: original generated geometry, no game files needed.
    Testbed,
    /// A content pack folder with `content.json` (see docs/content-packs.md).
    Pack(PathBuf),
}
impl ContentSource {
    /// `--content testbed`, `--content <pack id or folder>`, or the original
    /// installation in `game_dir`. Pack ids are looked up in `--content-dir`
    /// (default `content/`).
    pub fn from_args(args: &[String], game_dir: Option<PathBuf>) -> Result<Self> {
        match args.windows(2).find(|w| w[0] == "--content").map(|w| w[1].as_str()) {
            Some("testbed") => Ok(Self::Testbed),
            Some("original") | None => game_dir.map(Self::Original).ok_or_else(|| {
                anyhow::anyhow!(
                    "No San Andreas installation found. Pass --game-dir <folder>, or use --content testbed."
                )
            }),
            Some(other) => {
                let packs = args
                    .windows(2)
                    .find(|w| w[0] == "--content-dir")
                    .map(|w| PathBuf::from(&w[1]))
                    .unwrap_or_else(|| PathBuf::from(crate::content_pack::DEFAULT_DIR));
                crate::content_pack::resolve(other, &packs).map(Self::Pack).map_err(|error| {
                    anyhow::anyhow!("{error:#}. Use original, testbed or a content pack.")
                })
            }
        }
    }
    pub fn needs_original_game(&self) -> bool {
        matches!(self, Self::Original(_))
    }
    pub fn open(&self) -> Result<Box<dyn WorldSource + Send>> {
        Ok(match self {
            Self::Original(game) => Box::new(WorldLoader::open(game)?),
            Self::Testbed => Box::new(Testbed),
            Self::Pack(folder) => Box::new(crate::content_pack::PackWorld::open(folder)?),
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
    /// One interior dimension. Sources without interiors refuse.
    fn load_interior(
        &mut self,
        _center: [f32; 2],
        _origin: [f32; 2],
        _radius: f32,
        interior: u8,
    ) -> Result<Scene> {
        anyhow::bail!("{} has no interior {interior}", self.name())
    }
    /// Doors into interiors.
    fn interior_portals(&self) -> Vec<InteriorPortal> {
        Vec::new()
    }
    /// MLO metadata from resource manifests.
    fn mlo_definitions(&self) -> Vec<MloDefinition> {
        Vec::new()
    }
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
    fn load_interior(
        &mut self,
        center: [f32; 2],
        origin: [f32; 2],
        radius: f32,
        interior: u8,
    ) -> Result<Scene> {
        WorldLoader::load_interior(self, center, origin, radius, interior)
    }
    fn interior_portals(&self) -> Vec<InteriorPortal> {
        WorldLoader::interior_portals(self)
    }
    fn mlo_definitions(&self) -> Vec<MloDefinition> {
        WorldLoader::mlo_definitions(self).to_vec()
    }
}

/// The built-in testbed, centred on [`crate::testbed::CENTER`] so the default
/// runtime origin sits in the middle of it.
pub struct Testbed;
impl WorldSource for Testbed {
    fn name(&self) -> &str {
        "SARE Testbed"
    }
    fn load(&mut self, _center: [f32; 2], origin: [f32; 2], _radius: f32) -> Result<Scene> {
        // The testbed is small enough to always load whole.
        Ok(crate::testbed::build(crate::testbed::CENTER, origin))
    }
    fn spawn(&self) -> Option<[f32; 3]> {
        Some(crate::testbed::spawn(crate::testbed::CENTER))
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
        assert!(world
            .load_interior([0.0, 0.0], [0.0, 0.0], 50.0, 3)
            .is_err());
        assert!(world.interior_portals().is_empty() && world.mlo_definitions().is_empty());
        assert!(ContentSource::from_args(&[], None).is_err());
        assert!(ContentSource::from_args(&args(&["--content", "gta5"]), None).is_err());
        let original = ContentSource::from_args(&[], Some(PathBuf::from("/games/sa"))).unwrap();
        assert!(original.needs_original_game());
    }
}
