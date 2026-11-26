//! Recently opened worlds, newest first, for File › Open world. Kept in a
//! small JSON list beside the player's settings; folders that are gone or
//! no longer hold a world are left out when read.
use anyhow::Result;
use std::path::{Path, PathBuf};

/// How many worlds the list keeps.
pub const MAX: usize = 10;
/// The list's file name, beside `settings.json`.
pub const FILE: &str = "studio-recent.json";

/// The recent worlds in `file` that still open, newest first.
pub fn load(file: &Path) -> Vec<PathBuf> {
    let Ok(text) = std::fs::read_to_string(file) else {
        return Vec::new();
    };
    let list: Vec<PathBuf> = serde_json::from_str(&text).unwrap_or_default();
    list.into_iter()
        .filter(|folder| folder.join(sa_scene::content_pack::MANIFEST).is_file())
        .take(MAX)
        .collect()
}

/// Put `folder` first in the list in `file` (once), keeping at most [`MAX`].
pub fn remember(file: &Path, folder: &Path) -> Result<()> {
    let folder = std::fs::canonicalize(folder).unwrap_or_else(|_| folder.to_path_buf());
    let mut list = load(file);
    list.retain(|f| std::fs::canonicalize(f).unwrap_or_else(|_| f.clone()) != folder);
    list.insert(0, folder);
    list.truncate(MAX);
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent)?;
    }
    sa_scene::world_layout::write_atomic(file, serde_json::to_string_pretty(&list)?.as_bytes())?;
    Ok(())
}

/// Whether `folder` is a world Studio can open: it has a `content.json`
/// that reads. The error says why not, in plain words.
pub fn check_world(folder: &Path) -> Result<()> {
    anyhow::ensure!(
        folder.join(sa_scene::content_pack::MANIFEST).is_file(),
        "{} has no content.json, so it is not a SARE world (pick the folder that holds content.json)",
        folder.display()
    );
    sa_scene::content_pack::Manifest::read(folder)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::Project;

    #[test]
    fn remembers_newest_first_once_and_forgets_missing_worlds() {
        let root = std::env::temp_dir().join(format!("sare-recent-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let file = root.join("config").join(FILE);
        let a = root.join("a");
        let b = root.join("b");
        Project::create(&a, "a", "A").unwrap();
        Project::create(&b, "b", "B").unwrap();
        assert!(load(&file).is_empty());
        remember(&file, &a).unwrap();
        remember(&file, &b).unwrap();
        remember(&file, &a).unwrap();
        let list = load(&file);
        assert_eq!(list.len(), 2);
        assert!(list[0].ends_with("a") && list[1].ends_with("b"));
        std::fs::remove_dir_all(&b).unwrap();
        assert_eq!(load(&file).len(), 1, "a deleted world drops out");
        assert!(check_world(&a).is_ok());
        assert!(check_world(&root).is_err());
        std::fs::write(&file, "not json").unwrap();
        assert!(load(&file).is_empty(), "a damaged list reads as empty");
        let _ = std::fs::remove_dir_all(&root);
    }
}
