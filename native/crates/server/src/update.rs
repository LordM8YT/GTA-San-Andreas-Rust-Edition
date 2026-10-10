//! Self-update for packaged dedicated servers, like the launcher's client updates.
//! Downloads `SARE-server-<platform>.zip` from this repository's latest GitHub
//! release, verifies it and replaces only the files in the package's `server/`
//! folder while no players are connected. `server-data/` is never touched.
//! Never runs a build, shell script or downloaded installer.
//! Path rules are kept aligned with `launcher/src/updater.rs`.
use anyhow::{bail, ensure, Context, Result};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

const REPO: &str = "LordM8YT/GTA-San-Andreas-Rust-Edition";
const BUILD: &str = "sare-build.json";
const PREFIX: &str = "server/";
const MAX_PACKAGE: u64 = 128 * 1024 * 1024;
const MAX_CONTENT: u64 = 256 * 1024 * 1024;
const INTERVAL: Duration = Duration::from_secs(30 * 60);
/// Exit code asking start-server.cmd/.sh to start the updated server again.
pub const RESTART_CODE: i32 = 42;
/// Set by the packaged start scripts, which restart the server after an update.
pub const SUPERVISED: &str = "SARE_SERVER_SUPERVISED";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Build {
    schema: u32,
    commit: String,
    platform: String,
    files: Vec<Entry>,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    path: String,
    size: u64,
    sha256: String,
}
#[derive(Deserialize)]
struct Release {
    tag_name: String,
    draft: bool,
    prerelease: bool,
    assets: Vec<Asset>,
}
#[derive(Deserialize)]
struct Asset {
    name: String,
    size: u64,
    digest: Option<String>,
    browser_download_url: String,
}

enum Event {
    Current,
    Staged(PathBuf, String),
    Failed(String),
}

pub struct Updater {
    root: PathBuf,
    supervised: bool,
    rx: Option<mpsc::Receiver<Event>>,
    staged: Option<(PathBuf, String)>,
    next: Instant,
    announced: bool,
}

fn platform() -> &'static str {
    if cfg!(windows) {
        "windows"
    } else {
        "linux"
    }
}
fn binary(name: &str) -> String {
    format!("{name}{}", if cfg!(windows) { ".exe" } else { "" })
}
fn hex(s: &str, n: usize) -> bool {
    s.len() == n
        && s.bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}
