//! Resource discovery under `resources/` and `fxmanifest.lua` metadata.
//! Bracketed folders such as `[system]` are categories, as in FiveM.
use anyhow::{ensure, Context, Result};
use mlua::{Lua, Table, Value};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Default)]
pub struct Manifest {
    /// Ordered metadata, plural keys normalised: `server_scripts` -> `server_script`.
    pub entries: Vec<(String, String)>,
    /// Trailing tables, e.g. `resource_type 'map' { gameTypes = ... }`, as JSON.
    pub extra: BTreeMap<String, String>,
    /// Native SA assets: `resource.json` or `mod.json`.
    pub native_assets: bool,
}
impl Manifest {
    pub fn values(&self, key: &str) -> impl Iterator<Item = &str> + '_ {
        let key = key.to_string();
        self.entries
            .iter()
            .filter(move |(k, _)| *k == key)
            .map(|(_, v)| v.as_str())
    }
    #[cfg(test)]
    pub fn first(&self, key: &str) -> Option<&str> {
        self.values(key).next()
    }
}

/// Map of resource name to folder.
pub fn discover(root: &Path) -> Result<BTreeMap<String, PathBuf>> {
    fn walk(dir: &Path, depth: usize, found: &mut BTreeMap<String, PathBuf>) -> Result<()> {
        ensure!(depth <= 4, "resource category nesting exceeds four levels");
        let mut entries = fs::read_dir(dir)?.collect::<std::io::Result<Vec<_>>>()?;
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            let path = entry.path();
            if name.starts_with('[') && name.ends_with(']') {
                walk(&path, depth + 1, found)?;
            } else if is_resource(&path) {
                if let Some(previous) = found.get(&name) {
                    eprintln!(
                        "Duplicate resource {name}: using {}, ignoring {}",
                        previous.display(),
                        path.display()
                    );
                } else {
                    ensure!(found.len() < 512, "too many resources");
                    found.insert(name, path);
                }
            }
        }
        Ok(())
    }
    let mut found = BTreeMap::new();
    if root.is_dir() {
        walk(root, 0, &mut found)?;
    }
    Ok(found)
}
fn is_resource(path: &Path) -> bool {
    [
        "fxmanifest.lua",
        "__resource.lua",
        "resource.json",
        "mod.json",
    ]
    .iter()
    .any(|f| path.join(f).is_file())
}

const PLURALS: [(&str, &str); 7] = [
    ("server_scripts", "server_script"),
    ("client_scripts", "client_script"),
    ("shared_scripts", "shared_script"),
    ("files", "file"),
    ("dependencies", "dependency"),
    ("exports", "export"),
    ("server_exports", "server_export"),
];

/// Runs the manifest in an isolated Lua state with no standard library
/// access; unknown globals become metadata collectors, as in FiveM.
pub fn read(folder: &Path) -> Result<Manifest> {
    let mut manifest = Manifest {
        native_assets: folder.join("resource.json").is_file() || folder.join("mod.json").is_file(),
        ..Manifest::default()
    };
    let file = ["fxmanifest.lua", "__resource.lua"]
        .iter()
        .map(|f| folder.join(f))
        .find(|p| p.is_file());
    let Some(file) = file else {
        return Ok(manifest);
    };
    ensure!(
        fs::metadata(&file)?.len() <= 64 * 1024,
        "{} exceeds 64 KiB",
        file.display()
    );
    let source = fs::read_to_string(&file)?;
    let lua = Lua::new();
    lua.set_memory_limit(8 * 1024 * 1024)?;
    let collected = lua.create_table()?;
    let env = lua.create_table()?;
    let meta = lua.create_table()?;
    let collected_ref = collected.clone();
    meta.set(
        "__index",
        lua.create_function(move |lua, (_env, key): (Table, String)| {
            let key = PLURALS
                .iter()
                .find(|(plural, _)| *plural == key)
                .map(|(_, single)| single.to_string())
                .unwrap_or(key);
            let collected = collected_ref.clone();
            lua.create_function(move |lua, value: Value| {
                let push = |v: String| -> mlua::Result<()> {
                    let entry = lua.create_table()?;
                    entry.set(1, key.clone())?;
                    entry.set(2, v)?;
                    collected.push(entry)
                };
                match value {
                    Value::Table(list) => {
                        for v in list.sequence_values::<String>() {
                            push(v?)?;
                        }
                        Ok(Value::Nil)
                    }
                    other => {
                        let text = lua
                            .coerce_string(other)?
                            .map(|s| s.to_string_lossy())
                            .unwrap_or_default();
                        push(text.clone())?;
                        let collected = collected.clone();
                        let key = key.clone();
                        // Optional trailing table: `key 'value' { ... }`.
                        Ok(Value::Function(lua.create_function(
                            move |lua, extra: Value| {
                                let json: serde_json::Value =
                                    mlua::LuaSerdeExt::from_value(lua, extra)?;
                                let entry = lua.create_table()?;
                                entry.set(1, format!("{key}_extra"))?;
                                entry.set(2, json.to_string())?;
                                collected.push(entry)
                            },
                        )?))
                    }
                }
            })
        })?,
    )?;
    env.set_metatable(Some(meta))?;
    lua.load(&source)
        .set_name(file.display().to_string())
        .set_environment(env)
        .exec()
        .with_context(|| format!("Invalid manifest {}", file.display()))?;
    for entry in collected.sequence_values::<Table>() {
        let entry = entry?;
        let key: String = entry.get(1)?;
        let value: String = entry.get(2)?;
        if let Some(base) = key.strip_suffix("_extra") {
            manifest.extra.insert(base.to_string(), value);
        } else {
            manifest.entries.push((key, value));
        }
    }
    Ok(manifest)
}

