//! World (content pack) transfer. A host shares the folder of the content
//! pack it plays on; a joining player who lacks that exact world downloads it
//! into its own worlds folder and plays it from there. The player's own
//! `content/` folder is never written to.
//!
//! The inventory can be far larger than one network frame, so the host only
//! announces its hash and size (`WorldOffer`); the client fetches it like any
//! other file and checks it before trusting a single path in it.
use super::*;

pub const MAX_WORLD_FILES: usize = 1024;
pub const MAX_WORLD_BYTES: usize = 256 * 1024 * 1024;
pub const MAX_WORLD_FILE_BYTES: usize = 64 * 1024 * 1024;
const MAX_INVENTORY_BYTES: usize = 256 * 1024;
/// Worlds of other servers kept on disk before the oldest are removed.
const KEEP_WORLDS: usize = 8;
const COMPLETE: &str = ".complete";
/// What a content pack may hold: scenes, textures, data and scripts.
const EXTENSIONS: &[&str] = &[
    "glb", "gltf", "bin", "png", "jpg", "jpeg", "ktx2", "json", "lua", "md", "txt",
];

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct WorldInventory {
    pub id: String,
    pub files: Vec<File>,
}
fn validate_world_path(value: &str) -> io::Result<()> {
    safe_relative_path(value)?;
    let extension = value.rsplit('.').next().unwrap().to_ascii_lowercase();
    if !value.contains('.') || !EXTENSIONS.contains(&extension.as_str()) {
        return Err(error("A world file has an unsupported type"));
    }
    Ok(())
}
impl WorldInventory {
    pub fn validate(&self) -> io::Result<()> {
        if !valid_pack_id(&self.id) {
            return Err(error("Invalid world id"));
        }
        if self.files.len() > MAX_WORLD_FILES {
            return Err(error("The world has too many files"));
        }
        let mut paths = HashSet::new();
        let mut total = 0_usize;
        for file in &self.files {
            validate_world_path(&file.path)?;
            if !paths.insert(file.path.to_ascii_lowercase())
                || !valid_hash(&file.sha256)
                || file.bytes > MAX_WORLD_FILE_BYTES
            {
                return Err(error("Invalid, duplicate or oversized world file"));
            }
            total = total
                .checked_add(file.bytes)
                .ok_or_else(|| error("World size overflow"))?;
        }
        if total > MAX_WORLD_BYTES {
            return Err(error("The world is larger than 256 MiB"));
        }
        if !paths.contains("content.json") {
            return Err(error("The world has no content.json"));
        }
        Ok(())
    }
    pub fn total_bytes(&self) -> usize {
        self.files.iter().map(|f| f.bytes).sum()
    }
}

impl Share {
    /// Offer the content pack in `folder` (with id `id`) to joining players.
    /// Hidden files and files of other types are left out.
    pub fn add_world(&mut self, id: &str, folder: &Path) -> io::Result<()> {
        fn walk(
            root: &Path,
            directory: &Path,
            depth: usize,
            found: &mut Vec<(String, PathBuf)>,
        ) -> io::Result<()> {
            if depth > 6 {
                return Ok(());
            }
            let mut entries: Vec<_> = fs::read_dir(directory)?.collect::<Result<_, _>>()?;
            entries.sort_by_key(|e| e.file_name());
            for entry in entries {
                let name = entry.file_name();
                let Some(name) = name.to_str() else { continue };
                if name.starts_with('.') {
                    continue;
                }
                let path = entry.path();
                let metadata = fs::symlink_metadata(&path)?;
                if metadata.is_dir() {
                    walk(root, &path, depth + 1, found)?;
                } else if metadata.is_file() {
                    let relative = path
                        .strip_prefix(root)
                        .map_err(|_| error("World file outside its folder"))?
                        .components()
                        .map(|c| c.as_os_str().to_string_lossy())
                        .collect::<Vec<_>>()
                        .join("/");
                    if validate_world_path(&relative).is_ok() {
                        found.push((relative, path));
                    }
                }
            }
            Ok(())
        }
        let mut found = Vec::new();
        walk(folder, folder, 0, &mut found)?;
        let mut inventory = WorldInventory {
            id: id.to_owned(),
            files: Vec::new(),
        };
        for (relative, path) in found {
            let data = fs::read(&path)?;
            let sha256 = hash(&data);
            inventory.files.push(File {
                path: relative,
                bytes: data.len(),
                sha256: sha256.clone(),
            });
            self.blobs.entry(sha256).or_insert_with(|| data.into());
            if inventory.files.len() > MAX_WORLD_FILES || inventory.total_bytes() > MAX_WORLD_BYTES
            {
                break;
            }
        }
        inventory.validate()?;
        let data = serde_json::to_vec(&inventory)?;
        if data.len() > MAX_INVENTORY_BYTES {
            return Err(error("The world has too many files"));
        }
        let sha256 = hash(&data);
        self.world = Some((sha256.clone(), data.len()));
        self.blobs.insert(sha256, data.into());
        Ok(())
    }
    /// The shared world's inventory: SHA-256 and size.
    pub fn world_offer(&self) -> Option<(String, usize)> {
        self.world.clone()
    }
}

