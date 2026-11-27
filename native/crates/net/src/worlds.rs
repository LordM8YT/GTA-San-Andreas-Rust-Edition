//! Publish to Discover: creators publish worlds from SARE Studio through the
//! SARE directory, and SARE Client lists them under Discover.
//!
//! - **No accounts.** A world belongs to the SARE ID that published it first;
//!   only that ID (or a directory admin) can update or unpublish it. The
//!   catalog stores a one-way hash of the key ([`owner_of`]), never the key
//!   or friend code, so a world cannot be tied to its maker's friend code.
//! - **The directory stores nothing itself.** Worlds, covers and the catalog
//!   live in a [`Store`]; the official directory uses a GitHub repository
//!   ([`GitHub`]): each world is a release with its ZIP and cover, and
//!   `worlds.json` on the default branch is the catalog. Players download
//!   from GitHub directly, so Discover works while the directory sleeps.
//! - **Checked before it is listed.** The request is signed, the ZIP's size
//!   and SHA-256 must match what was signed, and the ZIP must hold a world
//!   whose `content.json` has the same id, within Discover's limits.
//! - **Reports** go to the store (a GitHub issue) for maintainers to act on.
//!
//! The wire format is in docs/sare-directory.md.
use crate::directory::{friend_code, Signed};
use crate::identity::{hex, unhex};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    io::Read,
    path::Path,
    sync::Mutex,
};

const SIGN_DOMAIN: &str = "sare-worlds-v1";
const OWNER_DOMAIN: &str = "sare-worlds-owner-v1";
/// Signed requests more than this far from the directory's clock are refused.
const CLOCK_SLACK: u64 = 300;
/// Discover's limits; the client refuses bigger worlds.
pub const MAX_ZIP_BYTES: u64 = 256 * 1024 * 1024;
const MAX_UNPACKED_BYTES: u64 = 256 * 1024 * 1024;
const MAX_ENTRIES: usize = 1024;
const MAX_COVER_BYTES: u64 = 4 * 1024 * 1024;
/// The signed part of a request, before the ZIP.
pub const MAX_REQUEST: usize = 16 * 1024;
/// Worlds the catalog holds, and worlds one creator may have in it.
pub const MAX_WORLDS: usize = 500;
pub const MAX_PER_OWNER: usize = 25;
pub const MAX_TAGS: usize = 5;
/// One creator may publish or unpublish this often.
const PUBLISH_EVERY: u64 = 30;
/// Reports one player may send per hour.
const REPORTS_PER_HOUR: usize = 10;

/// What a creator signs to publish or update a world. The ZIP follows it.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Publish {
    /// SARE ID public key, hex.
    pub key: String,
    /// Unix seconds when sent.
    pub time: u64,
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub description: String,
    pub version: String,
    pub license: String,
    #[serde(default)]
    pub tags: Vec<String>,
    /// SHA-256 of the ZIP, hex.
    pub sha256: String,
    pub bytes: u64,
}
/// What a player signs to unpublish their world or report someone's.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Action {
    pub key: String,
    pub time: u64,
    pub action: ActionKind,
    pub id: String,
    /// Why, for a report.
    #[serde(default)]
    pub reason: String,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ActionKind {
    Unpublish,
    Report,
}

/// One world in `worlds.json`. Its first fields are the ones every SARE
/// Client reads (see `community/README.md`); unknown fields are kept.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Entry {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub description: String,
    pub version: String,
    pub license: String,
    pub download: String,
    pub sha256: String,
    pub bytes: u64,
    /// `https://` link to the world's 16:9 cover picture.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cover: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    /// [`owner_of`] the SARE ID that published it.
    #[serde(default)]
    pub owner: String,
    /// Unix seconds of the first publish and the latest update.
    #[serde(default)]
    pub published: u64,
    #[serde(default)]
    pub updated: u64,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Catalog {
    pub version: u32,
    pub worlds: Vec<Entry>,
}
impl Default for Catalog {
    fn default() -> Self {
        Self {
            version: 1,
            worlds: Vec::new(),
        }
    }
}

fn message(payload: &str) -> Vec<u8> {
    format!("{SIGN_DOMAIN}\n{payload}").into_bytes()
}
fn sign_value(key: &SigningKey, value: &impl Serialize) -> Signed {
    let payload = serde_json::to_string(value).expect("a request serializes");
    let signature = hex(&key.sign(&message(&payload)).to_bytes());
    Signed { payload, signature }
}
pub fn sign_publish(key: &SigningKey, publish: &Publish) -> Signed {
    sign_value(key, publish)
}
pub fn sign_action(key: &SigningKey, action: &Action) -> Signed {
    sign_value(key, action)
}
/// The catalog's owner tag of a SARE ID: not its key, not its friend code.
pub fn owner_of(key: &VerifyingKey) -> String {
    let digest = Sha256::digest([OWNER_DOMAIN.as_bytes(), b"\n", key.as_bytes()].concat());
    hex(&digest[..16])
}

