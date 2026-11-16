//! Script tools: Lua resources in a server's `resources/` folder, laid out
//! like FiveM (`[category]` folders, one folder per resource with an
//! `fxmanifest.lua`). Only text files inside the resources folder are read
//! or written, and Lua is syntax-checked (with SARE's CfxLua rewrite)
//! before it is saved. Nothing is ever run.
use crate::protocol::Tool;
use crate::worlds::text;
use crate::Server;
use anyhow::{bail, ensure, Context, Result};
use serde_json::{json, Value};
use std::path::{Component, Path, PathBuf};

/// Text files the bridge may write into a resource.
const WRITABLE: [&str; 9] = [
    "lua", "json", "md", "txt", "cfg", "html", "css", "js", "xml",
];
/// Largest file the bridge reads or writes.
const MAX_FILE: usize = 256 * 1024;
/// How deep `[category]` folders may nest (same as the runtime).
const MAX_CATEGORY_DEPTH: usize = 4;

pub fn tools() -> Vec<Tool> {
    vec![
        Tool {
            name: "list_resources",
            description: "List the server resources (FiveM-style folders with fxmanifest.lua or resource.json) with their category, files and whether server.cfg starts them.",
            schema: || json!({ "type": "object", "properties": {} }),
            run: |s, _| list_resources(s),
        },
        Tool {
            name: "read_resource_file",
            description: "Read a text file from a resource, e.g. path \"fxmanifest.lua\" or \"server/main.lua\". Read the built-in resources (sare_rp_core, sarebox_core, ...) to learn how they are used.",
            schema: || json!({
                "type": "object",
                "properties": {
                    "resource": { "type": "string" },
                    "path": { "type": "string", "description": "Path inside the resource, with / separators" },
                },
                "required": ["resource", "path"],
            }),
            run: read_resource_file,
        },
        Tool {
            name: "write_resource_file",
            description: "Write (create or replace) a text file in a resource: .lua, .json, .md, .txt, .cfg, .html, .css, .js or .xml, at most 256 KB. Lua with a syntax error is refused with the error and line, so fix it and write again. Remember to list new script files in fxmanifest.lua.",
            schema: || json!({
                "type": "object",
                "properties": {
                    "resource": { "type": "string" },
                    "path": { "type": "string" },
                    "content": { "type": "string" },
                },
                "required": ["resource", "path", "content"],
            }),
            run: write_resource_file,
        },
        Tool {
            name: "create_resource",
            description: "Create a new script resource from SARE's template (fxmanifest.lua, server.lua, client.lua, README.md) in resources/[category]/<name>. With ensure true, `ensure <name>` is added to server.cfg so the server starts it.",
            schema: || json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Folder name: letters, digits, _ and -, e.g. my_garage" },
                    "category": { "type": "string", "description": "Category folder without brackets (default \"local\")" },
                    "description": { "type": "string" },
                    "ensure": { "type": "boolean" },
                },
                "required": ["name"],
            }),
            run: create_resource,
        },
        Tool {
            name: "check_lua",
            description: "Check Lua code for syntax errors the way SARE loads it (Lua 5.4 plus CfxLua: `hashes`, +=, ?.). Does not run it.",
            schema: || json!({
                "type": "object",
                "properties": { "code": { "type": "string" }, "name": { "type": "string", "description": "File name for error messages" } },
                "required": ["code"],
            }),
            run: |_, args| {
                let name = args["name"].as_str().unwrap_or("code.lua");
                Ok(match lua_syntax(name, args["code"].as_str().context("code is required")?) {
                    Ok(()) => json!({ "ok": true }),
                    Err(error) => json!({ "ok": false, "error": error }),
                })
            },
        },
    ]
}

/// Compile without running: CfxLua rewrite, then Lua 5.4.
pub fn lua_syntax(name: &str, code: &str) -> std::result::Result<(), String> {
    let plain = sa_lua::cfxlua::translate(code)?;
    let lua = mlua::Lua::new();
    lua.load(plain)
        .set_name(format!("@{name}"))
        .into_function()
        .map(drop)
        .map_err(|e| e.to_string())
}

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// A path inside a resource: plain relative parts only.
fn relative(path: &str) -> Result<PathBuf> {
    let p = Path::new(path);
    ensure!(
        !path.is_empty()
            && !path.contains('\\')
            && !path.contains(':')
            && p.components().all(|c| matches!(c, Component::Normal(_))),
        "{path:?} must be a relative path with / separators inside the resource"
    );
    // Windows device names are not files, whatever the extension.
    for part in path.split('/') {
        let stem = part
            .split('.')
            .next()
            .unwrap_or_default()
            .to_ascii_uppercase();
        let device = ["CON", "PRN", "AUX", "NUL"].contains(&stem.as_str())
            || (stem.len() == 4
                && (stem.starts_with("COM") || stem.starts_with("LPT"))
                && stem.as_bytes()[3].is_ascii_digit());
        ensure!(!device, "{part:?} is a reserved name on Windows");
    }
    Ok(p.to_path_buf())
}

