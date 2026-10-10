//! First-run server-data, mirroring FiveM's cfx-server-data layout.
//! The source of truth is the repository's `server-data/` folder.
use anyhow::Result;
use std::{fs, path::Path};

macro_rules! data {
    ($path:literal) => {
        (
            $path,
            include_str!(concat!("../../../../server-data/", $path)),
        )
    };
}
pub const FILES: [(&str, &str); 12] = [
    data!("server.cfg"),
    data!("resources/[system]/chat/fxmanifest.lua"),
    data!("resources/[system]/chat/sv_chat.lua"),
    data!("resources/[managers]/mapmanager/fxmanifest.lua"),
    data!("resources/[managers]/mapmanager/mapmanager_server.lua"),
    data!("resources/[managers]/spawnmanager/fxmanifest.lua"),
    data!("resources/[managers]/spawnmanager/spawnmanager_server.lua"),
    data!("resources/[gamemodes]/basic-gamemode/fxmanifest.lua"),
    data!("resources/[gamemodes]/basic-gamemode/basic_server.lua"),
    data!("resources/[gamemodes]/[maps]/sare-map-grove/fxmanifest.lua"),
    data!("resources/[gamemodes]/[maps]/sare-map-grove/map.lua"),
    ("resources/[local]/.gitkeep", ""),
];

/// Write missing template files only; existing files are never replaced.
pub fn create(root: &Path) -> Result<()> {
    let mut created = 0;
    for (relative, content) in FILES {
        let path = root.join(relative);
        if path.exists() {
            continue;
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, content)?;
        created += 1;
    }
    println!(
        "Created FiveM-style server data in {} ({created} files): server.cfg and resources/.",
        root.display()
    );
    Ok(())
}