/// Check a signed request and return it with its signer's key.
fn verify<T: for<'de> Deserialize<'de>>(
    signed: &Signed,
    key_of: impl Fn(&T) -> (&str, u64),
    now: u64,
) -> Result<(T, VerifyingKey), String> {
    if signed.payload.len() > MAX_REQUEST {
        return Err("Request too large".into());
    }
    let request: T = serde_json::from_str(&signed.payload).map_err(|_| "Invalid request")?;
    let (key, time) = key_of(&request);
    let key = unhex::<32>(key)
        .and_then(|k| VerifyingKey::from_bytes(&k).ok())
        .ok_or("Invalid SARE ID")?;
    let signature = unhex::<64>(&signed.signature).ok_or("Invalid signature")?;
    key.verify(
        &message(&signed.payload),
        &Signature::from_bytes(&signature),
    )
    .map_err(|_| "Signature check failed")?;
    if time.abs_diff(now) > CLOCK_SLACK {
        return Err("Your clock is off by more than five minutes".into());
    }
    Ok((request, key))
}

fn plain(text: &str, max: usize) -> bool {
    text.chars().count() <= max && !text.chars().any(char::is_control)
}
fn filled(text: &str, max: usize) -> bool {
    !text.trim().is_empty() && plain(text, max)
}
/// Tags as Discover shows them: lowercase letters, digits and `-`.
pub fn clean_tags(tags: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for tag in tags {
        let tag: String = tag
            .trim()
            .to_lowercase()
            .chars()
            .map(|c| if c == ' ' || c == '_' { '-' } else { c })
            .filter(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == '-')
            .take(20)
            .collect();
        let tag = tag.trim_matches('-').to_string();
        if !tag.is_empty() && !out.contains(&tag) && out.len() < MAX_TAGS {
            out.push(tag);
        }
    }
    out
}
fn check_publish(p: &Publish) -> Result<(), String> {
    if !crate::resources::valid_pack_id(&p.id) {
        return Err("The world id may only use a-z, 0-9, - and _".into());
    }
    if !filled(&p.name, 48) {
        return Err("The world needs a name of at most 48 characters".into());
    }
    if !plain(&p.author, 48) {
        return Err("The author name is longer than 48 characters".into());
    }
    if !plain(&p.description, 300) {
        return Err("The description is longer than 300 characters".into());
    }
    if !filled(&p.version, 24) {
        return Err("The world needs a version of at most 24 characters".into());
    }
    if !filled(&p.license, 64) {
        return Err("The world needs a licence".into());
    }
    if p.sha256.len() != 64 || !p.sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("Invalid SHA-256".into());
    }
    if p.bytes == 0 || p.bytes > MAX_ZIP_BYTES {
        return Err(format!(
            "The world ZIP is over Discover's {} MiB",
            MAX_ZIP_BYTES / (1024 * 1024)
        ));
    }
    Ok(())
}
/// A file name GitHub keeps as it is.
fn asset_name(id: &str, version: &str, extension: &str) -> String {
    let version: String = version
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect();
    format!("{id}-{version}.{extension}")
}

/// Where worlds, covers and the catalog are kept.
pub trait Store: Send + Sync {
    /// The catalog and an opaque version for [`Store::save_catalog`];
    /// `None` when there is no catalog yet.
    fn catalog(&self) -> Result<(Catalog, Option<String>), String>;
    /// Replace the catalog, if it is still at `version`.
    fn save_catalog(
        &self,
        catalog: &Catalog,
        version: Option<&str>,
        note: &str,
    ) -> Result<(), String>;
    /// Store a world's file and return its public `https://` link.
    fn upload(
        &self,
        id: &str,
        name: &str,
        file: &Path,
        content_type: &str,
    ) -> Result<String, String>;
    /// Delete a world's files except those named in `keep`.
    fn prune(&self, id: &str, keep: &[String]) -> Result<(), String>;
    /// File a report for the maintainers.
    fn report(&self, title: &str, body: &str) -> Result<(), String>;
}

/// The directory's publishing service.
pub struct Worlds {
    store: Box<dyn Store>,
    /// Friend codes that may unpublish any world.
    admins: HashSet<String>,
    /// One publish or unpublish at a time, so catalog writes never race.
    busy: Mutex<()>,
    last_change: Mutex<HashMap<String, u64>>,
    reports: Mutex<HashMap<String, Vec<(u64, String)>>>,
}

/// A checked publish request that may now send its ZIP.
pub struct Ticket {
    pub publish: Publish,
    owner: String,
    admin: bool,
}

