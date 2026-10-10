//! FiveM's resource KVP: small key-value storage per resource, kept in
//! `kvp/<resource>.json` next to server.cfg and written at most once a second.
use serde_json::Value as Json;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

pub const MAX_KEY: usize = 255;
pub const MAX_VALUE: usize = 64 * 1024;
/// Total stored bytes (keys and values) per resource.
pub const MAX_RESOURCE: usize = 8 * 1024 * 1024;

#[derive(Default)]
pub struct Store {
    dir: PathBuf,
    data: BTreeMap<String, BTreeMap<String, Json>>,
    dirty: BTreeSet<String>,
    flushed: Option<Instant>,
    /// StartFindKvp handles: matching keys left to return.
    finds: BTreeMap<i64, Vec<String>>,
    next_find: i64,
}

fn size(values: &BTreeMap<String, Json>) -> usize {
    values
        .iter()
        .map(|(k, v)| k.len() + v.to_string().len())
        .sum()
}

impl Store {
    pub fn new(dir: PathBuf) -> Self {
        Self {
            dir,
            ..Default::default()
        }
    }
    fn path(&self, resource: &str) -> PathBuf {
        self.dir.join(format!("{resource}.json"))
    }
    fn values(&mut self, resource: &str) -> &mut BTreeMap<String, Json> {
        if !self.data.contains_key(resource) {
            let loaded = fs::read(self.path(resource))
                .ok()
                .and_then(|bytes| serde_json::from_slice(&bytes).ok())
                .unwrap_or_default();
            self.data.insert(resource.to_string(), loaded);
        }
        self.data.get_mut(resource).unwrap()
    }
    pub fn get(&mut self, resource: &str, key: &str) -> Option<Json> {
        self.values(resource).get(key).cloned()
    }
    /// Returns an error message when a limit is exceeded.
    pub fn set(&mut self, resource: &str, key: &str, value: Json) -> Result<(), String> {
        if key.is_empty() || key.len() > MAX_KEY {
            return Err(format!("KVP key must be 1-{MAX_KEY} bytes"));
        }
        if value.to_string().len() > MAX_VALUE {
            return Err(format!("KVP value for {key} is over {MAX_VALUE} bytes"));
        }
        let values = self.values(resource);
        let old = values.insert(key.to_string(), value);
        if size(values) > MAX_RESOURCE {
            match old {
                Some(old) => values.insert(key.to_string(), old),
                None => values.remove(key),
            };
            return Err(format!(
                "KVP storage for {resource} is full ({MAX_RESOURCE} bytes)"
            ));
        }
        self.dirty.insert(resource.to_string());
        Ok(())
    }
    pub fn delete(&mut self, resource: &str, key: &str) {
        if self.values(resource).remove(key).is_some() {
            self.dirty.insert(resource.to_string());
        }
    }
    pub fn start_find(&mut self, resource: &str, prefix: &str) -> i64 {
        let keys: Vec<String> = self
            .values(resource)
            .keys()
            .filter(|k| k.starts_with(prefix))
            .rev()
            .cloned()
            .collect();
        self.next_find += 1;
        // Abandoned handles stay bounded.
        if self.finds.len() >= 64 {
            let oldest = *self.finds.keys().next().unwrap();
            self.finds.remove(&oldest);
        }
        self.finds.insert(self.next_find, keys);
        self.next_find
    }
    pub fn find(&mut self, handle: i64) -> Option<String> {
        self.finds.get_mut(&handle)?.pop()
    }
    pub fn end_find(&mut self, handle: i64) {
        self.finds.remove(&handle);
    }
    /// Write changed resources; `force` ignores the once-a-second limit.
    pub fn flush(&mut self, force: bool) -> Vec<String> {
        if self.dirty.is_empty()
            || (!force
                && self
                    .flushed
                    .is_some_and(|t| t.elapsed() < Duration::from_secs(1)))
        {
            return Vec::new();
        }
        self.flushed = Some(Instant::now());
        let mut errors = Vec::new();
        for resource in std::mem::take(&mut self.dirty) {
            let path = self.path(&resource);
            let data = serde_json::to_vec_pretty(&self.data[&resource]).expect("JSON values");
            if let Err(error) = write_atomic(&path, &data) {
                errors.push(format!("Cannot save KVP for {resource}: {error}"));
                self.dirty.insert(resource);
            }
        }
        errors
    }
}

fn write_atomic(path: &Path, data: &[u8]) -> std::io::Result<()> {
    fs::create_dir_all(path.parent().unwrap())?;
    let temporary = path.with_extension("json.tmp");
    fs::write(&temporary, data)?;
    fs::rename(&temporary, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn values_persist_limits_hold_and_finds_iterate_by_prefix() {
        let dir = std::env::temp_dir().join(format!("sare-kvp-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let mut store = Store::new(dir.clone());
        store
            .set("bank", "account:1", json!("{\"cash\":5}"))
            .unwrap();
        store.set("bank", "account:2", json!(7)).unwrap();
        store.set("bank", "other", json!(1.5)).unwrap();
        assert!(store.set("bank", "", json!(1)).is_err());
        assert!(store
            .set("bank", "big", json!("x".repeat(MAX_VALUE + 1)))
            .is_err());
        let handle = store.start_find("bank", "account:");
        assert_eq!(store.find(handle).as_deref(), Some("account:1"));
        assert_eq!(store.find(handle).as_deref(), Some("account:2"));
        assert_eq!(store.find(handle), None);
        store.end_find(handle);
        store.delete("bank", "other");
        assert!(store.flush(false).is_empty());
        let mut reloaded = Store::new(dir.clone());
        assert_eq!(reloaded.get("bank", "account:2"), Some(json!(7)));
        assert_eq!(reloaded.get("bank", "other"), None);
        let _ = fs::remove_dir_all(dir);
    }
}