/// Where downloaded worlds live: beside the server mod cache, not in it, so
/// worlds and mods have their own size limits.
pub fn worlds_dir(cache: &Path) -> PathBuf {
    cache.with_file_name("server-worlds")
}

fn complete(folder: &Path, inventory: &WorldInventory) -> io::Result<bool> {
    if !folder.join(COMPLETE).is_file() {
        return Ok(false);
    }
    for file in &inventory.files {
        if verified(&folder.join(&file.path), file)?.is_none() {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Keep the newest `KEEP_WORLDS` downloaded worlds besides `keep`.
fn prune(root: &Path, keep: &Path) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    let mut worlds: Vec<(std::time::SystemTime, PathBuf)> = entries
        .flatten()
        .filter(|e| e.path() != keep)
        .filter_map(|e| {
            let metadata = fs::symlink_metadata(e.path()).ok()?;
            if !metadata.is_dir() {
                return None;
            }
            Some((metadata.modified().ok()?, e.path()))
        })
        .collect();
    worlds.sort_by_key(|w| std::cmp::Reverse(w.0));
    for (_, path) in worlds.into_iter().skip(KEEP_WORLDS.saturating_sub(1)) {
        let _ = fs::remove_dir_all(path);
    }
}

/// Make sure the world `id` the server at `address` plays on is on disk, and
/// return its folder. Files the player already has in `local` (their own copy
/// of the pack) are reused instead of downloaded when they are identical.
#[allow(clippy::too_many_arguments)]
pub fn download_world(
    address: std::net::SocketAddr,
    join_code: Option<&str>,
    id: &str,
    root: &Path,
    local: Option<&Path>,
    allow_download: bool,
    cancel: &std::sync::atomic::AtomicBool,
    mut progress: impl FnMut(usize, usize),
) -> io::Result<PathBuf> {
    use crate::{Message, VERSION};
    let mut wire = open_wire(address, join_code)?;
    let mut pending = std::collections::VecDeque::new();
    wire.queue(&Message::WorldQuery { version: VERSION })?;
    let Message::WorldOffer { sha256, bytes } = next_message(&mut wire, &mut pending, cancel)?
    else {
        return Err(error("The server does not share its world"));
    };
    if !valid_hash(&sha256) || bytes == 0 || bytes > MAX_INVENTORY_BYTES {
        return Err(error("Invalid world inventory"));
    }
    let listing = File {
        path: "inventory.json".into(),
        bytes,
        sha256,
    };
    let data = fetch_file(&mut wire, &mut pending, cancel, &listing, |_| {})?;
    if hash(&data) != listing.sha256 {
        return Err(error("World inventory failed SHA-256 verification"));
    }
    let inventory: WorldInventory =
        serde_json::from_slice(&data).map_err(|_| error("Invalid world inventory"))?;
    inventory.validate()?;
    if inventory.id != id {
        return Err(error("The server offered a different world"));
    }
    fs::create_dir_all(root)?;
    reject_link(root)?;
    let root = root.canonicalize()?;
    let name = format!("{id}-{}", &listing.sha256[..16]);
    let folder = directory(&root, Path::new(&name))?;
    prune(&root, &folder);
    if complete(&folder, &inventory)? {
        return Ok(folder);
    }
    let _ = fs::remove_file(folder.join(COMPLETE));
    let total = inventory.total_bytes();
    let mut done = 0_usize;
    progress(0, total);
    for file in &inventory.files {
        let target = folder.join(&file.path);
        if let Some(parent) = Path::new(&name).join(&file.path).parent() {
            directory(&root, parent)?;
        }
        recover_staging(&target, &root, &mut 0)?;
        if verified(&target, file)?.is_none() {
            let mine = match local {
                Some(local) => verified(&local.join(&file.path), file).unwrap_or(None),
                None => None,
            };
            let data = match mine {
                Some(data) => data,
                None if !allow_download => {
                    return Err(error(
                        "The server's world is missing. Enable automatic downloads to get it.",
                    ))
                }
                None => {
                    let data = fetch_file(&mut wire, &mut pending, cancel, file, |bytes| {
                        progress(done + bytes, total)
                    })?;
                    if hash(&data) != file.sha256 {
                        return Err(error("A world file failed SHA-256 verification"));
                    }
                    data
                }
            };
            // Worlds have their own folder and limits (MAX_WORLD_BYTES per
            // world, KEEP_WORLDS worlds), not the mod cache's budget.
            write_checked(&target, &data, &mut 0)?;
        }
        done += file.bytes;
        progress(done, total);
    }
    fs::write(folder.join(COMPLETE), &listing.sha256)?;
    Ok(folder)
}

#[cfg(test)]
mod tests {
    use super::super::tests::Temp;
    use super::*;

    fn pack(dir: &Path) {
        fs::create_dir_all(dir.join("vehicles")).unwrap();
        fs::write(dir.join("content.json"), br#"{"id":"town"}"#).unwrap();
        fs::write(dir.join("world.glb"), vec![7u8; 5000]).unwrap();
        fs::write(dir.join("vehicles/taxi.glb"), b"taxi").unwrap();
        fs::write(dir.join("notes.exe"), b"skip me").unwrap();
        fs::write(dir.join(".hidden.json"), b"skip me").unwrap();
    }

    #[test]
    fn shared_worlds_list_their_files_and_skip_the_rest() {
        let temp = Temp::new();
        pack(&temp.0);
        let mut share = Share::default();
        share.add_world("town", &temp.0).unwrap();
        let (sha256, _) = share.world_offer().unwrap();
        let inventory: WorldInventory =
            serde_json::from_slice(&share.blob(&sha256).unwrap()).unwrap();
        let paths: Vec<_> = inventory.files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(paths, ["content.json", "vehicles/taxi.glb", "world.glb"]);
        for file in &inventory.files {
            assert_eq!(share.blob(&file.sha256).unwrap().len(), file.bytes);
        }
    }

    #[test]
    fn joiners_download_the_world_once_and_reuse_their_own_files() {
        let (server_pack, local, worlds) = (Temp::new(), Temp::new(), Temp::new());
        pack(&server_pack.0);
        // The player has an older copy: same world.glb, no taxi.
        fs::write(local.0.join("content.json"), br#"{"id":"old"}"#).unwrap();
        fs::write(local.0.join("world.glb"), vec![7u8; 5000]).unwrap();
        let mut share = Share::default();
        share.add_world("town", &server_pack.0).unwrap();
        let host = crate::Session::dedicated_resources(
            "127.0.0.1:0".parse().unwrap(),
            "World host",
            share,
        )
        .unwrap();
        let cancel = std::sync::atomic::AtomicBool::new(false);
        let mut seen = Vec::new();
        let folder = download_world(
            host.address,
            None,
            "town",
            &worlds.0,
            Some(&local.0),
            true,
            &cancel,
            |done, total| seen.push((done, total)),
        )
        .unwrap();
        assert_eq!(
            fs::read(folder.join("content.json")).unwrap(),
            br#"{"id":"town"}"#
        );
        assert_eq!(fs::read(folder.join("vehicles/taxi.glb")).unwrap(), b"taxi");
        assert_eq!(fs::read(folder.join("world.glb")).unwrap().len(), 5000);
        assert!(!folder.join("notes.exe").exists());
        assert_eq!(seen.last(), Some(&(5000 + 4 + 13, 5000 + 4 + 13)));
        // A second join finds the complete world and fetches nothing,
        // even with downloads switched off.
        let again = download_world(
            host.address,
            None,
            "town",
            &worlds.0,
            None,
            false,
            &cancel,
            |_, _| {},
        )
        .unwrap();
        assert_eq!(again, folder);
        // Another id than the server's world is refused.
        assert!(download_world(
            host.address,
            None,
            "other",
            &worlds.0,
            None,
            true,
            &cancel,
            |_, _| {}
        )
        .is_err());
        // A server without a shared world says so.
        let plain = crate::Session::dedicated_resources(
            "127.0.0.1:0".parse().unwrap(),
            "Plain",
            Share::default(),
        )
        .unwrap();
        let error = download_world(
            plain.address,
            None,
            "town",
            &worlds.0,
            None,
            true,
            &cancel,
            |_, _| {},
        )
        .unwrap_err();
        assert!(error.to_string().contains("does not share"), "{error}");
    }

    #[test]
    fn inventories_reject_escapes_and_foreign_types() {
        let file = |path: &str| File {
            path: path.into(),
            bytes: 1,
            sha256: "a".repeat(64),
        };
        let mut inventory = WorldInventory {
            id: "town".into(),
            files: vec![file("content.json")],
        };
        inventory.validate().unwrap();
        for bad in ["../x.json", "a/../../x.glb", "run.exe", "C:/x.glb", "noext"] {
            inventory.files.push(file(bad));
            assert!(inventory.validate().is_err(), "{bad}");
            inventory.files.pop();
        }
        inventory.files = vec![file("world.glb")];
        assert!(inventory.validate().is_err(), "content.json is required");
    }
}