/// Expand `*` within one path segment and `**` across folders, relative to
/// the resource folder, never escaping it.
pub fn expand(folder: &Path, pattern: &str) -> Result<Vec<PathBuf>> {
    let pattern = pattern.replace('\\', "/");
    ensure!(
        !pattern.starts_with('/') && !pattern.split('/').any(|p| p == ".."),
        "script path {pattern} escapes its resource"
    );
    if !pattern.contains('*') {
        let path = folder.join(&pattern);
        ensure!(path.is_file(), "missing file {pattern}");
        return Ok(vec![path]);
    }
    let mut out = Vec::new();
    let mut stack = vec![(folder.to_path_buf(), String::new())];
    while let Some((dir, prefix)) = stack.pop() {
        let mut entries = fs::read_dir(&dir)?.collect::<std::io::Result<Vec<_>>>()?;
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            let name = entry.file_name().to_string_lossy().into_owned();
            let relative = format!("{prefix}{name}");
            if entry.file_type()?.is_dir() {
                if stack.len() < 64 {
                    stack.push((entry.path(), format!("{relative}/")));
                }
            } else if glob(&pattern, &relative) {
                out.push(entry.path());
            }
        }
    }
    out.sort();
    Ok(out)
}
fn glob(pattern: &str, path: &str) -> bool {
    fn segment(p: &str, s: &str) -> bool {
        match p.split_once('*') {
            None => p == s,
            Some((head, tail)) => {
                s.starts_with(head)
                    && (0..=s.len() - head.len()).any(|i| {
                        s.is_char_boundary(head.len() + i) && segment(tail, &s[head.len() + i..])
                    })
            }
        }
    }
    fn parts(p: &[&str], s: &[&str]) -> bool {
        match (p.first(), s.first()) {
            (None, None) => true,
            (Some(&"**"), _) => parts(&p[1..], s) || (!s.is_empty() && parts(p, &s[1..])),
            (Some(a), Some(b)) => segment(a, b) && parts(&p[1..], &s[1..]),
            _ => false,
        }
    }
    let p: Vec<_> = pattern.split('/').collect();
    let s: Vec<_> = path.split('/').collect();
    parts(&p, &s)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn manifest_metadata_plurals_extras_and_globs() {
        let dir = std::env::temp_dir().join(format!("sare-manifest-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("server")).unwrap();
        fs::write(dir.join("server/a.lua"), "").unwrap();
        fs::write(dir.join("server/b.lua"), "").unwrap();
        fs::write(
            dir.join("fxmanifest.lua"),
            "fx_version 'cerulean'\ngame 'common'\nresource_type 'map' { gameTypes = { ['basic-gamemode'] = true } }\nserver_scripts { 'server/*.lua' }\nclient_script 'c.lua'\ndependency 'mapmanager'",
        )
        .unwrap();
        let m = read(&dir).unwrap();
        assert_eq!(m.first("fx_version"), Some("cerulean"));
        assert_eq!(
            m.values("server_script").collect::<Vec<_>>(),
            ["server/*.lua"]
        );
        assert_eq!(m.first("dependency"), Some("mapmanager"));
        assert!(m.extra["resource_type"].contains("basic-gamemode"));
        let scripts = expand(&dir, "server/*.lua").unwrap();
        assert_eq!(scripts.len(), 2);
        assert_eq!(expand(&dir, "**/b.lua").unwrap().len(), 1);
        assert!(expand(&dir, "../x.lua").is_err());
        // The manifest sandbox has no os/io: collectors only.
        fs::write(dir.join("fxmanifest.lua"), "os.exit()").unwrap();
        assert!(read(&dir).is_err());
        fs::remove_dir_all(&dir).unwrap();
    }
}
