//! The community world catalog: `community/worlds.json` in the SARE
//! repository lists worlds people share, each a ZIP download with its
//! SHA-256. Discover shows them and installs one into `content/` with a click.
//! The client checks the hash before it opens the archive and the world's
//! `content.json` before it moves it in place; a world the player made or
//! copied in themselves is never replaced.
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    io::Read,
    path::{Path, PathBuf},
    time::Duration,
};

/// `SARE_WORLD_LIST` overrides it with another `https://` URL or a file.
pub const LIST_URL: &str =
    "https://raw.githubusercontent.com/LordM8YT/SARE/main/community/worlds.json";
const MAX_WORLDS: usize = 200;
const MAX_LIST_BYTES: u64 = 512 * 1024;
const MAX_ZIP_BYTES: u64 = 256 * 1024 * 1024;
const MAX_UNPACKED_BYTES: u64 = 256 * 1024 * 1024;
const MAX_ENTRIES: usize = 1024;
/// Written into a world installed from the catalog, so an update may replace
/// it. Folders without it belong to the player and are left alone.
pub const MARKER: &str = ".sare-catalog";

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Entry {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub description: String,
    pub version: String,
    pub license: String,
    /// `https://` link to a ZIP with the world folder.
    pub download: String,
    pub sha256: String,
    pub bytes: u64,
}
#[derive(Deserialize)]
struct List {
    version: u32,
    worlds: Vec<Entry>,
}

fn valid(e: &Entry) -> bool {
    let short =
        |text: &str, max: usize| text.chars().count() <= max && !text.chars().any(char::is_control);
    sa_net::resources::valid_pack_id(&e.id)
        && !e.name.trim().is_empty()
        && short(&e.name, 48)
        && short(&e.author, 48)
        && short(&e.description, 300)
        && !e.version.trim().is_empty()
        && short(&e.version, 24)
        && !e.license.trim().is_empty()
        && short(&e.license, 64)
        && e.download.starts_with("https://")
        && e.download.len() <= 300
        && e.sha256.len() == 64
        && e.sha256.bytes().all(|b| b.is_ascii_hexdigit())
        && e.bytes > 0
        && e.bytes <= MAX_ZIP_BYTES
}

pub fn parse(data: &[u8]) -> Result<Vec<Entry>, String> {
    let list: List =
        serde_json::from_slice(data).map_err(|e| format!("The world list is not valid: {e}"))?;
    if list.version != 1 {
        return Err("The world list needs a newer SARE Client.".into());
    }
    let mut seen = std::collections::HashSet::new();
    Ok(list
        .worlds
        .into_iter()
        .filter(valid)
        .filter(|e| seen.insert(e.id.clone()))
        .take(MAX_WORLDS)
        .collect())
}

fn agent(timeout: u64) -> ureq::Agent {
    ureq::Agent::config_builder()
        .https_only(true)
        .timeout_global(Some(Duration::from_secs(timeout)))
        .max_redirects(5)
        .build()
        .into()
}
fn download(source: &str, limit: u64, timeout: u64) -> Result<Vec<u8>, String> {
    if !source.starts_with("https://") {
        return std::fs::read(source).map_err(|e| format!("Could not read {source}: {e}"));
    }
    agent(timeout)
        .get(source)
        .call()
        .map_err(|e| format!("Download failed: {e}"))?
        .body_mut()
        .with_config()
        .limit(limit)
        .read_to_vec()
        .map_err(|e| format!("Download failed: {e}"))
}

pub fn source() -> String {
    std::env::var("SARE_WORLD_LIST")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| LIST_URL.into())
}
pub fn load(source: &str) -> Result<Vec<Entry>, String> {
    parse(&download(source, MAX_LIST_BYTES, 15)?)
}

/// How a catalog world relates to what is in `content/`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum State {
    NotInstalled,
    Installed,
    /// Installed from the catalog, and the catalog has another version.
    Update,
    /// A world of the player's own uses this id; the catalog leaves it alone.
    Conflict,
}
pub fn state(entry: &Entry, content: &Path) -> State {
    let folder = content.join(&entry.id);
    if !folder.join(sa_scene::content_pack::MANIFEST).is_file() {
        return State::NotInstalled;
    }
    match std::fs::read_to_string(folder.join(MARKER)) {
        Ok(hash) if hash.trim() == entry.sha256 => State::Installed,
        Ok(_) => State::Update,
        Err(_) => State::Conflict,
    }
}

