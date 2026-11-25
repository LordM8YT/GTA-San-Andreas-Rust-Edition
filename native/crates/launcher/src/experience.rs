//! One model for everything SARE Client can show and start: the player's own
//! San Andreas installation, the built-in testbed and content packs in
//! `content/`. The pages only read this list; launching stays in `main.rs`.
use std::path::PathBuf;

/// How an experience starts. The launcher maps each kind to its existing
/// launch path, so this adds no second launch system.
#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    /// Free roam in the player's own San Andreas installation.
    SanAndreas,
    /// The small built-in test world.
    Testbed,
    /// A content pack folder (a standalone world).
    Pack(PathBuf),
}

/// Creator shown for worlds without an author, such as new Build Mode worlds.
pub const LOCAL_CREATOR: &str = "Made on this PC";

#[derive(Clone, Debug)]
pub struct Experience {
    pub id: String,
    pub name: String,
    pub creator: String,
    pub description: String,
    pub version: Option<String>,
    pub license: Option<String>,
    pub kind: Kind,
    /// The pack has a Build Mode layout, so Create can open it.
    pub editable: bool,
    /// The pack brings its own characters or vehicles (featured first).
    pub rich: bool,
}

impl Experience {
    pub fn pack_folder(&self) -> Option<&PathBuf> {
        match &self.kind {
            Kind::Pack(folder) => Some(folder),
            _ => None,
        }
    }
    /// Text the search box matches against.
    pub fn matches(&self, query: &str) -> bool {
        let query = query.trim().to_lowercase();
        query.is_empty()
            || [&self.name, &self.creator, &self.description]
                .iter()
                .any(|text| text.to_lowercase().contains(&query))
    }
}

/// Every experience on this PC: packs first, then the testbed and San Andreas.
/// Standalone worlds lead so the platform does not read as a single game.
pub fn local(packs: &[(PathBuf, sa_scene::content_pack::Manifest)]) -> Vec<Experience> {
    let mut list: Vec<_> = packs
        .iter()
        .map(|(folder, manifest)| Experience {
            id: manifest.id.clone(),
            name: manifest.name.clone(),
            creator: manifest
                .author
                .clone()
                .filter(|a| !a.trim().is_empty())
                .unwrap_or_else(|| LOCAL_CREATOR.into()),
            description: manifest
                .description
                .clone()
                .unwrap_or_else(|| "A standalone SARE world.".into()),
            version: Some(manifest.version.clone()),
            license: Some(manifest.license.clone()),
            kind: Kind::Pack(folder.clone()),
            editable: manifest.world.layout.is_some(),
            rich: manifest.characters.is_some() || !manifest.vehicles.is_empty(),
        })
        .collect();
    list.push(Experience {
        id: "sare-testbed".into(),
        name: "SARE Testbed".into(),
        creator: "SARE contributors".into(),
        description:
            "A small built-in world for walking, jumping and driving. No game files needed.".into(),
        version: None,
        license: None,
        kind: Kind::Testbed,
        editable: false,
        rich: false,
    });
    list.push(Experience {
        id: "san-andreas".into(),
        name: "San Andreas free roam".into(),
        creator: "Uses your own copy of the PC game".into(),
        description:
            "Free roam across the state, read at runtime from your own copy of the PC game.".into(),
        version: None,
        license: None,
        kind: Kind::SanAndreas,
        editable: false,
        rich: false,
    });
    list
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standalone_worlds_lead_and_search_matches_creator() {
        let list = local(&[]);
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].kind, Kind::Testbed);
        assert!(list[1].matches("own copy"));
        assert!(!list[1].matches("racing"));
        assert!(list[1].matches("  "));
    }
}
