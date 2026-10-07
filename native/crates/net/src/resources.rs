//! Data-only resource inventories and a content-addressed, isolated disk cache.
//! A hash proves integrity/version identity, not trust in the serving peer.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    fs, io,
    path::{Path, PathBuf},
    sync::Arc,
};

pub const MAX_FILE_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_PACK_BYTES: usize = 128 * 1024 * 1024;
pub const MAX_MANIFEST_BYTES: usize = 12 * 1024;
pub const MAX_FILES: usize = 64;
const MAX_CACHE_BYTES: u64 = 512 * 1024 * 1024;
fn error(reason: &str) -> io::Error {
    io::Error::other(reason)
}
pub fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn valid_hash(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// Reject paths with Windows devices/ADS as well as Unix/Windows traversal.
pub fn validate_path(value: &str) -> io::Result<()> {
    if value.is_empty() || value.len() > 128 || !value.is_ascii() || value.split('/').count() > 8 {
        return Err(error("Invalid resource path"));
    }
    for part in value.split('/') {
        if part.is_empty()
            || part == "."
            || part == ".."
            || part.ends_with(['.', ' '])
            || part.bytes().any(|b| {
                b < 32 || matches!(b, b'\\' | b':' | b'<' | b'>' | b'"' | b'|' | b'?' | b'*')
            })
        {
            return Err(error(
                "Resource path escapes or contains unsupported characters",
            ));
        }
        let stem = part.split('.').next().unwrap().to_ascii_uppercase();
        if ["CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$"].contains(&stem.as_str())
            || ((stem.starts_with("COM") || stem.starts_with("LPT"))
                && stem.len() == 4
                && matches!(stem.as_bytes()[3], b'1'..=b'9'))
        {
            return Err(error("Windows device path rejected"));
        }
    }
    let extension = value.rsplit('.').next().unwrap().to_ascii_lowercase();
    if !["dff", "txd", "png", "col", "ifp"].contains(&extension.as_str())
        && value != "mod.json"
        && value != "resource.json"
    {
        return Err(error("Only supported native data files may be shared"));
    }
    Ok(())
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct File {
    pub path: String,
    pub bytes: usize,
    pub sha256: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Resource {
    pub id: String,
    pub name: String,
    pub files: Vec<File>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Manifest {
    pub version: u32,
    pub resources: Vec<Resource>,
}
impl Default for Manifest {
    fn default() -> Self {
        Self {
            version: 1,
            resources: Vec::new(),
        }
    }
}
impl Manifest {
    pub fn validate(&self) -> io::Result<()> {
        if self.version != 1 || self.resources.len() > 16 {
            return Err(error(
                "Unsupported resource inventory version or resource count",
            ));
        }
        let mut ids = HashSet::new();
        let (mut count, mut bytes) = (0, 0_usize);
        for resource in &self.resources {
            if resource.id.is_empty()
                || resource.id.len() > 32
                || !resource
                    .id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
                || !ids.insert(resource.id.to_ascii_lowercase())
                || resource.name.len() > 128
            {
                return Err(error("Invalid or duplicate resource identity"));
            }
            let mut paths = HashSet::new();
            let mut manifests = 0;
            for file in &resource.files {
                validate_path(&file.path)?;
                if !paths.insert(file.path.to_ascii_lowercase())
                    || !valid_hash(&file.sha256)
                    || file.bytes > MAX_FILE_BYTES
                {
                    return Err(error("Invalid, duplicate or oversized resource file"));
                }
                if file.path == "mod.json" || file.path == "resource.json" {
                    manifests += 1;
                }
                count += 1;
                bytes = bytes
                    .checked_add(file.bytes)
                    .ok_or_else(|| error("Resource size overflow"))?;
            }
            if manifests != 1 {
                return Err(error("Each resource needs exactly one native manifest"));
            }
        }
        if count > MAX_FILES
            || bytes > MAX_PACK_BYTES
            || serde_json::to_vec(self)?.len() > MAX_MANIFEST_BYTES
        {
            return Err(error("Resource inventory exceeds transfer limits"));
        }
        Ok(())
    }
    pub fn fingerprint(&self) -> io::Result<String> {
        self.validate()?;
        Ok(hash(&serde_json::to_vec(self)?))
    }
    pub fn total_bytes(&self) -> usize {
        self.resources
            .iter()
            .flat_map(|r| &r.files)
            .map(|f| f.bytes)
            .sum()
    }
}
pub struct Input {
    pub name: String,
    pub files: Vec<(String, Vec<u8>)>,
}
#[derive(Clone, Default)]
pub struct Share {
    pub manifest: Manifest,
    blobs: HashMap<String, Arc<[u8]>>,
}
impl Share {
    pub fn build(inputs: Vec<Input>) -> io::Result<Self> {
        let mut share = Self::default();
        let mut total = 0_usize;
        for (index, input) in inputs.into_iter().enumerate() {
            let mut resource = Resource {
                id: format!("resource-{index:03}"),
                name: input.name,
                files: Vec::new(),
            };
            let mut files = input.files;
            files.sort_by(|a, b| a.0.cmp(&b.0));
            for (path, data) in files {
                validate_path(&path)?;
                total = total
                    .checked_add(data.len())
                    .ok_or_else(|| error("Resource size overflow"))?;
                if data.len() > MAX_FILE_BYTES || total > MAX_PACK_BYTES {
                    return Err(error("Shared resource memory budget exceeded"));
                }
                let sha256 = hash(&data);
                resource.files.push(File {
                    path,
                    bytes: data.len(),
                    sha256: sha256.clone(),
                });
                share.blobs.entry(sha256).or_insert_with(|| data.into());
            }
            share.manifest.resources.push(resource);
        }
        share.manifest.validate()?;
        Ok(share)
    }
    pub fn blob(&self, sha256: &str) -> Option<Arc<[u8]>> {
        self.blobs.get(sha256).cloned()
    }
}

fn reject_link(path: &Path) -> io::Result<fs::Metadata> {
    let metadata = fs::symlink_metadata(path)?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return Err(error("Cache reparse point rejected"));
        }
    }
    if metadata.file_type().is_symlink() {
        return Err(error("Cache symlink rejected"));
    }
    Ok(metadata)
}
fn directory(root: &Path, relative: &Path) -> io::Result<PathBuf> {
    let mut path = root.to_path_buf();
    for component in relative.components() {
        let std::path::Component::Normal(part) = component else {
            return Err(error("Invalid cache directory"));
        };
        path.push(part);
        match fs::create_dir(&path) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e),
        }
        if !reject_link(&path)?.is_dir() || !path.canonicalize()?.starts_with(root) {
            return Err(error("Cache directory escapes root"));
        }
    }
    Ok(path)
}
fn disk_usage(root: &Path) -> io::Result<u64> {
    fn walk(path: &Path, count: &mut usize, depth: usize) -> io::Result<u64> {
        if depth > 12 {
            return Err(error("Cache directory nesting limit"));
        }
        let mut bytes = 0_u64;
        for entry in fs::read_dir(path)? {
            *count += 1;
            if *count > 8192 {
                return Err(error("Cache entry limit; remove unused cache packs"));
            }
            let entry = entry?;
            let metadata = reject_link(&entry.path())?;
            bytes = bytes
                .checked_add(if metadata.is_dir() {
                    walk(&entry.path(), count, depth + 1)?
                } else {
                    metadata.len()
                })
                .ok_or_else(|| error("Cache size overflow"))?;
        }
        Ok(bytes)
    }
    walk(root, &mut 0, 0)
}
fn verified(path: &Path, file: &File) -> io::Result<Option<Vec<u8>>> {
    let metadata = match reject_link(path) {
        Ok(metadata) => metadata,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    if !metadata.is_file() {
        return Err(error("Cache file is not a regular file"));
    }
    if metadata.len() != file.bytes as u64 {
        return Ok(None);
    }
    let bytes = fs::read(path)?;
    Ok((hash(&bytes) == file.sha256).then_some(bytes))
}
fn write_checked(path: &Path, data: &[u8], usage: &mut u64) -> io::Result<()> {
    if usage.saturating_add(data.len() as u64) > MAX_CACHE_BYTES {
        return Err(error(
            "Mod cache is full (512 MiB); remove unused packs before joining",
        ));
    }
    let old = match reject_link(path) {
        Ok(metadata) if metadata.is_file() => metadata.len(),
        Ok(_) => return Err(error("Cache target is not a file")),
        Err(e) if e.kind() == io::ErrorKind::NotFound => 0,
        Err(e) => return Err(e),
    };
    let mut random = [0; 12];
    getrandom::fill(&mut random).map_err(|e| error(&e.to_string()))?;
    let temporary = path.with_extension(format!("{}.part", hash(&random)));
    let result = (|| {
        use std::io::Write;
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        file.write_all(data)?;
        file.sync_all()?;
        drop(file);
        if path.exists() {
            reject_link(path)?;
            fs::remove_file(path)?;
        }
        fs::rename(&temporary, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result?;
    *usage = usage.saturating_sub(old).saturating_add(data.len() as u64);
    Ok(())
}
#[derive(Debug)]
pub struct Cached {
    pub mods: PathBuf,
    pub fingerprint: String,
    pub downloaded_bytes: usize,
    pub reused_files: usize,
}
/// The separate preflight connection closes before the renderer changes worlds.
/// A subsequent game handshake pins the inventory fingerprint against races.
pub fn download(
    address: std::net::SocketAddr,
    join_code: Option<&str>,
    root: &Path,
    allow_download: bool,
    cancel: &std::sync::atomic::AtomicBool,
    progress: impl FnMut(usize, usize),
) -> io::Result<Cached> {
    use super::{Message, Wire, TIMEOUT, VERSION};
    use std::collections::VecDeque;
    use std::sync::atomic::Ordering;
    use std::time::{Duration, Instant};
    let stream = if let Some(code) = join_code {
        super::relay::open_tunnel(address, code)?
    } else {
        std::net::TcpStream::connect_timeout(&address, Duration::from_secs(3))?
    };
    let mut wire = Wire::new(stream)?;
    let mut pending = VecDeque::new();
    fn next(
        wire: &mut Wire,
        pending: &mut VecDeque<Message>,
        cancel: &std::sync::atomic::AtomicBool,
    ) -> io::Result<Message> {
        let started = Instant::now();
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err(error("Join cancelled"));
            }
            if let Some(message) = pending.pop_front() {
                if let Message::Reject(reason) = message {
                    return Err(error(&reason));
                }
                return Ok(message);
            }
            if started.elapsed() >= TIMEOUT {
                return Err(error("Server mod transfer timed out"));
            }
            wire.flush()?;
            pending.extend(wire.read()?);
            std::thread::sleep(Duration::from_millis(2));
        }
    }
    wire.queue(&Message::ResourceQuery { version: VERSION })?;
    let Message::ResourceManifest(manifest) = next(&mut wire, &mut pending, cancel)? else {
        return Err(error("Server does not support native resource downloads"));
    };
    manifest.validate()?;
    let progress = std::cell::RefCell::new(progress);
    let completed = std::cell::Cell::new(0_usize);
    progress.borrow_mut()(0, manifest.total_bytes());
    cache(
        root,
        &manifest,
        |file| {
            if !allow_download {
                return Err(error(
                    "Missing server mods. Enable automatic downloads or use an existing cache.",
                ));
            }
            wire.queue(&Message::ResourceFile {
                sha256: file.sha256.clone(),
            })?;
            let mut bytes = Vec::with_capacity(file.bytes);
            let started = Instant::now();
            loop {
                if started.elapsed() > Duration::from_secs(180) {
                    return Err(error("Server resource exceeded transfer deadline"));
                }
                let Message::ResourceChunk {
                    sha256,
                    offset,
                    data,
                    done,
                } = next(&mut wire, &mut pending, cancel)?
                else {
                    return Err(error("Invalid resource transfer message"));
                };
                if sha256 != file.sha256
                    || offset != bytes.len()
                    || data.len() > 2048
                    || bytes.len().saturating_add(data.len()) > file.bytes
                    || (data.is_empty() && !done)
                {
                    return Err(error("Invalid resource chunk size or offset"));
                }
                bytes.extend(data);
                progress.borrow_mut()(completed.get() + bytes.len(), manifest.total_bytes());
                if done {
                    if bytes.len() != file.bytes {
                        return Err(error("Incomplete resource download"));
                    }
                    return Ok(bytes);
                }
            }
        },
        |done, total| {
            completed.set(done);
            progress.borrow_mut()(done, total);
        },
    )
}
/// Download missing content with the caller's bounded transport, then publish
/// a complete manifest. Local mods live elsewhere and are never overwritten.
pub fn cache(
    root: &Path,
    manifest: &Manifest,
    mut fetch: impl FnMut(&File) -> io::Result<Vec<u8>>,
    mut progress: impl FnMut(usize, usize),
) -> io::Result<Cached> {
    let fingerprint = manifest.fingerprint()?;
    fs::create_dir_all(root)?;
    reject_link(root)?;
    let root = root.canonicalize()?;
    // OS ownership is released even after a process crash. Concurrent writers
    // must not invalidate each other's completion marker or size accounting.
    let lock_path = root.join(".lock");
    let _lock = match fs::File::create_new(&lock_path) {
        Ok(file) => file,
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
            if !reject_link(&lock_path)?.is_file() {
                return Err(error("Invalid cache lock"));
            }
            fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(&lock_path)?
        }
        Err(e) => return Err(e),
    };
    _lock.try_lock().map_err(|e| {
        error(&format!(
            "Cache is being prepared by another instance; retry when it finishes ({e})"
        ))
    })?;
    let mut usage = disk_usage(&root)?;
    let blobs = directory(&root, Path::new("blobs"))?;
    let pack = directory(&root, &PathBuf::from("packs").join(&fingerprint))?;
    let mods = directory(
        &root,
        &PathBuf::from("packs").join(&fingerprint).join("resources"),
    )?;
    let inventory = pack.join("inventory.json");
    if inventory.exists() {
        let metadata = reject_link(&inventory)?;
        if !metadata.is_file() {
            return Err(error("Invalid cache inventory marker"));
        }
        fs::remove_file(&inventory)?;
        usage = usage.saturating_sub(metadata.len());
    }
    let (mut downloaded_bytes, mut reused_files, mut processed) = (0, 0, 0);
    for (index, resource) in manifest.resources.iter().enumerate() {
        let relative = PathBuf::from("packs")
            .join(&fingerprint)
            .join("resources")
            .join(format!("{index:03}-{}", resource.id));
        let resource_root = directory(&root, &relative)?;
        for file in &resource.files {
            let path = resource_root.join(&file.path);
            let parent = Path::new(&file.path).parent().unwrap();
            directory(&root, &relative.join(parent))?;
            if verified(&path, file)?.is_some() {
                reused_files += 1;
            } else {
                let blob = blobs.join(&file.sha256);
                let data = if let Some(bytes) = verified(&blob, file)? {
                    reused_files += 1;
                    bytes
                } else {
                    let bytes = fetch(file)?;
                    if bytes.len() != file.bytes || hash(&bytes) != file.sha256 {
                        return Err(error("Downloaded mod failed size/SHA-256 verification"));
                    }
                    write_checked(&blob, &bytes, &mut usage)?;
                    downloaded_bytes += bytes.len();
                    bytes
                };
                write_checked(&path, &data, &mut usage)?;
            }
            processed += file.bytes;
            progress(processed, manifest.total_bytes());
        }
    }
    // No extra leaf resource or file may silently join the renderer's load set.
    let mut expected_files = HashSet::new();
    let mut expected_dirs = HashSet::new();
    for (index, resource) in manifest.resources.iter().enumerate() {
        for file in &resource.files {
            let path = PathBuf::from(format!("{index:03}-{}", resource.id)).join(&file.path);
            expected_files.insert(path.clone());
            let mut parent = path.parent();
            while let Some(directory) = parent {
                if directory.as_os_str().is_empty() {
                    break;
                }
                expected_dirs.insert(directory.to_path_buf());
                parent = directory.parent();
            }
        }
    }
    fn check_tree(
        directory: &Path,
        root: &Path,
        files: &HashSet<PathBuf>,
        dirs: &HashSet<PathBuf>,
    ) -> io::Result<()> {
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            let path = entry.path();
            let relative = path
                .strip_prefix(root)
                .map_err(|_| error("Cache path escaped root"))?;
            let metadata = reject_link(&path)?;
            if metadata.is_dir() {
                if !dirs.contains(relative) {
                    return Err(error("Mod cache contains an unexpected resource directory"));
                }
                check_tree(&path, root, files, dirs)?;
            } else if !metadata.is_file() || !files.contains(relative) {
                return Err(error(
                    "Mod cache contains a file outside the server inventory",
                ));
            }
        }
        Ok(())
    }
    check_tree(&mods, &mods, &expected_files, &expected_dirs)?;
    write_checked(&inventory, &serde_json::to_vec(manifest)?, &mut usage)?;
    Ok(Cached {
        mods,
        fingerprint,
        downloaded_bytes,
        reused_files,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            let mut random = [0; 12];
            getrandom::fill(&mut random).unwrap();
            let path =
                std::env::temp_dir().join(format!("sa-resource-cache-test-{}", hash(&random)));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let parent = std::env::temp_dir().canonicalize().unwrap();
            let resolved = self.0.canonicalize().unwrap();
            assert_eq!(resolved.parent(), Some(parent.as_path()));
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn share(data: &[u8]) -> Share {
        Share::build(vec![Input {
            name: "Test car".into(),
            files: vec![
                (
                    "mod.json".into(),
                    br#"{"enabled":true,"name":"Test car","vehicles":[{"dff":"car.dff"}]}"#
                        .to_vec(),
                ),
                ("car.dff".into(), data.to_vec()),
            ],
        }])
        .unwrap()
    }
    #[test]
    fn cache_reuses_content_and_only_fetches_changed_files_between_versions() {
        let temp = Temp::new();
        let source = share(b"original model");
        let mut calls = 0;
        let first = cache(
            &temp.0,
            &source.manifest,
            |f| {
                calls += 1;
                Ok(source.blob(&f.sha256).unwrap().to_vec())
            },
            |_, _| {},
        )
        .unwrap();
        assert_eq!(calls, 2);
        assert_eq!(first.downloaded_bytes, source.manifest.total_bytes());
        let second = cache(
            &temp.0,
            &source.manifest,
            |_| panic!("cached files fetched again"),
            |_, _| {},
        )
        .unwrap();
        assert_eq!(second.mods, first.mods);
        assert_eq!(second.reused_files, 2);
        assert_eq!(second.downloaded_bytes, 0);
        let unexpected = first.mods.join("000-resource-000/private.png");
        fs::write(&unexpected, b"not in the server inventory").unwrap();
        assert!(cache(&temp.0, &source.manifest, |_| panic!("cached"), |_, _| {}).is_err());
        assert!(!first.mods.parent().unwrap().join("inventory.json").exists());
        fs::remove_file(unexpected).unwrap();
        let updated = share(b"new model");
        let mut fetched = Vec::new();
        let next = cache(
            &temp.0,
            &updated.manifest,
            |f| {
                fetched.push(f.path.clone());
                Ok(updated.blob(&f.sha256).unwrap().to_vec())
            },
            |_, _| {},
        )
        .unwrap();
        assert_eq!(fetched, ["car.dff"]);
        assert_ne!(next.fingerprint, first.fingerprint);
        assert!(first.mods.is_dir());
        assert_eq!(
            fs::read(next.mods.join("000-resource-000/car.dff")).unwrap(),
            b"new model"
        );
    }
    #[test]
    fn incorrect_hash_and_interruption_never_publish_a_complete_inventory() {
        let temp = Temp::new();
        let source = share(b"model");
        assert!(cache(
            &temp.0,
            &source.manifest,
            |_| Ok(b"bad data".to_vec()),
            |_, _| {}
        )
        .is_err());
        let pack = temp
            .0
            .join("packs")
            .join(source.manifest.fingerprint().unwrap());
        assert!(!pack.join("inventory.json").exists());
        assert!(cache(
            &temp.0,
            &source.manifest,
            |_| Err(error("download interrupted")),
            |_, _| {}
        )
        .is_err());
        assert!(!pack.join("inventory.json").exists());
        let good = cache(
            &temp.0,
            &source.manifest,
            |f| Ok(source.blob(&f.sha256).unwrap().to_vec()),
            |_, _| {},
        )
        .unwrap();
        assert!(good.mods.parent().unwrap().join("inventory.json").is_file());
        fs::write(good.mods.join("000-resource-000/car.dff"), b"other").unwrap();
        let repaired = cache(
            &temp.0,
            &source.manifest,
            |_| panic!("valid blob should repair corrupt pack"),
            |_, _| {},
        )
        .unwrap();
        assert_eq!(
            fs::read(repaired.mods.join("000-resource-000/car.dff")).unwrap(),
            b"model"
        );
    }
    #[test]
    fn concurrent_cache_writer_is_rejected_and_released_lock_is_reusable() {
        let temp = Temp::new();
        fs::create_dir_all(&temp.0).unwrap();
        let lock = fs::File::create_new(temp.0.join(".lock")).unwrap();
        lock.try_lock().unwrap();
        let source = share(b"model");
        let result = cache(
            &temp.0,
            &source.manifest,
            |_| panic!("must not fetch under another writer"),
            |_, _| {},
        );
        assert!(result.unwrap_err().to_string().contains("another instance"));
        drop(lock);
        cache(
            &temp.0,
            &source.manifest,
            |f| Ok(source.blob(&f.sha256).unwrap().to_vec()),
            |_, _| {},
        )
        .unwrap();
    }
    #[test]
    fn traversal_devices_scripts_duplicate_case_and_unknown_versions_are_rejected() {
        for path in [
            "../car.dff",
            "/car.dff",
            "C:/car.dff",
            "folder\\car.dff",
            "car.dff:stream",
            "CON.dff",
            "LPT1.png",
            "folder./car.dff",
            "a//car.dff",
            "script.lua",
            "plugin.dll",
            "fxmanifest.json",
        ] {
            assert!(validate_path(path).is_err(), "accepted {path}");
        }
        assert!(validate_path("stream/a car.dff").is_ok());
        let mut manifest = share(b"model").manifest;
        let mut duplicate = manifest.resources[0].files[0].clone();
        duplicate.path = duplicate.path.to_ascii_uppercase();
        manifest.resources[0].files.push(duplicate);
        assert!(manifest.validate().is_err());
        manifest.resources[0].files.pop();
        manifest.version = 2;
        assert!(manifest.validate().is_err());
        manifest.version = 1;
        manifest.resources[0].files[0].bytes = MAX_FILE_BYTES + 1;
        assert!(manifest.validate().is_err());
    }
    fn wait(mut condition: impl FnMut() -> bool) {
        let started = std::time::Instant::now();
        while !condition() {
            assert!(
                started.elapsed() < std::time::Duration::from_secs(8),
                "resource handshake timed out"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }
    #[test]
    fn actual_host_transfers_chunks_reuses_cache_and_pins_join_versions() {
        use super::super::{Pose, Session};
        use std::sync::atomic::AtomicBool;
        let temp = Temp::new();
        let source = share(&(0..8193).map(|i| i as u8).collect::<Vec<_>>());
        let host = Session::host_resources("127.0.0.1:0".parse().unwrap(), "Host", source.clone())
            .unwrap();
        let cancel = AtomicBool::new(false);
        let cached = download(host.address, None, &temp.0, true, &cancel, |_, _| {}).unwrap();
        assert_eq!(cached.downloaded_bytes, source.manifest.total_bytes());
        let reused = download(host.address, None, &temp.0, false, &cancel, |_, _| {}).unwrap();
        assert_eq!(reused.downloaded_bytes, 0);
        let missing = Temp::new();
        assert!(
            download(host.address, None, &missing.0, false, &cancel, |_, _| {})
                .unwrap_err()
                .to_string()
                .contains("Missing server mods")
        );
        let unprepared = Session::join(host.address, "Unprepared").unwrap();
        wait(|| {
            unprepared
                .update(Pose::default())
                .unwrap()
                .status
                .contains("resource versions do not match")
        });
        let changed = Session::join_resources(host.address, "Wrong", Some("0".repeat(64))).unwrap();
        wait(|| {
            changed
                .update(Pose::default())
                .unwrap()
                .status
                .contains("resource versions do not match")
        });
        let guest =
            Session::join_resources(host.address, "Prepared", Some(cached.fingerprint)).unwrap();
        wait(|| guest.update(Pose::default()).is_some_and(|r| r.connected));
        wait(|| {
            host.update(Pose::default())
                .is_some_and(|r| r.peers.len() == 2)
        });
    }
    #[test]
    fn relay_preflight_and_game_join_use_the_same_host_inventory() {
        use super::super::{relay::Relay, Pose, Session};
        use std::sync::atomic::AtomicBool;
        let temp = Temp::new();
        let relay = Relay::start("127.0.0.1:0".parse().unwrap()).unwrap();
        let (host, publication) =
            Session::host_relay_resources(relay.address, "Host", false, share(b"model")).unwrap();
        wait(|| !publication.report().code.is_empty());
        let code = publication.report().code;
        let cached = download(
            relay.address,
            Some(&code),
            &temp.0,
            true,
            &AtomicBool::new(false),
            |_, _| {},
        )
        .unwrap();
        let guest =
            Session::join_relay_resources(relay.address, &code, "Guest", Some(cached.fingerprint))
                .unwrap();
        wait(|| guest.update(Pose::default()).is_some_and(|r| r.connected));
        wait(|| {
            host.update(Pose::default())
                .is_some_and(|r| r.peers.len() == 2)
        });
    }
}