pub struct Resource {
    pub name: String,
    /// e.g. `[sare-rp]` or `[gamemodes]/[maps]`; empty at the top level.
    pub category: String,
    pub folder: PathBuf,
}

/// Every resource folder under `dir`, in `[category]` folders like FiveM.
pub fn find_resources(dir: &Path) -> Vec<Resource> {
    let mut out = Vec::new();
    walk(dir, String::new(), 0, &mut out);
    out.sort_by_key(|r| r.name.to_lowercase());
    out
}
fn walk(dir: &Path, category: String, depth: usize, out: &mut Vec<Resource>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if !kind.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = entry.path();
        if name.starts_with('[') && name.ends_with(']') {
            if depth < MAX_CATEGORY_DEPTH {
                let category = match category.is_empty() {
                    true => name,
                    false => format!("{category}/{name}"),
                };
                walk(&path, category, depth + 1, out);
            }
        } else if ["fxmanifest.lua", "resource.json", "mod.json"]
            .iter()
            .any(|m| path.join(m).is_file())
        {
            out.push(Resource {
                name,
                category: category.clone(),
                folder: path,
            });
        }
    }
}

fn resource(server: &Server, name: &str) -> Result<Resource> {
    find_resources(server.resources_dir()?)
        .into_iter()
        .find(|r| r.name == name)
        .with_context(|| format!("no resource {name:?}; call list_resources"))
}

/// Names `ensure`d or `start`ed in server.cfg next to the resources folder.
fn started(resources_dir: &Path) -> Vec<String> {
    let Some(cfg) = server_cfg(resources_dir) else {
        return Vec::new();
    };
    std::fs::read_to_string(cfg)
        .unwrap_or_default()
        .lines()
        .filter_map(|line| {
            let mut words = line.split_whitespace();
            match words.next() {
                Some("ensure" | "start") => words.next().map(str::to_owned),
                _ => None,
            }
        })
        .collect()
}
fn server_cfg(resources_dir: &Path) -> Option<PathBuf> {
    Some(resources_dir.parent()?.join("server.cfg")).filter(|p| p.is_file())
}

fn files(folder: &Path) -> Vec<String> {
    fn collect(root: &Path, dir: &Path, depth: usize, out: &mut Vec<String>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if out.len() >= 200 || depth > 6 {
                return;
            }
            if path.is_dir() {
                collect(root, &path, depth + 1, out);
            } else if let Ok(rel) = path.strip_prefix(root) {
                let parts: Vec<_> = rel
                    .components()
                    .map(|c| c.as_os_str().to_string_lossy().into_owned())
                    .collect();
                out.push(parts.join("/"));
            }
        }
    }
    let mut out = Vec::new();
    collect(folder, folder, 0, &mut out);
    out.sort();
    out
}

fn list_resources(server: &Server) -> Result<Value> {
    let dir = server.resources_dir()?;
    let started = started(dir);
    let resources: Vec<Value> = find_resources(dir)
        .into_iter()
        .map(|r| {
            json!({
                "name": r.name,
                "category": r.category,
                "started_by_server_cfg": started.contains(&r.name),
                "files": files(&r.folder),
            })
        })
        .collect();
    Ok(json!({
        "resources_dir": dir.display().to_string(),
        "server_cfg": server_cfg(dir).map(|p| p.display().to_string()),
        "resources": resources,
    }))
}

fn read_resource_file(server: &Server, args: &Value) -> Result<Value> {
    let resource = resource(server, text(args, "resource")?)?;
    let path = resource.folder.join(relative(text(args, "path")?)?);
    inside(server, &path)?;
    let missing = || {
        format!(
            "{} has no file {:?}",
            resource.name,
            args["path"].as_str().unwrap_or_default()
        )
    };
    let size = std::fs::metadata(&path).with_context(missing)?.len();
    ensure!(size <= MAX_FILE as u64, "that file is larger than 256 KB");
    let bytes = std::fs::read(&path).with_context(missing)?;
    let text = String::from_utf8(bytes).context("that file is not text")?;
    Ok(Value::String(text))
}