fn link(path: &Path) -> bool {
    path.symlink_metadata().is_ok_and(|m| {
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            m.file_attributes() & 0x400 != 0
        }
        #[cfg(not(windows))]
        {
            m.is_symlink()
        }
    })
}
fn safe_path(name: &str) -> bool {
    const DEVICES: [&str; 22] = [
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
        "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ];
    !name.is_empty()
        && name.len() < 240
        && name.split('/').all(|p| {
            !p.is_empty()
                && p != "."
                && p != ".."
                && !DEVICES.contains(
                    &p.split('.')
                        .next()
                        .unwrap_or("")
                        .to_ascii_uppercase()
                        .as_str(),
                )
                && !p.ends_with(['.', ' '])
                && p.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._-+ ".contains(&b))
        })
}
/// Only the server binaries, the build manifest and legal notices may change.
fn allowed(name: &str) -> bool {
    safe_path(name)
        && (name == BUILD
            || name == binary("sa-server")
            || name == binary("sa-relay")
            || ["LICENSE", "THIRD-PARTY-NOTICES.txt"].contains(&name)
            || name.starts_with("licenses/"))
}
fn plain(root: &Path, name: &str) -> Result<PathBuf> {
    ensure!(allowed(name), "Unexpected update path: {name}");
    let mut path = root.to_path_buf();
    for part in name.split('/') {
        path.push(part);
        ensure!(!link(&path), "Update path is a link: {name}");
    }
    Ok(path)
}
fn parse_build(data: &[u8]) -> Result<Build> {
    ensure!(data.len() <= 1024 * 1024, "Package manifest too large");
    let build: Build = serde_json::from_slice(data)?;
    ensure!(
        build.schema == 1 && hex(&build.commit, 40) && build.platform == platform(),
        "Unsupported package manifest/platform"
    );
    ensure!(
        !build.files.is_empty() && build.files.len() <= 2048,
        "Invalid package file count"
    );
    let mut names = BTreeSet::new();
    let mut total = 0;
    for file in &build.files {
        ensure!(
            file.path != BUILD
                && allowed(&file.path)
                && names.insert(file.path.to_ascii_lowercase()),
            "Unexpected or duplicate package entry: {}",
            file.path
        );
        ensure!(
            file.size <= MAX_PACKAGE && hex(&file.sha256, 64),
            "Invalid package entry size/hash"
        );
        total += file.size;
        ensure!(total <= MAX_CONTENT, "Package too large");
    }
    for name in [binary("sa-server"), binary("sa-relay")] {
        ensure!(names.contains(&name), "Incomplete server package");
    }
    Ok(build)
}
fn read_build(root: &Path) -> Result<Build> {
    let mut data = Vec::new();
    File::open(plain(root, BUILD)?)?
        .take(1024 * 1024 + 1)
        .read_to_end(&mut data)?;
    parse_build(&data)
}
fn file_hash(path: &Path) -> Result<String> {
    let mut hasher = Sha256::new();
    std::io::copy(&mut File::open(path)?, &mut hasher)?;
    Ok(format!("{:x}", hasher.finalize()))
}
fn verify_tree(root: &Path) -> Result<Build> {
    let build = read_build(root)?;
    for entry in &build.files {
        let path = plain(root, &entry.path)?;
        ensure!(
            path.metadata()?.is_file() && path.metadata()?.len() == entry.size,
            "Changed update file: {}",
            entry.path
        );
        ensure!(
            file_hash(&path)? == entry.sha256,
            "Update file hash mismatch: {}",
            entry.path
        );
    }
    Ok(build)
}

/// The package's `server/` folder when this executable runs from an extracted
/// server package; `None` for source/Cargo builds.
pub fn install_root() -> Option<PathBuf> {
    if !(cfg!(target_arch = "x86_64") && (cfg!(windows) || cfg!(target_os = "linux"))) {
        return None;
    }
    let exe = std::env::current_exe().ok()?.canonicalize().ok()?;
    let root = exe.parent()?.to_path_buf();
    let packaged = exe.file_name()? == binary("sa-server").as_str()
        && root.join(BUILD).is_file()
        && !root.join("sa-launcher").exists()
        && !root.join("sa-launcher.exe").exists()
        && root.parent().is_some_and(|p| !p.join(".git").exists());
    (packaged && read_build(&root).is_ok()).then_some(root)
}

impl Updater {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            supervised: std::env::var_os(SUPERVISED).is_some_and(|v| v == "1"),
            rx: None,
            staged: None,
            next: Instant::now() + Duration::from_secs(5),
            announced: false,
        }
    }

    /// Call every frame. Returns console lines to print, and the commit of a
    /// verified update once it is ready to install (only while the server is empty).
    pub fn poll(&mut self, players: usize, log: &mut Vec<String>) -> Option<String> {
        if self.rx.is_none() && self.staged.is_none() && Instant::now() >= self.next {
            self.next = Instant::now() + INTERVAL;
            let (tx, rx) = mpsc::channel();
            let root = self.root.clone();
            thread::spawn(move || {
                let _ = tx.send(match prepare(&root) {
                    Ok(Some((stage, commit))) => Event::Staged(stage, commit),
                    Ok(None) => Event::Current,
                    Err(e) => Event::Failed(format!("{e:#}")),
                });
            });
            self.rx = Some(rx);
        }
        if let Some(rx) = &self.rx {
            match rx.try_recv() {
                Ok(Event::Current) => self.rx = None,
                Ok(Event::Staged(stage, commit)) => {
                    self.rx = None;
                    log.push(format!(
                        "Server update {} downloaded and verified.",
                        &commit[..12]
                    ));
                    self.staged = Some((stage, commit));
                }
                Ok(Event::Failed(error)) => {
                    self.rx = None;
                    log.push(format!(
                        "Server update check failed: {error}. The current server keeps running."
                    ));
                }
                Err(mpsc::TryRecvError::Disconnected) => self.rx = None,
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        let commit = self.staged.as_ref()?.1.clone();
        if !self.supervised {
            if !self.announced {
                self.announced = true;
                log.push(format!(
                    "Server update {} is ready. Start the server with start-server.cmd/.sh to install updates automatically.",
                    &commit[..12]
                ));
            }
            return None;
        }
        if players > 0 {
            if !self.announced {
                self.announced = true;
                log.push(
                    "The update installs and the server restarts when no players are online."
                        .into(),
                );
            }
            return None;
        }
        Some(commit)
    }

    /// Replace the server files with the staged update. On failure every
    /// changed file is restored and the staged update is discarded.
    pub fn install(&mut self) -> Result<()> {
        let (stage, _) = self.staged.take().context("No staged update")?;
        let result = replace(&self.root, &stage);
        if result.is_err() {
            let _ = fs::remove_dir_all(&stage);
        }
        result
    }
}

fn update_dir(root: &Path) -> Result<PathBuf> {
    let path = root.join(".sare-update");
    ensure!(!link(&path), "Update folder is a link");
    fs::create_dir_all(&path)?;
    Ok(path)
}
fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .https_only(true)
        .timeout_global(Some(Duration::from_secs(120)))
        .max_redirects(5)
        .build()
        .into()
}
fn select_asset(data: &[u8], current: &str) -> Result<Option<(String, Asset, String)>> {
    let release: Release = serde_json::from_slice(data)?;
    ensure!(
        !release.draft && !release.prerelease,
        "Release is not published/stable"
    );
    let commit = release
        .tag_name
        .strip_prefix("client-")
        .context("Unsupported release tag")?
        .to_owned();
    ensure!(hex(&commit, 40), "Invalid release commit");
    if current == commit {
        return Ok(None);
    }
    let name = format!("SARE-server-{}.zip", platform());
    let mut assets = release.assets.into_iter().filter(|a| a.name == name);
    let Some(asset) = assets.next() else {
        // Releases from before server packages existed carry no server ZIP.
        return Ok(None);
    };
    ensure!(
        assets.next().is_none() && asset.size > 0 && asset.size <= MAX_PACKAGE,
        "Invalid release asset"
    );
    ensure!(
        asset.browser_download_url
            == format!("https://github.com/{REPO}/releases/download/client-{commit}/{name}"),
        "Unexpected update download source"
    );
    let digest = asset
        .digest
        .as_ref()
        .and_then(|d| d.strip_prefix("sha256:"))
        .context("GitHub asset has no SHA-256 digest")?
        .to_owned();
    ensure!(hex(&digest, 64), "Invalid GitHub asset digest");
    Ok(Some((commit, asset, digest)))
}
fn prepare(root: &Path) -> Result<Option<(PathBuf, String)>> {
    let current = read_build(root)?;
    let http = agent();
    let mut response = match http
        .get(&format!(
            "https://api.github.com/repos/{REPO}/releases/latest"
        ))
        .header("User-Agent", "SARE-server")
        .header("Accept", "application/vnd.github+json")
        .call()
    {
        Ok(r) => r,
        Err(ureq::Error::StatusCode(404)) => bail!("No release has been published yet"),
        Err(e) => return Err(e.into()),
    };
    let mut metadata = Vec::new();
    response
        .body_mut()
        .as_reader()
        .take(1024 * 1024 + 1)
        .read_to_end(&mut metadata)?;
    ensure!(metadata.len() <= 1024 * 1024, "Release metadata too large");
    let Some((commit, asset, digest)) = select_asset(&metadata, &current.commit)? else {
        return Ok(None);
    };
    // Old staged downloads are never installed; start clean.
    let updates = update_dir(root)?;
    for entry in fs::read_dir(&updates)?.flatten() {
        if entry.file_name().to_string_lossy().len() > 40 && !link(&entry.path()) {
            let _ = fs::remove_dir_all(entry.path());
        }
    }
    let stage = updates.join(format!(
        "{commit}-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos()
    ));
    fs::create_dir(&stage)?;
    let result = (|| {
        let mut response = http
            .get(&asset.browser_download_url)
            .header("User-Agent", "SARE-server")
            .call()?;
        let package = stage.join("server.zip");
        download_verified(
            &mut response.body_mut().as_reader(),
            asset.size,
            &digest,
            &package,
        )?;
        extract(&package, &stage.join("new"), &commit)?;
        fs::remove_file(package)?;
        Ok(Some((stage.clone(), commit.clone())))
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(&stage);
    }
    result
}
fn download_verified(reader: &mut impl Read, size: u64, digest: &str, path: &Path) -> Result<()> {
    ensure!(
        size > 0 && size <= MAX_PACKAGE && hex(digest, 64),
        "Invalid download metadata"
    );
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    let mut hash = Sha256::new();
    let mut total = 0;
    let mut buffer = [0; 65536];
    loop {
        let n = reader.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        total += n as u64;
        ensure!(total <= size, "Download exceeds advertised size");
        file.write_all(&buffer[..n])?;
        hash.update(&buffer[..n]);
    }
    ensure!(
        total == size && format!("{:x}", hash.finalize()) == digest,
        "Downloaded package hash/size mismatch"
    );
    file.sync_all()?;
    Ok(())
}
/// Extract only `server/` entries. `server-data/`, start scripts and docs in
/// the same ZIP belong to the host after the first install.
fn extract(package: &Path, destination: &Path, commit: &str) -> Result<()> {
    let mut archive = zip::ZipArchive::new(File::open(package)?)?;
    ensure!(archive.len() <= 4096, "Too many ZIP entries");
    let mut selected = Vec::new();
    let mut names = BTreeSet::new();
    let mut total = 0;
    for i in 0..archive.len() {
        let file = archive.by_index(i)?;
        let Some(name) = file.name().strip_prefix(PREFIX) else {
            continue;
        };
        if name.is_empty() || name.ends_with('/') {
            continue;
        }
        ensure!(
            allowed(name) && names.insert(name.to_ascii_lowercase()),
            "Unexpected or duplicate ZIP path"
        );
        ensure!(
            file.unix_mode()
                .is_none_or(|m| m & 0o170000 == 0o100000 || m & 0o170000 == 0),
            "ZIP links/special files are not supported"
        );
        ensure!(file.size() <= MAX_PACKAGE, "ZIP entry too large");
        total += file.size();
        ensure!(total <= MAX_CONTENT, "ZIP expands beyond limit");
        selected.push((i, name.to_owned()));
    }
    ensure!(names.contains(BUILD), "ZIP has no server build manifest");
    fs::create_dir(destination)?;
    for (i, name) in selected {
        let mut input = archive.by_index(i)?;
        let path = plain(destination, &name)?;
        fs::create_dir_all(path.parent().context("Missing ZIP parent")?)?;
        let mut output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)?;
        let expected = input.size();
        let written = std::io::copy(&mut input.by_ref().take(expected + 1), &mut output)?;
        ensure!(written == expected, "Invalid ZIP length");
        output.sync_all()?;
    }
    let build = verify_tree(destination)?;
    ensure!(
        build.commit == commit && names.len() == build.files.len() + 1,
        "ZIP manifest does not match release"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for name in ["sa-server", "sa-relay"] {
            fs::set_permissions(
                destination.join(binary(name)),
                fs::Permissions::from_mode(0o755),
            )?;
        }
    }
    Ok(())
}
/// Running executables can be renamed on Windows and Linux, so the current
/// files move to a backup folder and the verified files move into place.
fn replace(root: &Path, stage: &Path) -> Result<()> {
    let build = verify_tree(&stage.join("new"))?;
    let mut paths: Vec<String> = build.files.iter().map(|e| e.path.clone()).collect();
    paths.push(BUILD.into());
    for name in &paths {
        let path = plain(root, name)?;
        ensure!(
            !path.exists() || path.is_file(),
            "Update destination is not a file"
        );
    }
    let backup_root = stage.join("backup");
    fs::create_dir(&backup_root)?;
    let mut changed: Vec<(String, bool)> = Vec::new();
    let result = (|| {
        for name in &paths {
            let target = plain(root, name)?;
            let source = plain(&stage.join("new"), name)?;
            let backup = plain(&backup_root, name)?;
            fs::create_dir_all(backup.parent().context("Missing backup parent")?)?;
            fs::create_dir_all(target.parent().context("Missing update parent")?)?;
            let existed = target.exists();
            if existed {
                fs::rename(&target, &backup)?;
            }
            changed.push((name.clone(), existed));
            fs::rename(source, target)?;
        }
        verify_tree(root)?;
        Ok(())
    })();
    if let Err(error) = result {
        for (name, existed) in changed.iter().rev() {
            let target = plain(root, name)?;
            let _ = fs::remove_file(&target);
            if *existed {
                fs::rename(plain(&backup_root, name)?, target)
                    .context("Server update rollback failed; see .sare-update backups")?;
            }
        }
        return Err(error);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release(tag: &str, name: &str) -> Vec<u8> {
        serde_json::json!({"tag_name": tag, "draft": false, "prerelease": false, "assets": [{
            "name": name, "size": 10, "digest": format!("sha256:{}", "b".repeat(64)),
            "browser_download_url": format!("https://github.com/{REPO}/releases/download/{tag}/{name}")
        }]})
        .to_string()
        .into_bytes()
    }

    #[test]
    fn selects_only_this_platforms_server_package() {
        let commit = "a".repeat(40);
        let tag = format!("client-{commit}");
        let name = format!("SARE-server-{}.zip", platform());
        let (found, asset, _) = select_asset(&release(&tag, &name), &"c".repeat(40))
            .unwrap()
            .unwrap();
        assert_eq!(
            (found.as_str(), asset.name.as_str()),
            (commit.as_str(), name.as_str())
        );
        assert!(select_asset(&release(&tag, &name), &commit)
            .unwrap()
            .is_none());
        let client = format!("SARE-{}-test.zip", platform());
        assert!(select_asset(&release(&tag, &client), &"c".repeat(40))
            .unwrap()
            .is_none());
    }

    #[test]
    fn update_paths_exclude_server_data_and_client_binaries() {
        assert!(allowed(&binary("sa-server")) && allowed("licenses/x-1.0/LICENSE"));
        for name in [
            "server.cfg",
            "../sa-server",
            "sa-launcher.exe",
            "sa-runtime",
            "resources/x.lua",
            "NUL.txt",
        ] {
            assert!(!allowed(name), "{name}");
        }
    }

    #[test]
    fn replace_installs_verified_files_and_keeps_backups() {
        let dir = std::env::temp_dir().join(format!("sare-server-update-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let root = dir.join("server");
        let stage = dir.join("stage");
        fs::create_dir_all(stage.join("new")).unwrap();
        fs::create_dir_all(&root).unwrap();
        let manifest = |dir: &Path, commit: char, content: &str| {
            let mut files = Vec::new();
            for name in [binary("sa-server"), binary("sa-relay")] {
                fs::write(dir.join(&name), content).unwrap();
                files.push(serde_json::json!({"path": name, "size": content.len(),
                    "sha256": file_hash(&dir.join(&name)).unwrap()}));
            }
            let build = serde_json::json!({"schema": 1, "commit": commit.to_string().repeat(40),
                "platform": platform(), "files": files});
            fs::write(dir.join(BUILD), build.to_string()).unwrap();
        };
        manifest(&root, 'a', "old");
        manifest(&stage.join("new"), 'b', "new build");
        replace(&root, &stage).unwrap();
        assert_eq!(read_build(&root).unwrap().commit, "b".repeat(40));
        assert_eq!(
            fs::read_to_string(root.join(binary("sa-server"))).unwrap(),
            "new build"
        );
        assert_eq!(
            fs::read_to_string(stage.join("backup").join(binary("sa-server"))).unwrap(),
            "old"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn extract_takes_only_server_folder_and_checks_manifest() {
        use std::io::Write as _;
        let dir = std::env::temp_dir().join(format!("sare-server-extract-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let commit = "d".repeat(40);
        let write_zip = |path: &Path, tamper: bool| {
            let mut zip = zip::ZipWriter::new(File::create(path).unwrap());
            let options = zip::write::SimpleFileOptions::default();
            let mut files = Vec::new();
            for name in [binary("sa-server"), binary("sa-relay")] {
                zip.start_file(format!("server/{name}"), options).unwrap();
                zip.write_all(b"binary").unwrap();
                files.push(serde_json::json!({"path": name, "size": 6,
                    "sha256": format!("{:x}", Sha256::digest(if tamper { &b"other!"[..] } else { &b"binary"[..] }))}));
            }
            zip.start_file("server-data/server.cfg", options).unwrap();
            zip.write_all(b"sv_hostname host").unwrap();
            zip.start_file("server/sare-build.json", options).unwrap();
            let build = serde_json::json!({"schema": 1, "commit": commit, "platform": platform(), "files": files});
            zip.write_all(build.to_string().as_bytes()).unwrap();
            zip.finish().unwrap();
        };
        write_zip(&dir.join("good.zip"), false);
        extract(&dir.join("good.zip"), &dir.join("good"), &commit).unwrap();
        assert!(dir.join("good").join(binary("sa-server")).is_file());
        assert!(!dir.join("good/server-data").exists() && !dir.join("good/server.cfg").exists());
        assert!(extract(&dir.join("good.zip"), &dir.join("other"), &"e".repeat(40)).is_err());
        write_zip(&dir.join("bad.zip"), true);
        assert!(extract(&dir.join("bad.zip"), &dir.join("bad"), &commit).is_err());
        let _ = fs::remove_dir_all(&dir);
    }
}