impl Worlds {
    pub fn new(store: Box<dyn Store>, admins: impl IntoIterator<Item = String>) -> Self {
        Self {
            store,
            admins: admins
                .into_iter()
                .filter_map(|c| crate::directory::normalize_code(&c))
                .collect(),
            busy: Mutex::new(()),
            last_change: Mutex::default(),
            reports: Mutex::default(),
        }
    }
    /// Hold this while receiving and publishing a world; `None` while
    /// another publish runs.
    pub fn try_busy(&self) -> Option<std::sync::MutexGuard<'_, ()>> {
        self.busy.try_lock().ok()
    }
    fn throttle(&self, owner: &str, now: u64) -> Result<(), String> {
        let mut last = self.last_change.lock().unwrap();
        if last
            .get(owner)
            .is_some_and(|at| now.saturating_sub(*at) < PUBLISH_EVERY)
        {
            return Err("Wait half a minute before publishing again".into());
        }
        if last.len() > 10_000 {
            last.retain(|_, at| now.saturating_sub(*at) < PUBLISH_EVERY);
        }
        last.insert(owner.to_string(), now);
        Ok(())
    }
    /// Check a publish request before its ZIP is received.
    pub fn check(&self, signed: &Signed, now: u64) -> Result<Ticket, String> {
        let (publish, key) = verify::<Publish>(signed, |p| (&p.key, p.time), now)?;
        check_publish(&publish)?;
        let owner = owner_of(&key);
        let admin = self.admins.contains(&friend_code(&key));
        self.throttle(&owner, now)?;
        Ok(Ticket {
            publish,
            owner,
            admin,
        })
    }
    /// Publish the world in `zip` (already received) and return its entry.
    pub fn publish(&self, ticket: Ticket, zip: &Path, now: u64) -> Result<Entry, String> {
        let p = &ticket.publish;
        let (sha256, bytes) = hash_file(zip)?;
        if bytes != p.bytes || !sha256.eq_ignore_ascii_case(&p.sha256) {
            return Err("The ZIP does not match what was signed".into());
        }
        let cover = inspect_zip(zip, &p.id)?;
        let (mut catalog, version) = self.store.catalog()?;
        let existing = catalog.worlds.iter().position(|e| e.id == p.id);
        if let Some(i) = existing {
            let old = &catalog.worlds[i];
            if old.owner != ticket.owner && !ticket.admin {
                return Err(
                    "Another creator already published a world with this id; change the id".into(),
                );
            }
            if old.version == p.version {
                return Err(format!(
                    "Version {} is already published; raise the version to update",
                    p.version
                ));
            }
        } else {
            if catalog.worlds.len() >= MAX_WORLDS {
                return Err("Discover is full right now; try again later".into());
            }
            let mine = catalog
                .worlds
                .iter()
                .filter(|e| e.owner == ticket.owner)
                .count();
            if mine >= MAX_PER_OWNER {
                return Err(format!(
                    "You already have {MAX_PER_OWNER} worlds in Discover; unpublish one first"
                ));
            }
        }
        let zip_name = asset_name(&p.id, &p.version, "zip");
        let download = self
            .store
            .upload(&p.id, &zip_name, zip, "application/zip")?;
        let mut keep = vec![zip_name];
        let cover = match cover {
            Some(png) => {
                let name = asset_name(&p.id, &p.version, "png");
                let path = zip.with_extension("cover.png");
                std::fs::write(&path, png).map_err(|e| format!("Could not save the cover: {e}"))?;
                let url = self.store.upload(&p.id, &name, &path, "image/png");
                let _ = std::fs::remove_file(&path);
                keep.push(name);
                Some(url?)
            }
            None => None,
        };
        let mut entry = Entry {
            id: p.id.clone(),
            name: p.name.trim().to_string(),
            author: p.author.trim().to_string(),
            description: p.description.trim().to_string(),
            version: p.version.trim().to_string(),
            license: p.license.trim().to_string(),
            download,
            sha256: sha256.to_lowercase(),
            bytes,
            cover,
            tags: clean_tags(&p.tags),
            owner: ticket.owner.clone(),
            published: now,
            updated: now,
            extra: Default::default(),
        };
        let note = match existing {
            Some(i) => {
                let old = catalog.worlds.remove(i);
                // An admin's update keeps the world with its maker.
                entry.owner = old.owner;
                entry.published = old.published;
                entry.extra = old.extra;
                catalog.worlds.insert(i, entry.clone());
                format!("Update {} to {}", p.id, p.version)
            }
            None => {
                catalog.worlds.push(entry.clone());
                format!("Publish {} {}", p.id, p.version)
            }
        };
        self.store
            .save_catalog(&catalog, version.as_deref(), &note)?;
        // Older versions' files are no longer listed.
        let _ = self.store.prune(&p.id, &keep);
        Ok(entry)
    }
    /// Unpublish or report; returns a message for the player.
    pub fn act(&self, signed: &Signed, now: u64) -> Result<String, String> {
        let (action, key) = verify::<Action>(signed, |a| (&a.key, a.time), now)?;
        let owner = owner_of(&key);
        match action.action {
            ActionKind::Unpublish => {
                let admin = self.admins.contains(&friend_code(&key));
                let _busy = self
                    .try_busy()
                    .ok_or("Another world is being published; try again in a minute")?;
                let (mut catalog, version) = self.store.catalog()?;
                let i = catalog
                    .worlds
                    .iter()
                    .position(|e| e.id == action.id)
                    .ok_or("That world is not in Discover")?;
                if catalog.worlds[i].owner != owner && !admin {
                    return Err("Only the world's creator can unpublish it".into());
                }
                self.throttle(&owner, now)?;
                catalog.worlds.remove(i);
                self.store.save_catalog(
                    &catalog,
                    version.as_deref(),
                    &format!("Unpublish {}", action.id),
                )?;
                let _ = self.store.prune(&action.id, &[]);
                Ok("The world is no longer in Discover".into())
            }
            ActionKind::Report => {
                if !crate::resources::valid_pack_id(&action.id) {
                    return Err("Invalid world id".into());
                }
                let reason: String = action
                    .reason
                    .chars()
                    .filter(|c| !c.is_control() || *c == '\n')
                    .take(500)
                    .collect();
                if reason.trim().is_empty() {
                    return Err("Say what is wrong with the world".into());
                }
                {
                    let mut reports = self.reports.lock().unwrap();
                    if reports.len() > 10_000 {
                        reports.clear();
                    }
                    let mine = reports.entry(owner.clone()).or_default();
                    mine.retain(|(at, _)| now.saturating_sub(*at) < 3600);
                    if mine.iter().any(|(_, id)| *id == action.id) {
                        return Ok("You already reported this world; thanks".into());
                    }
                    if mine.len() >= REPORTS_PER_HOUR {
                        return Err("Too many reports; try again later".into());
                    }
                    mine.push((now, action.id.clone()));
                }
                let (catalog, _) = self.store.catalog()?;
                let world = catalog
                    .worlds
                    .iter()
                    .find(|e| e.id == action.id)
                    .ok_or("That world is not in Discover")?;
                let body = format!(
                    "World: `{}` ({}) version {}\nCreator tag: `{}`\nReporter tag: `{}`\n\n> {}\n\n\
                     To remove it, an admin unpublishes it, or deletes its entry from `worlds.json`.",
                    world.id,
                    world.name,
                    world.version,
                    world.owner,
                    owner,
                    reason.trim().replace('\n', "\n> ")
                );
                self.store
                    .report(&format!("Report: {} ({})", world.name, world.id), &body)?;
                Ok("Thanks; the report was sent to the SARE maintainers".into())
            }
        }
    }
}

