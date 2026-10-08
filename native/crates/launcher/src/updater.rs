//! HTTPS GitHub release updates. Never runs a build, shell script or downloaded installer.
use anyhow::{bail, ensure, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    process::Command,
    sync::mpsc,
    time::{Duration, Instant},
};

const REPO: &str = "LordM8YT/GTA-San-Andreas-Rust-Edition";
const BUILD: &str = "sare-build.json";
const MAX_PACKAGE: u64 = 256 * 1024 * 1024;
const MAX_CONTENT: u64 = 512 * 1024 * 1024;
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Build {
    pub schema: u32,
    pub commit: String,
    pub platform: String,
    pub files: Vec<Entry>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub path: String,
    pub size: u64,
    pub sha256: String,
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
    Status(String),
    Done(Option<PathBuf>),
    Failed(String),
}
pub struct Updates {
    pub status: String,
    root: Option<PathBuf>,
    // Shared for the entire GUI lifetime. The helper obtains exclusive access after exit.
    lease: Option<File>,
    rx: Option<mpsc::Receiver<Event>>,
    staged: Option<PathBuf>,
    recovery: Option<PathBuf>,
    pub last_result: String,
    next: Instant,
    pub restarting: bool,
    pub recovery_blocked: bool,
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
    !name.is_empty()
        && name.len() < 240
        && name.split('/').all(|p| {
            !p.is_empty()
                && p != "."
                && p != ".."
                && !matches!(
                    p.split('.')
                        .next()
                        .unwrap_or("")
                        .to_ascii_uppercase()
                        .as_str(),
                    "CON"
                        | "PRN"
                        | "AUX"
                        | "NUL"
                        | "COM1"
                        | "COM2"
                        | "COM3"
                        | "COM4"
                        | "COM5"
                        | "COM6"
                        | "COM7"
                        | "COM8"
                        | "COM9"
                        | "LPT1"
                        | "LPT2"
                        | "LPT3"
                        | "LPT4"
                        | "LPT5"
                        | "LPT6"
                        | "LPT7"
                        | "LPT8"
                        | "LPT9"
                )
                && !p.ends_with(['.', ' '])
                && p.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._-+ ".contains(&b))
        })
}
fn allowed(name: &str) -> bool {
    safe_path(name)
        && (name == BUILD
            || ["sa-launcher", "sa-runtime", "sa-server", "sa-relay"]
                .iter()
                .any(|n| name == binary(n))
            || [
                "LICENSE",
                "START-HERE.md",
                "Cargo.lock",
                "THIRD-PARTY-NOTICES.txt",
            ]
            .contains(&name)
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
fn read_build(root: &Path) -> Result<Build> {
    let path = plain(root, BUILD)?;
    let mut data = Vec::new();
    File::open(path)?
        .take(1024 * 1024 + 1)
        .read_to_end(&mut data)?;
    ensure!(data.len() <= 1024 * 1024, "Package manifest too large");
    let build: Build = serde_json::from_slice(&data)?;
    validate_build(&build)?;
    Ok(build)
}
fn validate_build(build: &Build) -> Result<()> {
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
            file.size <= 128 * 1024 * 1024 && hex(&file.sha256, 64),
            "Invalid package entry size/hash"
        );
        total += file.size;
        ensure!(total <= MAX_CONTENT, "Package too large");
    }
    for name in ["sa-launcher", "sa-runtime", "sa-server", "sa-relay"] {
        ensure!(names.contains(&binary(name)), "Incomplete client package");
    }
    for name in [
        "LICENSE",
        "START-HERE.md",
        "Cargo.lock",
        "THIRD-PARTY-NOTICES.txt",
    ] {
        ensure!(
            names.contains(&name.to_ascii_lowercase()),
            "Missing package notices"
        );
    }
    Ok(())
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
        let mut hasher = Sha256::new();
        std::io::copy(&mut File::open(path)?, &mut hasher)?;
        ensure!(
            format!("{:x}", hasher.finalize()) == entry.sha256,
            "Update file hash mismatch: {}",
            entry.path
        );
    }
    Ok(build)
}
fn install_root() -> Result<PathBuf> {
    ensure!(
        cfg!(target_arch = "x86_64") && (cfg!(windows) || cfg!(target_os = "linux")),
        "Updates support x64 Windows/Linux packages"
    );
    let exe = std::env::current_exe()?.canonicalize()?;
    ensure!(
        exe.file_name()
            .is_some_and(|n| n == binary("sa-launcher").as_str()),
        "Not a packaged launcher"
    );
    let root = exe.parent().context("Missing client folder")?.to_path_buf();
    ensure!(
        !root.join(".git").exists()
            && !root.join("gta_sa.exe").exists()
            && !root.join("models/gta3.img").exists(),
        "Use a separate SARE client folder"
    );
    if !root.join(".sare-update/pending.json").exists() {
        read_build(&root)?;
    }
    Ok(root)
}
fn update_dir(root: &Path) -> Result<PathBuf> {
    let path = root.join(".sare-update");
    ensure!(!link(&path), "Update folder is a link");
    fs::create_dir_all(&path)?;
    Ok(path)
}
fn lock(root: &Path) -> Result<File> {
    let path = update_dir(root)?.join("client.lock");
    ensure!(!link(&path), "Update lock is a link");
    Ok(OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path)?)
}
impl Updates {
    pub fn new(detect: bool) -> Self {
        let root = detect.then(install_root).and_then(Result::ok);
        let lease = root
            .as_ref()
            .and_then(|p| lock(p).ok())
            .and_then(|f| f.try_lock_shared().ok().map(|()| f));
        let usable = root.is_some() && lease.is_some();
        let recovery_result = root.as_ref().map(|root| pending(root)).transpose();
        let recovery_blocked = recovery_result.is_err();
        let recovery = recovery_result.ok().flatten().flatten();
        let last_result = root
            .as_ref()
            .and_then(|root| {
                let path = root.join(".sare-update/last-result.txt");
                (!link(&path))
                    .then(|| fs::read_to_string(path).ok())
                    .flatten()
            })
            .unwrap_or_default();
        Self {status:if recovery_blocked {"Interrupted update journal is invalid. Extract a fresh client package to recover."}else if recovery.is_some(){"Recovering interrupted client update..."}else if usable {"Updates ready to check."}else{"Automatic updates require an extracted client package in a writable folder. Source builds use Git/Cargo."}.into(),root:if usable && !recovery_blocked {root}else{None},lease,rx:None,staged:None,recovery,last_result,next:Instant::now()+Duration::from_secs(3),restarting:false,recovery_blocked}
    }
    pub fn check(&mut self) {
        if self.rx.is_some() || self.staged.is_some() || self.restarting {
            return;
        }
        let Some(root) = self.root.clone() else {
            return;
        };
        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);
        self.status = "Checking GitHub for a tested client update...".into();
        self.next = Instant::now() + Duration::from_secs(1800);
        std::thread::spawn(move || {
            let result = prepare(&root, &tx);
            let _ = tx.send(match result {
                Ok(stage) => Event::Done(stage),
                Err(e) => Event::Failed(format!(
                    "Update unavailable: {e:#}. Your current client is unchanged."
                )),
            });
        });
    }
    pub fn poll(&mut self, automatic: bool, game_active: bool, ctx: &eframe::egui::Context) {
        if let Some(stage) = self.recovery.take() {
            match run_worker(&stage, true) {
                Ok(()) => {
                    self.restarting = true;
                    ctx.send_viewport_cmd(eframe::egui::ViewportCommand::Close);
                }
                Err(error) => {
                    self.status = format!("Interrupted update requires recovery: {error:#}")
                }
            }
            return;
        }
        let ready_before = self.staged.is_some();
        if automatic && Instant::now() >= self.next && self.rx.is_none() && self.root.is_some() {
            self.check();
        }
        if let Some(rx) = &self.rx {
            match rx.try_recv() {
                Ok(Event::Status(s)) => self.status = s,
                Ok(Event::Done(stage)) => {
                    self.rx = None;
                    self.staged = stage;
                    self.status = if self.staged.is_some() {
                        "Update verified. Waiting for the game to close..."
                    } else {
                        "Client is up to date (latest tested GitHub release)."
                    }
                    .into();
                }
                Ok(Event::Failed(s)) => {
                    self.rx = None;
                    self.status = s;
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.rx = None;
                    self.status = "Update worker stopped. Current client is unchanged.".into();
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        if ready_before && !game_active && automatic && !self.restarting {
            if let Some(stage) = self.staged.take() {
                match spawn_helper(&stage) {
                    Ok(()) => {
                        self.restarting = true;
                        self.status = "Installing verified update and restarting...".into();
                        ctx.send_viewport_cmd(eframe::egui::ViewportCommand::Close);
                    }
                    Err(e) => {
                        self.status =
                            format!("Could not start update: {e:#}. Current client is unchanged.")
                    }
                }
            }
        }
    }
    pub fn manual_install(&mut self, game_active: bool, ctx: &eframe::egui::Context) {
        if self.staged.is_some() {
            self.poll(true, game_active, ctx);
        } else {
            self.check();
        }
    }
    pub fn busy(&self) -> bool {
        self.rx.is_some()
    }
    pub fn available(&self) -> bool {
        self.root.is_some()
    }
    pub fn has_staged(&self) -> bool {
        self.staged.is_some()
    }
}

impl Drop for Updates {
    fn drop(&mut self) {
        self.lease.take();
    }
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
    let name = format!("SARE-{}-test.zip", platform());
    let mut assets = release.assets.into_iter().filter(|a| a.name == name);
    let asset = assets
        .next()
        .context("Release does not contain this platform")?;
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
fn prepare(root: &Path, tx: &mpsc::Sender<Event>) -> Result<Option<PathBuf>> {
    let current = read_build(root)?;
    let http = agent();
    let mut response = match http
        .get(&format!(
            "https://api.github.com/repos/{REPO}/releases/latest"
        ))
        .header("User-Agent", "SARE-launcher")
        .header("Accept", "application/vnd.github+json")
        .call()
    {
        Ok(r) => r,
        Err(ureq::Error::StatusCode(404)) => {
            bail!("No client release has been published yet");
        }
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
    let stage = update_dir(root)?.join(format!(
        "{commit}-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos()
    ));
    fs::create_dir(&stage)?;
    let result = (|| {
        let _ = tx.send(Event::Status("Downloading tested client update...".into()));
        let mut response = http
            .get(&asset.browser_download_url)
            .header("User-Agent", "SARE-launcher")
            .call()?;
        let package = stage.join("client.zip");
        let mut reader = response.body_mut().as_reader();
        download_verified(&mut reader, asset.size, &digest, &package)?;
        let _ = tx.send(Event::Status("Verifying client files...".into()));
        extract(&package, &stage.join("new"), &commit)?;
        fs::remove_file(package)?;
        Ok(Some(stage.clone()))
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
fn extract(package: &Path, destination: &Path, commit: &str) -> Result<()> {
    let mut archive = zip::ZipArchive::new(File::open(package)?)?;
    ensure!(archive.len() <= 2049, "Too many ZIP entries");
    // Validate the entire directory before writing even one entry.
    let mut names = BTreeSet::new();
    let mut total = 0;
    for i in 0..archive.len() {
        let file = archive.by_index(i)?;
        ensure!(
            allowed(file.name()) && names.insert(file.name().to_ascii_lowercase()),
            "Unexpected or duplicate ZIP path"
        );
        ensure!(
            file.unix_mode()
                .is_none_or(|m| m & 0o170000 == 0o100000 || m & 0o170000 == 0),
            "ZIP links/special files are not supported"
        );
        ensure!(file.size() <= 128 * 1024 * 1024, "ZIP entry too large");
        total += file.size();
        ensure!(total <= MAX_CONTENT, "ZIP expands beyond limit");
    }
    ensure!(names.contains(BUILD), "ZIP has no build manifest");
    fs::create_dir(destination)?;
    for i in 0..archive.len() {
        let mut input = archive.by_index(i)?;
        let path = plain(destination, input.name())?;
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
        for name in ["sa-launcher", "sa-runtime", "sa-server", "sa-relay"] {
            fs::set_permissions(
                destination.join(binary(name)),
                fs::Permissions::from_mode(0o755),
            )?;
        }
    }
    Ok(())
}
fn spawn_helper(stage: &Path) -> Result<()> {
    let root = stage
        .parent()
        .and_then(Path::parent)
        .context("Missing client folder")?;
    ensure!(
        stage.parent() == Some(update_dir(root)?.as_path()) && !link(stage),
        "Invalid update staging folder"
    );
    verify_tree(&stage.join("new"))?;
    let helper = stage.join(binary("update-worker"));
    // Use the currently trusted updater, not executable code from the download.
    fs::copy(std::env::current_exe()?, &helper)?;
    run_worker(stage, false)
}
fn run_worker(stage: &Path, recover: bool) -> Result<()> {
    let root = stage
        .parent()
        .and_then(Path::parent)
        .context("Missing update root")?;
    ensure!(
        stage.parent() == Some(update_dir(root)?.as_path()) && !link(stage),
        "Invalid update staging folder"
    );
    let helper = stage.join(binary("update-worker"));
    ensure!(
        !link(&helper) && helper.is_file(),
        "Missing update recovery worker"
    );
    let mut command = Command::new(helper);
    command
        .arg(if recover {
            "--recover-update"
        } else {
            "--apply-update"
        })
        .arg(stage)
        .current_dir(root);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    command.spawn()?;
    Ok(())
}
#[derive(Serialize, Deserialize)]
struct Change {
    name: String,
    previous: bool,
}
fn pending(root: &Path) -> Result<Option<PathBuf>> {
    let path = update_dir(root)?.join("pending.json");
    ensure!(!link(&path), "Update journal is a link");
    if !path.exists() {
        return Ok(None);
    }
    let name: String = sa_client::read_json(&path)?;
    ensure!(
        safe_path(&name) && !name.contains('/'),
        "Invalid recovery stage"
    );
    let stage = update_dir(root)?.join(name);
    ensure!(!link(&stage) && stage.is_dir(), "Missing recovery stage");
    Ok(Some(stage))
}
fn recover(root: &Path, stage: &Path) -> Result<()> {
    ensure!(
        pending(root)?.as_deref() == Some(stage),
        "Recovery journal does not match worker"
    );
    let journal = stage.join("rollback.json");
    ensure!(!link(&journal), "Rollback journal is a link");
    let changes: Vec<Change> = sa_client::read_json(&journal)?;
    ensure!(changes.len() <= 2049, "Recovery journal too large");
    restore(root, stage, &changes)?;
    verify_tree(root)?;
    fs::remove_file(root.join(".sare-update/pending.json"))?;
    Ok(())
}
fn restore(root: &Path, stage: &Path, changes: &[Change]) -> Result<()> {
    for change in changes.iter().rev() {
        let target = plain(root, &change.name)?;
        let backup = plain(&stage.join("backup"), &change.name)?;
        if change.previous {
            if backup.exists() {
                if target.exists() && file_hash(&target)? == file_hash(&backup)? {
                    continue;
                }
                fs::rename(backup, target)?;
            }
        } else if target.exists() {
            fs::remove_file(target)?;
        }
    }
    Ok(())
}
fn replace(root: &Path, stage: &Path) -> Result<()> {
    ensure!(
        pending(root)?.is_none(),
        "An interrupted update must be recovered first"
    );
    let build = verify_tree(&stage.join("new"))?;
    let mut paths: Vec<_> = build.files.iter().map(|e| e.path.clone()).collect();
    paths.push(BUILD.into());
    // Validate all existing destinations before changing the installed client.
    for name in &paths {
        let path = plain(root, name)?;
        ensure!(
            !path.exists() || path.is_file(),
            "Update destination is not a file"
        );
    }
    let mut changes = Vec::new();
    fs::create_dir(stage.join("backup"))?;
    sa_client::atomic_json(&stage.join("rollback.json"), &changes)?;
    sa_client::atomic_json(
        &root.join(".sare-update/pending.json"),
        &stage
            .file_name()
            .context("Missing stage name")?
            .to_string_lossy(),
    )?;
    let result = (|| {
        for name in paths {
            let target = plain(root, &name)?;
            let source = plain(&stage.join("new"), &name)?;
            let backup = plain(&stage.join("backup"), &name)?;
            fs::create_dir_all(backup.parent().context("Missing backup parent")?)?;
            fs::create_dir_all(target.parent().context("Missing update parent")?)?;
            changes.push(Change {
                name,
                previous: target.exists(),
            });
            sa_client::atomic_json(&stage.join("rollback.json"), &changes)?;
            if target.exists() {
                let temporary = backup.with_extension("copying");
                fs::copy(&target, &temporary)?;
                OpenOptions::new()
                    .write(true)
                    .open(&temporary)?
                    .sync_all()?;
                fs::rename(temporary, &backup)?;
            }
            fs::rename(source, target)?;
        }
        verify_tree(root)?;
        Ok(())
    })();
    if let Err(error) = result {
        restore(root, stage, &changes)
            .context("Update failed; automatic rollback needs attention (see backup folder)")?;
        fs::remove_file(root.join(".sare-update/pending.json"))?;
        return Err(error);
    }
    fs::remove_file(root.join(".sare-update/pending.json"))?;
    Ok(())
}
/// Called before GUI initialization. The copied worker can replace a closed Windows exe.
pub fn helper_mode() -> bool {
    let args: Vec<_> = std::env::args_os().collect();
    if args
        .get(1)
        .is_none_or(|a| a != "--apply-update" && a != "--recover-update")
    {
        return false;
    }
    let result = (|| -> Result<()> {
        ensure!(args.len() == 3, "Invalid update worker arguments");
        let stage = PathBuf::from(&args[2]).canonicalize()?;
        let root = stage
            .parent()
            .and_then(Path::parent)
            .context("Invalid update root")?
            .to_path_buf();
        ensure!(
            stage.parent() == Some(root.join(".sare-update").as_path())
                && !link(&root.join(".sare-update")),
            "Invalid update stage"
        );
        ensure!(
            std::env::current_exe()?.canonicalize()? == stage.join(binary("update-worker")),
            "Unexpected update worker location"
        );
        ensure!(
            !root.join("gta_sa.exe").exists()
                && !root.join("models/gta3.img").exists()
                && !root.join(".git").exists(),
            "Refusing to update an original game/source folder"
        );
        let lease = lock(&root)?;
        let start = Instant::now();
        while lease.try_lock().is_err() {
            ensure!(
                start.elapsed() < Duration::from_secs(45),
                "Another launcher is still open. Close it and retry the update."
            );
            std::thread::sleep(Duration::from_millis(200));
        }
        let result = (|| {
            let _game = sa_client::launch::RuntimeGuard::acquire()?;
            if args[1] == "--recover-update" {
                recover(&root, &stage)
            } else {
                replace(&root, &stage)
            }
        })();
        let status = match &result {
            Ok(()) => "Client update installed.".into(),
            Err(e) => {
                format!("Update failed: {e:#}. See .sare-update backups if recovery is needed.")
            }
        };
        fs::write(root.join(".sare-update/last-result.txt"), status)?;
        drop(lease);
        Command::new(root.join(binary("sa-launcher")))
            .current_dir(&root)
            .spawn()?;
        result
    })();
    if let Err(error) = result {
        eprintln!("Update worker: {error:#}");
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "sare-updater-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            fs::create_dir(&root).unwrap();
            Self(root)
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn fixture_build(root: &Path, byte: u8) -> Build {
        fs::create_dir_all(root).unwrap();
        let files: Vec<_> = ["sa-launcher", "sa-runtime", "sa-server", "sa-relay"]
            .into_iter()
            .map(binary)
            .chain(
                [
                    "LICENSE",
                    "START-HERE.md",
                    "Cargo.lock",
                    "THIRD-PARTY-NOTICES.txt",
                    "licenses/test+metadata/LICENSE",
                ]
                .map(String::from),
            )
            .map(|path| {
                let data = vec![byte; 32];
                let target = root.join(&path);
                fs::create_dir_all(target.parent().unwrap()).unwrap();
                fs::write(target, &data).unwrap();
                Entry {
                    path,
                    size: data.len() as u64,
                    sha256: format!("{:x}", Sha256::digest(&data)),
                }
            })
            .collect();
        let build = Build {
            schema: 1,
            commit: format!("{byte:040x}"),
            platform: platform().into(),
            files,
        };
        fs::write(root.join(BUILD), serde_json::to_vec(&build).unwrap()).unwrap();
        build
    }
    fn zip_fixture(path: &Path, root: &Path, extra: Option<&str>) {
        let build = read_build(root).unwrap();
        let mut zip = zip::ZipWriter::new(File::create(path).unwrap());
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        for name in build.files.iter().map(|e| e.path.as_str()).chain([BUILD]) {
            zip.start_file(name, options).unwrap();
            zip.write_all(&fs::read(root.join(name)).unwrap()).unwrap();
        }
        if let Some(name) = extra {
            zip.start_file(name, options).unwrap();
            zip.write_all(b"unexpected").unwrap();
        }
        zip.finish().unwrap();
    }
    #[test]
    fn downloads_require_exact_length_and_digest_before_installation() {
        let fixture = Fixture::new();
        let data = b"complete verified client";
        let hash = format!("{:x}", Sha256::digest(data));
        download_verified(
            &mut &data[..],
            data.len() as u64,
            &hash,
            &fixture.0.join("valid"),
        )
        .unwrap();
        assert!(download_verified(
            &mut &data[..data.len() - 1],
            data.len() as u64,
            &hash,
            &fixture.0.join("short")
        )
        .is_err());
        assert!(download_verified(
            &mut &data[..],
            data.len() as u64 - 1,
            &hash,
            &fixture.0.join("long")
        )
        .is_err());
        assert!(download_verified(
            &mut &data[..],
            data.len() as u64,
            &"0".repeat(64),
            &fixture.0.join("corrupt")
        )
        .is_err());
        assert!(!fixture.0.join("sa-launcher.exe").exists());
    }
    #[test]
    fn release_requires_expected_repository_digest_platform_and_full_commit() {
        let commit = "a".repeat(40);
        let name = format!("SARE-{}-test.zip", platform());
        let mut release = serde_json::json!({"tag_name":format!("client-{commit}"),"draft":false,"prerelease":false,"assets":[{"name":name,"size":123,"digest":format!("sha256:{}","b".repeat(64)),"browser_download_url":format!("https://github.com/{REPO}/releases/download/client-{commit}/{name}")}]});
        assert!(
            select_asset(&serde_json::to_vec(&release).unwrap(), &commit)
                .unwrap()
                .is_none()
        );
        assert!(
            select_asset(&serde_json::to_vec(&release).unwrap(), &"c".repeat(40))
                .unwrap()
                .is_some()
        );
        release["assets"][0]["browser_download_url"] = "https://evil.example/client.zip".into();
        assert!(select_asset(&serde_json::to_vec(&release).unwrap(), "old").is_err());
        release["draft"] = true.into();
        assert!(select_asset(&serde_json::to_vec(&release).unwrap(), "old").is_err());
    }
    #[test]
    fn zip_rejects_escape_unknown_paths_duplicates_and_changed_files() {
        let fixture = Fixture::new();
        let source = fixture.0.join("source");
        let build = fixture_build(&source, 1);
        let zip = fixture.0.join("package.zip");
        for (i, name) in [
            "../outside",
            "settings.json",
            "licenses/../../outside",
            "licenses/test+metadata/license",
            "licenses/test/../LICENSE",
            "licenses/test/CON",
        ]
        .iter()
        .enumerate()
        {
            zip_fixture(&zip, &source, Some(name));
            let dest = fixture.0.join(format!("bad-{i}"));
            assert!(
                extract(&zip, &dest, &build.commit).is_err(),
                "accepted {name}"
            );
            assert!(!dest.exists(), "wrote entries before validating ZIP paths");
        }
        zip_fixture(&zip, &source, None);
        let good = fixture.0.join("good");
        extract(&zip, &good, &build.commit).unwrap();
        assert_eq!(verify_tree(&good).unwrap().commit, build.commit);
        fs::write(source.join(binary("sa-runtime")), b"tampered").unwrap();
        zip_fixture(&zip, &source, None);
        assert!(extract(&zip, &fixture.0.join("tampered"), &build.commit).is_err());
    }
    #[test]
    fn update_changes_only_package_files_and_keeps_backup() {
        let fixture = Fixture::new();
        let root = fixture.0.join("client");
        fixture_build(&root, 1);
        fs::write(root.join("settings.json"), b"my settings").unwrap();
        fs::create_dir(root.join("mods")).unwrap();
        fs::write(root.join("mods/custom.json"), b"my mod").unwrap();
        let stage = update_dir(&root).unwrap().join("test-stage");
        fs::create_dir(&stage).unwrap();
        let build = fixture_build(&stage.join("new"), 2);
        replace(&root, &stage).unwrap();
        assert_eq!(verify_tree(&root).unwrap().commit, build.commit);
        assert_eq!(
            fs::read(root.join("settings.json")).unwrap(),
            b"my settings"
        );
        assert_eq!(fs::read(root.join("mods/custom.json")).unwrap(), b"my mod");
        assert_eq!(
            fs::read(stage.join("backup").join(binary("sa-runtime"))).unwrap(),
            vec![1; 32]
        );
        assert!(pending(&root).unwrap().is_none());
    }
    #[test]
    fn interrupted_update_restores_previous_files_and_clears_journal() {
        let fixture = Fixture::new();
        let root = fixture.0.join("client");
        let old = fixture_build(&root, 1);
        let stage = update_dir(&root).unwrap().join("interrupted");
        fs::create_dir(&stage).unwrap();
        fixture_build(&stage.join("new"), 2);
        fs::create_dir(stage.join("backup")).unwrap();
        let name = binary("sa-launcher");
        fs::rename(root.join(&name), stage.join("backup").join(&name)).unwrap();
        fs::rename(stage.join("new").join(&name), root.join(&name)).unwrap();
        sa_client::atomic_json(
            &stage.join("rollback.json"),
            &vec![Change {
                name,
                previous: true,
            }],
        )
        .unwrap();
        sa_client::atomic_json(&root.join(".sare-update/pending.json"), &"interrupted").unwrap();
        recover(&root, &stage).unwrap();
        assert_eq!(verify_tree(&root).unwrap().commit, old.commit);
        assert!(pending(&root).unwrap().is_none());
    }
    #[cfg(windows)]
    #[test]
    fn locked_windows_binary_rolls_back_already_replaced_launcher() {
        use std::os::windows::fs::OpenOptionsExt;
        let fixture = Fixture::new();
        let root = fixture.0.join("client");
        let old = fixture_build(&root, 1);
        let stage = update_dir(&root).unwrap().join("locked");
        fs::create_dir(&stage).unwrap();
        fixture_build(&stage.join("new"), 2);
        let locked = OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(root.join(binary("sa-runtime")))
            .unwrap();
        assert!(replace(&root, &stage).is_err());
        drop(locked);
        assert_eq!(verify_tree(&root).unwrap().commit, old.commit);
        assert!(pending(&root).unwrap().is_none());
    }
    #[test]
    fn launcher_lease_prevents_replacement_until_all_launchers_close() {
        let fixture = Fixture::new();
        let shared = lock(&fixture.0).unwrap();
        shared.try_lock_shared().unwrap();
        let exclusive = lock(&fixture.0).unwrap();
        assert!(exclusive.try_lock().is_err());
        drop(shared);
        exclusive.try_lock().unwrap();
    }
}