/// Refuse paths that leave the resources folder through a link.
fn inside(server: &Server, path: &Path) -> Result<()> {
    let root = server.resources_dir()?.canonicalize()?;
    let existing = path
        .ancestors()
        .find(|p| p.exists())
        .context("bad path")?
        .canonicalize()?;
    ensure!(
        existing.starts_with(&root),
        "that path leaves the resources folder"
    );
    Ok(())
}

fn write_checked(server: &Server, path: &Path, content: &str) -> Result<bool> {
    let extension = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    ensure!(
        WRITABLE.contains(&extension.as_str()),
        "only text files can be written: .{}",
        WRITABLE.join(", .")
    );
    ensure!(content.len() <= MAX_FILE, "that file is larger than 256 KB");
    if extension == "lua" {
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        if let Err(error) = lua_syntax(&name, content) {
            bail!("not saved, the Lua has a syntax error: {error}");
        }
    }
    if extension == "json" {
        serde_json::from_str::<Value>(content).context("not saved, the JSON is invalid")?;
    }
    inside(server, path)?;
    let created = !path.exists();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    sa_scene::world_layout::write_atomic(path, content.as_bytes())?;
    Ok(created)
}

fn write_resource_file(server: &Server, args: &Value) -> Result<Value> {
    let resource = resource(server, text(args, "resource")?)?;
    let relative_path = relative(text(args, "path")?)?;
    let content = args["content"].as_str().context("content is required")?;
    let created = write_checked(server, &resource.folder.join(&relative_path), content)?;
    let mut reply = json!({
        "resource": resource.name,
        "path": args["path"],
        "bytes": content.len(),
        "created": created,
    });
    if created && relative_path.extension().is_some_and(|e| e == "lua") {
        reply["note"] = json!("New Lua file: make sure fxmanifest.lua lists it (server_script, client_script or shared_script).");
    }
    Ok(reply)
}

fn create_resource(server: &Server, args: &Value) -> Result<Value> {
    let dir = server.resources_dir()?;
    let name = text(args, "name")?.trim();
    ensure!(
        valid_name(name),
        "resource names are 1-64 letters, digits, _ or -"
    );
    ensure!(
        !find_resources(dir).iter().any(|r| r.name == name),
        "a resource named {name} already exists"
    );
    let category = args["category"]
        .as_str()
        .unwrap_or("local")
        .trim()
        .trim_start_matches('[')
        .trim_end_matches(']');
    ensure!(
        valid_name(category),
        "category names are letters, digits, _ or -"
    );
    let folder = dir.join(format!("[{category}]")).join(name);
    ensure!(!folder.exists(), "{} already exists", folder.display());
    let description = args["description"]
        .as_str()
        .map(str::trim)
        .filter(|d| !d.is_empty())
        .unwrap_or("Made with the SARE creator bridge.")
        .replace(['\'', '\n', '\r', '\\'], " ");
    let files = template(name, &description);
    for (file, content) in &files {
        write_checked(server, &folder.join(file), content)?;
    }
    let mut ensured = false;
    if args["ensure"].as_bool() == Some(true) {
        let cfg = server_cfg(dir).context(
            "the resource was made, but there is no server.cfg next to the resources folder to add it to",
        )?;
        if !started(dir).iter().any(|r| r == name) {
            let mut text = std::fs::read_to_string(&cfg)?;
            if !text.ends_with('\n') && !text.is_empty() {
                text.push('\n');
            }
            text.push_str(&format!("ensure {name}\n"));
            sa_scene::world_layout::write_atomic(&cfg, text.as_bytes())?;
        }
        ensured = true;
    }
    Ok(json!({
        "resource": name,
        "category": format!("[{category}]"),
        "files": files.iter().map(|(f, _)| *f).collect::<Vec<_>>(),
        "ensured_in_server_cfg": ensured,
        "next": "Edit server.lua and client.lua with write_resource_file. Restart the server (or run `ensure <name>` in its console) to load it.",
    }))
}