fn hash_file(path: &Path) -> Result<(String, u64), String> {
    let mut file = std::fs::File::open(path).map_err(|e| format!("Could not read the ZIP: {e}"))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0; 64 * 1024];
    let mut total = 0_u64;
    loop {
        let n = file
            .read(&mut buffer)
            .map_err(|e| format!("Could not read the ZIP: {e}"))?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
        total += n as u64;
    }
    Ok((hex(&hasher.finalize()), total))
}

/// Check that `zip` holds the world `id` within Discover's limits, the same
/// way SARE Client unpacks it, and return its `cover.png` if it has one.
fn inspect_zip(zip: &Path, id: &str) -> Result<Option<Vec<u8>>, String> {
    let file = std::fs::File::open(zip).map_err(|e| format!("Could not read the ZIP: {e}"))?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|_| "The upload is not a valid ZIP".to_string())?;
    if archive.len() > MAX_ENTRIES {
        return Err(format!("The world has more than {MAX_ENTRIES} files"));
    }
    let mut unpacked = 0_u64;
    let mut manifests = Vec::new();
    for i in 0..archive.len() {
        let entry = archive.by_index_raw(i).map_err(|e| e.to_string())?;
        unpacked = unpacked.saturating_add(entry.size());
        let name = entry.name().trim_end_matches('/');
        if name == "content.json" {
            manifests.push(String::new());
        } else if let Some(root) = name.strip_suffix("/content.json") {
            if !root.contains('/') {
                manifests.push(format!("{root}/"));
            }
        }
    }
    if unpacked > MAX_UNPACKED_BYTES {
        return Err("The world is over 256 MiB unpacked".into());
    }
    let root = match manifests.as_slice() {
        [root] => root.clone(),
        [] => return Err("The ZIP has no content.json".into()),
        _ => return Err("The ZIP holds more than one world".into()),
    };
    let mut text = String::new();
    archive
        .by_name(&format!("{root}content.json"))
        .map_err(|e| e.to_string())?
        .take(1024 * 1024)
        .read_to_string(&mut text)
        .map_err(|_| "The world's content.json is not readable".to_string())?;
    let manifest: serde_json::Value = serde_json::from_str(&text)
        .map_err(|_| "The world's content.json is not valid JSON".to_string())?;
    if manifest.get("id").and_then(|v| v.as_str()) != Some(id) {
        return Err(format!(
            "The world's content.json does not have the id {id}"
        ));
    }
    let cover = match archive.by_name(&format!("{root}cover.png")) {
        Ok(entry) if entry.size() <= MAX_COVER_BYTES => {
            let mut png = Vec::new();
            entry
                .take(MAX_COVER_BYTES)
                .read_to_end(&mut png)
                .map_err(|_| "The cover is not readable".to_string())?;
            png.starts_with(b"\x89PNG\r\n\x1a\n").then_some(png)
        }
        _ => None,
    };
    Ok(cover)
}

