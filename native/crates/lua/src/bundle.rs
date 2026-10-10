//! A resource's client-side Lua: the scripts to run in order and the files
//! `LoadResourceFile` can read. Served once per resource version by SHA-256.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const MAX_FILES: usize = 1024;

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Bundle {
    pub resource: String,
    /// Script paths in load order. `@other/file.lua` names another resource's
    /// file, stored under that key in `files`.
    pub scripts: Vec<String>,
    /// Text files by path relative to the resource (or `@other/...`).
    pub files: BTreeMap<String, String>,
}

/// Resource names as FiveM accepts them, without path separators.
pub fn valid_resource_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-.".contains(&b))
        && name != "."
        && name != ".."
}
fn valid_path(path: &str) -> bool {
    let path = path.strip_prefix('@').unwrap_or(path);
    !path.is_empty()
        && path.len() <= 256
        && !path.starts_with('/')
        && !path.contains('\\')
        && path
            .split('/')
            .all(|p| !p.is_empty() && p != "." && p != "..")
}

impl Bundle {
    pub fn to_bytes(&self) -> Vec<u8> {
        serde_json::to_vec(self).expect("bundle serializes")
    }
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, String> {
        let bundle: Self = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
        bundle.validate()?;
        Ok(bundle)
    }
    pub fn validate(&self) -> Result<(), String> {
        if !valid_resource_name(&self.resource) {
            return Err("invalid resource name".into());
        }
        if self.files.len() > MAX_FILES || self.scripts.len() > MAX_FILES {
            return Err("too many files".into());
        }
        if let Some(path) = self.files.keys().find(|p| !valid_path(p)) {
            return Err(format!("invalid file path {path}"));
        }
        if let Some(path) = self.scripts.iter().find(|p| !self.files.contains_key(*p)) {
            return Err(format!("script {path} is missing"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bundles_reject_escaping_paths_and_missing_scripts() {
        let mut bundle = Bundle {
            resource: "demo".into(),
            scripts: vec!["client.lua".into(), "@lib/init.lua".into()],
            files: BTreeMap::from([
                ("client.lua".into(), "print(1)".into()),
                ("@lib/init.lua".into(), "lib = {}".into()),
            ]),
        };
        assert_eq!(Bundle::from_bytes(&bundle.to_bytes()).unwrap(), bundle);
        bundle.files.insert("../x.lua".into(), String::new());
        assert!(bundle.validate().is_err());
        bundle.files.remove("../x.lua");
        bundle.scripts.push("gone.lua".into());
        assert!(bundle.validate().is_err());
        assert!(!valid_resource_name("a/b") && !valid_resource_name(".."));
    }
}
