//! Conservative cleanup: all cached packs are protected while any session uses them.
use super::*;
#[derive(Clone, Debug)]
pub struct CachePack {
    pub fingerprint: String,
    pub names: Vec<String>,
    pub bytes: u64,
    pub complete: bool,
}
#[derive(Clone, Debug)]
pub struct CachePlan {
    pub packs: Vec<CachePack>,
    pub bytes: u64,
    pub busy: bool,
    root: PathBuf,
    generation: String,
}
fn lock(root: &Path, name: &str) -> io::Result<fs::File> {
    let path = root.join(name);
    if path.exists() && !reject_link(&path)?.is_file() {
        return Err(error("Invalid cache lock"));
    }
    fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path)
}
pub(super) fn active_lease(root: &Path) -> io::Result<Arc<fs::File>> {
    let file = lock(root, ".lease")?;
    file.try_lock_shared()
        .map_err(|_| error("Cache cleanup is running. Retry after it finishes."))?;
    Ok(Arc::new(file))
}
fn generation(root: &Path) -> io::Result<String> {
    fn walk(root: &Path, path: &Path, rows: &mut Vec<String>, depth: usize) -> io::Result<()> {
        if depth > 12 || rows.len() > 8192 {
            return Err(error("Cache inspection budget exceeded"));
        }
        for entry in fs::read_dir(path)? {
            let path = entry?.path();
            let meta = reject_link(&path)?;
            let relative = path
                .strip_prefix(root)
                .map_err(|_| error("Cache escapes root"))?;
            if relative == Path::new(".lock") || relative == Path::new(".lease") {
                continue;
            }
            if !meta.is_file() && !meta.is_dir() {
                return Err(error("Unexpected cache file type"));
            }
            rows.push(format!(
                "{}:{}:{:?}",
                relative.display(),
                meta.len(),
                meta.modified()?
            ));
            if meta.is_dir() {
                walk(root, &path, rows, depth + 1)?;
            }
        }
        Ok(())
    }
    let mut rows = Vec::new();
    walk(root, root, &mut rows, 0)?;
    rows.sort();
    Ok(hash(rows.join("\n").as_bytes()))
}
fn inspect_locked(root: &Path) -> io::Result<CachePlan> {
    let tree_generation = generation(root)?;
    let mut packs = Vec::new();
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !["packs", "blobs", ".lock", ".lease"].contains(&name.as_ref()) {
            return Err(error("Unexpected cache root entry; cleanup refused"));
        }
        reject_link(&entry.path())?;
    }
    let packs_root = root.join("packs");
    if packs_root.exists() {
        for entry in fs::read_dir(&packs_root)? {
            let path = entry?.path();
            let fingerprint = path.file_name().unwrap().to_string_lossy().to_string();
            if !valid_hash(&fingerprint) || !reject_link(&path)?.is_dir() {
                return Err(error("Unrecognized cache pack; cleanup refused"));
            }
            let manifest = (|| -> io::Result<Manifest> {
                let inventory = path.join("inventory.json");
                if reject_link(&inventory)?.len() > MAX_MANIFEST_BYTES as u64 {
                    return Err(error("Inventory too large"));
                }
                let bytes = fs::read(inventory)?;
                if bytes.len() > MAX_MANIFEST_BYTES {
                    return Err(error("Inventory too large"));
                }
                let m: Manifest = serde_json::from_slice(&bytes)?;
                if m.fingerprint()? != fingerprint {
                    return Err(error("Inventory fingerprint differs"));
                }
                Ok(m)
            })();
            packs.push(CachePack {
                fingerprint,
                names: manifest
                    .as_ref()
                    .map(|m| m.resources.iter().map(|r| r.name.clone()).collect())
                    .unwrap_or_default(),
                bytes: disk_usage(&path)?,
                complete: manifest.is_ok(),
            });
        }
    }
    let blobs = root.join("blobs");
    if blobs.exists() {
        for entry in fs::read_dir(&blobs)? {
            let path = entry?.path();
            let name = path.file_name().unwrap().to_string_lossy();
            if !valid_hash(&name) || !reject_link(&path)?.is_file() {
                return Err(error("Unrecognized cache blob; cleanup refused"));
            }
        }
    }
    packs.sort_by(|a, b| a.fingerprint.cmp(&b.fingerprint));
    let lease = lock(root, ".lease")?;
    let busy = lease.try_lock().is_err();
    Ok(CachePlan {
        packs,
        bytes: disk_usage(root)?,
        busy,
        root: root.to_path_buf(),
        generation: tree_generation,
    })
}
pub fn inspect_cache(root: &Path) -> io::Result<CachePlan> {
    fs::create_dir_all(root)?;
    if !reject_link(root)?.is_dir() {
        return Err(error("Invalid cache root"));
    }
    let root = root.canonicalize()?;
    let writer = lock(&root, ".lock")?;
    writer
        .try_lock()
        .map_err(|_| error("Cache download is active. Retry after it finishes."))?;
    inspect_locked(&root)
}
pub fn cleanup_cache(root: &Path, preview: &CachePlan) -> io::Result<u64> {
    reject_link(root)?;
    let root = root.canonicalize()?;
    if root != preview.root {
        return Err(error("Cache location changed. Preview again."));
    }
    let writer = lock(&root, ".lock")?;
    writer
        .try_lock()
        .map_err(|_| error("Cache download is active. Close the session and retry."))?;
    let lease = lock(&root, ".lease")?;
    lease
        .try_lock()
        .map_err(|_| error("A running session is using cached mods. Close all sessions first."))?;
    if generation(&root)? != preview.generation {
        return Err(error(
            "Cache changed after preview. Review a new preview before deleting.",
        ));
    }
    // Validate the entire tree before the first deletion. Never follow a link.
    generation(&root)?;
    fn remove(root: &Path, path: &Path) -> io::Result<()> {
        let meta = reject_link(path)?;
        if !path.canonicalize()?.starts_with(root) {
            return Err(error("Cache target escapes root"));
        }
        if meta.is_dir() {
            for entry in fs::read_dir(path)? {
                remove(root, &entry?.path())?;
            }
            fs::remove_dir(path)
        } else if meta.is_file() {
            fs::remove_file(path)
        } else {
            Err(error("Unsafe cache target"))
        }
    }
    for name in ["packs", "blobs"] {
        let path = root.join(name);
        if path.exists() {
            remove(&root, &path)?;
        }
    }
    Ok(preview.bytes.saturating_sub(disk_usage(&root)?))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cleanup_requires_unused_cache_and_fresh_preview() {
        let temp = super::super::tests::Temp::new();
        let source = super::super::tests::share(b"owned");
        let active = cache(
            &temp.0,
            &source.manifest,
            |f| Ok(source.blob(&f.sha256).unwrap().to_vec()),
            |_, _| {},
        )
        .unwrap();
        let preview = inspect_cache(&temp.0).unwrap();
        assert!(preview.busy);
        assert!(cleanup_cache(&temp.0, &preview).is_err());
        drop(active);
        let preview = inspect_cache(&temp.0).unwrap();
        assert!(!preview.busy);
        assert!(preview.bytes > 0);
        fs::write(temp.0.join("blobs").join("a".repeat(64)), b"changed").unwrap();
        assert!(cleanup_cache(&temp.0, &preview).is_err());
        let current = inspect_cache(&temp.0).unwrap();
        assert!(cleanup_cache(&temp.0, &current).unwrap() > 0);
        assert!(!temp.0.join("packs").exists());
        assert!(temp.0.join(".lock").exists());
    }
    #[test]
    fn unrelated_directory_is_never_cleaned() {
        let temp = super::super::tests::Temp::new();
        fs::create_dir_all(&temp.0).unwrap();
        fs::write(temp.0.join("gta_sa.exe"), b"protected").unwrap();
        assert!(inspect_cache(&temp.0).is_err());
        assert_eq!(fs::read(temp.0.join("gta_sa.exe")).unwrap(), b"protected");
    }
    #[test]
    fn writer_lock_and_changed_locations_are_rejected() {
        let temp = super::super::tests::Temp::new();
        let other = super::super::tests::Temp::new();
        let preview = inspect_cache(&temp.0).unwrap();
        assert!(cleanup_cache(&other.0, &preview).is_err());
        let writer = lock(&temp.0, ".lock").unwrap();
        writer.try_lock().unwrap();
        assert!(inspect_cache(&temp.0).is_err());
        assert!(cleanup_cache(&temp.0, &preview).is_err());
    }
    #[cfg(unix)]
    #[test]
    fn linked_pack_cannot_delete_external_files() {
        let temp = super::super::tests::Temp::new();
        let outside = super::super::tests::Temp::new();
        fs::write(outside.0.join("keep"), b"protected").unwrap();
        fs::create_dir(temp.0.join("packs")).unwrap();
        std::os::unix::fs::symlink(&outside.0, temp.0.join("packs").join("a".repeat(64))).unwrap();
        assert!(inspect_cache(&temp.0).is_err());
        assert_eq!(fs::read(outside.0.join("keep")).unwrap(), b"protected");
    }
    #[cfg(windows)]
    #[test]
    fn junction_pack_is_rejected_before_any_external_file_is_read_or_deleted() {
        let temp = super::super::tests::Temp::new();
        let outside = super::super::tests::Temp::new();
        fs::write(outside.0.join("keep"), b"protected").unwrap();
        fs::create_dir(temp.0.join("packs")).unwrap();
        let link = temp.0.join("packs").join("a".repeat(64));
        let result = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&link)
            .arg(&outside.0)
            .output()
            .unwrap();
        assert!(result.status.success(), "junction fixture creation failed");
        let rejected = inspect_cache(&temp.0).is_err();
        let kept = fs::read(outside.0.join("keep")).unwrap();
        fs::remove_dir(&link).unwrap();
        assert!(rejected);
        assert_eq!(kept, b"protected");
    }
}