/// The official store: a GitHub repository. Each world is a release tagged
/// `world-<id>` holding its ZIP and cover; `worlds.json` on the default
/// branch is the catalog. The token needs *Contents* and *Issues*
/// read and write on that repository only.
pub struct GitHub {
    repo: String,
    token: String,
    agent: ureq::Agent,
    api: String,
}
impl GitHub {
    pub fn new(repo: &str, token: &str) -> Self {
        let agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_global(Some(std::time::Duration::from_secs(900)))
            .build()
            .into();
        Self {
            repo: repo.to_string(),
            token: token.to_string(),
            agent,
            api: "https://api.github.com".into(),
        }
    }
    /// The catalog's public address, which SARE Client reads.
    pub fn catalog_url(&self) -> String {
        format!(
            "https://raw.githubusercontent.com/{}/main/worlds.json",
            self.repo
        )
    }
    fn call(
        &self,
        method: &str,
        url: &str,
        body: impl ureq::AsSendBody,
        content_type: &str,
    ) -> Result<(u16, serde_json::Value), String> {
        let request = ureq::http::Request::builder()
            .method(method)
            .uri(url)
            .header("Authorization", format!("Bearer {}", self.token))
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .header("User-Agent", "sare-directory")
            .header("Content-Type", content_type)
            .body(body)
            .map_err(|e| e.to_string())?;
        let mut response = self
            .agent
            .run(request)
            .map_err(|e| format!("Storage is unreachable: {e}"))?;
        let status = response.status().as_u16();
        let text = response
            .body_mut()
            .with_config()
            .limit(4 * 1024 * 1024)
            .read_to_string()
            .unwrap_or_default();
        let value = serde_json::from_str(&text).unwrap_or(serde_json::Value::Null);
        Ok((status, value))
    }
    fn json(
        &self,
        method: &str,
        url: &str,
        body: &serde_json::Value,
    ) -> Result<(u16, serde_json::Value), String> {
        self.call(method, url, body.to_string(), "application/json")
    }
    fn fail(what: &str, status: u16, value: &serde_json::Value) -> String {
        let detail = value.get("message").and_then(|m| m.as_str()).unwrap_or("");
        eprintln!("storage: {what} failed: {status} {detail}");
        format!("Storage refused {what} ({status}); try again later")
    }
    /// The world's release, made when it is missing.
    fn release(&self, id: &str, create: bool) -> Result<Option<serde_json::Value>, String> {
        let tag = format!("world-{id}");
        let url = format!("{}/repos/{}/releases/tags/{tag}", self.api, self.repo);
        let (status, value) = self.call("GET", &url, (), "application/json")?;
        match status {
            200 => Ok(Some(value)),
            404 if create => {
                let url = format!("{}/repos/{}/releases", self.api, self.repo);
                let body = serde_json::json!({
                    "tag_name": tag,
                    "name": id,
                    "body": "A world published from SARE Studio. Listed in worlds.json.",
                    "make_latest": "false",
                });
                let (status, value) = self.json("POST", &url, &body)?;
                (status == 201)
                    .then_some(Some(value.clone()))
                    .ok_or_else(|| Self::fail("making the world's release", status, &value))
            }
            404 => Ok(None),
            _ => Err(Self::fail("reading the world's release", status, &value)),
        }
    }
    fn assets(release: &serde_json::Value) -> Vec<(String, u64)> {
        release["assets"]
            .as_array()
            .map(|assets| {
                assets
                    .iter()
                    .filter_map(|a| Some((a["name"].as_str()?.to_string(), a["id"].as_u64()?)))
                    .collect()
            })
            .unwrap_or_default()
    }
    fn delete_asset(&self, asset: u64) -> Result<(), String> {
        let url = format!("{}/repos/{}/releases/assets/{asset}", self.api, self.repo);
        let (status, value) = self.call("DELETE", &url, (), "application/json")?;
        (status == 204 || status == 404)
            .then_some(())
            .ok_or_else(|| Self::fail("deleting an old file", status, &value))
    }
}
impl Store for GitHub {
    fn catalog(&self) -> Result<(Catalog, Option<String>), String> {
        let url = format!("{}/repos/{}/contents/worlds.json", self.api, self.repo);
        let (status, value) = self.call("GET", &url, (), "application/json")?;
        match status {
            200 => {
                let text = value["content"].as_str().unwrap_or("");
                let data = base64_decode(text).ok_or("The catalog is not readable")?;
                let catalog = serde_json::from_slice(&data)
                    .map_err(|e| format!("The catalog is broken: {e}"))?;
                Ok((catalog, value["sha"].as_str().map(str::to_string)))
            }
            404 => Ok((Catalog::default(), None)),
            _ => Err(Self::fail("reading the catalog", status, &value)),
        }
    }
    fn save_catalog(
        &self,
        catalog: &Catalog,
        version: Option<&str>,
        note: &str,
    ) -> Result<(), String> {
        let url = format!("{}/repos/{}/contents/worlds.json", self.api, self.repo);
        let text = serde_json::to_string_pretty(catalog).map_err(|e| e.to_string())? + "\n";
        let mut body = serde_json::json!({
            "message": note,
            "content": base64_encode(text.as_bytes()),
        });
        if let Some(sha) = version {
            body["sha"] = sha.into();
        }
        let (status, value) = self.json("PUT", &url, &body)?;
        match status {
            200 | 201 => Ok(()),
            409 | 422 => Err("Discover changed at the same time; publish again".into()),
            _ => Err(Self::fail("saving the catalog", status, &value)),
        }
    }
    fn upload(
        &self,
        id: &str,
        name: &str,
        file: &Path,
        content_type: &str,
    ) -> Result<String, String> {
        let release = self.release(id, true)?.ok_or("No release")?;
        // A file left by a publish that failed half-way is replaced.
        for (existing, asset) in Self::assets(&release) {
            if existing == name {
                self.delete_asset(asset)?;
            }
        }
        let release_id = release["id"].as_u64().ok_or("The release has no id")?;
        let url = format!(
            "https://uploads.github.com/repos/{}/releases/{release_id}/assets?name={name}",
            self.repo
        );
        let file = std::fs::File::open(file).map_err(|e| e.to_string())?;
        let (status, value) = self.call("POST", &url, file, content_type)?;
        if status != 201 {
            return Err(Self::fail("uploading the world", status, &value));
        }
        value["browser_download_url"]
            .as_str()
            .filter(|u| u.starts_with("https://"))
            .map(str::to_string)
            .ok_or_else(|| "Storage gave no download link".into())
    }
    fn prune(&self, id: &str, keep: &[String]) -> Result<(), String> {
        let Some(release) = self.release(id, false)? else {
            return Ok(());
        };
        for (name, asset) in Self::assets(&release) {
            if !keep.contains(&name) {
                self.delete_asset(asset)?;
            }
        }
        if keep.is_empty() {
            if let Some(release_id) = release["id"].as_u64() {
                let url = format!("{}/repos/{}/releases/{release_id}", self.api, self.repo);
                self.call("DELETE", &url, (), "application/json")?;
            }
            let url = format!("{}/repos/{}/git/refs/tags/world-{id}", self.api, self.repo);
            self.call("DELETE", &url, (), "application/json")?;
        }
        Ok(())
    }
    fn report(&self, title: &str, body: &str) -> Result<(), String> {
        let url = format!("{}/repos/{}/issues", self.api, self.repo);
        let request = serde_json::json!({ "title": title, "body": body, "labels": ["report"] });
        let (status, value) = self.json("POST", &url, &request)?;
        (status == 201)
            .then_some(())
            .ok_or_else(|| Self::fail("filing the report", status, &value))
    }
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
fn base64_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let n = chunk.iter().fold(0_u32, |n, b| n << 8 | u32::from(*b)) << (8 * (3 - chunk.len()));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(B64[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}
/// Standard base64; whitespace (GitHub wraps lines) is skipped.
fn base64_decode(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() * 3 / 4);
    let (mut n, mut bits) = (0_u32, 0);
    for c in text.bytes() {
        if c.is_ascii_whitespace() || c == b'=' {
            continue;
        }
        let v = B64.iter().position(|b| *b == c)? as u32;
        n = n << 6 | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((n >> bits) as u8);
        }
    }
    Some(out)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::directory::sare_id;
    use std::io::Write;

