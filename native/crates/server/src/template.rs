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
/// Server templates for the first start: name, title, description, server.cfg.
pub const TEMPLATES: [(&str, &str, &str, &str); 2] = [
    (
        "freeroam",
        "Freeroam",
        "free roam with chat, spawn points and the example framework",
        include_str!("../../../../server-data/server.cfg"),
    ),
    (
        "sarebox",
        "SARE Box",
        "roleplay framework: saved characters, money, jobs, paychecks and an admin menu",
        include_str!("../../../../server-data/templates/sarebox.cfg"),
    ),
];

/// Resources every template gets; server.cfg decides which ones start.
pub const FILES: [(&str, &str); 29] = [
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
    data!("resources/[examples]/sare_lib/fxmanifest.lua"),
    data!("resources/[examples]/sare_lib/init.lua"),
    data!("resources/[examples]/sare_lib/server.lua"),
    data!("resources/[examples]/sare_lib/client.lua"),
    data!("resources/[examples]/sare_lib/modules/math.lua"),
    data!("resources/[examples]/sare_core/fxmanifest.lua"),
    data!("resources/[examples]/sare_core/server.lua"),
    data!("resources/[examples]/sare_jobs/fxmanifest.lua"),
    data!("resources/[examples]/sare_jobs/server.lua"),
    data!("resources/[examples]/sare_jobs/client.lua"),
    data!("resources/[sarebox]/sarebox_core/fxmanifest.lua"),
    data!("resources/[sarebox]/sarebox_core/shared/config.lua"),
    data!("resources/[sarebox]/sarebox_core/server/player.lua"),
    data!("resources/[sarebox]/sarebox_core/server/main.lua"),
    data!("resources/[sarebox]/sarebox_core/client/main.lua"),
    data!("resources/[sarebox]/sarebox_admin/fxmanifest.lua"),
    data!("resources/[sarebox]/sarebox_admin/server.lua"),
    data!("resources/[sarebox]/sarebox_admin/client.lua"),
    ("resources/[local]/.gitkeep", ""),
];

pub fn find(
    name: &str,
) -> Option<&'static (&'static str, &'static str, &'static str, &'static str)> {
    TEMPLATES
        .iter()
        .find(|(n, ..)| n.eq_ignore_ascii_case(name))
}

/// Write missing template files only; existing files are never replaced.
pub fn create(root: &Path, template: &str) -> Result<()> {
    let Some((_, title, ..)) = find(template) else {
        let names: Vec<&str> = TEMPLATES.iter().map(|(n, ..)| *n).collect();
        anyhow::bail!(
            "Unknown server template {template}; use one of: {}",
            names.join(", ")
        );
    };
    let config = find(template).unwrap().3;
    let mut created = 0;
    for (relative, content) in std::iter::once(("server.cfg", config)).chain(FILES) {
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
        "Created a {title} server in {} ({created} files): server.cfg and resources/.",
        root.display()
    );
    Ok(())
}

/// First start: `+set sv_template <name>`, else ask on a terminal, else
/// Freeroam. FiveM asks the same in txAdmin's setup.
pub fn choose(requested: Option<&str>) -> Result<&'static str> {
    use std::io::{BufRead, IsTerminal, Write};
    if let Some(name) = requested {
        return find(name)
            .map(|t| t.0)
            .ok_or_else(|| anyhow::anyhow!("Unknown sv_template {name}"));
    }
    if !std::io::stdin().is_terminal() {
        println!("No server.cfg: creating the Freeroam template (choose another with +set sv_template sarebox).");
        return Ok(TEMPLATES[0].0);
    }
    println!("No server.cfg here yet. Which server do you want?");
    for (index, (name, title, description, _)) in TEMPLATES.iter().enumerate() {
        println!("  {}) {title} ({name}): {description}", index + 1);
    }
    loop {
        print!("Choose 1-{} [1]: ", TEMPLATES.len());
        std::io::stdout().flush()?;
        let mut line = String::new();
        if std::io::stdin().lock().read_line(&mut line)? == 0 {
            return Ok(TEMPLATES[0].0);
        }
        let line = line.trim();
        if line.is_empty() {
            return Ok(TEMPLATES[0].0);
        }
        let picked = line
            .parse::<usize>()
            .ok()
            .and_then(|n| TEMPLATES.get(n.wrapping_sub(1)))
            .or_else(|| find(line));
        if let Some((name, ..)) = picked {
            return Ok(name);
        }
    }
}