/// Unpack `zip` into `target`. The world may sit at the archive's root or
/// in one top folder; anything outside that folder is ignored.
fn unpack(zip: &[u8], target: &Path) -> Result<(), String> {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(zip))
        .map_err(|e| format!("The download is not a ZIP: {e}"))?;
    if archive.len() > MAX_ENTRIES {
        return Err("The world has too many files.".into());
    }
    let mut names = Vec::new();
    for i in 0..archive.len() {
        let file = archive.by_index(i).map_err(|e| e.to_string())?;
        let Some(path) = file.enclosed_name() else {
            return Err("The ZIP has a file outside its folder.".into());
        };
        names.push((path, file.is_dir()));
    }
    let manifest = Path::new(sa_scene::content_pack::MANIFEST);
    let root = if names.iter().any(|(p, _)| p == manifest) {
        PathBuf::new()
    } else {
        // Exactly one top folder may hold the world's content.json.
        let mut tops: Vec<PathBuf> = names
            .iter()
            .filter(|(p, _)| p.components().count() == 2 && p.ends_with(manifest))
            .filter_map(|(p, _)| p.parent().map(Path::to_path_buf))
            .collect();
        tops.dedup();
        match tops.as_slice() {
            [top] => top.clone(),
            [] => return Err("The ZIP has no content.json.".into()),
            _ => return Err("The ZIP holds more than one world.".into()),
        }
    };
    let mut total = 0_u64;
    for (i, (path, is_dir)) in names.into_iter().enumerate() {
        let Ok(relative) = path.strip_prefix(&root) else {
            continue;
        };
        if relative.as_os_str().is_empty() {
            continue;
        }
        let out = target.join(relative);
        if is_dir {
            std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
            continue;
        }
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let file = archive.by_index(i).map_err(|e| e.to_string())?;
        let mut data = Vec::new();
        // Count what is really unpacked, not what the archive claims.
        file.take(MAX_UNPACKED_BYTES - total + 1)
            .read_to_end(&mut data)
            .map_err(|e| e.to_string())?;
        total += data.len() as u64;
        if total > MAX_UNPACKED_BYTES {
            return Err("The world is larger than 256 MiB unpacked.".into());
        }
        std::fs::write(&out, data).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Download, check and install `entry` into `content/<id>`.
pub fn install(entry: &Entry, content: &Path) -> Result<PathBuf, String> {
    if state(entry, content) == State::Conflict {
        return Err(format!(
            "You already have a world with the id \"{}\". Rename or remove that folder first.",
            entry.id
        ));
    }
    let zip = download(&entry.download, entry.bytes + 1, 600)?;
    if zip.len() as u64 != entry.bytes || format!("{:x}", Sha256::digest(&zip)) != entry.sha256 {
        return Err("The download does not match the catalog (size or SHA-256).".into());
    }
    std::fs::create_dir_all(content).map_err(|e| e.to_string())?;
    let staging = content.join(format!(".{}-installing", entry.id));
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir(&staging).map_err(|e| e.to_string())?;
    let result = (|| {
        unpack(&zip, &staging)?;
        let manifest = sa_scene::content_pack::Manifest::read(&staging)
            .map_err(|e| format!("The world does not open: {e:#}"))?;
        if manifest.id != entry.id {
            return Err(format!(
                "The world's content.json says \"{}\", not \"{}\".",
                manifest.id, entry.id
            ));
        }
        std::fs::write(staging.join(MARKER), &entry.sha256).map_err(|e| e.to_string())?;
        let folder = content.join(&entry.id);
        if folder.exists() {
            // Only a catalog install gets here (`Conflict` was refused).
            std::fs::remove_dir_all(&folder).map_err(|e| e.to_string())?;
        }
        std::fs::rename(&staging, &folder).map_err(|e| e.to_string())?;
        Ok(folder)
    })();
    if result.is_err() {
        let _ = std::fs::remove_dir_all(&staging);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    struct Temp(PathBuf);
    impl Temp {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "sare-world-catalog-{name}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn zip(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut out = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        for (name, data) in files {
            out.start_file(*name, zip::write::SimpleFileOptions::default())
                .unwrap();
            out.write_all(data).unwrap();
        }
        out.finish().unwrap().into_inner()
    }
    const MANIFEST: &[u8] = br#"{"id":"hill-race","name":"Hill Race","version":"1.0.0","license":"CC0-1.0","world":{"scene":"world.glb","spawn":[0,0,1]}}"#;
    fn entry(zip_path: &Path, data: &[u8]) -> Entry {
        Entry {
            id: "hill-race".into(),
            name: "Hill Race".into(),
            author: "Sam".into(),
            description: String::new(),
            version: "1.0.0".into(),
            license: "CC0-1.0".into(),
            download: zip_path.to_string_lossy().into(),
            sha256: format!("{:x}", Sha256::digest(data)),
            bytes: data.len() as u64,
        }
    }

    #[test]
    fn lists_keep_only_complete_https_entries() {
        let good = r#"{"id":"a","name":"A","version":"1","license":"MIT","download":"https://x/a.zip","sha256":"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA","bytes":10}"#;
        let list = format!(
            r#"{{"version":1,"worlds":[{good},{},{}, {}]}}"#,
            good.replace("https://", "http://")
                .replace("\"a\"", "\"b\""),
            good.replace("\"bytes\":10", "\"bytes\":0")
                .replace("\"a\"", "\"c\""),
            good,
        );
        let entries = parse(list.as_bytes()).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].id, "a");
    }

    #[test]
    fn worlds_install_update_and_never_replace_the_players_own() {
        let (temp, content) = (Temp::new("files"), Temp::new("content"));
        // Packed inside a top folder, with a stray file outside it.
        let data = zip(&[
            ("Hill Race/content.json", MANIFEST),
            ("Hill Race/world.glb", b"glb"),
            ("readme.txt", b"outside"),
        ]);
        let path = temp.0.join("hill.zip");
        std::fs::write(&path, &data).unwrap();
        let world = entry(&path, &data);
        assert_eq!(state(&world, &content.0), State::NotInstalled);
        let folder = install(&world, &content.0).unwrap();
        assert_eq!(std::fs::read(folder.join("world.glb")).unwrap(), b"glb");
        assert!(!folder.join("readme.txt").exists());
        assert_eq!(state(&world, &content.0), State::Installed);

        // A new catalog version replaces the catalog install.
        let data2 = zip(&[("content.json", MANIFEST), ("world.glb", b"glb2")]);
        let path2 = temp.0.join("hill2.zip");
        std::fs::write(&path2, &data2).unwrap();
        let world2 = entry(&path2, &data2);
        assert_eq!(state(&world2, &content.0), State::Update);
        install(&world2, &content.0).unwrap();
        assert_eq!(std::fs::read(folder.join("world.glb")).unwrap(), b"glb2");

        // The player's own world with that id is never touched.
        std::fs::remove_file(folder.join(MARKER)).unwrap();
        assert_eq!(state(&world, &content.0), State::Conflict);
        assert!(install(&world, &content.0).is_err());
        assert_eq!(std::fs::read(folder.join("world.glb")).unwrap(), b"glb2");
    }

    #[test]
    fn bad_downloads_leave_nothing_behind() {
        let (temp, content) = (Temp::new("bad"), Temp::new("bad-content"));
        let data = zip(&[("content.json", MANIFEST)]);
        let path = temp.0.join("w.zip");
        std::fs::write(&path, &data).unwrap();
        let mut wrong_hash = entry(&path, &data);
        wrong_hash.sha256 = "0".repeat(64);
        assert!(install(&wrong_hash, &content.0).is_err());
        let other = zip(&[(
            "content.json",
            &br#"{"id":"other","name":"O","version":"1","license":"MIT","world":{"scene":"w.glb","spawn":[0,0,1]}}"#[..],
        )]);
        std::fs::write(&path, &other).unwrap();
        assert!(install(&entry(&path, &other), &content.0).is_err());
        assert_eq!(std::fs::read_dir(&content.0).unwrap().count(), 0);
    }

    #[test]
    fn the_bundled_list_parses() {
        parse(include_bytes!("../../../../community/worlds.json")).unwrap();
    }
}