    /// A store in memory, for tests and local runs.
    #[derive(Default)]
    pub struct Memory {
        pub catalog: Mutex<(Catalog, u32)>,
        pub files: Mutex<HashMap<String, Vec<String>>>,
        pub reports: Mutex<Vec<String>>,
    }
    impl Store for std::sync::Arc<Memory> {
        fn catalog(&self) -> Result<(Catalog, Option<String>), String> {
            let c = self.catalog.lock().unwrap();
            Ok((c.0.clone(), Some(c.1.to_string())))
        }
        fn save_catalog(
            &self,
            catalog: &Catalog,
            version: Option<&str>,
            _: &str,
        ) -> Result<(), String> {
            let mut c = self.catalog.lock().unwrap();
            if version != Some(&c.1.to_string()) {
                return Err("changed".into());
            }
            *c = (catalog.clone(), c.1 + 1);
            Ok(())
        }
        fn upload(&self, id: &str, name: &str, file: &Path, _: &str) -> Result<String, String> {
            assert!(file.is_file());
            let mut files = self.files.lock().unwrap();
            let list = files.entry(id.into()).or_default();
            list.retain(|n| n != name);
            list.push(name.into());
            Ok(format!("https://files.test/{id}/{name}"))
        }
        fn prune(&self, id: &str, keep: &[String]) -> Result<(), String> {
            let mut files = self.files.lock().unwrap();
            if let Some(list) = files.get_mut(id) {
                list.retain(|n| keep.contains(n));
            }
            Ok(())
        }
        fn report(&self, title: &str, _: &str) -> Result<(), String> {
            self.reports.lock().unwrap().push(title.into());
            Ok(())
        }
    }

