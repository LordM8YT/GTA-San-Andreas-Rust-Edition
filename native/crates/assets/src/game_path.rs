//! Case-insensitive original-installation lookup on case-sensitive platforms.
use anyhow::{ensure, Result};
use std::{
    fs,
    path::{Component, Path, PathBuf},
};

pub fn resolve(root: &Path, relative: &str) -> Result<PathBuf> {
    let root = root.canonicalize()?;
    let relative = relative.replace('\\', "/");
    let mut path = root.clone();
    for component in Path::new(&relative).components() {
        let Component::Normal(name) = component else {
            anyhow::bail!("asset path must stay relative to the installation");
        };
        ensure!(
            !name.to_string_lossy().contains(':'),
            "absolute asset path rejected"
        );
        let exact = path.join(name);
        path = if exact.exists() || !path.is_dir() {
            exact
        } else {
            let mut matches = fs::read_dir(&path)?
                .filter_map(|entry| entry.ok())
                .filter(|entry| {
                    entry
                        .file_name()
                        .to_string_lossy()
                        .eq_ignore_ascii_case(&name.to_string_lossy())
                });
            let matched = matches.next();
            ensure!(matches.next().is_none(), "ambiguous asset filename casing");
            matched.map(|entry| entry.path()).unwrap_or(exact)
        };
        ensure!(!path.is_symlink(), "symlink asset path rejected");
    }
    if path.exists() {
        ensure!(
            path.canonicalize()?.starts_with(&root),
            "asset path escapes installation"
        );
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn resolves_mixed_case_and_rejects_escape_without_changing_files() {
        let root = std::env::temp_dir().join(format!("sa-case-path-{}", std::process::id()));
        fs::create_dir_all(root.join("DaTa/Maps/LA")).unwrap();
        fs::write(root.join("DaTa/Maps/LA/LAn.IDE"), b"fixture").unwrap();
        let path = resolve(&root, "data\\maps\\la\\lan.ide").unwrap();
        assert_eq!(fs::read(path).unwrap(), b"fixture");
        assert!(!resolve(&root, "DATA/MAPS/optional.ipl").unwrap().exists());
        assert!(resolve(&root, "../outside").is_err());
        assert!(resolve(&root, "/outside").is_err());
        assert!(resolve(&root, "C:/outside").is_err());
        fs::remove_file(root.join("DaTa/Maps/LA/LAn.IDE")).unwrap();
        for part in ["DaTa/Maps/LA", "DaTa/Maps", "DaTa", ""] {
            fs::remove_dir(root.join(part)).unwrap();
        }
    }
}
