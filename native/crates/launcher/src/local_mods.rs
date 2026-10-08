use anyhow::{ensure, Context, Result};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
};
pub struct Entry {
    pub name: String,
    pub status: String,
    pub bytes: u64,
    pub error: Option<String>,
}
pub fn root() -> PathBuf {
    let exe = std::env::current_exe().unwrap_or_default();
    let sibling = exe.parent().unwrap_or(Path::new(".")).join("mods");
    if sibling.is_dir() {
        return sibling;
    }
    exe.parent()
        .and_then(Path::parent)
        .and_then(Path::parent)
        .and_then(Path::parent)
        .map(|p| p.join("mods"))
        .filter(|p| p.is_dir())
        .unwrap_or(sibling)
}
fn regular(path: &Path) -> Result<fs::Metadata> {
    let meta = fs::symlink_metadata(path)?;
    ensure!(!meta.is_symlink(), "Linked resources are not inspected");
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        ensure!(
            meta.file_attributes() & 0x400 == 0,
            "Reparse resources are not inspected"
        );
    }
    Ok(meta)
}
fn size(path: &Path, depth: usize, count: &mut usize) -> Result<u64> {
    ensure!(
        depth <= 12 && *count < 8192,
        "Resource inspection budget exceeded"
    );
    *count += 1;
    let meta = regular(path)?;
    if meta.is_file() {
        return Ok(meta.len());
    }
    ensure!(meta.is_dir(), "Unexpected resource file type");
    let mut bytes = 0u64;
    for entry in fs::read_dir(path)? {
        bytes = bytes.saturating_add(size(&entry?.path(), depth + 1, count)?);
    }
    Ok(bytes)
}
pub fn inspect(root: &Path) -> Result<Vec<Entry>> {
    if !root.exists() {
        return Ok(Vec::new());
    }
    regular(root)?;
    let mut output = Vec::new();
    collect(root, 0, &mut output)?;
    Ok(output)
}
fn collect(root: &Path, depth: usize, output: &mut Vec<Entry>) -> Result<()> {
    ensure!(depth <= 4, "Category nesting exceeds four levels");
    let mut entries = fs::read_dir(root)?
        .take(129)
        .collect::<std::io::Result<Vec<_>>>()?;
    ensure!(entries.len() <= 128, "Too many resource entries");
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let path = entry.path();
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let label = entry.file_name().to_string_lossy().into_owned();
        if label.starts_with('[') && label.ends_with(']') {
            regular(&path)?;
            collect(&path, depth + 1, output)?;
            continue;
        }
        ensure!(output.len() < 128, "Too many resources");
        let mut item = Entry {
            name: label,
            status: "Not loaded".into(),
            bytes: 0,
            error: None,
        };
        let result = (|| -> Result<()> {
            item.bytes = size(&path, 0, &mut 0)?;
            let manifest = if path.join("mod.json").is_file() {
                path.join("mod.json")
            } else {
                path.join("resource.json")
            };
            regular(&manifest)?;
            let mut data = Vec::new();
            fs::File::open(manifest)?
                .take(64 * 1024 + 1)
                .read_to_end(&mut data)?;
            ensure!(data.len() <= 64 * 1024, "Manifest exceeds 64 KiB");
            let value: serde_json::Value =
                serde_json::from_slice(&data).context("Invalid native JSON manifest")?;
            if let Some(name) = value["name"].as_str() {
                item.name = name.chars().filter(|c| !c.is_control()).take(96).collect();
            }
            item.status = if value["enabled"].as_bool() == Some(true) {
                "Enabled; runtime validates assets"
            } else {
                "Disabled"
            }
            .into();
            Ok(())
        })();
        if let Err(error) = result {
            item.status = "Inspection failed".into();
            item.error = Some(format!("{error:#}"));
        }
        output.push(item);
    }
    Ok(())
}