    pub fn world_zip(dir: &Path, id: &str, cover: bool) -> std::path::PathBuf {
        let path = dir.join(format!("{id}.zip"));
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
        let options = zip::write::SimpleFileOptions::default();
        zip.start_file(format!("{id}/content.json"), options)
            .unwrap();
        write!(zip, "{{\"id\":\"{id}\",\"name\":\"Test\"}}").unwrap();
        if cover {
            zip.start_file(format!("{id}/cover.png"), options).unwrap();
            zip.write_all(b"\x89PNG\r\n\x1a\nrest").unwrap();
        }
        zip.finish().unwrap();
        path
    }
    pub fn publish_for(key: &SigningKey, zip: &Path, id: &str, version: &str, now: u64) -> Signed {
        let (sha256, bytes) = hash_file(zip).unwrap();
        sign_publish(
            key,
            &Publish {
                key: hex(key.verifying_key().as_bytes()),
                time: now,
                id: id.into(),
                name: "Hill Race".into(),
                author: "Alice".into(),
                description: "Downhill with jumps.".into(),
                version: version.into(),
                license: "CC-BY-4.0".into(),
                tags: vec!["Racing".into(), "racing".into(), "Big Jumps!".into()],
                sha256,
                bytes,
            },
        )
    }
    pub fn temp(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("sare-worlds-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }
    fn action(key: &SigningKey, kind: ActionKind, id: &str, reason: &str, now: u64) -> Signed {
        sign_action(
            key,
            &Action {
                key: hex(key.verifying_key().as_bytes()),
                time: now,
                action: kind,
                id: id.into(),
                reason: reason.into(),
            },
        )
    }

    #[test]
    fn creators_publish_update_and_unpublish_their_own_worlds() {
        let dir = temp("own");
        let memory = std::sync::Arc::new(Memory::default());
        let worlds = Worlds::new(Box::new(memory.clone()), []);
        let (alice, mallory) = (sare_id("alice"), sare_id("mallory"));
        let mut now = 1_800_000_000;
        let zip = world_zip(&dir, "hill-race", true);

        let ticket = worlds
            .check(&publish_for(&alice, &zip, "hill-race", "1.0.0", now), now)
            .unwrap();
        let entry = worlds.publish(ticket, &zip, now).unwrap();
        assert_eq!(entry.tags, ["racing", "big-jumps"]);
        assert_eq!(
            entry.download,
            "https://files.test/hill-race/hill-race-1.0.0.zip"
        );
        assert_eq!(
            entry.cover.as_deref(),
            Some("https://files.test/hill-race/hill-race-1.0.0.png")
        );
        assert_eq!(entry.owner, owner_of(&alice.verifying_key()));
        assert!(!entry.owner.contains(&friend_code(&alice.verifying_key())));

        // Too soon, the same version, and someone else's update are refused.
        assert!(worlds
            .check(
                &publish_for(&alice, &zip, "hill-race", "1.1.0", now + 1),
                now + 1
            )
            .is_err());
        now += 60;
        let ticket = worlds
            .check(&publish_for(&alice, &zip, "hill-race", "1.0.0", now), now)
            .unwrap();
        assert!(worlds
            .publish(ticket, &zip, now)
            .unwrap_err()
            .contains("raise the version"));
        let ticket = worlds
            .check(&publish_for(&mallory, &zip, "hill-race", "9.0.0", now), now)
            .unwrap();
        assert!(worlds
            .publish(ticket, &zip, now)
            .unwrap_err()
            .contains("Another creator"));

        now += 60;
        let ticket = worlds
            .check(
                &publish_for(&alice, &zip, "hill-race", "1.1 beta", now),
                now,
            )
            .unwrap();
        let updated = worlds.publish(ticket, &zip, now).unwrap();
        assert_eq!(updated.published, 1_800_000_000);
        assert_eq!(updated.updated, now);
        let catalog = memory.catalog.lock().unwrap().0.clone();
        assert_eq!(catalog.worlds.len(), 1);
        assert_eq!(catalog.worlds[0].version, "1.1 beta");
        // The old version's files are gone.
        assert_eq!(
            memory.files.lock().unwrap()["hill-race"],
            ["hill-race-1.1-beta.zip", "hill-race-1.1-beta.png"]
        );

        now += 60;
        assert!(worlds
            .act(
                &action(&mallory, ActionKind::Unpublish, "hill-race", "", now),
                now
            )
            .is_err());
        worlds
            .act(
                &action(&alice, ActionKind::Unpublish, "hill-race", "", now),
                now,
            )
            .unwrap();
        assert!(memory.catalog.lock().unwrap().0.worlds.is_empty());
        assert!(memory.files.lock().unwrap()["hill-race"].is_empty());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn bad_uploads_are_refused() {
        let dir = temp("bad");
        let memory = std::sync::Arc::new(Memory::default());
        let worlds = Worlds::new(Box::new(memory.clone()), []);
        let alice = sare_id("alice");
        let mut now = 1_800_000_000;
        let zip = world_zip(&dir, "hill-race", false);
        let other = world_zip(&dir, "other", false);
        // Wrong clock, tampered payload, bad metadata.
        assert!(worlds
            .check(
                &publish_for(&alice, &zip, "hill-race", "1", now - 3600),
                now
            )
            .is_err());
        let mut tampered = publish_for(&alice, &zip, "hill-race", "1", now);
        tampered.payload = tampered.payload.replace("Hill Race", "Evil Race");
        assert!(worlds.check(&tampered, now).is_err());
        assert!(worlds
            .check(&publish_for(&alice, &zip, "Bad Id", "1", now), now)
            .is_err());
        // A ZIP that is not the signed one, or holds another world.
        let ticket = worlds
            .check(&publish_for(&alice, &other, "hill-race", "1", now), now)
            .unwrap();
        assert!(worlds
            .publish(ticket, &zip, now)
            .unwrap_err()
            .contains("does not match"));
        now += 60;
        let ticket = worlds
            .check(&publish_for(&alice, &other, "hill-race", "1", now), now)
            .unwrap();
        assert!(worlds
            .publish(ticket, &other, now)
            .unwrap_err()
            .contains("id hill-race"));
        let junk = dir.join("junk.zip");
        std::fs::write(&junk, b"not a zip").unwrap();
        now += 60;
        let ticket = worlds
            .check(&publish_for(&alice, &junk, "hill-race", "1", now), now)
            .unwrap();
        assert!(worlds
            .publish(ticket, &junk, now)
            .unwrap_err()
            .contains("not a valid ZIP"));
        assert!(memory.catalog.lock().unwrap().0.worlds.is_empty());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn reports_reach_the_maintainers_and_admins_can_remove() {
        let dir = temp("report");
        let memory = std::sync::Arc::new(Memory::default());
        let admin = sare_id("admin");
        let worlds = Worlds::new(
            Box::new(memory.clone()),
            [friend_code(&admin.verifying_key()).to_lowercase()],
        );
        let (alice, bob) = (sare_id("alice"), sare_id("bob"));
        let now = 1_800_000_000;
        let zip = world_zip(&dir, "hill-race", false);
        let ticket = worlds
            .check(&publish_for(&alice, &zip, "hill-race", "1", now), now)
            .unwrap();
        worlds.publish(ticket, &zip, now).unwrap();

        assert!(worlds
            .act(
                &action(&bob, ActionKind::Report, "hill-race", " ", now),
                now
            )
            .is_err());
        worlds
            .act(
                &action(&bob, ActionKind::Report, "hill-race", "Stolen models", now),
                now,
            )
            .unwrap();
        // Reporting the same world again files nothing new.
        worlds
            .act(
                &action(&bob, ActionKind::Report, "hill-race", "Again", now),
                now,
            )
            .unwrap();
        assert_eq!(
            *memory.reports.lock().unwrap(),
            ["Report: Hill Race (hill-race)"]
        );
        assert!(worlds
            .act(&action(&bob, ActionKind::Report, "nothing", "x", now), now)
            .is_err());

        worlds
            .act(
                &action(&admin, ActionKind::Unpublish, "hill-race", "", now),
                now,
            )
            .unwrap();
        assert!(memory.catalog.lock().unwrap().0.worlds.is_empty());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn catalogs_keep_unknown_fields_and_old_clients_can_read_them() {
        let text = r#"{"version":1,"worlds":[{"id":"a","name":"A","version":"1","license":"MIT",
            "download":"https://x/a.zip","sha256":"00","bytes":1,"featured":true}]}"#;
        let catalog: Catalog = serde_json::from_str(text).unwrap();
        assert_eq!(catalog.worlds[0].extra["featured"], true);
        let again = serde_json::to_string(&catalog).unwrap();
        assert!(again.contains("\"featured\":true"));
    }

    #[test]
    fn base64_round_trips() {
        for data in [&b""[..], b"a", b"ab", b"abc", b"{\"version\":1}\n"] {
            let text = base64_encode(data);
            assert_eq!(base64_decode(&text).unwrap(), data);
        }
        assert_eq!(base64_encode(b"Man"), "TWFu");
        assert_eq!(base64_decode("TW\nFu").unwrap(), b"Man");
    }
}