fn template(name: &str, description: &str) -> Vec<(&'static str, String)> {
    vec![
        (
            "fxmanifest.lua",
            format!(
                "fx_version 'cerulean'\ngame 'common'\nlua54 'yes'\n\n\
                 description '{description}'\nversion '0.1.0'\n\n\
                 server_script 'server.lua'\nclient_script 'client.lua'\n"
            ),
        ),
        (
            "server.lua",
            format!(
                "-- Server side of {name}: runs on the SARE server.\n\n\
                 RegisterCommand('{name}', function(source, args)\n\
                 \x20 print(('[{name}] player %s used /{name}'):format(source))\n\
                 \x20 TriggerClientEvent('{name}:hello', source, 'Hello from {name}!')\n\
                 end, false)\n"
            ),
        ),
        (
            "client.lua",
            format!(
                "-- Client side of {name}: runs in each player's game, sandboxed.\n\n\
                 RegisterNetEvent('{name}:hello', function(message)\n\
                 \x20 UI.notify({{ title = '{name}', description = message, type = 'success' }})\n\
                 end)\n"
            ),
        ),
        (
            "README.md",
            format!(
                "# {name}\n\n{description}\n\nType `/{name}` in game to test it. \
                 See docs/creating-resources.md and docs/client-scripts.md.\n"
            ),
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("sare-mcp-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("resources/[local]")).unwrap();
        dir
    }
    fn run(server: &Server, name: &str, args: Value) -> Result<Value> {
        let tool = tools().into_iter().find(|t| t.name == name).unwrap();
        (tool.run)(server, &args)
    }

    #[test]
    fn create_write_and_read_a_script_resource() {
        let root = temp("scripts");
        std::fs::write(root.join("server.cfg"), "ensure chat").unwrap();
        let server = Server {
            content: None,
            resources: Some(root.join("resources")),
        };
        let made = run(
            &server,
            "create_resource",
            json!({ "name": "my_garage", "description": "It's a garage", "ensure": true }),
        )
        .unwrap();
        assert_eq!(made["category"], "[local]");
        assert_eq!(
            std::fs::read_to_string(root.join("server.cfg")).unwrap(),
            "ensure chat\nensure my_garage\n"
        );
        // Every template file passes the same check as written files.
        let folder = root.join("resources/[local]/my_garage");
        for file in ["server.lua", "client.lua", "fxmanifest.lua"] {
            let code = std::fs::read_to_string(folder.join(file)).unwrap();
            lua_syntax(file, &code).unwrap();
        }
        assert!(run(&server, "create_resource", json!({ "name": "my_garage" })).is_err());
        let listed = run(&server, "list_resources", json!({})).unwrap();
        let garage = &listed["resources"][0];
        assert_eq!(garage["name"], "my_garage");
        assert_eq!(garage["started_by_server_cfg"], true);

        let wrote = run(
            &server,
            "write_resource_file",
            json!({ "resource": "my_garage", "path": "shared/config.lua", "content": "Config = {}\nConfig.Price += 0\n" }),
        )
        .unwrap();
        assert_eq!(wrote["created"], true);
        assert!(wrote["note"].as_str().unwrap().contains("fxmanifest"));
        let read = run(
            &server,
            "read_resource_file",
            json!({ "resource": "my_garage", "path": "shared/config.lua" }),
        )
        .unwrap();
        assert!(read.as_str().unwrap().starts_with("Config = {}"));

        let broken = run(
            &server,
            "write_resource_file",
            json!({ "resource": "my_garage", "path": "server.lua", "content": "if true then\n" }),
        )
        .unwrap_err();
        assert!(format!("{broken:#}").contains("syntax error"));
        assert!(std::fs::read_to_string(folder.join("server.lua"))
            .unwrap()
            .contains("RegisterCommand"));
        for (path, content) in [
            ("../escape.lua", "x = 1"),
            ("run.exe", "MZ"),
            ("data.json", "{nope"),
            ("C:\\x.lua", "x = 1"),
            ("con.lua", "x = 1"),
            ("c:/x.lua", "x = 1"),
        ] {
            assert!(
                run(
                    &server,
                    "write_resource_file",
                    json!({ "resource": "my_garage", "path": path, "content": content })
                )
                .is_err(),
                "{path}"
            );
        }
        let check = run(&server, "check_lua", json!({ "code": "local x = `hash`" })).unwrap();
        assert_eq!(check["ok"], true);
        let check = run(&server, "check_lua", json!({ "code": "local = 1" })).unwrap();
        assert_eq!(check["ok"], false);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn finds_the_shipped_resources() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../server-data/resources");
        let all = find_resources(&dir);
        let core = all
            .iter()
            .find(|r| r.name == "sare_rp_core")
            .expect("sare_rp_core");
        assert_eq!(core.category, "[sare-rp]");
        assert!(all.iter().any(|r| r.category == "[gamemodes]/[maps]"));
    }
}
